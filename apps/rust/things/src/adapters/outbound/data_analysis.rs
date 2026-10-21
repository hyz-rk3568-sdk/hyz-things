//! Native-only official-text caching and Codex App Server question preparation.
//! Only verified numerical recipes are admitted into the training database.
use super::codex_app_server::{validate_token, CodexSession, TURN_TIMEOUT};
use crate::domain::data_analysis::{
    select_material_batch, verify_recipe, Material, QuestionRecipe, VerifiedQuestion,
    OFFICIAL_SOURCES, QUIZ_GROUPS, QUIZ_PER_GROUP, QUIZ_SECONDS, SECTIONS, SOURCE_URL,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    fs,
    future::Future,
    os::unix::fs::MetadataExt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::Mutex, task::JoinSet};
use uuid::Uuid;

mod locating;
pub use locating::{LocateClick, LocatingRequest, LocatingTopic, LocatingView};

const ENV_PATH: &str = "/userdata/hyz-things/.env";
const MAX_DOCUMENT_BYTES: usize = 3 * 1024 * 1024;
const MAX_CACHED_SECTION_CHARS: usize = 30000;
const MAX_GENERATION_CHARS: usize = 3000;

#[derive(Clone)]
pub struct CodexConfig {
    // Fixed-address LAN authentication only; this is NOT the ChatGPT OAuth token.
    token: String,
}

/// Root-only .env file. Ignore unrelated configuration rather than accepting
/// user-supplied URLs, model IDs, or legacy OpenAI-compatible credentials.
pub fn load_codex_env() -> Result<Option<CodexConfig>, &'static str> {
    let meta = match fs::symlink_metadata(ENV_PATH) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Codex .env cannot be read"),
    };
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.uid() != 0
        || meta.mode() & 0o077 != 0
        || meta.len() > 16 * 1024
    {
        return Err("Codex .env must be a root-owned 0600 regular file");
    }
    let input = fs::read_to_string(ENV_PATH).map_err(|_| "Codex .env cannot be read")?;
    parse_env(&input)
}

fn parse_env(input: &str) -> Result<Option<CodexConfig>, &'static str> {
    let mut token = None;
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("Invalid .env entry")?;
        if key.trim() != "HYZ_THINGS_CODEX_WS_TOKEN" {
            continue;
        }
        if token.is_some() {
            return Err("Duplicate Codex Token");
        }
        let mut value = value.trim();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = &value[1..value.len() - 1];
        }
        validate_token(value)
            .map_err(|_| "Codex WS Token contains invalid bearer-token characters")?;
        token = Some(value.to_owned());
    }
    Ok(token.map(|token| CodexConfig { token }))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct QuestionGroup {
    #[serde(default)]
    pub version: u32,
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
    config: Option<CodexConfig>,
    client: reqwest::Client,
    jobs: Arc<Mutex<HashMap<String, PrepareProgress>>>,
    gate: Arc<tokio::sync::Semaphore>,
}

/// Run one bounded wave. Each task establishes its own Codex WS session and
/// ephemeral Thread, avoiding response-ID multiplexing and mixed conversations.
/// Results retain their source slot even if the network finishes out of order.
async fn run_generation_wave<F, Fut>(
    candidates: Vec<(usize, Material)>,
    producer: F,
) -> Vec<(usize, Result<QuestionGroup, String>)>
where
    F: Fn(Material) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<QuestionGroup, String>> + Send + 'static,
{
    debug_assert!(candidates.len() <= QUIZ_GROUPS);
    let mut running = JoinSet::new();
    for (index, material) in candidates {
        let produce = producer.clone();
        running.spawn(async move { (index, produce(material).await) });
    }
    let mut results = Vec::new();
    while let Some(outcome) = running.join_next().await {
        match outcome {
            Ok(result) => results.push(result),
            Err(_) => results.push((usize::MAX, Err("Codex 并行出题任务意外中断".to_owned()))),
        }
    }
    results
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
    pub fn new(pool: SqlitePool, config: Option<CodexConfig>) -> Result<Self, String> {
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
        self.config.as_ref().map(|_| "Codex App Server".to_owned())
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
            return Err(
                "请在 RK3568 的 /userdata/hyz-things/.env 配置 Codex WebSocket Token".into(),
            );
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
        let mut sections = Vec::new();
        let mut cached_urls = HashSet::new();
        for (year, url) in OFFICIAL_SOURCES {
            let row = sqlx::query_scalar::<_, String>(
                "SELECT sections_json FROM data_analysis_sources WHERE url=?",
            )
            .bind(url)
            .fetch_optional(&self.pool)
            .await
            .map_err(as_error)?;
            if let Some(cached) = row {
                if let Ok(values) = serde_json::from_str::<Vec<Material>>(&cached) {
                    if values.len() >= 4 {
                        cached_urls.insert(url);
                        sections.extend(values);
                    }
                }
            }
            let _ = year;
        }
        // No outbound request is needed when all years have been cached.
        // Native offline tests can use prepared groups without a public site.
        if cached_urls.len() == OFFICIAL_SOURCES.len() || (cfg!(test) && sections.len() >= 4) {
            return Ok(sections);
        }
        for (year, url) in OFFICIAL_SOURCES {
            if cached_urls.contains(url) {
                continue;
            }
            let Ok(response) = self
                .client
                .get(url)
                .timeout(Duration::from_secs(15))
                .send()
                .await
            else {
                continue; // retain usable offline cached years
            };
            if !response.status().is_success()
                || response.content_length().unwrap_or(0) > MAX_DOCUMENT_BYTES as u64
            {
                continue;
            }
            let Ok(bytes) = response.bytes().await else {
                continue;
            };
            if bytes.len() > MAX_DOCUMENT_BYTES {
                continue;
            }
            let Ok(html) = String::from_utf8(bytes.to_vec()) else {
                continue;
            };
            let values = parse_official_sections_from(&html, url);
            if values.len() < 4 {
                continue;
            }
            let hash = Sha256::digest(html.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let encoded = serde_json::to_string(&values).map_err(as_error)?;
            sqlx::query("INSERT INTO data_analysis_sources(url,title,content_hash,fetched_at_ms,sections_json) VALUES(?,?,?,?,?) ON CONFLICT(url) DO UPDATE SET content_hash=excluded.content_hash,fetched_at_ms=excluded.fetched_at_ms,sections_json=excluded.sections_json")
                .bind(url).bind(format!("中华人民共和国{year}年国民经济和社会发展统计公报"))
                .bind(hash).bind(now_ms()).bind(encoded)
                .execute(&self.pool).await.map_err(as_error)?;
            sections.extend(values);
        }
        if sections.len() < 4 {
            return Err("年度官方资料不足，无法选出4组材料；请检查国家统计局网络连接".into());
        }
        Ok(sections)
    }
    async fn generate(&self, job: &str) -> Result<String, String> {
        self.set_progress(job, "fetching", 0, "正在读取官方统计资料", None)
            .await;
        let materials = self.load_sections().await?;
        let seen = self.sync_seen_materials().await?;
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
        ordered.retain(|m| !seen.contains(&format!("{}\u{1f}{}", m.source_url, m.section)));
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
                if group.version >= 3 && group.questions.len() == QUIZ_PER_GROUP {
                    let key = format!("{}|{}", group.material.source_url, group.material.section);
                    cache.entry(key).or_insert(group);
                }
            }
        }
        let mut ready: Vec<QuestionGroup> = Vec::new();
        let mut attempted = HashSet::<(String, String)>::new();
        let mut last_error = String::new();
        let mut attempts = 0;
        // The strict pass avoids repeated official years and topics. A fallback
        // pass only relaxes those two diversity constraints; it never repeats a
        // source/section identity or requeues a failed generation indefinitely.
        for strict_diversity in [true, false] {
            loop {
                if ready.len() == QUIZ_GROUPS {
                    break;
                }
                let completed: Vec<Material> =
                    ready.iter().map(|group| group.material.clone()).collect();
                let wave = select_material_batch(
                    &ordered,
                    &completed,
                    &attempted,
                    strict_diversity,
                    QUIZ_GROUPS - ready.len(),
                );
                if wave.is_empty() {
                    break;
                }

                let mut ordered_results: Vec<Option<QuestionGroup>> = vec![None; wave.len()];
                let mut needs_store = vec![false; wave.len()];
                let mut requests = Vec::new();
                for (slot, material) in wave.iter().enumerate() {
                    attempted.insert((material.source_url.clone(), material.section.clone()));
                    let key = format!("{}|{}", material.source_url, material.section);
                    if let Some(group) = cache.get(&key) {
                        ordered_results[slot] = Some(group.clone());
                    } else if attempts < 12 {
                        attempts += 1;
                        needs_store[slot] = true;
                        requests.push((slot, material.clone()));
                    }
                }
                if requests.is_empty() && ordered_results.iter().all(Option::is_none) {
                    break;
                }
                if !requests.is_empty() {
                    self.set_progress(
                        job,
                        "generating",
                        ready.len() * QUIZ_PER_GROUP,
                        "最多4份不同材料正在并行生成，每份使用独立的 Codex Thread",
                        None,
                    )
                    .await;
                    let worker = self.clone();
                    let weak = Arc::new(weaknesses.clone());
                    for (slot, outcome) in run_generation_wave(requests, move |material| {
                        let worker = worker.clone();
                        let weak = Arc::clone(&weak);
                        async move { worker.generate_group(&material, &weak).await }
                    })
                    .await
                    {
                        match outcome {
                            Ok(group) if slot < ordered_results.len() => {
                                ordered_results[slot] = Some(group);
                            }
                            Ok(_) => last_error = "Codex 并行任务返回异常序号".into(),
                            Err(error) => last_error = error,
                        }
                    }
                }

                // Only the coordinator persists groups and updates the run,
                // preserving the deterministic order selected before spawning.
                for (slot, group) in ordered_results.into_iter().enumerate() {
                    let Some(group) = group else {
                        continue;
                    };
                    if needs_store[slot] {
                        sqlx::query("INSERT INTO data_analysis_groups(id,source_url,section,topic,group_json,created_at_ms) VALUES (?,?,?,?,?,?)")
                            .bind(Uuid::now_v7().to_string())
                            .bind(&group.material.source_url)
                            .bind(&group.material.section)
                            .bind(&group.material.topic)
                            .bind(serde_json::to_string(&group).map_err(as_error)?)
                            .bind(now_ms())
                            .execute(&self.pool)
                            .await
                            .map_err(as_error)?;
                    }
                    ready.push(group);
                    self.set_progress(
                        job,
                        "validating",
                        ready.len() * QUIZ_PER_GROUP,
                        "已核对来源数字、题目答案与主题分布",
                        None,
                    )
                    .await;
                }
            }
            if ready.len() == QUIZ_GROUPS {
                break;
            }
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
    /// Read a small set of hyz's actual wrong questions as question-form
    /// patterns (up to twenty). Historical numbers are masked before reaching the AI provider.
    /// These are never treated as official statistical evidence.
    async fn wrong_question_patterns(&self, skills: &[String]) -> Vec<String> {
        let rows=sqlx::query(
            "SELECT f.deck_path, f.front_markdown, f.back_markdown FROM flashcards f \
             WHERE f.active=1 AND f.deck_path LIKE '资料/错题/%' \
             AND EXISTS (SELECT 1 FROM flashcard_card_tags t WHERE t.card_id=f.id AND t.tag='错题') \
             ORDER BY f.updated_at_ms DESC LIMIT 160"
        ).fetch_all(&self.pool).await;
        let Ok(rows) = rows else { return Vec::new() };
        let mut examples = rows
            .into_iter()
            .filter_map(|row| {
                let deck: String = row.try_get("deck_path").ok()?;
                let front: String = row.try_get("front_markdown").ok()?;
                let back: String = row.try_get("back_markdown").ok()?;
                let skill = deck.rsplit('/').next()?.to_owned();
                if front.chars().count() < 20 || front.starts_with("- 放弃") {
                    return None;
                }
                Some((
                    skill,
                    mask_wrong_question(&front, 180),
                    mask_wrong_question(&back, 120),
                ))
            })
            .collect::<Vec<_>>();
        examples.sort_by_key(|(skill, _, _)| {
            skills
                .iter()
                .position(|target| {
                    skill.contains(target.as_str()) || target.contains(skill.as_str())
                })
                .unwrap_or(usize::MAX)
        });
        // Round-robin across wrong-question skills: twenty identical base-period
        // questions would give the model less useful coverage than five skills.
        let mut by_skill = std::collections::BTreeMap::<String, Vec<(String, String)>>::new();
        for (skill, front, back) in examples {
            let bucket = by_skill.entry(skill).or_default();
            if !bucket.iter().any(|(existing, _)| existing == &front) {
                bucket.push((front, back));
            }
        }
        let mut priorities = by_skill.keys().cloned().collect::<Vec<_>>();
        priorities.sort_by_key(|skill| skills.iter().position(|target| {
            skill.contains(target) || target.contains(skill)
        }).unwrap_or(usize::MAX));
        let mut output = Vec::new();
        for depth in 0..5 {
            for skill in &priorities {
                if let Some((front, back)) = by_skill.get(skill).and_then(|items| items.get(depth)) {
                    output.push(format!(
                        "题型/{skill}；旧题问法（遮蔽数值）：{front}；原复盘提示（仅参考）：{back}"
                    ));
                    if output.len() == 20 { return output; }
                }
            }
        }
        output
    }
    async fn generate_group(
        &self,
        material: &Material,
        weaknesses: &[String],
    ) -> Result<QuestionGroup, String> {
        let config = self.config.as_ref().ok_or("Codex Token 未配置")?;
        let earlier = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM data_analysis_groups WHERE section=?",
        )
        .bind(&material.section)
        .fetch_one(&self.pool)
        .await
        .map_err(as_error)?;
        let material = generation_excerpt(material, earlier.max(0) as usize);
        let wrong_patterns = self.wrong_question_patterns(weaknesses).await;
        let source_year = OFFICIAL_SOURCES
            .iter()
            .find(|(_, url)| *url == material.source_url)
            .map(|(year, _)| *year)
            .unwrap_or(2025);
        let system = r#"你是一名公务员行测资料分析命题编辑。请基于提供的政府统计原文，改写成一份紧凑、可做5道题的“国考风格文字资料”，然后恰好生成5道数值计算题，每道题都应有独立的计算依据。必须返回单个严格JSON对象：
{"material_text":"改写后的正式材料全文","questions":[{"question":"包含明确统计年份与比较年份的国考资料分析题干","evidence":"原文中连续复制且包含a、b两个数字的原始证据，至少15字","operation":"base_period|growth_amount|growth_rate|average_growth_rate|share|average|difference|ratio","a":1234,"b":5.6,"unit":"万吨","explanation":"简短、可复核的速算提示"}]}
材料：请按统计口径提炼、适度调整顺序和句式、适当分段，保留必要数值、单位、年份；不要逐字照抄，也不能编造、推测或转换新增任何统计数值。材料建议350至650汉字（以资料充足为准，不为凑字数虚构事实），按考试常见材料自然分成2至4个相关段落，兼有真实多主体、近似指标、现期与同比等合理定位干扰；同一事实仍应集中表达，不可为了难度强行打散。不要用新闻宣传口吻。最多只出现给定年份和原文已有的数字。每道题使用的a、b必须在“改写材料”中可见，evidence必须是从官方原文逐字摘取的连续片段。
命题：优先采用下方20个真实错题的考点、复合问法和易混口径；尽量使5题中至少3题针对不同薄弱点，不允许退化成5道换主体的基期量/现期量入门题。新增 average_growth_rate 表示平均数增长率，a=总量同比增长率（百分数）、b=份数/人数同比增长率（百分数），结果=(a-b)/(1+b/100)，选项单位为%。禁止输出尚未支持运算规则的题型；实在缺乏可验证的来源操作数时宁可拒绝该材料，不能捏造高级题。每道题干必须明确使用官方统计年度（例如2022年末），涉及同比/上一年的比较时必须明确写出上一具体年度（例如2021年末），不得只写“年末”“比上年”“比去年”。语义务必和 operation 对齐：问“增加多少”必须为增长量而非基期量，问上年末存量才是基期量。只允许有唯一数值答案的四选一计算题；绝对不要生成综合判断、正误辨析、四个说法选正确或错误、能够推出的是等题型。每份正式材料至少覆盖3种不同的数值计算考点，如基期、增量、增长率、比重、倍数、平均数增长率、平均数、差额，围绕不同指标提问。题干优先“根据上述资料，……约为多少”“与上年相比……”“……占……的比重约为……”等公考式表述；不得使用“求”“按同比增长率估算”等课堂口吻；不要泄漏公式和答案。growth_amount、base_period的a=现期量、b=增长率百分数（下降是负数）；growth_rate的a=现期、b=基期；share的a=部分、b=总体；average/ratio a/b；difference a-b。不要混用统计口径、年份或量纲；不提供选项，选项由程序独立计算。不得引用没有出现在改写材料中的数字。"#;
        let user = format!(
            "官方{}年统计公报资料（唯一可用统计事实）：\n{}\n资料主题：{} → {}\n优先考点：{}\n【用户错题仅供参考命题问法、干扰项和复盘方法，不可作为数值来源】\n{}\n以用户错题的考点、易错口径和速算方法为出题主线，优先多步骤和综合审题，但操作必须在允许枚举中且所有数字来自官方来源。生成国考风格新材料及恰好5道数值计算变式题；忽略综合判断题型，不能照抄旧题。",
            source_year, material.text, material.topic, material.subtopic, weaknesses.join("、"), wrong_patterns.join("\n")
        );
        // One ephemeral remote thread per material. Retry the rejected batch as a
        // second turn in the SAME thread, supplying only validation reasons.
        let mut session = tokio::time::timeout(
            Duration::from_secs(12),
            CodexSession::connect(&config.token),
        )
        .await
        .map_err(|_| "连接 Codex App Server 超时".to_owned())??;
        let mut last_reason = String::new();
        for round in 0..2 {
            let prompt = if round == 0 {
                format!("{system}\n\n{user}\n\n只输出符合 JSON Schema 的完整对象，不调用工具或修改文件。")
            } else {
                format!(
                    "上次出题未通过 RK3568 的确定性验证：{last_reason}。请根据本轮对话给出的官方资料重新生成完整的一组5道数值计算题，修正全部验证错误，只返回 JSON；不得新增统计数据。"
                )
            };
            let content = tokio::time::timeout(TURN_TIMEOUT, session.turn(&prompt))
                .await
                .map_err(|_| "Codex 出题超时，请检查电脑服务与账号额度".to_owned())??;
            #[derive(Deserialize)]
            struct Batch {
                material_text: String,
                questions: Vec<QuestionRecipe>,
            }
            let batch: Batch = match serde_json::from_str(&content) {
                Ok(batch) => batch,
                Err(_) => {
                    last_reason = "Codex JSON 结构不符合题目格式".into();
                    continue;
                }
            };
            if batch.questions.len() != QUIZ_PER_GROUP {
                last_reason = format!(
                    "「{}」AI 返回 {} 题，要求每组恰好 {} 题",
                    material.section,
                    batch.questions.len(),
                    QUIZ_PER_GROUP
                );
                continue;
            }
            if !grounded_exam_material(&material.text, &batch.material_text, source_year) {
                last_reason = format!(
                    "「{}」改写材料有无来源的数值或不符合篇幅要求",
                    material.section
                );
                continue;
            }
            let mut shown_material = material.clone();
            shown_material.text = batch.material_text;
            let mut result = Vec::new();
            let mut seen = HashSet::new();
            let mut skill_types = HashSet::new();
            for candidate in &batch.questions {
                if !operand_visible(&shown_material.text, candidate.a)
                    || !operand_visible(&shown_material.text, candidate.b)
                    || !question_has_explicit_years(&candidate.question, source_year)
                {
                    continue;
                }
                if let Ok(q) = verify_recipe(&material, candidate, result.len()) {
                    let fingerprint =
                        format!("{:?}|{}|{}", candidate.operation, candidate.a, candidate.b);
                    if seen.insert(fingerprint) {
                        skill_types.insert(q.skill.clone());
                        result.push(q);
                    }
                }
                if result.len() == QUIZ_PER_GROUP {
                    break;
                }
            }
            if result.len() == QUIZ_PER_GROUP && skill_types.len() >= 3 {
                return Ok(QuestionGroup {
                    version: 3,
                    material: shown_material,
                    questions: result,
                });
            }
            last_reason = format!("「{}」只通过 {}/5 题校验", material.section, result.len());
        }
        Err(format!("{last_reason}；Codex 修复后仍未通过校验"))
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
        for group in &groups {
            self.mark_material_seen(&group.material, "comprehensive")
                .await?;
        }
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
        if groups.len() != QUIZ_GROUPS
            || groups.iter().any(|g| g.questions.len() != QUIZ_PER_GROUP)
            || answers.len() != QUIZ_GROUPS * QUIZ_PER_GROUP
        {
            return Err("训练题量错误，不能交卷".into());
        }
        let mut questions = Vec::with_capacity(QUIZ_GROUPS * QUIZ_PER_GROUP);
        for (group_index, group) in groups.iter().enumerate() {
            for (local_index, q) in group.questions.iter().enumerate() {
                let index = group_index * QUIZ_PER_GROUP + local_index;
                questions.push(GradedQuestion {
                    number: index + 1,
                    chosen: answers[index],
                    correct_index: q.correct_index,
                    answer: q.answer.clone(),
                    explanation: q.explanation.clone(),
                    topic: group.material.topic.clone(),
                    skill: q.skill.clone(),
                    evidence: q.evidence.clone(),
                });
            }
        }
        if status != "submitted" {
            let mut txn = self.pool.begin().await.map_err(as_error)?;
            sqlx::query("UPDATE data_analysis_runs SET status='submitted',ended_at_ms=? WHERE id=? AND status!='submitted'")
                .bind(now_ms()).bind(id).execute(&mut *txn).await.map_err(as_error)?;
            for q in &questions {
                sqlx::query("INSERT OR IGNORE INTO data_analysis_results(run_id,question_no,topic,skill,is_correct,elapsed_ms) VALUES(?,?,?,?,?,?)")
                    .bind(id).bind(q.number as i64).bind(&q.topic).bind(&q.skill)
                    .bind((q.chosen==Some(q.correct_index)) as i64).bind(0i64)
                    .execute(&mut *txn).await.map_err(as_error)?;
            }
            txn.commit().await.map_err(as_error)?;
        }
        let correct = questions
            .iter()
            .filter(|q| q.chosen == Some(q.correct_index))
            .count();
        Ok(RunResult {
            id: id.into(),
            correct,
            total: QUIZ_GROUPS * QUIZ_PER_GROUP,
            questions,
        })
    }
}

/// A rewritten material must never introduce a fresh quantitative datum.
/// The only new year label permitted is the known source publication year.
fn mask_wrong_question(value: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut number = false;
    for c in value.chars().take(limit) {
        if c.is_ascii_digit() || number && (c == '.' || c == '%' || c == ',') {
            if !number {
                out.push('□');
            }
            number = true;
        } else {
            number = false;
            if c == '\n' {
                out.push(' ')
            } else {
                out.push(c)
            }
        }
    }
    out
}
fn numeric_tokens(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || c == '.' && !word.is_empty() {
            word.push(c);
        } else if !word.is_empty() {
            if let Ok(v) = word.parse::<f64>() {
                if v.is_finite() {
                    out.push(v);
                }
            }
            word.clear();
        }
    }
    out
}
fn operand_visible(text: &str, value: f64) -> bool {
    let value = value.abs();
    numeric_tokens(text)
        .iter()
        .any(|v| (v - value).abs() < 0.0001)
}
fn question_has_explicit_years(question: &str, year: u16) -> bool {
    if !question.contains(&format!("{year}年")) {
        return false;
    }
    let compares_previous = ["上年", "去年", "同比", "增加", "减少", "增长", "增速", "增幅"]
        .iter().any(|term| question.contains(term));
    !compares_previous || question.contains(&format!("{}年", year.saturating_sub(1)))
}

fn grounded_exam_material(source: &str, rewritten: &str, year: u16) -> bool {
    let n = rewritten.chars().count();
    if !(100..=1800).contains(&n) || source.trim() == rewritten.trim() {
        return false;
    }
    let original = numeric_tokens(source);
    let rewritten_values = numeric_tokens(rewritten);
    if rewritten_values.len() < 4 {
        return false;
    }
    rewritten_values.iter().all(|v| {
        (*v - year as f64).abs() < 0.001 || original.iter().any(|known| (known - v).abs() < 0.0001)
    })
}

/// Keep complete per-chapter official text in SQLite, but pass one bounded
/// paragraph-coherent fragment to the AI and the iPad. Repeated generation can
/// rotate over different paragraphs without re-fetching the official source.
fn generation_excerpt(material: &Material, sequence: usize) -> Material {
    let mut chunks = Vec::<String>::new();
    let mut chunk = String::new();
    for paragraph in material.text.split("\n\n") {
        let chars = paragraph.chars().count();
        if chars == 0 {
            continue;
        }
        let candidate = if chars > MAX_GENERATION_CHARS {
            paragraph
                .chars()
                .take(MAX_GENERATION_CHARS)
                .collect::<String>()
        } else {
            paragraph.to_owned()
        };
        if !chunk.is_empty()
            && chunk.chars().count() + candidate.chars().count() + 2 > MAX_GENERATION_CHARS
        {
            chunks.push(std::mem::take(&mut chunk));
        }
        if !chunk.is_empty() {
            chunk.push_str("\n\n");
        }
        chunk.push_str(&candidate);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    let mut excerpt = material.clone();
    if !chunks.is_empty() {
        excerpt.text = chunks[sequence % chunks.len()].clone();
    }
    // Official section is the parent topic; fragment-specific topic is
    // independent of the arithmetic skill required to solve the questions.
    let text = excerpt.text.as_str();
    let rules: &[(&str, &str, &[&str])] = &[
        ("宏观经济", "人口与就业", &["人口", "就业", "失业率"]),
        ("宏观经济", "居民价格", &["居民消费价格", "生产者价格"]),
        ("宏观经济", "区域经济", &["东部地区", "中部地区", "长三角"]),
        ("宏观经济", "绿色低碳", &["二氧化碳排放", "清洁能源"]),
        ("宏观经济", "产业结构与GDP", &["国内生产总值", "第一产业"]),
        ("农业", "粮食生产", &["粮食产量", "粮食播种"]),
        ("农业", "畜牧业", &["猪肉产量", "羊肉产量", "牛奶产量"]),
        ("农业", "渔业", &["水产品", "养殖产量"]),
        ("农业", "经济作物", &["棉花", "茶叶", "油料"]),
        ("工业能源", "工业增加值", &["工业增加值", "制造业"]),
        ("工业能源", "能源电力", &["发电", "煤炭", "原油"]),
        ("工业能源", "建筑业", &["建筑业"]),
        ("交通物流与服务", "交通运输", &["旅客运输", "货物运输"]),
        ("交通物流与服务", "邮政快递", &["邮政", "快递"]),
        ("交通物流与服务", "信息通信", &["电信", "互联网", "通信"]),
        ("国内消费", "社会消费品零售", &["社会消费品零售"]),
        ("国内消费", "网上零售", &["网上零售"]),
        ("投资房地产", "房地产开发", &["房地产开发", "商品房"]),
        ("投资房地产", "固定资产投资", &["固定资产投资"]),
        ("对外经济", "货物进出口", &["进口", "出口"]),
        ("对外经济", "外商投资", &["外商投资", "实际使用外资"]),
        ("财政金融", "财政收支", &["财政收入", "财政支出"]),
        ("财政金融", "货币信贷", &["货币供应", "贷款"]),
        ("居民民生", "居民收入", &["居民人均可支配收入", "居民收入"]),
        ("居民民生", "社会保障", &["社会保险", "参保"]),
        ("科技教育", "科研创新", &["专利", "研究与试验发展"]),
        ("科技教育", "教育", &["在校生", "招生"]),
        ("文旅卫生体育", "旅游", &["旅游", "游客"]),
        ("文旅卫生体育", "卫生医疗", &["卫生", "医疗"]),
        ("资源环境安全", "能源消费", &["能源消费"]),
        (
            "资源环境安全",
            "生态环境",
            &["空气质量", "水质", "森林覆盖率"],
        ),
        ("资源环境安全", "安全生产", &["安全生产", "事故"]),
    ];
    if let Some((_, subtopic, _)) = rules.iter().find(|(topic, _, words)| {
        *topic == excerpt.topic && words.iter().any(|word| text.contains(word))
    }) {
        excerpt.subtopic = (*subtopic).to_owned();
    }
    excerpt
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
    parse_official_sections_from(html, SOURCE_URL)
}
fn parse_official_sections_from(html: &str, source_url: &str) -> Vec<Material> {
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
            if raw[i].iter().map(|x| x.chars().count()).sum::<usize>() < MAX_CACHED_SECTION_CHARS {
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
                source_url: source_url.into(),
            });
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn env_accepts_non_hex_token_without_a_fixed_length() {
        let value = "short-token-12";
        let config = parse_env(&format!("HYZ_THINGS_CODEX_WS_TOKEN={value}"))
            .unwrap()
            .unwrap();
        assert_eq!(config.token, value);
    }

    #[test]
    fn token_is_the_only_supported_model_credential() {
        let sample = format!(
            "HYZ_THINGS_AI_BASE_URL=https://api.deepseek.com\nHYZ_THINGS_AI_API_KEY=legacy\nHYZ_THINGS_AI_MODEL=legacy\nHYZ_THINGS_CODEX_WS_TOKEN={}\n",
            "a".repeat(64)
        );
        let config = parse_env(&sample).unwrap().unwrap();
        assert_eq!(config.token.len(), 64);
        assert!(parse_env("HYZ_THINGS_AI_BASE_URL=https://api.deepseek.com")
            .unwrap()
            .is_none());
        assert!(parse_env("HYZ_THINGS_CODEX_WS_TOKEN=bad token").is_err());
        assert!(parse_env(&format!(
            "HYZ_THINGS_CODEX_WS_TOKEN={0}\nHYZ_THINGS_CODEX_WS_TOKEN={0}",
            "a".repeat(64)
        ))
        .is_err());
    }
    #[tokio::test]
    async fn parallel_wave_runs_four_sessions_and_keeps_source_slots() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::Barrier;

        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(QUIZ_GROUPS + 1));
        let candidates = (0..QUIZ_GROUPS)
            .map(|slot| {
                (
                    slot,
                    Material {
                        section: format!("section-{slot}"),
                        topic: format!("topic-{slot}"),
                        subtopic: "test".into(),
                        text: "test".into(),
                        source_url: format!("year-{slot}"),
                    },
                )
            })
            .collect();
        let task = tokio::spawn(run_generation_wave(candidates, {
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            let barrier = Arc::clone(&barrier);
            move |material| {
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                let barrier = Arc::clone(&barrier);
                async move {
                    let running = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(running, Ordering::SeqCst);
                    barrier.wait().await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    if material.section == "section-2" {
                        Err("simulate invalid Codex response".into())
                    } else {
                        Ok(QuestionGroup {
                            version: 2,
                            material,
                            questions: Vec::new(),
                        })
                    }
                }
            }
        }));
        tokio::time::timeout(Duration::from_secs(4), barrier.wait())
            .await
            .expect("all four independent workers should reach the same barrier");
        let mut outcomes = tokio::time::timeout(Duration::from_secs(4), task)
            .await
            .unwrap()
            .unwrap();
        outcomes.sort_by_key(|(slot, _)| *slot);
        assert_eq!(peak.load(Ordering::SeqCst), QUIZ_GROUPS);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(outcomes.len(), QUIZ_GROUPS);
        for (slot, item) in outcomes {
            if slot == 2 {
                assert!(item.is_err());
            } else {
                assert_eq!(item.unwrap().material.section, format!("section-{slot}"));
            }
        }
    }

    #[tokio::test]
    async fn cached_official_groups_support_full_twenty_question_lifecycle() {
        // Exercise native SQLite/service paths without a paid AI API or external network.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("temporary in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");

        let materials = SECTIONS
            .iter()
            .take(4)
            .map(|(section, topic)| Material {
                section: (*section).into(),
                topic: (*topic).into(),
                subtopic: "测试子主题".into(),
                text: "本年度业务量1200亿元，比上年增长15.2%。".repeat(8),
                source_url: SOURCE_URL.into(),
            })
            .collect::<Vec<_>>();

        sqlx::query("INSERT INTO data_analysis_sources(url,title,content_hash,fetched_at_ms,sections_json) VALUES (?,?,?,?,?)")
            .bind(SOURCE_URL).bind("fixture official cache").bind("offline hash")
            .bind(now_ms())
            .bind(serde_json::to_string(&materials).unwrap())
            .execute(&pool).await.unwrap();

        for material in &materials {
            let questions = (0..5)
                .map(|idx| VerifiedQuestion {
                    question: format!("第{}题：上一年业务量约为多少？", idx + 1),
                    options: [
                        "1000亿元".into(),
                        "1042亿元".into(),
                        "1200亿元".into(),
                        "1400亿元".into(),
                    ],
                    correct_index: 1,
                    answer: "1042亿元".into(),
                    explanation: "用现期除以1.152".into(),
                    skill: "基期量".into(),
                    evidence: "本年度业务量1200亿元，比上年增长15.2%。".into(),
                })
                .collect::<Vec<_>>();
            let group = QuestionGroup {
                version: 2,
                material: material.clone(),
                questions,
            };
            sqlx::query("INSERT INTO data_analysis_groups(id,source_url,section,topic,group_json,created_at_ms) VALUES(?,?,?,?,?,?)")
                .bind(Uuid::now_v7().to_string())
                .bind(SOURCE_URL)
                .bind(&material.section)
                .bind(&material.topic)
                .bind(serde_json::to_string(&group).unwrap())
                .bind(now_ms())
                .execute(&pool).await.unwrap();
        }

        let config = parse_env(&format!("HYZ_THINGS_CODEX_WS_TOKEN={}", "a".repeat(64)))
            .unwrap()
            .unwrap();
        let service = DataAnalysisService::new(pool, Some(config)).unwrap();
        let id = service.prepare().await.expect("start generation");
        let mut run_id = None;
        for _ in 0..100 {
            if let Some(progress) = service.job(&id).await {
                if progress.state == "ready" {
                    run_id = progress.run_id;
                    break;
                }
                assert_ne!(progress.state, "failed", "{}", progress.message);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let run_id = run_id.expect("cached groups should be ready without network");
        let active = service.run(&run_id).await.expect("start 20-minute timer");
        assert_eq!(active.groups.len(), 4);
        assert!(active.groups.iter().all(|group| group.questions.len() == 5));
        assert_eq!(active.duration_seconds, 1200);
        assert!(active.deadline_at_ms.unwrap() > active.started_at_ms.unwrap());
        assert_eq!(active.answers.len(), 20);
        service.answer(&run_id, 1, 1).await.expect("save an answer");
        let refreshed = service.run(&run_id).await.expect("restore an active run");
        assert_eq!(refreshed.answers[0], Some(1));
        assert_eq!(refreshed.started_at_ms, active.started_at_ms);
        let result = service.submit(&run_id).await.expect("submit");
        assert_eq!(result.correct, 1);
        assert_eq!(result.total, 20);
        assert_eq!(result.questions.len(), 20);
        assert!(
            service.answer(&run_id, 2, 1).await.is_err(),
            "submitted run must reject answers"
        );
        let replay = service.submit(&run_id).await.expect("idempotent result");
        assert_eq!(replay.correct, 1);
        let (source_count, group_count) = service.counts().await.unwrap();
        assert_eq!((source_count, group_count), (1, 4));
    }

    #[test]
    fn wrong_question_templates_mask_original_numbers() {
        let masked = mask_wrong_question(
            "2023年末，粮食产量71488万吨，同比增长1.2%。能够从资料中推出的是",
            200,
        );
        assert!(!masked.contains("71488"));
        assert!(!masked.contains("2023"));
        assert!(masked.contains("能够从资料中推出的是"));
    }
    #[test]
    fn complete_official_text_is_cached_while_generation_is_bounded() {
        let first = "全年粮食产量71488万吨，比上年增加838万吨。";
        let second = "年末水产品总产量7657万吨，同比增长4.1%。";
        let html = format!(
            "<article><p>二、农业</p><p>{}</p><p>{}</p></article>",
            first.repeat(120),
            second.repeat(120)
        );
        let sections = parse_official_sections(&html);
        assert_eq!(sections.len(), 1);
        assert!(sections[0].text.chars().count() > 1600);
        let excerpt = generation_excerpt(&sections[0], 1);
        assert!(excerpt.text.chars().count() <= MAX_GENERATION_CHARS);
        assert_eq!(excerpt.section, sections[0].section);
        assert_eq!(excerpt.topic, sections[0].topic);
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
    #[test]
    fn year_specific_question_rejects_generic_year_end() {
        assert!(!question_has_explicit_years("年末规模比上年末增加多少？",2022));
        assert!(!question_has_explicit_years("2022年末规模比上年末增加多少？",2022));
        assert!(question_has_explicit_years("2022年末规模比2021年末增加多少？",2022));
    }

}
