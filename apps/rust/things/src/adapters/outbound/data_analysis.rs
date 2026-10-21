//! Native-only official-text caching and Codex App Server question preparation.
//! Codex owns the answer; the application stores each freshly prepared practice attempt.
use super::codex_app_server::{validate_token, CodexSession, TURN_TIMEOUT};
use crate::domain::data_analysis::{
    select_material_batch, Material, VerifiedQuestion, OFFICIAL_SOURCES, QUIZ_GROUPS,
    QUIZ_PER_GROUP, QUIZ_SECONDS, SECTIONS, SOURCE_URL,
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
    cwd: String,
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

fn valid_remote_repo_path(value: &str) -> bool {
    // The path is sent to the PC, and must NOT be resolved on RK3568.
    // Never accept shell expansions, parent traversal or browser-selected paths.
    let path = std::path::Path::new(value);
    !value.is_empty()
        && value.len() <= 512
        && path.is_absolute()
        && path.components().all(|part| {
            matches!(
                part,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
        && !value.chars().any(|c| c.is_control())
}

fn parse_env(input: &str) -> Result<Option<CodexConfig>, &'static str> {
    let mut token = None;
    let mut cwd = None;
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("Invalid .env entry")?;
        let mut value = value.trim();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = &value[1..value.len() - 1];
        }
        match key.trim() {
            "HYZ_THINGS_CODEX_WS_TOKEN" => {
                if token.is_some() {
                    return Err("Duplicate Codex Token");
                }
                validate_token(value).map_err(|_| "Codex WS Token has invalid characters")?;
                token = Some(value.to_owned());
            }
            "HYZ_THINGS_CODEX_CWD" => {
                if cwd.is_some() {
                    return Err("Duplicate Codex CWD");
                }
                if !valid_remote_repo_path(value) {
                    return Err("Codex CWD must be a safe absolute path on the PC");
                }
                cwd = Some(value.to_owned());
            }
            _ => {}
        }
    }
    match (token, cwd) {
        (Some(token), Some(cwd)) => Ok(Some(CodexConfig { token, cwd })),
        (None, None) => Ok(None),
        _ => Err("Codex needs both HYZ_THINGS_CODEX_WS_TOKEN and HYZ_THINGS_CODEX_CWD"),
    }
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
    pub paused_remaining_ms: Option<i64>,
    pub duration_seconds: u64,
    pub groups: Vec<PublicGroup>,
    pub answers: Vec<Option<usize>>,
}
#[derive(Clone, Serialize)]
pub struct GradedQuestion {
    pub number: usize,
    pub question: String,
    pub options: [String; 4],
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
    pub materials: Vec<Material>,
}

#[derive(Clone, Serialize)]
pub struct ComprehensiveHistoryEntry {
    pub id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub elapsed_ms: i64,
    pub correct: usize,
    pub total: usize,
    pub topics: Vec<String>,
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
    F: Fn(Material, usize) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<QuestionGroup, String>> + Send + 'static,
{
    debug_assert!(candidates.len() <= QUIZ_GROUPS);
    let mut running = JoinSet::new();
    for (index, material) in candidates {
        let produce = producer.clone();
        running.spawn(async move { (index, produce(material, index).await) });
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
    /// Use the user's exact wrong-question directory names as the canonical
    /// training taxonomy, not the model's arbitrary formula labels or tags.
    pub async fn weaknesses(&self) -> Vec<String> {
        let rows = match sqlx::query(
            "SELECT f.deck_path, COUNT(*) AS cards, COALESCE(SUM(rs.lapse_count),0) AS lapses
             FROM flashcards f LEFT JOIN flashcard_review_state rs ON rs.card_id=f.id
             WHERE f.active=1 GROUP BY f.deck_path",
        ).fetch_all(&self.pool).await {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        let mut weights = std::collections::BTreeMap::<String, i64>::new();
        for row in rows {
            let Ok(deck) = row.try_get::<String,_>("deck_path") else {continue};
            let Some(category)=wrong_question_category(&deck) else {continue};
            let cards = row.try_get::<i64,_>("cards").unwrap_or(0);
            let lapses = row.try_get::<i64,_>("lapses").unwrap_or(0);
            *weights.entry(category.to_owned()).or_default() += cards + lapses * 3;
        }
        // Recent exercises supplement the wrong-card counts only when the
        // model returned the exact directory category, not a guessed synonym.
        if let Ok(rows)=sqlx::query(
            "SELECT skill, SUM(CASE WHEN is_correct=0 THEN 1 ELSE 0 END) AS misses
             FROM data_analysis_results GROUP BY skill"
        ).fetch_all(&self.pool).await {
            for row in rows {
                let Ok(skill)=row.try_get::<String,_>("skill") else {continue};
                if let Some(weight)=weights.get_mut(&skill) {
                    *weight += row.try_get::<i64,_>("misses").unwrap_or(0) * 8;
                }
            }
        }
        // Newly added folders are selectable before the SQLite card sync.
        if let Ok(available)=self.available_skills().await {
            let set=available.iter().cloned().collect::<HashSet<_>>();
            weights.retain(|category,_|set.contains(category));
            for category in available {weights.entry(category).or_insert(1);}
        }
        let mut ranked=weights.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|a,b|b.1.cmp(&a.1).then_with(||a.0.cmp(&b.0)));
        ranked.into_iter().take(4).map(|(category,_)|category).collect()
    }

    /// Derive categories from "错题/<分类>" whether RK3568 mounted
    /// flashcards/, flashcards/资料/, or the enclosing repository.
    pub async fn available_skills(&self) -> Result<Vec<String>, String> {
        // Prefer the live mounted folder tree to the previously imported cards.
        if let Some(live)=mounted_wrong_question_categories().await {
            return Ok(live);
        }
        let rows = sqlx::query(
            "SELECT DISTINCT deck_path FROM flashcards WHERE active=1",
        ).fetch_all(&self.pool).await.map_err(as_error)?;
        let mut categories=std::collections::BTreeSet::new();
        for row in rows {
            let deck:String=row.try_get("deck_path").map_err(as_error)?;
            if let Some(name)=wrong_question_category(&deck) {
                categories.insert(name.to_owned());
            }
        }
        Ok(categories.into_iter().collect())
    }

    pub async fn chosen_manual_skills(&self, requested: &[String]) -> Result<Vec<String>, String> {
        if requested.len() > 12 {
            return Err("每次最多勾选12个考点".into());
        }
        let available = self.available_skills().await?;
        let mut chosen = Vec::new();
        for skill in requested {
            if !available.contains(skill) || chosen.contains(skill) {
                return Err("所选考点不在当前资料分析卡片目录中".into());
            }
            chosen.push(skill.clone());
        }
        Ok(chosen)
    }

    /// Current running comprehensive preparation, including progress across page changes.
    pub async fn preparing_job(&self) -> Option<String> {
        self.jobs
            .lock()
            .await
            .iter()
            .find(|(_, progress)| !matches!(progress.state.as_str(), "ready" | "failed"))
            .map(|(id, _)| id.clone())
    }

    pub async fn prepare(&self, manual_skills: Vec<String>) -> Result<String, String> {
        if self.config.is_none() {
            return Err(
                "请在 RK3568 的 /userdata/hyz-things/.env 配置 Codex WebSocket Token".into(),
            );
        }
        let manual_skills = self.chosen_manual_skills(&manual_skills).await?;
        if let Some(id) = self.preparing_job().await {
            return Ok(id);
        }
        // Reserve the exclusive AI slot *before* publishing a job ID. A second
        // click or another tab must not queue an unintended second paper.
        let guard = self
            .gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| "已有其他出题任务正在执行，请等当前任务完成".to_owned())?;
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
            let result = worker.generate(&job, &manual_skills).await;
            drop(guard);
            match result {
                Ok(run_id) => {
                    worker
                        .set_progress(
                            &job,
                            "ready",
                            20,
                            "Codex 已交付20道题，可以开始训练",
                            Some(run_id),
                        )
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
    async fn generate(&self, job: &str, manual_skills: &[String]) -> Result<String, String> {
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
        // A simulated paper is new even when its theme uses a previously read official chapter.
        ordered.sort_by(|a, b| {
            let (na, ma) = scores.get(&a.topic).copied().unwrap_or((0, 0));
            let (nb, mb) = scores.get(&b.topic).copied().unwrap_or((0, 0));
            let score_a = if na == 0 { 2.0 } else { ma as f64 / na as f64 };
            let score_b = if nb == 0 { 2.0 } else { mb as f64 / nb as f64 };
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Deliberately never reuse a prior generated group. Saved groups are
        // historical evidence for review/locating, not a source of fresh papers.
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
                let mut requests = Vec::new();
                for (slot, material) in wave.iter().enumerate() {
                    attempted.insert((material.source_url.clone(), material.section.clone()));
                    if attempts < 12 {
                        attempts += 1;
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
                        "最多4个 Codex Thread 正在按题型规划生成模拟题",
                        None,
                    )
                    .await;
                    let worker = self.clone();
                    let weak = Arc::new(weaknesses.clone());
                    let manual = Arc::new(manual_skills.to_vec());
                    let group_offset = ready.len();
                    for (slot, outcome) in
                        run_generation_wave(requests, move |material, ordinal| {
                            let worker = worker.clone();
                            let weak = Arc::clone(&weak);
                            let manual = Arc::clone(&manual);
                            async move {
                                worker
                                    .generate_group(
                                        &material,
                                        &weak,
                                        &manual,
                                        group_offset + ordinal,
                                    )
                                    .await
                            }
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
                for group in ordered_results.into_iter().flatten() {
                    {
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
                        "generating",
                        ready.len() * QUIZ_PER_GROUP,
                        "已接收 Codex 完成的模拟试题并保存",
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
                "已获得{}/20题，无法组成整卷：{}",
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
    /// Codex reads the current hyz Skill/Flashcards from the PC repository.
    /// Its final answers/options are accepted as delivered; the server only
    /// decodes the contract the UI needs and never recomputes the solution.
    async fn generate_group(
        &self,
        source: &Material,
        weaknesses: &[String],
        manual_skills: &[String],
        ordinal: usize,
    ) -> Result<QuestionGroup, String> {
        let config = self.config.as_ref().ok_or("Codex Token 未配置")?;
        let earlier = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM data_analysis_groups WHERE section=?",
        )
        .bind(&source.section)
        .fetch_one(&self.pool)
        .await
        .map_err(as_error)?;
        let context = generation_excerpt(source, earlier.max(0) as usize);
        let source_year = OFFICIAL_SOURCES
            .iter()
            .find(|(_, url)| *url == context.source_url)
            .map(|(year, _)| *year)
            .unwrap_or(2025);
        let prompt = format!(
            r#"你是 hyz 的公务员行测资料分析综合训练命题 Agent。当前任务是**题型驱动**，不是材料驱动。
工作目录是 hyz-things 仓库。请先使用 hyz-xingce Skill（.agents/skills/行测做题/SKILL.md），
按其中资料分析导航读取 references/资料分析.md，题型分类以 flashcards/资料/错题/ 的直接子目录名为准。
对于本次自动推荐和手动选中的每个分类，进入 flashcards/资料/错题/<分类目录>/ 读取原始 cards.md 或 card.md 的命题变式、hyz 速算动作与易错点。
每题 skill 使用对应的错题目录分类名；细分命题变式写进解析，不自行发明另一套基础公式标签。
每次都是新建 Thread，必须基于这次工作树中的最新内容工作；不需要用户在提示词里复制卡片。
本次是整套4篇×5题的第 {group_no} 篇；全卷软目标为12道薄弱点、6道其他核心考点、2道基础题，
各篇尽量覆盖不同题型与命题变式，优先兼顾个人薄弱考点：{weaknesses}。
用户额外手选考点：{manual_all}。本篇必须至少有一道题命中每个分配到本篇的手选考点：{manual_focus}。
手动考点优先于系统推荐考点，但不因此忽略题型多样性；不要把标签当材料标题。
先选择本篇5题的题型、命题变式与考查的 hyz 速算/审题动作，再构造共享且自洽的模拟统计数据，
最后写成约350–650汉字、2～4个自然段的国考风格材料，并拟制5道有唯一正确答案的四选一题。
整篇只围绕同一统计大主题；按总体概况、细分主体、相关指标或年份口径等小主题组织不同段落，每段有清晰的主体、指标及时间口径。
不同段落可设置合理相似指标、同比与现期的检索干扰，但不可把同一事实人为拆得不合常理。
material_text 必须使用两个换行符分隔自然段（\n\n，JSON中正确转义），不要写成无分段的数字清单。
四篇材料尽量使用不同大主题，5道题可跨段落定位，不必按材料段落顺序排列。
应覆盖真实错题中的乘积增长率（乘/除/倒推/间隔）、混合增长、平均数增长率/增量、两期比重差、
年均变化、增长贡献/拉动、分数比较、基期量与增长量错配等，可按适合本篇的5种变式灵活选择；
不要机械地大量重复简单基期量、增长量或现期量。
此篇背景：{year}年，官方参考章节「{section}」，主题「{topic}／{subtopic}」。
官方公报摘录仅供了解主体、指标、统计口径和写作语气，不是数值限制，也不是模拟数字的官方证明：
{excerpt}
模拟数据必须有真实统计公报的数值精度与自然分布：不要为方便心算而大量使用整百、整千、整万和连续零结尾的“漂亮数字”（如50000元、20000元、70000万吨、4200万公顷）。
例如，年度总量可写382738亿元，居民收入可写38273元，人口可写936.8万人，增速可写3.7%或-1.2%；这些只是精度示例，不要照抄或把各指标一律放大。
根据指标与单位采用自然精度：亿元、万吨、元等统计量通常可以是多位非整百的整数；万亿元、万人及比率按公报口径合理保留小数；不要滥用无意义的多位小数。
同一篇材料大多数总量与分项应有非整百尾数，仍允许少量自然整值。增长量、分项合计、同比和基期由同一套数值关系推导，并按展示精度协调四舍五入；不能为制造整数答案篡改数字。
题目要符合 hyz 选项驱动的近似速算：数据自然但量级合理，选项可区分，不故意制造繁琐的纯精算。
允许合理构造所有训练数值；模拟材料必须清楚标注为“模拟训练数据”，
不得声称模拟数字是国家统计局真实公布的数值。年份、主体、指标和单位要明确，
基期、现期、增长量与增速、部分/总体、混合权重之间必须自洽。
题干涉及同比时明确写具体比较年份（如“2024年比2023年”），不能只说“年末”“去年”。
根据考生能在最终材料中实际看到的数字判断正确项；四项的区分度应适合 hyz 速算方法。
对每道题返回：question题干；options恰好4个不带ABCD标记的选项字符串；
correct_index为0至3的正确项下标；answer为该正确选项的完整内容；
explanation为依照 hyz 方法的简短速算解释；skill为具体考点；evidence为模拟材料中支持题目的连续文字片段。
先自行核对数学及选项，再仅输出符合结构化 Schema 的JSON，不输出计划、过程或代码块。
输出 JSON 必须具有 material_text 和 questions 两字段，questions 恰好5项。"#,
            group_no = ordinal % QUIZ_GROUPS + 1,
            weaknesses = weaknesses.join("、"),
            manual_all = manual_skills.join("、"),
            manual_focus = manual_skills
                .iter()
                .enumerate()
                .filter(|(index, _)| index % QUIZ_GROUPS == ordinal % QUIZ_GROUPS)
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join("、"),
            year = source_year,
            section = context.section,
            topic = context.topic,
            subtopic = context.subtopic,
            excerpt = context.text
        );
        let mut session = tokio::time::timeout(
            Duration::from_secs(12),
            CodexSession::connect(&config.token, &config.cwd),
        )
        .await
        .map_err(|_| "连接 Codex App Server 超时".to_owned())??;
        let output = tokio::time::timeout(TURN_TIMEOUT, session.turn(&prompt))
            .await
            .map_err(|_| "Codex 出题超时，请检查电脑服务与账号额度".to_owned())??;
        #[derive(Deserialize)]
        struct Batch {
            material_text: String,
            questions: Vec<VerifiedQuestion>,
        }
        let batch: Batch =
            serde_json::from_str(&output).map_err(|_| "Codex 返回的试卷 JSON 与展示格式不匹配")?;
        // Structural safety only: protect fixed UI indexing. No math/content
        // verification and no answer rewriting by hyz-things.
        if batch.questions.len() != QUIZ_PER_GROUP
            || batch.questions.iter().any(|q| q.correct_index >= 4)
        {
            return Err("Codex 返回的试卷结构不完整（需要5题，每题4个选项）".into());
        }
        let mut material = context;
        material.text = format!("【模拟训练数据，非官方发布数值】\n{}", batch.material_text);
        Ok(QuestionGroup {
            version: 4,
            material,
            questions: batch.questions,
        })
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
        let timing =
            sqlx::query("SELECT paused_at_ms,paused_total_ms FROM data_analysis_runs WHERE id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(as_error)?;
        let paused_at: Option<i64> = timing.try_get("paused_at_ms").map_err(as_error)?;
        let paused_total: i64 = timing.try_get("paused_total_ms").map_err(as_error)?;
        let deadline = started_at_ms.map(|ms| ms + QUIZ_SECONDS as i64 * 1000 + paused_total);
        let paused_remaining_ms = paused_at
            .zip(deadline)
            .map(|(since, until)| (until - since).max(0));
        Ok(RunView {
            id: id.into(),
            status,
            started_at_ms,
            deadline_at_ms: if paused_at.is_some() { None } else { deadline },
            paused_remaining_ms,
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
        let timing =
            sqlx::query("SELECT paused_at_ms,paused_total_ms FROM data_analysis_runs WHERE id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(as_error)?;
        let paused_at: Option<i64> = timing.try_get("paused_at_ms").map_err(as_error)?;
        let paused_total: i64 = timing.try_get("paused_total_ms").map_err(as_error)?;
        if status != "active"
            || paused_at.is_some()
            || started.is_none_or(|ms| now_ms() >= ms + QUIZ_SECONDS as i64 * 1000 + paused_total)
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
                    question: q.question.clone(),
                    options: q.options.clone(),
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
            materials: groups.into_iter().map(|g| g.material).collect(),
        })
    }

    pub async fn pause_toggle(&self, id: &str) -> Result<RunView, String> {
        let row=sqlx::query("SELECT status,started_at_ms,paused_at_ms,paused_total_ms FROM data_analysis_runs WHERE id=?")
            .bind(id).fetch_optional(&self.pool).await.map_err(as_error)?.ok_or("训练不存在")?;
        let status: String = row.try_get("status").map_err(as_error)?;
        let started: Option<i64> = row.try_get("started_at_ms").map_err(as_error)?;
        let paused_at: Option<i64> = row.try_get("paused_at_ms").map_err(as_error)?;
        let paused_total: i64 = row.try_get("paused_total_ms").map_err(as_error)?;
        if status != "active" {
            return Err("只有进行中的训练可以暂停".into());
        }
        let now = now_ms();
        let result = if let Some(paused_at) = paused_at {
            sqlx::query("UPDATE data_analysis_runs SET paused_at_ms=NULL,paused_total_ms=paused_total_ms+? WHERE id=? AND status='active' AND paused_at_ms=?")
                .bind((now-paused_at).max(0)).bind(id).bind(paused_at)
                .execute(&self.pool).await.map_err(as_error)?
        } else {
            if started.is_none_or(|ms| now >= ms + QUIZ_SECONDS as i64 * 1000 + paused_total) {
                return Err("训练计时已结束，不能暂停".into());
            }
            sqlx::query("UPDATE data_analysis_runs SET paused_at_ms=? WHERE id=? AND status='active' AND paused_at_ms IS NULL")
                .bind(now).bind(id).execute(&self.pool).await.map_err(as_error)?
        };
        if result.rows_affected() != 1 {
            return Err("计时状态已变化，请刷新".into());
        }
        self.run(id).await
    }

    pub async fn result(&self, id: &str) -> Result<RunResult, String> {
        let (_groups, _answers, status, _started) = self.fetch_row(id).await?;
        if status != "submitted" {
            return Err("训练尚未交卷".into());
        }
        self.submit(id).await
    }

    pub async fn history(&self) -> Result<Vec<ComprehensiveHistoryEntry>, String> {
        let rows=sqlx::query(
            "SELECT id,groups_json,answers_json,started_at_ms,ended_at_ms,paused_total_ms FROM data_analysis_runs WHERE status='submitted' AND ended_at_ms IS NOT NULL ORDER BY ended_at_ms DESC LIMIT 100"
        ).fetch_all(&self.pool).await.map_err(as_error)?;
        let mut result = Vec::new();
        for row in rows {
            let groups: Vec<QuestionGroup> =
                serde_json::from_str(&row.try_get::<String, _>("groups_json").map_err(as_error)?)
                    .map_err(as_error)?;
            let answers: Vec<Option<usize>> =
                serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                    .map_err(as_error)?;
            let total: usize = groups.iter().map(|g| g.questions.len()).sum();
            if answers.len() != total {
                continue;
            }
            let correct = groups
                .iter()
                .flat_map(|g| g.questions.iter())
                .zip(answers.iter())
                .filter(|(q, a)| **a == Some(q.correct_index))
                .count();
            let started: i64 = row
                .try_get::<Option<i64>, _>("started_at_ms")
                .map_err(as_error)?
                .unwrap_or(0);
            let ended: i64 = row.try_get("ended_at_ms").map_err(as_error)?;
            let paused_total: i64 = row.try_get("paused_total_ms").map_err(as_error)?;
            result.push(ComprehensiveHistoryEntry {
                id: row.try_get("id").map_err(as_error)?,
                started_at_ms: started,
                ended_at_ms: ended,
                elapsed_ms: (ended - started - paused_total).max(0),
                correct,
                total,
                topics: groups.iter().map(|g| g.material.subtopic.clone()).collect(),
            });
        }
        Ok(result)
    }
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
/// Look at the directory currently mounted into RK3568 rather than
/// requiring a card sync just to show a newly created folder name.
async fn mounted_wrong_question_categories() -> Option<Vec<String>> {
    let root=super::flashcard_source::FLASHCARD_SOURCE_ROOT;
    for relative in ["资料/错题","错题","flashcards/资料/错题"] {
        let path=std::path::Path::new(root).join(relative);
        let Ok(mut reader)=tokio::fs::read_dir(path).await else {continue};
        let mut result=std::collections::BTreeSet::<String>::new();
        while let Ok(Some(entry))=reader.next_entry().await {
            let name=entry.file_name().to_string_lossy().into_owned();
            if valid_wrong_category(&name)
                && entry.file_type().await.is_ok_and(|kind|kind.is_dir()) {
                result.insert(name);
            }
        }
        return Some(result.into_iter().collect());
    }
    None
}

fn valid_wrong_category(category:&str)->bool {
    let count=category.chars().count();
    count>0 && count<=60 && category!="." && category!=".."
        && !category.chars().any(char::is_control)
}

/// The directory immediately beneath flashcards/资料/错题/ is the skill.
/// The flashcard import root can point at flashcards/, 资料/ or 错题's parent.
fn wrong_question_category(deck: &str) -> Option<&str> {
    let parts=deck.split('/').filter(|part|!part.is_empty()).collect::<Vec<_>>();
    let index=parts.iter().position(|part|*part=="错题")?;
    let prefix=&parts[..index];
    if prefix!=[] && prefix!=["资料"] && prefix!=["flashcards","资料"] {
        return None;
    }
    let category=*parts.get(index+1)?;
    if !valid_wrong_category(category) {
        return None;
    }
    Some(category)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_exact_wrong_question_subdirectories_as_skills(){
        assert_eq!(wrong_question_category("资料/错题/乘积增长率"),Some("乘积增长率"));
        assert_eq!(wrong_question_category("错题/混合增长率"),Some("混合增长率"));
        assert_eq!(wrong_question_category("flashcards/资料/错题/综合分析与审题/小结"),Some("综合分析与审题"));
        assert_eq!(wrong_question_category("资料/错题/平均数增量"),Some("平均数增量"));
        assert_eq!(wrong_question_category("资料/错题"),None);
        assert_eq!(wrong_question_category("资料/线段混合"),None);
        assert_eq!(wrong_question_category("其他/错题/乘积增长率"),None);
    }

    #[test]
    fn env_accepts_non_hex_token_without_a_fixed_length() {
        let value = "short-token-12";
        let config = parse_env(&format!(
            "HYZ_THINGS_CODEX_WS_TOKEN={value}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(config.token, value);
    }

    #[test]
    fn final_codex_answers_deserialize_without_backend_recalculation() {
        // Deliberately mathematically arbitrary. The app consumes the model's
        // selected option rather than a Rust formula/answer builder.
        let item = serde_json::json!({
            "question":"2024年比2023年增长约多少？",
            "options":["17%","19%","21%","23%"],
            "correct_index":2,
            "answer":"21%",
            "explanation":"按照本题对应 hyz 卡片的速算策略判断。",
            "skill":"间隔增长率",
            "evidence":"2023年增长10%，2024年增长10%"
        });
        let q: VerifiedQuestion = serde_json::from_value(item).unwrap();
        assert_eq!(q.correct_index, 2);
        assert_eq!(q.answer, "21%");
        assert_eq!(q.skill, "间隔增长率");
    }

    #[test]
    fn token_is_the_only_supported_model_credential() {
        let sample = format!(
            "HYZ_THINGS_AI_BASE_URL=https://api.deepseek.com\nHYZ_THINGS_AI_API_KEY=legacy\nHYZ_THINGS_AI_MODEL=legacy\nHYZ_THINGS_CODEX_WS_TOKEN={}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things\n",
            "a".repeat(64)
        );
        let config = parse_env(&sample).unwrap().unwrap();
        assert_eq!(config.token.len(), 64);
        assert_eq!(config.cwd, "/home/hyz/hyz_things");
        assert!(parse_env("HYZ_THINGS_AI_BASE_URL=https://api.deepseek.com")
            .unwrap()
            .is_none());
        assert!(parse_env(
            "HYZ_THINGS_CODEX_WS_TOKEN=bad token\nHYZ_THINGS_CODEX_CWD=/home/user/repo"
        )
        .is_err());
        assert!(parse_env("HYZ_THINGS_CODEX_WS_TOKEN=short-token-12").is_err());
        assert!(parse_env("HYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things").is_err());
        assert!(!valid_remote_repo_path("hyz-things"));
        assert!(!valid_remote_repo_path("/home/../etc"));
        assert!(!valid_remote_repo_path("/home/hyz\nhead"));
        assert!(valid_remote_repo_path("/home/hyz/hyz_things"));
        assert!(parse_env(&format!(
            "HYZ_THINGS_CODEX_WS_TOKEN={0}\nHYZ_THINGS_CODEX_WS_TOKEN={0}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things",
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
            move |material, _ordinal| {
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
    async fn saved_official_groups_support_history_without_new_codex_generation() {
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
                version: 4,
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

        let config = parse_env(&format!(
            "HYZ_THINGS_CODEX_WS_TOKEN={}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things",
            "a".repeat(64)
        ))
        .unwrap()
        .unwrap();
        // Previously saved groups are still readable in history, but a new
        // generation must no longer short-circuit to cached group JSON.
        let historical_rows = sqlx::query("SELECT group_json FROM data_analysis_groups")
            .fetch_all(&pool)
            .await
            .unwrap();
        let archived_groups = historical_rows
            .into_iter()
            .map(|row| {
                serde_json::from_str::<QuestionGroup>(
                    &row.try_get::<String, _>("group_json").unwrap(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let run_id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO data_analysis_runs(id,groups_json,answers_json,status,created_at_ms) VALUES(?,?,?,'ready',?)")
            .bind(&run_id)
            .bind(serde_json::to_string(&archived_groups).unwrap())
            .bind(serde_json::to_string(&vec![None::<usize>;20]).unwrap())
            .bind(now_ms()).execute(&pool).await.unwrap();
        let service = DataAnalysisService::new(pool, Some(config)).unwrap();
        let active = service.run(&run_id).await.expect("start 20-minute timer");
        assert_eq!(active.groups.len(), 4);
        assert!(active.groups.iter().all(|group| group.questions.len() == 5));
        assert_eq!(active.duration_seconds, 1200);
        assert!(active.deadline_at_ms.unwrap() > active.started_at_ms.unwrap());
        assert_eq!(active.answers.len(), 20);
        let paused = service.pause_toggle(&run_id).await.expect("pause timer");
        assert!(paused.paused_remaining_ms.is_some());
        assert!(paused.deadline_at_ms.is_none());
        assert!(
            service.answer(&run_id, 1, 1).await.is_err(),
            "pausing blocks answers"
        );
        let reloaded = service.run(&run_id).await.expect("paused reload");
        assert!(reloaded.paused_remaining_ms.is_some());
        let resumed = service.pause_toggle(&run_id).await.expect("resume timer");
        assert!(resumed.paused_remaining_ms.is_none());
        assert!(resumed.deadline_at_ms.is_some());
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
        let entries = service.history().await.expect("history listing");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].correct, 1);
        assert_eq!(entries[0].total, 20);
        let historical = service.result(&run_id).await.expect("historical review");
        assert_eq!(historical.materials.len(), 4);
        assert_eq!(historical.questions.len(), 20);
        let (source_count, group_count) = service.counts().await.unwrap();
        assert_eq!((source_count, group_count), (1, 4));
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
}
