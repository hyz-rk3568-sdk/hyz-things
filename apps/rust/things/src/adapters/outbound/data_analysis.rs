//! Native-only official-text caching and OpenAI-compatible question preparation.
//! All provider credentials are server-only; AI candidates must pass domain verification.
use crate::domain::data_analysis::{
    verify_recipe, Material, QuestionRecipe, VerifiedQuestion, QUIZ_GROUPS, QUIZ_PER_GROUP,
    QUIZ_SECONDS, SECTIONS, SOURCE_URL,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::MetadataExt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use uuid::Uuid;

const ENV_PATH: &str = "/userdata/hyz-things/.env";
const PROVIDER_LIMIT: usize = 128 * 1024;
const MAX_DOCUMENT_BYTES: usize = 3 * 1024 * 1024;
const MAX_SECTION_CHARS: usize = 1600;

#[derive(Clone)]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
    api_key: String,
}

impl AiConfig {
    fn parse_pairs(pairs: &HashMap<String, String>) -> Result<Option<Self>, &'static str> {
        let base = pairs
            .get("HYZ_THINGS_AI_BASE_URL")
            .map(String::as_str)
            .unwrap_or("");
        let key = pairs
            .get("HYZ_THINGS_AI_API_KEY")
            .map(String::as_str)
            .unwrap_or("");
        let model = pairs
            .get("HYZ_THINGS_AI_MODEL")
            .map(String::as_str)
            .unwrap_or("");
        if base.is_empty() && key.is_empty() && model.is_empty() {
            return Ok(None);
        }
        if base.is_empty()
            || key.is_empty()
            || model.is_empty()
            || base.len() > 500
            || key.len() > 1024
            || model.len() > 120
        {
            return Err("AI configuration is missing a valid URL, Key, or Model");
        }
        let parsed = url::Url::parse(base).map_err(|_| "AI URL is invalid")?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err("AI URL must be HTTPS without credentials, query or fragment");
        }
        if key.chars().any(char::is_whitespace) || model.chars().any(char::is_whitespace) {
            return Err("AI Key or Model contains whitespace");
        }
        Ok(Some(Self {
            base_url: base.trim_end_matches('/').to_owned(),
            api_key: key.to_owned(),
            model: model.to_owned(),
        }))
    }
    fn completion_url(&self) -> String {
        let root = self.base_url.trim_end_matches('/');
        if root.ends_with("/chat/completions") {
            root.to_owned()
        } else {
            format!("{root}/chat/completions")
        }
    }
}

/// Fixed root-owned configuration path: no cwd-dependent dotenv behavior.
pub fn load_ai_env() -> Result<Option<AiConfig>, &'static str> {
    let meta = match fs::symlink_metadata(ENV_PATH) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("AI configuration cannot be read"),
    };
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.uid() != 0
        || meta.mode() & 0o077 != 0
        || meta.len() > 16 * 1024
    {
        return Err("AI .env must be a root-owned 0600 regular file");
    }
    let input = fs::read_to_string(ENV_PATH).map_err(|_| "AI configuration cannot be read")?;
    parse_env(&input)
}
fn parse_env(input: &str) -> Result<Option<AiConfig>, &'static str> {
    let mut pairs = HashMap::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("Invalid .env entry")?;
        let key = key.trim();
        if !matches!(
            key,
            "HYZ_THINGS_AI_BASE_URL" | "HYZ_THINGS_AI_API_KEY" | "HYZ_THINGS_AI_MODEL"
        ) {
            continue;
        }
        let mut value = value.trim();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = &value[1..value.len() - 1];
        }
        if value.contains('\n') || value.contains('\r') {
            return Err("Invalid AI env value");
        }
        pairs.insert(key.to_owned(), value.to_owned());
    }
    AiConfig::parse_pairs(&pairs)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct QuestionGroup {
    pub material: Material,
    pub questions: Vec<VerifiedQuestion>,
}

#[derive(Clone, Serialize)]
pub struct PublicQuestion {
    pub question: String,
    pub options: [String; 4],
    pub skill: String,
}
#[derive(Clone, Serialize)]
pub struct PublicGroup {
    pub material: Material,
    pub questions: Vec<PublicQuestion>,
}
#[derive(Clone, Serialize)]
pub struct RunView {
    pub id: String,
    pub status: String,
    pub started_at_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
    pub duration_seconds: u64,
    pub groups: Vec<PublicGroup>,
    pub answers: Vec<Option<usize>>,
}
#[derive(Clone, Serialize)]
pub struct GradedQuestion {
    pub number: usize,
    pub chosen: Option<usize>,
    pub correct_index: usize,
    pub answer: String,
    pub explanation: String,
    pub topic: String,
    pub skill: String,
    pub evidence: String,
}
#[derive(Clone, Serialize)]
pub struct RunResult {
    pub id: String,
    pub correct: usize,
    pub total: usize,
    pub questions: Vec<GradedQuestion>,
}

#[derive(Clone, Serialize)]
pub struct PrepareProgress {
    pub state: String,
    pub verified: usize,
    pub message: String,
    pub run_id: Option<String>,
}

#[derive(Clone)]
pub struct DataAnalysisService {
    pool: SqlitePool,
    config: Option<AiConfig>,
    client: reqwest::Client,
    jobs: Arc<Mutex<HashMap<String, PrepareProgress>>>,
    gate: Arc<tokio::sync::Semaphore>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
fn as_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

impl DataAnalysisService {
    pub fn new(pool: SqlitePool, config: Option<AiConfig>) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(48))
            .redirect(reqwest::redirect::Policy::limited(2))
            .build()
            .map_err(as_error)?;
        Ok(Self {
            pool,
            config,
            client,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
        })
    }
    pub fn configured_model(&self) -> Option<String> {
        self.config.as_ref().map(|x| x.model.clone())
    }
    pub async fn counts(&self) -> Result<(i64, i64), String> {
        let sources = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM data_analysis_sources")
            .fetch_one(&self.pool)
            .await
            .map_err(as_error)?;
        let groups = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM data_analysis_groups")
            .fetch_one(&self.pool)
            .await
            .map_err(as_error)?;
        Ok((sources, groups))
    }
    pub async fn weaknesses(&self) -> Vec<String> {
        let mut skills = Vec::new();
        if let Ok(rows)=sqlx::query("SELECT skill, count(*) AS n, sum(CASE WHEN is_correct=0 THEN 1 ELSE 0 END) AS misses FROM data_analysis_results GROUP BY skill ORDER BY (sum(CASE WHEN is_correct=0 THEN 1 ELSE 0 END)*1.0 / count(*)) DESC LIMIT 4")
            .fetch_all(&self.pool).await {
            for row in rows { if let Ok(name)=row.try_get::<String,_>("skill"){skills.push(name);} }
        }
        if skills.is_empty() {
            if let Ok(rows)=sqlx::query("SELECT tag FROM flashcard_card_tags WHERE tag IN ('平均数增量','增量','基期','混合增长率','比重','综合分析与审题') GROUP BY tag ORDER BY count(*) DESC LIMIT 4")
                .fetch_all(&self.pool).await {
                for row in rows { if let Ok(name)=row.try_get::<String,_>("tag"){skills.push(name);} }
            }
        }
        if skills.is_empty() {
            skills = vec!["基期量".into(), "增长量".into(), "比重".into()];
        }
        skills
    }
    pub async fn prepare(&self) -> Result<String, String> {
        if self.config.is_none() {
            return Err("请在 RK3568 .env 配置 AI URL、Key、Model 并重启服务".into());
        }
        if self.gate.available_permits() == 0 {
            return Err("已有综合出题任务正在执行，请稍后重试".into());
        }
        let id = Uuid::now_v7().to_string();
        self.jobs.lock().await.insert(
            id.clone(),
            PrepareProgress {
                state: "pending".into(),
                verified: 0,
                message: "正在准备资料".into(),
                run_id: None,
            },
        );
        let worker = self.clone();
        let job = id.clone();
        tokio::spawn(async move {
            let guard = match worker.gate.clone().acquire_owned().await {
                Ok(g) => g,
                Err(_) => return,
            };
            let result = worker.generate(&job).await;
            drop(guard);
            match result {
                Ok(run_id) => {
                    worker
                        .set_progress(&job, "ready", 20, "已完成校验，可以开始训练", Some(run_id))
                        .await
                }
                Err(msg) => worker.set_progress(&job, "failed", 0, &msg, None).await,
            }
        });
        Ok(id)
    }
    async fn set_progress(
        &self,
        id: &str,
        state: &str,
        verified: usize,
        message: &str,
        run_id: Option<String>,
    ) {
        let mut jobs = self.jobs.lock().await;
        if let Some(job) = jobs.get_mut(id) {
            job.state = state.to_owned();
            job.verified = verified;
            job.message = message.to_owned();
            job.run_id = run_id;
        }
    }
    pub async fn job(&self, id: &str) -> Option<PrepareProgress> {
        self.jobs.lock().await.get(id).cloned()
    }
    async fn load_sections(&self) -> Result<Vec<Material>, String> {
        if let Some(cached) = sqlx::query_scalar::<_, String>(
            "SELECT sections_json FROM data_analysis_sources WHERE url=?",
        )
        .bind(SOURCE_URL)
        .fetch_optional(&self.pool)
        .await
        .map_err(as_error)?
        {
            if let Ok(values) = serde_json::from_str::<Vec<Material>>(&cached) {
                if values.len() >= 4 {
                    return Ok(values);
                }
            }
        }
        let response = self
            .client
            .get(SOURCE_URL)
            .send()
            .await
            .map_err(|_| "官方资料抓取失败；请检查 RK3568 出站 HTTPS 连接".to_owned())?;
        if !response.status().is_success() {
            return Err(format!(
                "国家统计局网站返回 HTTP {}",
                response.status().as_u16()
            ));
        }
        if response.content_length().unwrap_or(0) > MAX_DOCUMENT_BYTES as u64 {
            return Err("官方文章超过大小限制".into());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "官方文章读取失败".to_owned())?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("官方文章超过大小限制".into());
        }
        let html =
            String::from_utf8(bytes.to_vec()).map_err(|_| "官方文章不是 UTF-8".to_owned())?;
        let sections = parse_official_sections(&html);
        if sections.len() < 4 {
            return Err(format!(
                "官方文章仅解析出{}个可用文字章节，需要至少4个",
                sections.len()
            ));
        }
        let hash = Sha256::digest(html.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let serialized = serde_json::to_string(&sections).map_err(as_error)?;
        sqlx::query("INSERT INTO data_analysis_sources(url,title,content_hash,fetched_at_ms,sections_json) VALUES(?,?,?,?,?) ON CONFLICT(url) DO UPDATE SET content_hash=excluded.content_hash,fetched_at_ms=excluded.fetched_at_ms,sections_json=excluded.sections_json")
            .bind(SOURCE_URL).bind("中华人民共和国2025年国民经济和社会发展统计公报")
            .bind(hash).bind(now_ms()).bind(serialized)
            .execute(&self.pool).await.map_err(as_error)?;
        Ok(sections)
    }
    async fn generate(&self, job: &str) -> Result<String, String> {
        self.set_progress(job, "fetching", 0, "正在读取官方统计资料", None)
            .await;
        let materials = self.load_sections().await?;
        self.set_progress(job, "selecting", 0, "正在选择薄弱和未练主题", None)
            .await;
        let weaknesses = self.weaknesses().await;
        let mut scores = HashMap::<String, (i64, i64)>::new();
        let rows=sqlx::query("SELECT topic, count(*) n, sum(CASE WHEN is_correct=0 THEN 1 ELSE 0 END) missed FROM data_analysis_results GROUP BY topic")
            .fetch_all(&self.pool).await.map_err(as_error)?;
        for row in rows {
            let topic: String = row.try_get("topic").map_err(as_error)?;
            scores.insert(
                topic,
                (
                    row.try_get("n").unwrap_or(0),
                    row.try_get("missed").unwrap_or(0),
                ),
            );
        }
        // Explore never-seen topics before repeating familiar source sections.
        // Once covered, prioritize topics with high error ratios.
        let mut ordered = materials;
        ordered.sort_by(|a, b| {
            let (na, ma) = scores.get(&a.topic).copied().unwrap_or((0, 0));
            let (nb, mb) = scores.get(&b.topic).copied().unwrap_or((0, 0));
            let score_a = if na == 0 { 2.0 } else { ma as f64 / na as f64 };
            let score_b = if nb == 0 { 2.0 } else { mb as f64 / nb as f64 };
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let existing = sqlx::query(
            "SELECT group_json FROM data_analysis_groups ORDER BY created_at_ms DESC LIMIT 200",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(as_error)?;
        let mut cache = HashMap::<String, QuestionGroup>::new();
        for row in existing {
            let data: String = row.try_get("group_json").map_err(as_error)?;
            if let Ok(group) = serde_json::from_str::<QuestionGroup>(&data) {
                if group.questions.len() == 5 {
                    cache.entry(group.material.section.clone()).or_insert(group);
                }
            }
        }
        let mut ready = Vec::new();
        let mut last_error = String::new();
        let mut attempts = 0;
        for material in &ordered {
            if ready.len() == QUIZ_GROUPS {
                break;
            }
            if let Some(group) = cache.get(&material.section) {
                ready.push(group.clone());
                continue;
            }
            if attempts >= 8 {
                break;
            }
            attempts += 1;
            self.set_progress(
                job,
                "generating",
                ready.len() * 5,
                "正在调用AI生成新的官方资料题",
                None,
            )
            .await;
            let group = match self.generate_group(material, &weaknesses).await {
                Ok(group) => group,
                Err(err) => {
                    last_error = err;
                    if last_error.contains("HTTP 401")
                        || last_error.contains("HTTP 403")
                        || last_error.contains("HTTP 429")
                    {
                        break;
                    }
                    continue;
                }
            };
            let group_id = Uuid::now_v7().to_string();
            sqlx::query("INSERT INTO data_analysis_groups(id,source_url,section,topic,group_json,created_at_ms) VALUES (?,?,?,?,?,?)")
                .bind(group_id).bind(&material.source_url).bind(&material.section).bind(&material.topic)
                .bind(serde_json::to_string(&group).map_err(as_error)?).bind(now_ms())
                .execute(&self.pool).await.map_err(as_error)?;
            ready.push(group);
            self.set_progress(
                job,
                "validating",
                ready.len() * 5,
                "已通过严格校验，正在组卷",
                None,
            )
            .await;
        }
        if ready.len() != QUIZ_GROUPS {
            return Err(format!(
                "只验证通过{}/20题，暂不开始：{}",
                ready.len() * 5,
                last_error
            ));
        }
        let id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO data_analysis_runs(id,groups_json,answers_json,status,created_at_ms) VALUES(?,?,?,'ready',?)")
            .bind(&id)
            .bind(serde_json::to_string(&ready).map_err(as_error)?)
            .bind(serde_json::to_string(&vec![None::<usize>;20]).map_err(as_error)?)
            .bind(now_ms()).execute(&self.pool).await.map_err(as_error)?;
        Ok(id)
    }
    async fn generate_group(
        &self,
        material: &Material,
        weaknesses: &[String],
    ) -> Result<QuestionGroup, String> {
        let config = self.config.as_ref().ok_or("AI 未配置")?;
        let system="你是公务员资料分析命题助手。只使用用户提供的真实官方统计段落。输出严格JSON对象：{\"questions\":[{\"question\":\"...\",\"evidence\":\"从原文完整原样复制至少15字且包含a和b两个数字的连续片段\",\"operation\":\"base_period|growth_amount|growth_rate|share|average|difference|ratio\",\"a\":数值,\"b\":数值,\"unit\":\"亿元|万吨|%等\",\"explanation\":\"简短速算解析\"}]}。请生成10道不同候选题；base_period和growth_amount里 a 是现期量，b 是同比增长率（百分数，下降时为负）。growth_rate里a现期量b基期量；share a部分量b总体量；average/ratio是a除以b；difference是a减去b。题干必须有唯一答案，标注'约'，不得自行补充原文没有的数字，不得把不同口径指标相除。不要生成题外文字，不返回标准选项。";
        let user = format!(
            "材料原文：{}\n章节：{}\n重点训练考点：{}\n请从这一段材料生成10道不同的候选题。",
            material.text,
            material.section,
            weaknesses.join("、")
        );
        let payload = json!({"model":config.model,"temperature":0.4,"response_format":{"type":"json_object"},
            "messages":[{"role":"system","content":system},{"role":"user","content":user}]});
        let mut last_reason = String::new();
        for _ in 0..2 {
            let response = self
                .client
                .post(config.completion_url())
                .bearer_auth(&config.api_key)
                .json(&payload)
                .send()
                .await
                .map_err(|_| "AI 服务网络请求失败或超时".to_owned())?;
            if !response.status().is_success() {
                return Err(format!(
                    "AI 服务返回 HTTP {}；请检查模型、额度和密钥",
                    response.status().as_u16()
                ));
            }
            if response.content_length().unwrap_or(0) > PROVIDER_LIMIT as u64 {
                return Err("AI 响应超过大小限制".into());
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|_| "AI 响应读取失败".to_owned())?;
            if bytes.len() > PROVIDER_LIMIT {
                return Err("AI 响应超过大小限制".into());
            }
            let doc: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| "AI 响应不是 JSON".to_owned())?;
            let content = doc
                .pointer("/choices/0/message/content")
                .and_then(|x| x.as_str())
                .ok_or("AI 响应缺少 message.content")?;
            #[derive(Deserialize)]
            struct Batch {
                questions: Vec<QuestionRecipe>,
            }
            let batch: Batch =
                serde_json::from_str(content).map_err(|_| "AI 题目 JSON 结构无效".to_owned())?;
            let mut result = Vec::new();
            let mut seen = HashSet::new();
            for candidate in &batch.questions {
                if let Ok(q) = verify_recipe(material, candidate, result.len()) {
                    let fingerprint = format!(
                        "{}:{:?}:{}:{}",
                        q.evidence, candidate.operation, candidate.a, candidate.b
                    );
                    if seen.insert(fingerprint) {
                        result.push(q);
                    }
                }
                if result.len() == QUIZ_PER_GROUP {
                    break;
                }
            }
            if result.len() == QUIZ_PER_GROUP {
                return Ok(QuestionGroup {
                    material: material.clone(),
                    questions: result,
                });
            }
            last_reason = format!("「{}」只通过 {}/5 题校验", material.section, result.len());
        }
        Err(format!("{last_reason}；请重试生成或更换兼容模型"))
    }
    async fn fetch_row(
        &self,
        id: &str,
    ) -> Result<(Vec<QuestionGroup>, Vec<Option<usize>>, String, Option<i64>), String> {
        let row=sqlx::query("SELECT groups_json,answers_json,status,started_at_ms FROM data_analysis_runs WHERE id=?")
            .bind(id).fetch_optional(&self.pool).await.map_err(as_error)?.ok_or("训练不存在")?;
        let groups =
            serde_json::from_str(&row.try_get::<String, _>("groups_json").map_err(as_error)?)
                .map_err(as_error)?;
        let answers =
            serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                .map_err(as_error)?;
        Ok((
            groups,
            answers,
            row.try_get("status").map_err(as_error)?,
            row.try_get("started_at_ms").map_err(as_error)?,
        ))
    }
    pub async fn run(&self, id: &str) -> Result<RunView, String> {
        sqlx::query("UPDATE data_analysis_runs SET status='active',started_at_ms=? WHERE id=? AND status='ready'")
            .bind(now_ms()).bind(id).execute(&self.pool).await.map_err(as_error)?;
        let (groups, answers, status, started_at_ms) = self.fetch_row(id).await?;
        Ok(RunView {
            id: id.into(),
            status,
            started_at_ms,
            deadline_at_ms: started_at_ms.map(|ms| ms + QUIZ_SECONDS as i64 * 1000),
            duration_seconds: QUIZ_SECONDS,
            groups: groups
                .into_iter()
                .map(|g| PublicGroup {
                    material: g.material,
                    questions: g
                        .questions
                        .into_iter()
                        .map(|q| PublicQuestion {
                            question: q.question,
                            options: q.options,
                            skill: q.skill,
                        })
                        .collect(),
                })
                .collect(),
            answers,
        })
    }
    pub async fn answer(&self, id: &str, number: usize, choice: usize) -> Result<(), String> {
        if !(1..=20).contains(&number) || choice > 3 {
            return Err("题号或选项无效".into());
        }
        let (_groups, mut answers, status, started) = self.fetch_row(id).await?;
        if status != "active"
            || started.is_none_or(|ms| now_ms() >= ms + QUIZ_SECONDS as i64 * 1000)
        {
            return Err("训练已交卷或超时".into());
        }
        answers[number - 1] = Some(choice);
        sqlx::query("UPDATE data_analysis_runs SET answers_json=? WHERE id=? AND status='active'")
            .bind(serde_json::to_string(&answers).map_err(as_error)?)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(as_error)?;
        Ok(())
    }
    pub async fn submit(&self, id: &str) -> Result<RunResult, String> {
        let (groups, answers, status, _started) = self.fetch_row(id).await?;
        if status != "submitted" {
            let mut txn = self.pool.begin().await.map_err(as_error)?;
            sqlx::query("UPDATE data_analysis_runs SET status='submitted',ended_at_ms=? WHERE id=? AND status!='submitted'")
                .bind(now_ms()).bind(id).execute(&mut *txn).await.map_err(as_error)?;
            for (idx, q) in groups
                .iter()
                .flat_map(|g| g.questions.iter().map(move |q| (g, q)))
                .enumerate()
            {
                let (g, q) = q;
                let is_correct = (answers[idx] == Some(q.correct_index)) as i64;
                sqlx::query("INSERT OR IGNORE INTO data_analysis_results(run_id,question_no,topic,skill,is_correct,elapsed_ms) VALUES(?,?,?,?,?,?)")
                    .bind(id).bind((idx+1) as i64).bind(&g.material.topic)
                    .bind(&q.skill).bind(is_correct).bind(0i64)
                    .execute(&mut *txn).await.map_err(as_error)?;
            }
            txn.commit().await.map_err(as_error)?;
        }
        let questions = groups
            .iter()
            .flat_map(|g| g.questions.iter().map(move |q| (g, q)))
            .enumerate()
            .map(|(index, (g, q))| GradedQuestion {
                number: index + 1,
                chosen: answers[index],
                correct_index: q.correct_index,
                answer: q.answer.clone(),
                explanation: q.explanation.clone(),
                topic: g.material.topic.clone(),
                skill: q.skill.clone(),
                evidence: q.evidence.clone(),
            })
            .collect::<Vec<_>>();
        let correct = questions
            .iter()
            .filter(|q| q.chosen == Some(q.correct_index))
            .count();
        Ok(RunResult {
            id: id.into(),
            correct,
            total: 20,
            questions,
        })
    }
}

fn normalize(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace('\u{00a0}', " ")
        .trim()
        .to_owned()
}
fn compact_heading(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_whitespace()).collect()
}
/// Do not accept tables or images as the first-generation source. Retain the
/// official section and a fixed topic, with a material snapshot per section.
pub fn parse_official_sections(html: &str) -> Vec<Material> {
    let dom = scraper::Html::parse_document(html);
    let selector = match scraper::Selector::parse("p") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut result = Vec::new();
    let mut current: Option<usize> = None;
    let mut raw: Vec<Vec<String>> = vec![Vec::new(); SECTIONS.len()];
    for p in dom.select(&selector) {
        let text = normalize(&p.text().collect::<String>());
        let heading = compact_heading(&text);
        if let Some(i) = SECTIONS.iter().position(|(title, _)| heading == *title) {
            current = Some(i);
            continue;
        }
        if let Some(i) = current {
            if heading.starts_with("注：")
                || heading.starts_with("注[")
                || text.chars().count() < 28
                || !text.chars().any(|c| c.is_ascii_digit())
            {
                continue;
            }
            if raw[i].iter().map(|x| x.chars().count()).sum::<usize>() < MAX_SECTION_CHARS {
                raw[i].push(text);
            }
        }
    }
    for (i, (heading, topic)) in SECTIONS.iter().enumerate() {
        let text = raw[i].join("\n\n");
        if text.chars().count() >= 180 {
            let subtopic = match i {
                0 => "GDP与区域",
                1 => "农业生产",
                2 => "工业与建筑业",
                3 => "交通邮政服务",
                4 => "零售消费",
                5 => "投资与房地产",
                6 => "进出口贸易",
                7 => "金融财政",
                8 => "收入社会保障",
                9 => "科研教育",
                10 => "文化卫生旅游",
                _ => "资源环境",
            };
            result.push(Material {
                section: (*heading).into(),
                topic: (*topic).into(),
                subtopic: subtopic.into(),
                text,
                source_url: SOURCE_URL.into(),
            });
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_not_reflected_and_endpoint_uses_base_path() {
        let env=parse_env("HYZ_THINGS_AI_BASE_URL=https://api.deepseek.com/v1\nHYZ_THINGS_AI_API_KEY=secret-key\nHYZ_THINGS_AI_MODEL=my-model\n").unwrap().unwrap();
        assert_eq!(
            env.completion_url(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(env.model, "my-model");
        assert!(!format!("{:?}", env.model).contains("secret-key"));
        assert!(parse_env("HYZ_THINGS_AI_BASE_URL=http://localhost:9999\nHYZ_THINGS_AI_API_KEY=x\nHYZ_THINGS_AI_MODEL=m").is_err());
    }
    #[test]
    fn parser_keeps_sections_separate() {
        let html = format!(
            "<article><p>一、综合</p><p>{}</p><p>二、农业</p><p>{}</p></article>",
            "国内生产总值1401879亿元，比上年增长5.0%。".repeat(9),
            "全年粮食产量71488万吨，比上年增加838万吨。".repeat(9)
        );
        let sections = parse_official_sections(&html);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].topic, "宏观经济");
        assert!(sections[1].text.contains("71488"));
    }
}
