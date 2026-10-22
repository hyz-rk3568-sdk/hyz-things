//! Native Codex App Server question preparation from exam-inspired simulated topics.
//! Codex owns the answer; the application stores each freshly prepared practice attempt.
use super::codex_app_server::{validate_token, CodexSession, TURN_TIMEOUT};
use crate::domain::data_analysis::{
    select_material_batch, topic_scope, Material, VerifiedQuestion, QUIZ_GROUPS, QUIZ_PER_GROUP,
    QUIZ_SECONDS, TOPICS,
};
use serde::{Deserialize, Serialize};
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

const ENV_PATH: &str = "/userdata/hyz-things/.env";
const RECENT_THEME_PAPERS: usize = 4;

// User-owned style and difficulty policy. Hard limits, selected categories,
// JSON shape and sources remain server-controlled even if this text changes.
pub const DEFAULT_USER_PROMPT: &str = r#"请生成符合国考、省考资料分析风格的原创模拟题。

材料：400～1000汉字，分2～4段，统计指标、单位、年份和同比关系自然且自洽；数字接近公报精度，避免刻意凑整。只陈述统计事实，不给公式、找数路径、方法提示或教学说明。

命题：优先考查对应 Flashcard 的审题、速算、易错点及变式，避免机械换数字或重复基础题。题干仅提出所求年份、主体和指标，不提示计算步骤。不出“下列说法正确的是”“下列说法错误的是”“根据资料可以推出的是”等需要逐项核查多个独立陈述的综合判断题。每题围绕一个明确的数量关系、指标或比较目标命题；即使选中了“综合分析与审题”类别，也优先将其转化为具体计算或比较题。

选项：每题四个合理干扰项，适合行测近似速算；答案唯一，材料、选项与解析相符。解析简洁，优先使用 hyz Flashcard 方法。

先自行检查数值关系和选项正确性，不得照搬真题原文或数字。"#;

#[derive(Clone, Serialize)]
pub struct UserPromptSettings {
    pub user_prompt: String,
    pub customized: bool,
}

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
    /// Authoritative server clock sampled for this response; clients must
    /// not compare a device-local clock directly with deadline_at_ms.
    pub server_now_ms: i64,
    pub duration_seconds: u64,
    pub groups: Vec<PublicGroup>,
    pub answers: Vec<Option<usize>>,
    pub current_question: usize,
}
#[derive(Clone, Serialize)]
pub struct ClaimedRun {
    pub run: RunView,
    pub control_token: String,
}
#[derive(Clone, Serialize)]
pub struct UnfinishedRun {
    pub id: String,
    pub status: String,
    pub answered: usize,
    pub current_question: usize,
    pub deadline_at_ms: Option<i64>,
    pub paused_remaining_ms: Option<i64>,
    pub created_at_ms: i64,
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
    pub attempt_no: i64,
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

// Each selected wrong-category consumes exactly one guaranteed question,
// balanced randomly across the 4 passages. A plan is created once per job,
// so failed passage retries retain exactly the same requirements.
#[derive(Clone, Debug)]
struct QuestionPlan {
    required: [Vec<String>; QUIZ_GROUPS],
    knowledge: Vec<String>,
}

fn distribute_wrong_categories(
    selected: &[String],
    random_seed: u64,
) -> [Vec<String>; QUIZ_GROUPS] {
    let mut required: [Vec<String>; QUIZ_GROUPS] = std::array::from_fn(|_| Vec::new());
    let mut state = random_seed | 1;
    let mut next_random = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for batch in selected.chunks(QUIZ_GROUPS) {
        let mut slots: Vec<_> = (0..QUIZ_GROUPS).collect();
        for i in (1..QUIZ_GROUPS).rev() {
            let j = next_random() as usize % (i + 1);
            slots.swap(i, j);
        }
        for (category, slot) in batch.iter().zip(slots.into_iter()) {
            required[slot].push(category.clone());
        }
    }
    required
}

// Exactly one marked wrong question per assigned category. All other slots
// come from a verified general-knowledge directory. The displayed skill may
// coincide with a wrong category; source_kind disambiguates those questions.
fn mixed_category_coverage(
    labels: &[&str],
    source_kinds: &[&str],
    required: &[String],
    knowledge: &[String],
) -> bool {
    if labels.len() != QUIZ_PER_GROUP || source_kinds.len() != QUIZ_PER_GROUP {
        return false;
    }
    let mut wrong_seen = std::collections::HashSet::new();
    for (&skill, &source) in labels.iter().zip(source_kinds) {
        match source {
            "wrong" if required.iter().any(|name| name == skill) => {
                if !wrong_seen.insert(skill) {
                    return false;
                }
            }
            "knowledge" if knowledge.iter().any(|name| name == skill) => {}
            _ => return false,
        }
    }
    wrong_seen.len() == required.len()
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

/// Build a graded view from saved answers without changing the run or requiring
/// the editing lease. Callers must check submission status before history reads.
fn grade_run(
    id: &str,
    groups: Vec<QuestionGroup>,
    answers: Vec<Option<usize>>,
) -> Result<RunResult, String> {
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

impl DataAnalysisService {
    pub fn new(pool: SqlitePool, config: Option<CodexConfig>) -> Result<Self, String> {
        Ok(Self {
            pool,
            config,
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
    /// Read only newly generated papers, newest first. A replay copies its
    /// parent topics and must never count as an independent theme selection.
    /// Both unfinished and submitted originals count as recently used.
    async fn recent_paper_themes(&self) -> Result<Vec<Vec<String>>, String> {
        let rows = sqlx::query(
            "SELECT groups_json FROM data_analysis_runs
             WHERE origin_run_id IS NULL
             ORDER BY created_at_ms DESC, id DESC LIMIT ?",
        )
        .bind(RECENT_THEME_PAPERS as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(as_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let json: String = row.try_get("groups_json").ok()?;
                let groups: Vec<QuestionGroup> = serde_json::from_str(&json).ok()?;
                (groups.len() == QUIZ_GROUPS).then(|| {
                    groups
                        .into_iter()
                        .map(|group| group.material.topic)
                        .collect::<Vec<_>>()
                })
            })
            .collect())
    }

    /// Derive categories from "错题/<分类>" whether RK3568 mounted
    /// flashcards/, flashcards/资料/, or the enclosing repository.
    pub async fn available_skills(&self) -> Result<Vec<String>, String> {
        // Prefer the live mounted folder tree to the previously imported cards.
        if let Some(live) = mounted_wrong_question_categories().await {
            return Ok(live);
        }
        let rows = sqlx::query("SELECT DISTINCT deck_path FROM flashcards WHERE active=1")
            .fetch_all(&self.pool)
            .await
            .map_err(as_error)?;
        let mut categories = std::collections::BTreeSet::new();
        for row in rows {
            let deck: String = row.try_get("deck_path").map_err(as_error)?;
            if let Some(name) = wrong_question_category(&deck) {
                categories.insert(name.to_owned());
            }
        }
        Ok(categories.into_iter().collect())
    }

    async fn general_knowledge_skills(&self) -> Result<Vec<String>, String> {
        if let Some(live) = mounted_knowledge_categories().await {
            return Ok(live);
        }
        let rows = sqlx::query("SELECT DISTINCT deck_path FROM flashcards WHERE active=1")
            .fetch_all(&self.pool)
            .await
            .map_err(as_error)?;
        let mut skills = std::collections::BTreeSet::new();
        for row in rows {
            let deck: String = row.try_get("deck_path").map_err(as_error)?;
            if let Some(skill) = knowledge_card_category(&deck) {
                skills.insert(skill.to_owned());
            }
        }
        if skills.is_empty() {
            return Err(
                "没有找到 flashcards/资料/ 中的普通知识点卡片，请检查卡片目录挂载或同步".into(),
            );
        }
        Ok(skills.into_iter().collect())
    }

    pub async fn user_prompt_settings(&self) -> Result<UserPromptSettings, String> {
        let saved = sqlx::query_scalar::<_, String>(
            "SELECT user_prompt FROM data_analysis_prompt_settings WHERE id=1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(as_error)?;
        Ok(match saved {
            Some(user_prompt) => UserPromptSettings {
                user_prompt,
                customized: true,
            },
            None => UserPromptSettings {
                user_prompt: DEFAULT_USER_PROMPT.to_owned(),
                customized: false,
            },
        })
    }

    /// None restores the default. User input can never replace system constraints.
    pub async fn save_user_prompt(&self, text: Option<&str>) -> Result<UserPromptSettings, String> {
        if let Some(prompt) = text {
            if prompt.trim().is_empty()
                || prompt.len() > 8000
                || prompt
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
            {
                return Err("出题要求不能为空，且不能超过8000字节".into());
            }
            sqlx::query(
                "INSERT INTO data_analysis_prompt_settings(id,user_prompt,updated_at_ms)
                 VALUES(1,?,?)
                 ON CONFLICT(id) DO UPDATE SET
                     user_prompt=excluded.user_prompt,
                     updated_at_ms=excluded.updated_at_ms",
            )
            .bind(prompt.trim())
            .bind(now_ms())
            .execute(&self.pool)
            .await
            .map_err(as_error)?;
        } else {
            sqlx::query("DELETE FROM data_analysis_prompt_settings WHERE id=1")
                .execute(&self.pool)
                .await
                .map_err(as_error)?;
        }
        self.user_prompt_settings().await
    }

    pub async fn chosen_manual_skills(&self, requested: &[String]) -> Result<Vec<String>, String> {
        if requested.len() > 12 {
            return Err("每次最多勾选12个考点".into());
        }
        if requested.is_empty() {
            return Err("请至少选择一个错题考点".into());
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
        // Freeze the current user instructions for one entire 20-question job.
        let user_prompt = self.user_prompt_settings().await?.user_prompt;
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
            let result = worker.generate(&job, &manual_skills, &user_prompt).await;
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
    async fn generate(
        &self,
        job: &str,
        manual_skills: &[String],
        user_prompt: &str,
    ) -> Result<String, String> {
        self.set_progress(job, "selecting", 0, "正在规划参考真题的统计主题", None)
            .await;
        // Keep a single randomized, balanced quota plan for the entire job,
        // including retries after a Codex passage fails validation.
        let plan = Arc::new(QuestionPlan {
            required: distribute_wrong_categories(
                manual_skills,
                Uuid::parse_str(job).map_err(as_error)?.as_u128() as u64,
            ),
            knowledge: self.general_knowledge_skills().await?,
        });
        let recent_papers = self.recent_paper_themes().await?;
        let theme_seed =
            Uuid::parse_str(job).map_err(as_error)?.as_u128() as u64 ^ 0x9e3779b97f4a7c15;
        let ordered = order_exam_themes(exam_theme_materials(), &recent_papers, theme_seed);
        // Keep the original four slots even if only some parallel tasks succeed.
        // Retried slots must retain their assigned wrong/knowledge quotas.
        let mut ready = vec![None::<QuestionGroup>; QUIZ_GROUPS];
        let mut attempted = HashSet::<String>::new();
        let mut last_error = String::new();
        let mut attempts = 0;
        loop {
            let missing = ready
                .iter()
                .enumerate()
                .filter_map(|(slot, group)| group.is_none().then_some(slot))
                .collect::<Vec<_>>();
            if missing.is_empty() {
                break;
            }
            let completed: Vec<Material> = ready
                .iter()
                .flatten()
                .map(|group| group.material.clone())
                .collect();
            let wave = select_material_batch(&ordered, &completed, &attempted, missing.len());
            if wave.is_empty() {
                break;
            }

            let mut ordered_results = vec![None::<QuestionGroup>; QUIZ_GROUPS];
            let mut requests = Vec::new();
            for (slot, material) in missing.into_iter().zip(wave.iter()) {
                attempted.insert(material.topic.clone());
                if attempts < 12 {
                    attempts += 1;
                    requests.push((slot, material.clone()));
                }
            }
            if requests.is_empty() {
                break;
            }

            self.set_progress(
                job,
                "generating",
                ready.iter().filter(|group| group.is_some()).count() * QUIZ_PER_GROUP,
                "最多4个 Codex Thread 正在生成错题强化与资料知识点混合试卷",
                None,
            )
            .await;
            let worker = self.clone();
            let plan = Arc::clone(&plan);
            let user_prompt = Arc::new(user_prompt.to_owned());
            for (slot, outcome) in run_generation_wave(requests, move |material, slot| {
                let worker = worker.clone();
                let plan = Arc::clone(&plan);
                let user_prompt = Arc::clone(&user_prompt);
                async move {
                    worker
                        .generate_group(&material, &plan, &user_prompt, slot)
                        .await
                }
            })
            .await
            {
                match outcome {
                    Ok(group) if slot < QUIZ_GROUPS => {
                        ordered_results[slot] = Some(group);
                    }
                    Ok(_) => last_error = "Codex 并行任务返回异常序号".into(),
                    Err(error) => last_error = error,
                }
            }

            // Only the coordinator persists groups in the assigned slot.
            for (slot, group) in ordered_results.into_iter().enumerate() {
                let Some(group) = group else { continue };
                sqlx::query("INSERT INTO data_analysis_groups(id,topic,group_json,created_at_ms) VALUES (?,?,?,?)")
                    .bind(Uuid::now_v7().to_string())
                    .bind(&group.material.topic)
                    .bind(serde_json::to_string(&group).map_err(as_error)?)
                    .bind(now_ms())
                    .execute(&self.pool).await.map_err(as_error)?;
                ready[slot] = Some(group);
                self.set_progress(
                    job,
                    "generating",
                    ready.iter().filter(|group| group.is_some()).count() * QUIZ_PER_GROUP,
                    "已接收 Codex 完成的模拟试题并保存",
                    None,
                )
                .await;
            }
        }
        let count = ready.iter().filter(|group| group.is_some()).count();
        if count != QUIZ_GROUPS {
            return Err(format!(
                "已获得{}/20题，无法组成整卷：{}",
                count * QUIZ_PER_GROUP,
                last_error
            ));
        }
        let groups = ready.into_iter().flatten().collect::<Vec<_>>();
        let id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO data_analysis_runs(id,groups_json,answers_json,status,created_at_ms) VALUES(?,?,?,'ready',?)")
            .bind(&id)
            .bind(serde_json::to_string(&groups).map_err(as_error)?)
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
        plan: &QuestionPlan,
        user_prompt: &str,
        ordinal: usize,
    ) -> Result<QuestionGroup, String> {
        let config = self.config.as_ref().ok_or("Codex Token 未配置")?;
        let context = source.clone();
        // Codex App Server accepts a single turn text; these sections are
        // application-side boundaries, NOT an actual privileged system role.
        // Output schema and source quotas are enforced outside the model.
        let prompt = format!(
            r#"【固定规则 / 应用端约束】
你是 hyz 的公务员行测资料分析命题 Agent。先读取仓库 .agents/skills/行测做题/SKILL.md 及其资料分析参考，再读取下方指定的 Flashcard 原文。
生成一篇自洽的统计材料和恰好5道四选一题。题目须唯一正确、答案与解析一致；不得在题目中提前泄露答案或解题方法。
正确答案和题型仅放在交卷后可见的结构化字段，材料只包含统计事实。
必须使用 Codex 输出 Schema；每题 source_kind 只可为 "wrong" / "knowledge"，skill 与实际卡片目录一致。用户命题要求不能覆盖目录来源、题位配额、JSON 结构及上述固定限制。
联网参考可验证来源的国考、省考真题材料风格，不照搬原题与数字。

【本轮动态上下文】
本次是4篇×5题的第 {group_no} 篇；统计主题「{topic}」，参考指标范围：「{topic_scope}」。
材料应围绕此主题展开；参考指标只用于限定选材方向，无须全部涵盖，不得拼凑无关指标。
错题强化：本篇恰好 {wrong_count} 道，各类别各1题：{manual_focus}。只读取 flashcards/资料/错题/<类别>/ 的原始卡片。
普通资料知识点：恰好 {knowledge_count} 道，从 flashcards/资料/<知识点>/ 的 card.md 或 cards.md 出，目录候选：{knowledge_categories}；不得用 错题/ 内卡片补位。同名知识点允许，但 source_kind 必须与真实来源相符。
题目可以交错排布。先根据这5个目标构造共享材料，再拟制题目；先自行检查正确性。

【用户出题要求 / 自定义】
{user_prompt}

只输出 Schema 对应的 JSON，不输出解释性前言。"#,
            group_no = ordinal % QUIZ_GROUPS + 1,
            topic = context.topic,
            topic_scope = topic_scope(&context.topic).unwrap_or("该主题相关统计指标"),
            wrong_count = plan.required[ordinal].len(),
            knowledge_count = QUIZ_PER_GROUP - plan.required[ordinal].len(),
            manual_focus = plan.required[ordinal].join("、"),
            knowledge_categories = plan.knowledge.join("、"),
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
        struct GeneratedQuestion {
            #[serde(flatten)]
            question: VerifiedQuestion,
            source_kind: String,
        }
        #[derive(Deserialize)]
        struct Batch {
            material_text: String,
            questions: Vec<GeneratedQuestion>,
        }
        let batch: Batch =
            serde_json::from_str(&output).map_err(|_| "Codex 返回的试卷 JSON 与展示格式不匹配")?;
        // Structural safety only: protect fixed UI indexing. No math/content
        // verification and no answer rewriting by hyz-things.
        if batch.questions.len() != QUIZ_PER_GROUP
            || batch
                .questions
                .iter()
                .any(|q| q.question.correct_index >= 4)
        {
            return Err("Codex 返回的试卷结构不完整（需要5题，每题4个选项）".into());
        }
        let labels: Vec<_> = batch
            .questions
            .iter()
            .map(|q| q.question.skill.as_str())
            .collect();
        let kinds: Vec<_> = batch
            .questions
            .iter()
            .map(|q| q.source_kind.as_str())
            .collect();
        if !mixed_category_coverage(&labels, &kinds, &plan.required[ordinal], &plan.knowledge) {
            return Err("Codex 生成题目未满足错题保底及普通知识点配额，请重试".into());
        }
        let mut material = context;
        material.text = batch.material_text;
        Ok(QuestionGroup {
            version: 4,
            material,
            questions: batch.questions.into_iter().map(|q| q.question).collect(),
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
    /// Create a fresh, untimed replay of a completed paper. Never touch the
    /// source record or invoke Codex, even when replaying a previous replay.
    pub async fn retry(&self, source_id: &str) -> Result<String, String> {
        let mut tx = self.pool.begin().await.map_err(as_error)?;
        let row = sqlx::query(
            "SELECT groups_json, answers_json, status, COALESCE(origin_run_id, id) AS root_id
             FROM data_analysis_runs WHERE id=?",
        )
        .bind(source_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(as_error)?
        .ok_or("历史训练不存在")?;
        let status: String = row.try_get("status").map_err(as_error)?;
        if status != "submitted" {
            return Err("只有已交卷的训练可以重做".into());
        }
        let groups_json: String = row.try_get("groups_json").map_err(as_error)?;
        let groups: Vec<QuestionGroup> = serde_json::from_str(&groups_json).map_err(as_error)?;
        if groups.len() != QUIZ_GROUPS || groups.iter().any(|g| g.questions.len() != QUIZ_PER_GROUP)
        {
            return Err("历史试卷题目不完整，无法重做".into());
        }
        let original_answers: Vec<Option<usize>> =
            serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                .map_err(as_error)?;
        if original_answers.len() != QUIZ_GROUPS * QUIZ_PER_GROUP {
            return Err("历史答题记录不完整，无法重做".into());
        }
        let root_id: String = row.try_get("root_id").map_err(as_error)?;
        let attempt_no: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(attempt_no), 1) + 1
             FROM data_analysis_runs WHERE id=? OR origin_run_id=?",
        )
        .bind(&root_id)
        .bind(&root_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(as_error)?;
        let id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO data_analysis_runs
             (id,groups_json,answers_json,status,created_at_ms,origin_run_id,attempt_no)
             VALUES (?,?,?,'ready',?,?,?)",
        )
        .bind(&id)
        // Keep old questions verbatim, but do not carry legacy Material fields
        // into a new attempt's persisted JSON.
        .bind(serde_json::to_string(&groups).map_err(as_error)?)
        .bind(
            serde_json::to_string(&vec![None::<usize>; QUIZ_GROUPS * QUIZ_PER_GROUP])
                .map_err(as_error)?,
        )
        .bind(now_ms())
        .bind(root_id)
        .bind(attempt_no)
        .execute(&mut *tx)
        .await
        .map_err(as_error)?;
        tx.commit().await.map_err(as_error)?;
        Ok(id)
    }

    /// One server-owned live position and a single random editing lease.
    /// Claim is an explicit takeover: all older tokens stop authorizing writes.
    pub async fn claim(&self, id: &str) -> Result<ClaimedRun, String> {
        let token = Uuid::now_v7().to_string();
        let updated = sqlx::query(
            "UPDATE data_analysis_runs
             SET controller_token=?, status='active',
                 started_at_ms=COALESCE(started_at_ms, ?)
             WHERE id=? AND status IN ('ready','active')",
        )
        .bind(&token)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(as_error)?;
        if updated.rows_affected() != 1 {
            return Err("训练已交卷或不存在，无法接管".into());
        }
        Ok(ClaimedRun {
            run: self.run(id).await?,
            control_token: token,
        })
    }

    /// Server discovery works across browsers/devices; never expose answer keys
    /// or control tokens. Ready runs stay untimed until explicitly claimed.
    pub async fn unfinished(&self) -> Result<Vec<UnfinishedRun>, String> {
        let rows = sqlx::query(
            "SELECT id,status,answers_json,current_question,started_at_ms,
                    paused_at_ms,paused_total_ms,created_at_ms
             FROM data_analysis_runs WHERE status IN ('ready','active')
             ORDER BY created_at_ms DESC LIMIT 30",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(as_error)?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let answers: Vec<Option<usize>> =
                serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                    .map_err(as_error)?;
            if answers.len() != QUIZ_GROUPS * QUIZ_PER_GROUP {
                continue;
            }
            let status: String = row.try_get("status").map_err(as_error)?;
            let start: Option<i64> = row.try_get("started_at_ms").map_err(as_error)?;
            let paused_at: Option<i64> = row.try_get("paused_at_ms").map_err(as_error)?;
            let paused_total: i64 = row.try_get("paused_total_ms").map_err(as_error)?;
            let deadline = start.map(|at| at + QUIZ_SECONDS as i64 * 1000 + paused_total);
            items.push(UnfinishedRun {
                id: row.try_get("id").map_err(as_error)?,
                status,
                answered: answers.iter().filter(|a| a.is_some()).count(),
                current_question: row
                    .try_get::<i64, _>("current_question")
                    .map_err(as_error)?
                    .clamp(0, 19) as usize,
                deadline_at_ms: if paused_at.is_none() { deadline } else { None },
                paused_remaining_ms: paused_at
                    .zip(deadline)
                    .map(|(since, end)| (end - since).max(0)),
                created_at_ms: row.try_get("created_at_ms").map_err(as_error)?,
            });
        }
        Ok(items)
    }

    async fn check_control(&self, id: &str, token: Option<&str>) -> Result<(), String> {
        let stored: Option<String> =
            sqlx::query_scalar("SELECT controller_token FROM data_analysis_runs WHERE id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(as_error)?
                .ok_or("训练不存在")?;
        if stored.as_deref() != token {
            return Err("本次训练已在其他设备接管，请重新进入".into());
        }
        Ok(())
    }

    /// A position change cannot alter answers or active timer.
    pub async fn save_position(
        &self,
        id: &str,
        question: usize,
        token: &str,
    ) -> Result<(), String> {
        if question >= QUIZ_GROUPS * QUIZ_PER_GROUP {
            return Err("题号无效".into());
        }
        let updated = sqlx::query(
            "UPDATE data_analysis_runs SET current_question=?
             WHERE id=? AND status='active' AND controller_token=?",
        )
        .bind(question as i64)
        .bind(id)
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(as_error)?;
        if updated.rows_affected() == 0 {
            return Err("本次训练已在其他设备接管，请重新进入".into());
        }
        Ok(())
    }

    pub async fn run(&self, id: &str) -> Result<RunView, String> {
        sqlx::query("UPDATE data_analysis_runs SET status='active',started_at_ms=? WHERE id=? AND status='ready'")
            .bind(now_ms()).bind(id).execute(&self.pool).await.map_err(as_error)?;
        let (groups, answers, status, started_at_ms) = self.fetch_row(id).await?;
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
            server_now_ms: now_ms(),
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
                        })
                        .collect(),
                })
                .collect(),
            answers,
            current_question: sqlx::query_scalar::<_, i64>(
                "SELECT current_question FROM data_analysis_runs WHERE id=?",
            )
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(as_error)?
            .clamp(0, 19) as usize,
        })
    }
    pub async fn answer(&self, id: &str, number: usize, choice: usize) -> Result<(), String> {
        self.answer_controlled(id, number, choice, None).await
    }

    pub async fn answer_controlled(
        &self,
        id: &str,
        number: usize,
        choice: usize,
        token: Option<&str>,
    ) -> Result<(), String> {
        if !(1..=QUIZ_GROUPS * QUIZ_PER_GROUP).contains(&number) || choice > 3 {
            return Err("题号或选项无效".into());
        }
        // A single SQL JSON update avoids lost answers from concurrent requests.
        // The token predicate also rejects a write racing with a new claim.
        let updated = sqlx::query(
            "UPDATE data_analysis_runs
             SET answers_json=json_set(answers_json, ?, ?)
             WHERE id=? AND status='active' AND paused_at_ms IS NULL
               AND started_at_ms IS NOT NULL
               AND (? < started_at_ms + ? + paused_total_ms)
               AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)",
        )
        .bind(format!("$[{}]", number - 1))
        .bind(choice as i64)
        .bind(id)
        .bind(now_ms())
        .bind(QUIZ_SECONDS as i64 * 1000)
        .bind(token)
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(as_error)?;
        if updated.rows_affected() == 0 {
            return Err("训练已暂停、超时或控制权已移交".into());
        }
        Ok(())
    }

    pub async fn submit(&self, id: &str) -> Result<RunResult, String> {
        self.submit_controlled(id, None).await
    }
    pub async fn submit_controlled(
        &self,
        id: &str,
        token: Option<&str>,
    ) -> Result<RunResult, String> {
        self.check_control(id, token).await?;
        let (groups, answers, status, _started) = self.fetch_row(id).await?;
        let result = grade_run(id, groups, answers)?;
        if status != "submitted" {
            let mut txn = self.pool.begin().await.map_err(as_error)?;
            let updated = sqlx::query(
                "UPDATE data_analysis_runs SET status='submitted',ended_at_ms=?
                 WHERE id=? AND status='active'
                 AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)",
            )
            .bind(now_ms())
            .bind(id)
            .bind(token)
            .bind(token)
            .execute(&mut *txn)
            .await
            .map_err(as_error)?;
            if updated.rows_affected() != 1 {
                return Err("训练已由其他设备接管或已交卷".into());
            }
            for q in &result.questions {
                sqlx::query("INSERT OR IGNORE INTO data_analysis_results(run_id,question_no,topic,skill,is_correct,elapsed_ms) VALUES(?,?,?,?,?,?)")
                    .bind(id).bind(q.number as i64).bind(&q.topic).bind(&q.skill)
                    .bind((q.chosen==Some(q.correct_index)) as i64).bind(0i64)
                    .execute(&mut *txn).await.map_err(as_error)?;
            }
            txn.commit().await.map_err(as_error)?;
        }
        Ok(result)
    }

    pub async fn pause_toggle(&self, id: &str) -> Result<RunView, String> {
        self.pause_toggle_controlled(id, None).await
    }
    pub async fn pause_toggle_controlled(
        &self,
        id: &str,
        token: Option<&str>,
    ) -> Result<RunView, String> {
        self.check_control(id, token).await?;
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
            sqlx::query("UPDATE data_analysis_runs SET paused_at_ms=NULL,paused_total_ms=paused_total_ms+? WHERE id=? AND status='active' AND paused_at_ms=? AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)")
                .bind((now-paused_at).max(0)).bind(id).bind(paused_at).bind(token).bind(token)
                .execute(&self.pool).await.map_err(as_error)?
        } else {
            if started.is_none_or(|ms| now >= ms + QUIZ_SECONDS as i64 * 1000 + paused_total) {
                return Err("训练计时已结束，不能暂停".into());
            }
            sqlx::query("UPDATE data_analysis_runs SET paused_at_ms=? WHERE id=? AND status='active' AND paused_at_ms IS NULL AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)")
                .bind(now).bind(id).bind(token).bind(token).execute(&self.pool).await.map_err(as_error)?
        };
        if result.rows_affected() != 1 {
            return Err("计时状态已变化，请刷新".into());
        }
        self.run(id).await
    }

    pub async fn result(&self, id: &str) -> Result<RunResult, String> {
        let (groups, answers, status, _started) = self.fetch_row(id).await?;
        if status != "submitted" {
            return Err("训练尚未交卷".into());
        }
        grade_run(id, groups, answers)
    }

    pub async fn history(&self) -> Result<Vec<ComprehensiveHistoryEntry>, String> {
        let rows=sqlx::query(
            "SELECT id,groups_json,answers_json,started_at_ms,ended_at_ms,paused_total_ms,attempt_no FROM data_analysis_runs WHERE status='submitted' AND ended_at_ms IS NOT NULL ORDER BY ended_at_ms DESC LIMIT 100"
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
                attempt_no: row.try_get("attempt_no").map_err(as_error)?,
                started_at_ms: started,
                ended_at_ms: ended,
                elapsed_ms: (ended - started - paused_total).max(0),
                correct,
                total,
                topics: groups.iter().map(|g| g.material.topic.clone()).collect(),
            });
        }
        Ok(result)
    }
}

/// Shuffle equal-priority themes per newly created paper, while strongly
/// preferring topics not seen in the previous four original papers. Historical
/// replays are excluded by the server query. Sorting stays stable after shuffle
/// so equal reuse penalties retain a randomized order.
fn order_exam_themes(
    mut themes: Vec<Material>,
    recent_papers: &[Vec<String>],
    seed: u64,
) -> Vec<Material> {
    let mut state = seed | 1;
    for i in (1..themes.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        themes.swap(i, state as usize % (i + 1));
    }

    // Newest papers are penalized most. When four unseen themes exist, none
    // of the recently used topics can enter the initial four assignments.
    let mut penalties: HashMap<&str, usize> = HashMap::new();
    for (age, topics) in recent_papers.iter().take(RECENT_THEME_PAPERS).enumerate() {
        let weight = 1 << (RECENT_THEME_PAPERS - 1 - age);
        for topic in topics.iter().map(String::as_str).collect::<HashSet<_>>() {
            *penalties.entry(topic).or_default() += weight;
        }
    }
    themes.sort_by_key(|material| penalties.get(material.topic.as_str()).copied().unwrap_or(0));
    themes
}

/// Lightweight theme seeds only. No National Bureau of Statistics fetch is
/// needed: Codex researches actual exam-style passages before inventing a new
/// self-contained statistical dataset for each theme.
fn exam_theme_materials() -> Vec<Material> {
    TOPICS
        .iter()
        .map(|(topic, _)| Material {
            topic: (*topic).to_owned(),
            text: String::new(),
        })
        .collect()
}

/// Look at the directory currently mounted into RK3568 rather than
/// requiring a card sync just to show a newly created folder name.
async fn mounted_wrong_question_categories() -> Option<Vec<String>> {
    let root = super::flashcard_source::FLASHCARD_SOURCE_ROOT;
    for relative in ["资料/错题", "错题", "flashcards/资料/错题"] {
        let path = std::path::Path::new(root).join(relative);
        let Ok(mut reader) = tokio::fs::read_dir(path).await else {
            continue;
        };
        let mut result = std::collections::BTreeSet::<String>::new();
        while let Ok(Some(entry)) = reader.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if valid_wrong_category(&name)
                && entry.file_type().await.is_ok_and(|kind| kind.is_dir())
            {
                result.insert(name);
            }
        }
        return Some(result.into_iter().collect());
    }
    None
}

// Normal knowledge cards live directly in flashcards/资料/<skill>/card.md,
// not in 资料/错题/. Support all three source mount layouts already used.
async fn mounted_knowledge_categories() -> Option<Vec<String>> {
    let root = super::flashcard_source::FLASHCARD_SOURCE_ROOT;
    for relative in ["资料", ".", "flashcards/资料"] {
        let path = std::path::Path::new(root).join(relative);
        let Ok(mut reader) = tokio::fs::read_dir(path).await else {
            continue;
        };
        let mut names = std::collections::BTreeSet::new();
        while let Ok(Some(entry)) = reader.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "错题"
                || !valid_wrong_category(&name)
                || !entry.file_type().await.is_ok_and(|file| file.is_dir())
            {
                continue;
            }
            if tokio::fs::metadata(entry.path().join("card.md"))
                .await
                .is_ok()
                || tokio::fs::metadata(entry.path().join("cards.md"))
                    .await
                    .is_ok()
            {
                names.insert(name);
            }
        }
        if !names.is_empty() {
            return Some(names.into_iter().collect());
        }
    }
    None
}

fn knowledge_card_category(deck: &str) -> Option<&str> {
    let parts: Vec<_> = deck.split('/').filter(|s| !s.is_empty()).collect();
    let category = match parts.as_slice() {
        [skill] | ["资料", skill] | ["flashcards", "资料", skill] => *skill,
        _ => return None,
    };
    valid_wrong_category(category).then_some(category)
}

fn valid_wrong_category(category: &str) -> bool {
    let count = category.chars().count();
    count > 0
        && count <= 60
        && category != "."
        && category != ".."
        && !category.chars().any(char::is_control)
}

/// The directory immediately beneath flashcards/资料/错题/ is the skill.
/// The flashcard import root can point at flashcards/, 资料/ or 错题's parent.
fn wrong_question_category(deck: &str) -> Option<&str> {
    let parts = deck
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let index = parts.iter().position(|part| *part == "错题")?;
    let prefix = &parts[..index];
    if !prefix.is_empty() && prefix != ["资料"] && prefix != ["flashcards", "资料"] {
        return None;
    }
    let category = *parts.get(index + 1)?;
    if !valid_wrong_category(category) {
        return None;
    }
    Some(category)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_exact_wrong_question_subdirectories_as_skills() {
        assert_eq!(
            wrong_question_category("资料/错题/乘积增长率"),
            Some("乘积增长率")
        );
        assert_eq!(
            wrong_question_category("错题/混合增长率"),
            Some("混合增长率")
        );
        assert_eq!(
            wrong_question_category("flashcards/资料/错题/综合分析与审题/小结"),
            Some("综合分析与审题")
        );
        assert_eq!(
            wrong_question_category("资料/错题/平均数增量"),
            Some("平均数增量")
        );
        assert_eq!(wrong_question_category("资料/错题"), None);
        assert_eq!(wrong_question_category("资料/线段混合"), None);
        assert_eq!(wrong_question_category("其他/错题/乘积增长率"), None);
    }

    #[test]
    fn wrong_category_quotas_randomize_balanced_and_survive_retries() {
        for count in 1..=12 {
            let selected = (0..count)
                .map(|i| format!("错题分类{i}"))
                .collect::<Vec<_>>();
            let quotas = distribute_wrong_categories(&selected, 123456789);
            let all = quotas.iter().flatten().cloned().collect::<Vec<_>>();
            assert_eq!(all.len(), count);
            assert!(selected
                .iter()
                .all(|name| all.iter().filter(|n| *n == name).count() == 1));
            assert!(quotas.iter().all(|group| group.len() <= 3));
            let smallest = quotas.iter().map(Vec::len).min().unwrap();
            let largest = quotas.iter().map(Vec::len).max().unwrap();
            assert!(largest - smallest <= 1);
            assert_eq!(quotas, distribute_wrong_categories(&selected, 123456789));
        }
        let list = (0..12).map(|i| format!("topic{i}")).collect::<Vec<_>>();
        assert_ne!(
            distribute_wrong_categories(&list, 123456789),
            distribute_wrong_categories(&list, 7850213)
        );
    }

    #[test]
    fn mixed_paper_accepts_one_per_selected_wrong_and_general_knowledge_filler() {
        let required = vec!["线段混合".into(), "年均变化".into()];
        let knowledge = vec!["线段混合".into(), "年均变化".into(), "乘除变化".into()];
        assert!(mixed_category_coverage(
            &["线段混合", "乘除变化", "年均变化", "线段混合", "乘除变化"],
            &["wrong", "knowledge", "wrong", "knowledge", "knowledge"],
            &required,
            &knowledge,
        ));
        assert!(!mixed_category_coverage(
            &["线段混合", "乘除变化", "年均变化", "线段混合", "乘除变化"],
            &["wrong", "wrong", "wrong", "knowledge", "knowledge"],
            &required,
            &knowledge,
        ));
        assert!(!mixed_category_coverage(
            &["线段混合", "乘除变化", "年均变化", "线段混合", "乘除变化"],
            &["wrong", "knowledge", "knowledge", "knowledge", "knowledge"],
            &required,
            &knowledge,
        ));
        assert!(!mixed_category_coverage(
            &["线段混合", "乘除变化", "年均变化", "线段混合", "其他分类"],
            &["wrong", "knowledge", "wrong", "knowledge", "knowledge"],
            &required,
            &knowledge,
        ));
        assert!(!mixed_category_coverage(
            &["乘除变化", "乘除变化", "乘除变化", "乘除变化", "乘除变化"],
            &[
                "knowledge",
                "knowledge",
                "knowledge",
                "knowledge",
                "knowledge"
            ],
            &required,
            &knowledge,
        ));
        assert!(mixed_category_coverage(
            &["线段混合", "年均变化", "乘除变化", "年均变化", "乘除变化"],
            &[
                "knowledge",
                "knowledge",
                "knowledge",
                "knowledge",
                "knowledge"
            ],
            &[],
            &knowledge,
        ));
    }

    #[test]
    fn knowledge_decks_exclude_wrong_questions_and_unrelated_modules() {
        assert_eq!(knowledge_card_category("资料/年均变化"), Some("年均变化"));
        assert_eq!(
            knowledge_card_category("flashcards/资料/线段混合"),
            Some("线段混合")
        );
        assert_eq!(knowledge_card_category("乘除变化"), Some("乘除变化"));
        assert_eq!(knowledge_card_category("资料/错题/年均变化"), None);
        assert_eq!(knowledge_card_category("判断/论证推理"), None);
        assert_eq!(knowledge_card_category("资料/images"), Some("images"));
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
    fn default_user_prompt_contains_optional_judgment_question_preference() {
        // An opinionated default, not an immutable source, quota, or schema rule.
        assert!(DEFAULT_USER_PROMPT.contains("下列说法正确的是"));
        assert!(DEFAULT_USER_PROMPT.contains("根据资料可以推出的是"));
        assert!(DEFAULT_USER_PROMPT.contains("综合分析与审题"));
        assert!(DEFAULT_USER_PROMPT.contains("不出"));
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
                        topic: format!("topic-{slot}"),
                        text: "test".into(),
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
                    if material.topic == "topic-2" {
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
                assert_eq!(item.unwrap().material.topic, format!("topic-{slot}"));
            }
        }
    }

    #[tokio::test]
    async fn user_prompt_is_shared_by_sqlite_and_default_is_restorable() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let ipad = DataAnalysisService::new(pool.clone(), None).unwrap();
        let phone = DataAnalysisService::new(pool, None).unwrap();
        let initial = ipad.user_prompt_settings().await.unwrap();
        assert!(!initial.customized);
        assert_eq!(initial.user_prompt, DEFAULT_USER_PROMPT);
        let customized = ipad
            .save_user_prompt(Some("只出带合理干扰项的真实模拟数据题。"))
            .await
            .unwrap();
        assert!(customized.customized);
        assert_eq!(
            phone.user_prompt_settings().await.unwrap().user_prompt,
            "只出带合理干扰项的真实模拟数据题。"
        );
        assert!(phone.save_user_prompt(Some(" ")).await.is_err());
        assert!(phone
            .save_user_prompt(Some(&"x".repeat(8001)))
            .await
            .is_err());
        assert!(phone.save_user_prompt(Some("提示\0注入")).await.is_err());
        assert_eq!(
            phone.user_prompt_settings().await.unwrap().user_prompt,
            "只出带合理干扰项的真实模拟数据题。"
        );
        let restored = phone.save_user_prompt(None).await.unwrap();
        assert!(!restored.customized);
        assert_eq!(
            ipad.user_prompt_settings().await.unwrap().user_prompt,
            DEFAULT_USER_PROMPT
        );
    }

    #[tokio::test]
    async fn submitted_claimed_paper_history_is_read_only_without_controller_token() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let group = QuestionGroup {
            version: 4,
            material: Material {
                topic: "测试主题".into(),
                text: "测试统计材料".into(),
            },
            questions: (0..QUIZ_PER_GROUP)
                .map(|index| VerifiedQuestion {
                    question: format!("第{}题", index + 1),
                    options: ["A".into(), "B".into(), "C".into(), "D".into()],
                    correct_index: 1,
                    answer: "B".into(),
                    explanation: "历史解析".into(),
                    skill: "基期量".into(),
                    evidence: "原始统计数据".into(),
                })
                .collect(),
        };
        let id = Uuid::now_v7().to_string();
        // Simulate JSON created before section/subtopic/source_url removal.
        let mut archived = serde_json::to_value(vec![group; QUIZ_GROUPS]).unwrap();
        for group in archived.as_array_mut().unwrap() {
            group["material"]["section"] = serde_json::json!("测试章节");
            group["material"]["subtopic"] = serde_json::json!("测试子主题");
            group["material"]["source_url"] = serde_json::json!("practice-test");
        }
        sqlx::query(
            "INSERT INTO data_analysis_runs (id,groups_json,answers_json,status,created_at_ms)
             VALUES (?,?,?,'ready',?)",
        )
        .bind(&id)
        .bind(archived.to_string())
        .bind(serde_json::to_string(&vec![None::<usize>; QUIZ_GROUPS * QUIZ_PER_GROUP]).unwrap())
        .bind(now_ms())
        .execute(&pool)
        .await
        .unwrap();

        let service = DataAnalysisService::new(pool.clone(), None).unwrap();
        let claimed = service.claim(&id).await.unwrap();
        assert_eq!(claimed.run.groups[0].material.topic, "测试主题");
        assert_eq!(claimed.run.groups[0].material.text, "测试统计材料");
        assert_eq!(
            serde_json::to_value(&claimed.run.groups[0].material)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            2
        );
        assert!(
            service.result(&id).await.is_err(),
            "active answers stay hidden"
        );
        service
            .answer_controlled(&id, 1, 1, Some(&claimed.control_token))
            .await
            .unwrap();
        let submitted = service
            .submit_controlled(&id, Some(&claimed.control_token))
            .await
            .unwrap();
        assert_eq!(submitted.correct, 1);
        let before: (String, String, i64) = sqlx::query_as(
            "SELECT status,controller_token,ended_at_ms FROM data_analysis_runs WHERE id=?",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let history = service.history().await.unwrap();
        assert!(history.iter().any(|entry| entry.id == id));
        let reviewed = service.result(&id).await.expect("read submitted history");
        assert_eq!(reviewed.correct, submitted.correct);
        assert_eq!(reviewed.questions.len(), QUIZ_GROUPS * QUIZ_PER_GROUP);
        assert_eq!(reviewed.questions[0].explanation, "历史解析");
        let after: (String, String, i64) = sqlx::query_as(
            "SELECT status,controller_token,ended_at_ms FROM data_analysis_runs WHERE id=?",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, after, "history read must not update submitted run");

        let redo_id = service.retry(&id).await.unwrap();
        let copied_json: String =
            sqlx::query_scalar("SELECT groups_json FROM data_analysis_runs WHERE id=?")
                .bind(&redo_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!copied_json.contains("source_url"));
        assert!(!copied_json.contains("subtopic"));
        assert!(!copied_json.contains("section"));
        let redo = service.run(&redo_id).await.unwrap();
        assert_eq!(redo.groups[0].material.topic, "测试主题");
        assert_eq!(redo.groups[0].questions.len(), QUIZ_PER_GROUP);
    }

    #[tokio::test]
    async fn saved_groups_support_history_without_new_codex_generation() {
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

        let columns = sqlx::query("PRAGMA table_info(data_analysis_groups)")
            .fetch_all(&pool)
            .await
            .unwrap();
        let columns = columns
            .iter()
            .map(|row| row.try_get::<String, _>("name").unwrap())
            .collect::<HashSet<_>>();
        assert!(columns.contains("topic"));
        assert!(columns.contains("group_json"));
        assert!(!columns.contains("source_url"));
        assert!(!columns.contains("section"));

        let materials = TOPICS
            .iter()
            .take(4)
            .map(|(topic, _)| Material {
                topic: (*topic).into(),
                text: "本年度业务量1200亿元，比上年增长15.2%。".repeat(8),
            })
            .collect::<Vec<_>>();

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
            sqlx::query("INSERT INTO data_analysis_groups(id,topic,group_json,created_at_ms) VALUES(?,?,?,?)")
                .bind(Uuid::now_v7().to_string())
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
        // Repeating a completed paper produces a new blank, separately graded
        // attempt with exactly the same material, questions and options.
        let repeat_id = service.retry(&run_id).await.expect("retry finished paper");
        assert_ne!(repeat_id, run_id);
        let repeated = service.run(&repeat_id).await.expect("start repeated paper");
        assert_eq!(repeated.answers, vec![None; 20]);
        assert_eq!(repeated.groups[0].material, active.groups[0].material);
        assert_eq!(
            repeated.groups[0].questions[0].question,
            active.groups[0].questions[0].question
        );
        assert_eq!(
            repeated.groups[0].questions[0].options,
            active.groups[0].questions[0].options
        );
        assert_eq!(repeated.duration_seconds, QUIZ_SECONDS);
        assert!(
            service.retry(&repeat_id).await.is_err(),
            "active repeat cannot be cloned"
        );
        service
            .answer(&repeat_id, 1, 0)
            .await
            .expect("repeat answer");
        let repeat_result = service.submit(&repeat_id).await.expect("submit repeat");
        assert_eq!(repeat_result.correct, 0);
        assert_eq!(service.result(&run_id).await.unwrap().correct, 1);
        let third_id = service.retry(&repeat_id).await.expect("repeat from repeat");
        let third = service.run(&third_id).await.unwrap();
        assert_eq!(third.answers, vec![None; 20]);
        let before = service.unfinished().await.expect("server discovery");
        assert!(before.iter().any(|item| item.id == third_id));
        let ipad = service.claim(&third_id).await.expect("iPad claim");
        assert_eq!(ipad.run.current_question, 0);
        service
            .save_position(&third_id, 7, &ipad.control_token)
            .await
            .expect("save server position");
        service
            .answer_controlled(&third_id, 1, 1, Some(&ipad.control_token))
            .await
            .expect("iPad answer");
        let phone = service.claim(&third_id).await.expect("phone takeover");
        assert_ne!(phone.control_token, ipad.control_token);
        assert_eq!(phone.run.current_question, 7);
        assert_eq!(phone.run.answers[0], Some(1));
        assert!(
            service
                .save_position(&third_id, 8, &ipad.control_token)
                .await
                .is_err(),
            "stale iPad position rejected"
        );
        assert!(
            service
                .answer_controlled(&third_id, 2, 1, Some(&ipad.control_token))
                .await
                .is_err(),
            "stale iPad answer rejected"
        );
        assert!(
            service
                .pause_toggle_controlled(&third_id, Some(&ipad.control_token))
                .await
                .is_err(),
            "stale iPad pause rejected"
        );
        assert!(
            service
                .submit_controlled(&third_id, Some(&ipad.control_token))
                .await
                .is_err(),
            "stale iPad submit rejected"
        );
        let paused_phone = service
            .pause_toggle_controlled(&third_id, Some(&phone.control_token))
            .await
            .expect("phone pause");
        assert!(paused_phone.paused_remaining_ms.is_some());
        let on_other_device = service.unfinished().await.unwrap();
        let pending = on_other_device
            .iter()
            .find(|item| item.id == third_id)
            .unwrap();
        assert_eq!(pending.current_question, 7);
        assert_eq!(pending.answered, 1);
        assert!(pending.paused_remaining_ms.is_some());
        assert!(
            service
                .answer_controlled(&third_id, 2, 1, Some(&phone.control_token))
                .await
                .is_err(),
            "paused paper cannot accept answers"
        );
        let after_reload = service
            .claim(&third_id)
            .await
            .expect("same paper after reload");
        assert!(after_reload.run.paused_remaining_ms.is_some());
        assert_eq!(after_reload.run.current_question, 7);
        assert_eq!(after_reload.run.answers[0], Some(1));
        let resumed = service
            .pause_toggle_controlled(&third_id, Some(&after_reload.control_token))
            .await
            .expect("continue on phone");
        assert!(resumed.paused_remaining_ms.is_none());
        service
            .answer_controlled(&third_id, 2, 1, Some(&after_reload.control_token))
            .await
            .expect("new lease can answer");
        assert!(
            service.claim(&run_id).await.is_err(),
            "submitted paper cannot be reclaimed"
        );
        assert!(service.result(&third_id).await.is_err());
        let attempt_entries = service.history().await.unwrap();
        assert_eq!(
            attempt_entries
                .iter()
                .find(|entry| entry.id == run_id)
                .unwrap()
                .attempt_no,
            1
        );
        assert_eq!(
            attempt_entries
                .iter()
                .find(|entry| entry.id == repeat_id)
                .unwrap()
                .attempt_no,
            2
        );
        let historical = service.result(&run_id).await.expect("historical review");
        assert_eq!(historical.materials.len(), 4);
        assert_eq!(historical.questions.len(), 20);
        let (source_count, group_count) = service.counts().await.unwrap();
        // Newly generated material is self-contained: no official-source cache.
        assert_eq!((source_count, group_count), (0, 4));
    }

    #[test]
    fn themes_change_by_paper_and_prefer_unused_recent_topics() {
        let themes = exam_theme_materials();
        let first = order_exam_themes(themes.clone(), &[], 123456789);
        let another = order_exam_themes(themes.clone(), &[], 987654321);
        assert_ne!(
            first
                .iter()
                .take(QUIZ_GROUPS)
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            another
                .iter()
                .take(QUIZ_GROUPS)
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            "different generation IDs should produce different theme choices"
        );
        let previous = first
            .iter()
            .take(QUIZ_GROUPS)
            .map(|m| m.topic.clone())
            .collect::<Vec<_>>();
        let second = order_exam_themes(themes.clone(), &[previous.clone()], 20261023);
        assert!(second
            .iter()
            .take(QUIZ_GROUPS)
            .all(|m| !previous.contains(&m.topic)));
        let second_topics = second
            .iter()
            .take(QUIZ_GROUPS)
            .map(|m| m.topic.clone())
            .collect::<Vec<_>>();
        let third = order_exam_themes(themes, &[second_topics.clone(), previous.clone()], 20261024);
        assert!(third
            .iter()
            .take(QUIZ_GROUPS)
            .all(|m| !second_topics.contains(&m.topic) && !previous.contains(&m.topic)));
        assert_eq!(
            third
                .iter()
                .take(QUIZ_GROUPS)
                .map(|m| m.topic.as_str())
                .collect::<HashSet<_>>()
                .len(),
            QUIZ_GROUPS
        );
        assert_eq!(
            order_exam_themes(exam_theme_materials(), &[previous.clone()], 42)
                .iter()
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            order_exam_themes(exam_theme_materials(), &[previous], 42)
                .iter()
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            "retries within one preparation must keep the chosen order"
        );
    }

    #[tokio::test]
    async fn recent_theme_history_excludes_replays_and_includes_unfinished_papers() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mut root_id = String::new();
        for (index, topic) in ["农业", "工业能源", "国内消费"].iter().enumerate() {
            let id = Uuid::now_v7().to_string();
            if index == 0 {
                root_id = id.clone();
            }
            let group = QuestionGroup {
                version: 4,
                material: Material {
                    topic: (*topic).to_owned(),
                    text: "模拟材料".into(),
                },
                questions: Vec::new(),
            };
            sqlx::query(
                "INSERT INTO data_analysis_runs
                 (id,groups_json,answers_json,status,created_at_ms)
                 VALUES(?,?,?,'ready',?)",
            )
            .bind(id)
            .bind(serde_json::to_string(&vec![group; QUIZ_GROUPS]).unwrap())
            .bind("[]")
            .bind(index as i64 + 1)
            .execute(&pool)
            .await
            .unwrap();
        }
        let replay = QuestionGroup {
            version: 4,
            material: Material {
                topic: "虚假的最近主题".into(),
                text: "模拟材料".into(),
            },
            questions: Vec::new(),
        };
        sqlx::query(
            "INSERT INTO data_analysis_runs
             (id,groups_json,answers_json,status,created_at_ms,origin_run_id)
             VALUES(?,?,?,'ready',?,?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(serde_json::to_string(&vec![replay; QUIZ_GROUPS]).unwrap())
        .bind("[]")
        .bind(99_i64)
        .bind(root_id)
        .execute(&pool)
        .await
        .unwrap();

        let service = DataAnalysisService::new(pool, None).unwrap();
        let recent = service.recent_paper_themes().await.unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0], vec!["国内消费"; QUIZ_GROUPS]);
        assert_eq!(recent[1], vec!["工业能源"; QUIZ_GROUPS]);
        assert_eq!(recent[2], vec!["农业"; QUIZ_GROUPS]);
        assert!(!recent
            .iter()
            .flatten()
            .any(|topic| topic == "虚假的最近主题"));
    }

    #[test]
    fn topic_seeds_do_not_depend_on_official_articles() {
        let topics = exam_theme_materials();
        assert_eq!(topics.len(), TOPICS.len());
        assert!(topics.iter().all(|m| m.text.is_empty()));
        let chosen = select_material_batch(&topics, &[], &HashSet::new(), QUIZ_GROUPS);
        assert_eq!(chosen.len(), QUIZ_GROUPS);
        assert_eq!(
            chosen
                .iter()
                .map(|m| m.topic.as_str())
                .collect::<HashSet<_>>()
                .len(),
            QUIZ_GROUPS
        );
    }
}
