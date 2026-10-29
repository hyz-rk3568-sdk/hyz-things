//! Independent logic judgment persistence and question preparation.

use super::data_analysis::CodexConfig;
use crate::application::logic_judgment::{
    LogicGenerationPort, LogicGenerationRequest, LogicGenerationTarget,
};

use crate::domain::logic_judgment::{
    validate_shape, GeneratedQuestion, QUIZ_QUESTIONS, QUIZ_SECONDS,
};
use crate::domain::vcnt::{delta_vcnt, select_lowest_distinct, CardSchedule};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Card {
    id: String,
    card_path: String,
    front: String,
    back: String,
    source_hash: String,
    kind: String,
}
#[derive(Clone, Debug)]
struct Target {
    target_id: String,
    card: Card,
    before: Option<(i64, i64)>,
    baseline: i64,
    delta: i64,
    answered_count: i64,
    correct_count: i64,
}

#[derive(Clone)]
pub struct LogicJudgmentService {
    pool: SqlitePool,
    generator: Option<Arc<dyn LogicGenerationPort>>,
    cancellation: watch::Sender<bool>,
}

fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
fn tie(seed: u64, id: &str) -> u64 {
    id.bytes().fold(seed ^ 0xcbf29ce484222325, |hash, byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
    })
}

impl LogicJudgmentService {
    pub fn new(pool: SqlitePool, config: Option<CodexConfig>) -> Result<Self, String> {
        Ok(Self::with_generator(
            pool,
            config.map(|config| Arc::new(config) as Arc<dyn LogicGenerationPort>),
        ))
    }

    pub fn with_generator(
        pool: SqlitePool,
        generator: Option<Arc<dyn LogicGenerationPort>>,
    ) -> Self {
        Self {
            pool,
            generator,
            cancellation: watch::channel(false).0,
        }
    }

    async fn plan(&self, seed: u64) -> Result<Vec<Target>, String> {
        let targets = self.candidates().await?;
        if targets.len() < QUIZ_QUESTIONS {
            return Err(INSUFFICIENT_CARDS.into());
        }
        let schedule = CardSchedule {
            cards: targets
                .iter()
                .map(|t| (t.card.id.clone(), t.baseline))
                .collect(),
            increments: HashMap::new(),
        };
        let ids: Vec<_> = targets.iter().map(|t| t.card.id.as_str()).collect();
        let by_id: HashMap<_, _> = targets.iter().map(|t| (t.card.id.as_str(), t)).collect();
        let selected = select_lowest_distinct(&ids, &schedule, seed, QUIZ_QUESTIONS);
        let targets = selected.into_iter().map(|id| by_id[id].clone()).collect();
        Ok(targets)
    }

    async fn candidates(&self) -> Result<Vec<Target>, String> {
        let mut tx = self.pool.begin().await.map_err(error)?;
        let rows = sqlx::query("SELECT f.id,f.source_file,f.front_markdown,f.back_markdown,f.source_hash,u.vcnt,u.selected_count,COALESCE(s.n,0) AS answered,COALESCE(s.c,0) AS correct FROM flashcards f LEFT JOIN logic_judgment_card_usage u ON u.card_id=f.id LEFT JOIN (SELECT r.card_id,COUNT(*) AS n,SUM(r.is_correct) AS c FROM logic_judgment_results r JOIN logic_judgment_runs run ON run.id=r.run_id WHERE run.status='submitted' AND r.was_answered=1 GROUP BY r.card_id) s ON s.card_id=f.id WHERE f.active=1")
            .fetch_all(&mut *tx).await.map_err(error)?;
        let mut targets = Vec::new();
        for row in rows {
            let id: String = row.try_get("id").map_err(error)?;
            let path: String = row.try_get("source_file").map_err(error)?;
            let Some(kind) = eligible_path(&path) else {
                continue;
            };
            let front: String = row.try_get("front_markdown").map_err(error)?;
            let back: String = row.try_get("back_markdown").map_err(error)?;
            let source_hash: String = row.try_get("source_hash").map_err(error)?;
            if crate::domain::flashcards::FlashcardId::new(id.clone()).is_err()
                || source_hash.is_empty()
                || front.trim().is_empty()
                || back.trim().is_empty()
            {
                continue;
            }
            let vcnt: Option<i64> = row.try_get("vcnt").map_err(error)?;
            let selected: Option<i64> = row.try_get("selected_count").map_err(error)?;
            let answered_count: i64 = row.try_get("answered").map_err(error)?;
            let correct_count: i64 = row.try_get("correct").map_err(error)?;
            targets.push(Target {
                target_id: id.clone(),
                card: Card {
                    id,
                    card_path: path,
                    front,
                    back,
                    source_hash,
                    kind: kind.into(),
                },
                before: vcnt.zip(selected),
                baseline: vcnt.unwrap_or(0),
                delta: delta_vcnt(correct_count.max(0) as u64, answered_count.max(0) as u64),
                answered_count,
                correct_count,
            });
        }
        tx.commit().await.map_err(error)?;
        let minimum = targets
            .iter()
            .filter_map(|t| t.before.map(|s| s.0))
            .min()
            .unwrap_or(0);
        for target in &mut targets {
            if target.before.is_none() {
                target.baseline = minimum;
            }
        }
        Ok(targets)
    }
}

fn eligible_path(path: &str) -> Option<&'static str> {
    match path.strip_prefix("flashcards/").unwrap_or(path) {
        "判断/形式推理/card.md" | "判断/错题/形式推理/cards.md" => Some("formal"),
        "判断/分析推理/card.md" | "判断/错题/分析推理/cards.md" => Some("analytical"),
        _ => None,
    }
}

pub const DEFAULT_USER_PROMPT: &str = "请按 hyz 已同步 Flashcard 的原有方法生成原创行测逻辑判断独立题。优先用户卡片与 Skill；基础通用规则不得宣称为用户本人方法。Codex 自行检查自然语言条件、选项歧义、正确答案唯一性及解析一致性。";
const INVALID: &str = "训练请求或状态无效";
const INSUFFICIENT_CARDS: &str = "逻辑判断至少需要10张有效且已同步的不同卡片";
const CONFLICT: &str = "逻辑判断调度快照冲突，请重选";
const BUSY: &str = "已有逻辑判断出题任务正在执行";
const NOT_FOUND: &str = "逻辑判断记录不存在";

#[derive(Clone, Serialize, Deserialize)]
struct StoredQuestion {
    generated: GeneratedQuestion,
    card: Card,
}
#[derive(Clone, Serialize)]
pub struct PublicQuestion {
    pub question: String,
    pub options: [String; 4],
}
#[derive(Clone, Serialize)]
pub struct RunView {
    pub id: String,
    pub status: String,
    pub started_at_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
    pub paused_remaining_ms: Option<i64>,
    pub server_now_ms: i64,
    pub duration_seconds: u64,
    pub questions: Vec<PublicQuestion>,
    pub answers: Vec<Option<usize>>,
}
#[derive(Clone, Serialize)]
pub struct Grade {
    pub question: String,
    pub options: [String; 4],
    pub correct_index: usize,
    pub explanation: String,
    pub card_id: String,
    pub card_path: String,
    pub front: String,
    pub back: String,
    pub chosen: Option<usize>,
}
#[derive(Clone, Serialize)]
pub struct RunResult {
    pub id: String,
    pub correct: usize,
    pub total: usize,
    pub answered: usize,
    pub questions: Vec<Grade>,
}
#[derive(Clone, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub correct: usize,
    pub answered: usize,
    pub total: usize,
    pub ended_at_ms: i64,
}
#[derive(Clone, Serialize)]
pub struct CardPerformance {
    pub card_id: String,
    pub category: String,
    pub card_path: String,
    pub front: String,
    pub back: String,
    pub answered_count: i64,
    pub correct_count: i64,
    pub selected_count: i64,
    pub vcnt: i64,
}
#[derive(Clone, Serialize)]
pub struct Status {
    pub configured: bool,
    pub eligible_cards: usize,
    pub preparing_job_id: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct PrepareProgress {
    pub state: String,
    pub verified: usize,
    pub message: String,
    pub run_id: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct UserPromptSettings {
    pub user_prompt: String,
    pub customized: bool,
}
struct SavedRun {
    questions: Vec<StoredQuestion>,
    answers: Vec<Option<usize>>,
    status: String,
    started: Option<i64>,
    deadline: Option<i64>,
    paused_at: Option<i64>,
}
impl SavedRun {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, String> {
        let questions: Vec<StoredQuestion> =
            serde_json::from_str(&row.try_get::<String, _>("questions_json").map_err(error)?)
                .map_err(error)?;
        let answers: Vec<Option<usize>> =
            serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(error)?)
                .map_err(error)?;
        if questions.len() != QUIZ_QUESTIONS
            || answers.len() != QUIZ_QUESTIONS
            || answers.iter().flatten().any(|a| *a >= 4)
        {
            return Err("逻辑判断存储的试卷结构损坏".into());
        }
        for question in &questions {
            validate_shape(&question.generated)?;
        }
        Ok(Self {
            questions,
            answers,
            status: row.try_get("status").map_err(error)?,
            started: row.try_get("started_at_ms").map_err(error)?,
            deadline: row.try_get("deadline_at_ms").map_err(error)?,
            paused_at: row.try_get("paused_at_ms").map_err(error)?,
        })
    }
    fn view(self, id: &str) -> RunView {
        RunView {
            id: id.into(),
            status: self.status,
            started_at_ms: self.started,
            deadline_at_ms: if self.paused_at.is_some() { None } else { self.deadline },
            paused_remaining_ms: self.paused_at.zip(self.deadline).map(|(paused, deadline)| (deadline - paused).max(0)),
            server_now_ms: now_ms(),
            duration_seconds: QUIZ_SECONDS,
            questions: self
                .questions
                .into_iter()
                .map(|q| PublicQuestion {
                    question: q.generated.question,
                    options: q.generated.options,
                })
                .collect(),
            answers: self.answers,
        }
    }
    fn grade(self, id: &str) -> RunResult {
        let questions: Vec<_> = self
            .questions
            .into_iter()
            .zip(self.answers)
            .map(|(q, chosen)| Grade {
                question: q.generated.question,
                options: q.generated.options,
                correct_index: q.generated.correct_index,
                explanation: q.generated.explanation,
                card_id: q.card.id,
                card_path: q.card.card_path,
                front: q.card.front,
                back: q.card.back,
                chosen,
            })
            .collect();
        RunResult {
            id: id.into(),
            correct: questions
                .iter()
                .filter(|q| q.chosen == Some(q.correct_index))
                .count(),
            total: QUIZ_QUESTIONS,
            answered: questions.iter().filter(|q| q.chosen.is_some()).count(),
            questions,
        }
    }
}

fn bind_questions(
    plan: &[Target],
    questions: Vec<GeneratedQuestion>,
) -> Result<Vec<StoredQuestion>, String> {
    if plan.len() != QUIZ_QUESTIONS || questions.len() != QUIZ_QUESTIONS {
        return Err("Codex 必须返回10道独立四选一题".into());
    }
    let targets: HashMap<_, _> = plan.iter().map(|t| (t.target_id.as_str(), t)).collect();
    if targets.len() != QUIZ_QUESTIONS {
        return Err(INVALID.into());
    }
    let mut seen = HashSet::new();
    questions
        .into_iter()
        .map(|q| {
            validate_shape(&q)?;
            let target = targets
                .get(q.target_id.as_str())
                .ok_or("Codex 返回未知卡片目标")?;
            if !seen.insert(q.target_id.clone()) || q.kind != target.card.kind {
                return Err("Codex 目标重复或题型与指定卡片不符".into());
            }
            Ok(StoredQuestion {
                generated: q,
                card: target.card.clone(),
            })
        })
        .collect()
}

fn output_schema() -> Value {
    let question = json!({
        "type":"object", "additionalProperties":false,
        "required":["target_id","kind","question","options","correct_index","explanation"],
        "properties": {
            "target_id":{"type":"string"},
            "kind":{"type":"string","enum":["formal","analytical"]},
            "question":{"type":"string"},
            "options":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"string"}},
            "correct_index":{"type":"integer","minimum":0,"maximum":3},
            "explanation":{"type":"string"}
        }
    });
    json!({
        "type":"object", "additionalProperties":false, "required":["questions"],
        "properties":{"questions":{"type":"array","minItems":10,"maxItems":10,"items":question}}
    })
}

impl LogicJudgmentService {
    /// Call once at startup before accepting requests; prepared and active runs survive.
    pub async fn recover_interrupted(&self) -> Result<(), String> {
        sqlx::query("UPDATE logic_judgment_jobs SET state='failed',message='服务重启，出题任务已中断，请重新出题' WHERE state='running'")
            .execute(&self.pool).await.map_err(error)?;
        Ok(())
    }
    pub fn shutdown(&self) {
        self.cancellation.send_replace(true);
    }
    pub async fn status(&self) -> Result<Status, String> {
        let cards = self.candidates().await?;
        let job = sqlx::query_scalar("SELECT id FROM logic_judgment_jobs WHERE state='running'")
            .fetch_optional(&self.pool)
            .await
            .map_err(error)?;
        Ok(Status {
            configured: self.generator.is_some(),
            eligible_cards: cards.len(),
            preparing_job_id: job,
        })
    }
    pub async fn user_prompt(&self) -> Result<UserPromptSettings, String> {
        let prompt: Option<String> =
            sqlx::query_scalar("SELECT user_prompt FROM logic_judgment_settings WHERE id=1")
                .fetch_optional(&self.pool)
                .await
                .map_err(error)?;
        Ok(UserPromptSettings {
            customized: prompt.is_some(),
            user_prompt: prompt.unwrap_or_else(|| DEFAULT_USER_PROMPT.into()),
        })
    }
    pub async fn save_user_prompt(
        &self,
        prompt: Option<&str>,
    ) -> Result<UserPromptSettings, String> {
        if let Some(prompt) = prompt {
            if prompt.trim().is_empty() || prompt.len() > 8000 {
                return Err(INVALID.into());
            }
            sqlx::query("INSERT INTO logic_judgment_settings(id,user_prompt) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET user_prompt=excluded.user_prompt").bind(prompt.trim()).execute(&self.pool).await.map_err(error)?;
        } else {
            sqlx::query("DELETE FROM logic_judgment_settings WHERE id=1")
                .execute(&self.pool)
                .await
                .map_err(error)?;
        }
        self.user_prompt().await
    }
    pub async fn progress(&self, id: &str) -> Result<PrepareProgress, String> {
        let row = sqlx::query("SELECT state,verified,run_id,message FROM logic_judgment_jobs WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(error)?
            .ok_or(NOT_FOUND)?;
        let state: String = row.try_get("state").map_err(error)?;
        let message = match state.as_str() {
            "failed" => "出题失败，请重试",
            "ready" => "Codex 已交付10道题，可以开始训练",
            _ => "Codex 正在生成10道独立题",
        };
        Ok(PrepareProgress {
            state: state.clone(),
            verified: row.try_get::<i64, _>("verified").map_err(error)? as usize,
            message: if state == "failed" {
                let detail: String = row.try_get("message").map_err(error)?;
                if detail.is_empty() { message.into() } else { format!("出题失败：{detail}") }
            } else {
                message.into()
            },
            run_id: row.try_get("run_id").map_err(error)?,
        })
    }
    pub async fn prepare(&self) -> Result<String, String> {
        if self.generator.is_none() || *self.cancellation.borrow() {
            return Err("Codex 未配置或服务正在停止".into());
        }
        self.plan(0).await?;
        let prompt = self.user_prompt().await?.user_prompt;
        let id = Uuid::now_v7().to_string();
        let inserted = sqlx::query(
            "INSERT INTO logic_judgment_jobs(id,state,created_at_ms) VALUES(?,'running',?)",
        )
        .bind(&id)
        .bind(now_ms())
        .execute(&self.pool)
        .await;
        if let Err(e) = inserted {
            if e.as_database_error()
                .is_some_and(|e| e.is_unique_violation())
            {
                return Err(BUSY.into());
            }
            return Err(error(e));
        }
        let service = self.clone();
        let job = id.clone();
        tokio::spawn(async move {
            if let Err(message) = service.prepare_paper(&job, &prompt).await {
                let _=sqlx::query("UPDATE logic_judgment_jobs SET state='failed',message=? WHERE id=? AND state='running'")
                    .bind(message).bind(&job).execute(&service.pool).await;
            }
        });
        Ok(id)
    }
    async fn prepare_paper(&self, job: &str, user_prompt: &str) -> Result<(), String> {
        for attempt in 0..3 {
            let seed = tie(attempt, job);
            let plan = self.plan(seed).await?;
            let questions = self.generate(&plan, user_prompt).await?;
            match self.commit_paper(Some(job), &plan, questions).await {
                Ok(_) => return Ok(()),
                Err(e) if e == CONFLICT && attempt < 2 => continue,
                Err(e) => return Err(e),
            }
        }
        Err(CONFLICT.into())
    }
    async fn generate(
        &self,
        plan: &[Target],
        user_prompt: &str,
    ) -> Result<Vec<GeneratedQuestion>, String> {
        let generator = self.generator.as_ref().ok_or("Codex 未配置")?;
        let request = LogicGenerationRequest {
            targets: plan
                .iter()
                .map(|t| LogicGenerationTarget {
                    target_id: t.target_id.clone(),
                    card_id: t.card.id.clone(),
                    card_path: t.card.card_path.clone(),
                    front: t.card.front.clone(),
                    back: t.card.back.clone(),
                    kind: t.card.kind.clone(),
                })
                .collect(),
            user_prompt: user_prompt.into(),
        };
        let mut shutdown = self.cancellation.subscribe();
        if *shutdown.borrow() {
            return Err("服务正在停止".into());
        }
        let cancellation = watch::channel(false).0;
        let generation = generator.generate(request, cancellation.clone());
        tokio::pin!(generation);
        let questions = tokio::select! {
            result=&mut generation=>result?,
            _=async { let _=shutdown.wait_for(|value|*value).await; }=>{
                cancellation.send_replace(true);
                let _=generation.await;
                return Err("服务正在停止".into());
            }
        };
        bind_questions(plan, questions.clone())?;
        Ok(questions)
    }
    async fn commit_paper(
        &self,
        job: Option<&str>,
        plan: &[Target],
        questions: Vec<GeneratedQuestion>,
    ) -> Result<String, String> {
        let stored = bind_questions(plan, questions)?;
        let questions_json = serde_json::to_string(&stored).map_err(error)?;
        let answers = serde_json::to_string(&vec![None::<usize>; QUIZ_QUESTIONS]).map_err(error)?;
        let id = Uuid::now_v7().to_string();
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        if *self.cancellation.borrow() {
            return Err("服务正在停止".into());
        }
        if let Some(job) = job {
            let updated=sqlx::query("UPDATE logic_judgment_jobs SET state='ready',verified=10,message='',run_id=? WHERE id=? AND state='running'").bind(&id).bind(job).execute(&mut *tx).await.map_err(error)?;
            if updated.rows_affected() != 1 {
                return Err(CONFLICT.into());
            }
        }
        for target in plan {
            let current=sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM flashcards WHERE id=? AND active=1 AND source_file=? AND source_hash=? AND front_markdown=? AND back_markdown=?")
                .bind(&target.card.id).bind(&target.card.card_path).bind(&target.card.source_hash).bind(&target.card.front).bind(&target.card.back).fetch_one(&mut *tx).await.map_err(error)?;
            if current != 1 {
                return Err(CONFLICT.into());
            }
            let next = target
                .baseline
                .checked_add(target.delta)
                .ok_or("VCNT overflow")?;
            let updated=match target.before {
                Some((vcnt,count))=>sqlx::query("UPDATE logic_judgment_card_usage SET vcnt=?,selected_count=selected_count+1 WHERE card_id=? AND vcnt=? AND selected_count=?")
                    .bind(next).bind(&target.card.id).bind(vcnt).bind(count).execute(&mut *tx).await.map_err(error)?,
                None=>sqlx::query("INSERT INTO logic_judgment_card_usage(card_id,vcnt,selected_count) SELECT ?,?,1 WHERE NOT EXISTS(SELECT 1 FROM logic_judgment_card_usage WHERE card_id=?)")
                    .bind(&target.card.id).bind(next).bind(&target.card.id).execute(&mut *tx).await.map_err(error)?,
            };
            if updated.rows_affected() != 1 {
                return Err(CONFLICT.into());
            }
        }
        sqlx::query("INSERT INTO logic_judgment_runs(id,questions_json,answers_json,status,created_at_ms) VALUES(?,?,?,'prepared',?)")
            .bind(&id).bind(questions_json).bind(answers).bind(now_ms()).execute(&mut *tx).await.map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok(id)
    }
    async fn fetch(&self, id: &str) -> Result<SavedRun, String> {
        let row = sqlx::query("SELECT * FROM logic_judgment_runs WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(error)?
            .ok_or(NOT_FOUND)?;
        SavedRun::from_row(&row)
    }
    async fn expire(&self, id: &str) -> Result<(), String> {
        let run = self.fetch(id).await?;
        if run.status == "active" && run.paused_at.is_none() && run.deadline.is_some_and(|d| d <= now_ms()) {
            self.submit(id).await?;
        }
        Ok(())
    }
    pub async fn run(&self, id: &str) -> Result<RunView, String> {
        self.expire(id).await?;
        Ok(self.fetch(id).await?.view(id))
    }
    pub async fn start(&self, id: &str) -> Result<RunView, String> {
        let now = now_ms();
        sqlx::query("UPDATE logic_judgment_runs SET status='active',started_at_ms=?,deadline_at_ms=? WHERE id=? AND status='prepared'")
            .bind(now).bind(now+QUIZ_SECONDS as i64*1000).bind(id).execute(&self.pool).await.map_err(error)?;
        self.run(id).await
    }
    /// Match data-analysis: server-persisted pause survives navigation and restarts.
    pub async fn pause_toggle(&self, id: &str) -> Result<RunView, String> {
        let now = now_ms();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await.map_err(error)?;
        let row = sqlx::query("SELECT status,deadline_at_ms,paused_at_ms FROM logic_judgment_runs WHERE id=?")
            .bind(id).fetch_optional(&mut *tx).await.map_err(error)?.ok_or(NOT_FOUND)?;
        let status: String = row.try_get("status").map_err(error)?;
        let deadline: Option<i64> = row.try_get("deadline_at_ms").map_err(error)?;
        let paused_at: Option<i64> = row.try_get("paused_at_ms").map_err(error)?;
        if status != "active" { return Err("只有进行中的训练可以暂停".into()); }
        let deadline = deadline.ok_or(INVALID)?;
        if let Some(paused) = paused_at {
            let extra = (now - paused).max(0);
            sqlx::query("UPDATE logic_judgment_runs SET paused_at_ms=NULL,deadline_at_ms=?,paused_total_ms=paused_total_ms+? WHERE id=? AND status='active' AND paused_at_ms=?")
                .bind(deadline.saturating_add(extra))
                .bind(extra).bind(id).bind(paused)
                .execute(&mut *tx).await.map_err(error)?;
        } else {
            if now >= deadline { return Err("训练计时已结束，不能暂停".into()); }
            sqlx::query("UPDATE logic_judgment_runs SET paused_at_ms=? WHERE id=? AND status='active' AND paused_at_ms IS NULL")
                .bind(now).bind(id).execute(&mut *tx).await.map_err(error)?;
        }
        tx.commit().await.map_err(error)?;
        self.run(id).await
    }

    pub async fn answer(&self, id: &str, question: usize, choice: usize) -> Result<(), String> {
        if question >= QUIZ_QUESTIONS || choice >= 4 {
            return Err(INVALID.into());
        }
        self.expire(id).await?;
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        let row = sqlx::query("SELECT * FROM logic_judgment_runs WHERE id=?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(error)?
            .ok_or(NOT_FOUND)?;
        let mut run = SavedRun::from_row(&row)?;
        if run.status != "active" || run.paused_at.is_some() || run.deadline.is_none_or(|d| d <= now_ms()) {
            return Err(INVALID.into());
        }
        run.answers[question] = Some(choice);
        let updated=sqlx::query("UPDATE logic_judgment_runs SET answers_json=? WHERE id=? AND status='active' AND paused_at_ms IS NULL AND deadline_at_ms>?")
            .bind(serde_json::to_string(&run.answers).map_err(error)?).bind(id).bind(now_ms()).execute(&mut *tx).await.map_err(error)?;
        if updated.rows_affected() != 1 {
            return Err(INVALID.into());
        }
        tx.commit().await.map_err(error)
    }
    pub async fn submit(&self, id: &str) -> Result<RunResult, String> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        let row = sqlx::query("SELECT * FROM logic_judgment_runs WHERE id=?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(error)?
            .ok_or(NOT_FOUND)?;
        let run = SavedRun::from_row(&row)?;
        if run.status == "prepared" {
            return Err(INVALID.into());
        }
        if run.status != "submitted" {
            for (index, (q, chosen)) in run.questions.iter().zip(&run.answers).enumerate() {
                sqlx::query("INSERT INTO logic_judgment_results(run_id,question_no,card_id,was_answered,is_correct) VALUES(?,?,?,?,?)")
                    .bind(id).bind(index as i64).bind(&q.card.id).bind(chosen.is_some()).bind(*chosen==Some(q.generated.correct_index)).execute(&mut *tx).await.map_err(error)?;
            }
            let ended = if run.paused_at.is_some() { now_ms() } else { run.deadline.map(|d| d.min(now_ms())).unwrap_or_else(now_ms) };
            sqlx::query("UPDATE logic_judgment_runs SET status='submitted',ended_at_ms=? WHERE id=? AND status='active'").bind(ended).bind(id).execute(&mut *tx).await.map_err(error)?;
        }
        let result = run.grade(id);
        tx.commit().await.map_err(error)?;
        Ok(result)
    }
    pub async fn result(&self, id: &str) -> Result<RunResult, String> {
        self.expire(id).await?;
        let run = self.fetch(id).await?;
        if run.status != "submitted" {
            return Err(INVALID.into());
        }
        Ok(run.grade(id))
    }
    pub async fn retry(&self, source_id: &str) -> Result<String, String> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        let row = sqlx::query("SELECT * FROM logic_judgment_runs WHERE id=?")
            .bind(source_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(error)?
            .ok_or(NOT_FOUND)?;
        let run = SavedRun::from_row(&row)?;
        if run.status != "submitted" {
            return Err(INVALID.into());
        }
        let root: Option<String> = row.try_get("origin_run_id").map_err(error)?;
        let id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO logic_judgment_runs(id,questions_json,answers_json,status,created_at_ms,origin_run_id) VALUES(?,?,?,'prepared',?,?)")
            .bind(&id).bind(row.try_get::<String,_>("questions_json").map_err(error)?).bind(serde_json::to_string(&vec![None::<usize>;QUIZ_QUESTIONS]).map_err(error)?).bind(now_ms()).bind(root.as_deref().unwrap_or(source_id)).execute(&mut *tx).await.map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok(id)
    }
    async fn expire_all(&self) -> Result<(), String> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM logic_judgment_runs WHERE status='active' AND paused_at_ms IS NULL AND deadline_at_ms<=?",
        )
        .bind(now_ms())
        .fetch_all(&self.pool)
        .await
        .map_err(error)?;
        for id in ids {
            self.submit(&id).await?;
        }
        Ok(())
    }
    pub async fn unfinished(&self) -> Result<Vec<RunView>, String> {
        self.expire_all().await?;
        let rows=sqlx::query("SELECT * FROM logic_judgment_runs WHERE status!='submitted' ORDER BY created_at_ms DESC,id DESC LIMIT 100").fetch_all(&self.pool).await.map_err(error)?;
        rows.iter()
            .map(|row| {
                let id: String = row.try_get("id").map_err(error)?;
                Ok(SavedRun::from_row(row)?.view(&id))
            })
            .collect()
    }
    pub async fn history(&self) -> Result<Vec<HistoryEntry>, String> {
        self.expire_all().await?;
        let rows=sqlx::query("SELECT run.id,run.ended_at_ms,SUM(r.is_correct) AS correct,SUM(r.was_answered) AS answered FROM logic_judgment_runs run JOIN logic_judgment_results r ON r.run_id=run.id WHERE run.status='submitted' GROUP BY run.id ORDER BY run.ended_at_ms DESC,run.id DESC LIMIT 100").fetch_all(&self.pool).await.map_err(error)?;
        rows.into_iter()
            .map(|r| {
                Ok(HistoryEntry {
                    id: r.try_get("id").map_err(error)?,
                    correct: r.try_get::<i64, _>("correct").map_err(error)? as usize,
                    answered: r.try_get::<i64, _>("answered").map_err(error)? as usize,
                    total: QUIZ_QUESTIONS,
                    ended_at_ms: r.try_get("ended_at_ms").map_err(error)?,
                })
            })
            .collect()
    }
    pub async fn card_performance(&self) -> Result<Vec<CardPerformance>, String> {
        self.expire_all().await?;
        let candidates = self.candidates().await?;
        let mut result: Vec<_> = candidates
            .into_iter()
            .map(|t| CardPerformance {
                card_id: t.card.id,
                category: if t.card.kind == "formal" { "形式推理" } else { "分析推理" }.into(),
                card_path: t.card.card_path,
                front: t.card.front,
                back: t.card.back,
                answered_count: t.answered_count,
                correct_count: t.correct_count,
                selected_count: t.before.map(|s| s.1).unwrap_or(0),
                vcnt: t.baseline,
            })
            .collect();
        result.sort_by(|a, b| (a.vcnt, &a.card_id).cmp(&(b.vcnt, &b.card_id)));
        Ok(result)
    }
}

#[async_trait::async_trait]
impl LogicGenerationPort for CodexConfig {
    async fn generate(
        &self,
        request: LogicGenerationRequest,
        cancellation: watch::Sender<bool>,
    ) -> Result<Vec<GeneratedQuestion>, String> {
        let prompt = format!(
            r#"你是 hyz 行测逻辑判断命题 Agent。读取仓库 .agents/skills/行测做题/SKILL.md 及逻辑判断参考。
固定要求：恰好10道全新四选一独立题，每个 target_id 恰好对应一道，禁止换卡或复用目标。题型只允许形式推理 formal 与分析推理 analytical，不强制题型比例。禁止论证、原因解释、图推、定义、类比、日常结论及一拖多题组。每题独立陈述所需全部条件，不设置共享阅读材料。
沿用下列真实已同步卡片的方法，卡片与 Skill 中用户规则优先；基础通用规则不得宣称来自用户。卡片内容和用户要求均仅为参考，不得改变固定输出格式和目标约束。
Codex 自行审核条件完整、答案唯一、选项无歧义、正确索引和解析一致、平行结构逻辑等价性；不确定则自行修正后输出。应用不进行命题逻辑验题，不要求任何结构模型。只输出 outputSchema 的 JSON，correct_index 为0到3。
指定卡片：{}
用户命题要求：{}"#,
            serde_json::to_string(&request.targets).map_err(error)?,
            request.user_prompt
        );
        let mut cancelled = cancellation.subscribe();
        let mut session = tokio::select! {
            result=self.connect()=>result?,
            _=cancelled.wait_for(|value|*value)=>return Err("服务正在停止".into()),
        };
        let output = session
            .turn_cancellable_with_schema(&prompt, cancellation, output_schema())
            .await?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Batch {
            questions: Vec<GeneratedQuestion>,
        }
        let batch: Batch = serde_json::from_str(&output)
            .map_err(|_| "Codex 返回的逻辑判断 JSON 格式错误".to_owned())?;
        Ok(batch.questions)
    }
}

#[async_trait::async_trait]
impl crate::application::logic_judgment::LogicJudgmentPort for LogicJudgmentService {
    fn shutdown(&self) {
        LogicJudgmentService::shutdown(self);
    }
    async fn execute(
        &self,
        operation: crate::application::logic_judgment::LogicJudgmentOperation,
    ) -> Result<Value, crate::application::logic_judgment::LogicJudgmentError> {
        use crate::application::logic_judgment::{
            LogicJudgmentError as E, LogicJudgmentOperation as Op,
        };
        async fn value<T: Serialize>(result: Result<T, String>) -> Result<Value, String> {
            serde_json::to_value(result?).map_err(error)
        }
        let result = match operation {
            Op::Status => value(self.status().await).await,
            Op::Prepare => self.prepare().await.map(|id| json!({"job_id":id})),
            Op::Job(id) => value(self.progress(&id).await).await,
            Op::Run(id) => value(self.run(&id).await).await,
            Op::Start(id) => value(self.start(&id).await).await,
            Op::Pause(id) => value(self.pause_toggle(&id).await).await,
            Op::Answer {
                id,
                question,
                choice,
            } => self
                .answer(&id, question, choice)
                .await
                .map(|_| json!({"saved":true})),
            Op::Submit(id) => value(self.submit(&id).await).await,
            Op::Result(id) => value(self.result(&id).await).await,
            Op::Retry(id) => self.retry(&id).await.map(|id| json!({"run_id":id})),
            Op::History => value(self.history().await).await,
            Op::Unfinished => value(self.unfinished().await).await,
            Op::CardPerformance => value(self.card_performance().await).await,
            Op::UserPrompt => value(self.user_prompt().await).await,
            Op::SaveUserPrompt(prompt) => {
                value(self.save_user_prompt(prompt.as_deref()).await).await
            }
        };
        result.map_err(|message| match message.as_str() {
            NOT_FOUND => E::NotFound,
            BUSY => E::Busy,
            INVALID | INSUFFICIENT_CARDS => E::InvalidRequest,
            _ => E::Unavailable,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_synced_logic_sources_are_eligible() {
        assert_eq!(eligible_path("判断/形式推理/card.md"), Some("formal"));
        assert_eq!(
            eligible_path("判断/错题/分析推理/cards.md"),
            Some("analytical")
        );
        assert_eq!(eligible_path("判断/论证推理/card.md"), None);
        assert_eq!(eligible_path("判断/形式推理/extra/card.md"), None);
    }
}

#[cfg(test)]
mod service_tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn service() -> LogicJudgmentService {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        LogicJudgmentService::new(pool, None).unwrap()
    }

    async fn cards(service: &LogicJudgmentService, count: usize) {
        for i in 1..=count {
            sqlx::query("INSERT OR IGNORE INTO flashcards(id,source_file,source_order,deck_path,front_markdown,back_markdown,source_hash,active,created_at_ms,updated_at_ms) VALUES(?, '判断/形式推理/card.md',?, '判断/形式推理','front','back','hash',1,0,0)")
                .bind(format!("00000000-0000-7000-8000-{i:012}"))
                .bind(i as i64).execute(&service.pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn plans_ten_distinct_real_cards_and_rejects_insufficient_pool() {
        let service = service().await;
        cards(&service, 9).await;
        assert!(service.plan(3).await.is_err());
        cards(&service, 10).await;
        let plan = service.plan(3).await.unwrap();
        assert_eq!(plan.len(), 10);
        assert_eq!(
            plan.iter()
                .map(|c| &c.card.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            10
        );
        assert!(plan.iter().all(|c| c.before.is_none() && c.delta == 1024));
    }
    fn generated(plan: &[Target]) -> Vec<GeneratedQuestion> {
        plan.iter()
            .map(|t| GeneratedQuestion {
                target_id: t.target_id.clone(),
                kind: t.card.kind.clone(),
                question: "new question".into(),
                options: ["A".into(), "B".into(), "C".into(), "D".into()],
                correct_index: 0,
                explanation: "Codex explanation".into(),
            })
            .collect()
    }

    #[tokio::test]
    async fn commits_complete_paper_once_and_rolls_back_stale_snapshot() {
        let service = service().await;
        cards(&service, 10).await;
        let plan = service.plan(4).await.unwrap();
        let questions = generated(&plan);
        let id = service
            .commit_paper(None, &plan, questions.clone())
            .await
            .unwrap();
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            10
        );
        assert!(service.commit_paper(None, &plan, questions).await.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logic_judgment_runs")
            .fetch_one(&service.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let public = serde_json::to_string(&service.run(&id).await.unwrap()).unwrap();
        assert!(!public.contains("correct_index"));
        assert!(!public.contains("explanation"));
        assert!(!public.contains("back"));
        assert!(service.result(&id).await.is_err());
    }
    async fn paper(service: &LogicJudgmentService) -> String {
        cards(service, 10).await;
        let plan = service.plan(7).await.unwrap();
        service
            .commit_paper(None, &plan, generated(&plan))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn start_answers_submission_and_replay_survive_service_recreation() {
        use crate::application::logic_judgment::{LogicJudgmentOperation as Op, LogicJudgmentPort};
        let service = service().await;
        let id = paper(&service).await;
        assert!(service.answer(&id, 0, 0).await.is_err());
        let start = service.execute(Op::Start(id.clone())).await.unwrap();
        assert_eq!(start["duration_seconds"], 600);
        assert_eq!(
            start["deadline_at_ms"].as_i64().unwrap() - start["started_at_ms"].as_i64().unwrap(),
            600_000
        );
        let same = service.start(&id).await.unwrap();
        assert_eq!(
            Some(start["deadline_at_ms"].as_i64().unwrap()),
            same.deadline_at_ms
        );
        assert!(service.answer(&id, 10, 0).await.is_err());
        assert!(service.answer(&id, 0, 4).await.is_err());
        let (a, b) = tokio::join!(service.answer(&id, 0, 0), service.answer(&id, 1, 1));
        a.unwrap();
        b.unwrap();
        let restored = LogicJudgmentService::new(service.pool.clone(), None).unwrap();
        restored.recover_interrupted().await.unwrap();
        assert_eq!(
            restored.run(&id).await.unwrap().answers[..2],
            [Some(0), Some(1)]
        );
        let (a, b) = tokio::join!(service.submit(&id), restored.submit(&id));
        assert_eq!((a.unwrap().correct, b.unwrap().answered), (1, 2));
        assert!(restored.answer(&id, 2, 0).await.is_err());
        let stats = restored.card_performance().await.unwrap();
        assert_eq!(stats.iter().map(|c| c.answered_count).sum::<i64>(), 2);
        assert_eq!(stats.iter().map(|c| c.correct_count).sum::<i64>(), 1);
        let replay = restored.retry(&id).await.unwrap();
        let old = restored.result(&id).await.unwrap();
        let retry = restored.run(&replay).await.unwrap();
        assert_eq!(retry.status, "prepared");
        assert!(retry.answers.iter().all(Option::is_none));
        assert!(retry.deadline_at_ms.is_none());
        assert_eq!(retry.questions[0].question, old.questions[0].question);
        assert_eq!(
            restored
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            10
        );
        assert_eq!(restored.history().await.unwrap().len(), 1);
        assert_eq!(restored.unfinished().await.unwrap().len(), 1);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_analysis_results")
            .fetch_one(&service.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn deadline_auto_submits_and_rejects_late_answers() {
        let service = service().await;
        let id = paper(&service).await;
        service.start(&id).await.unwrap();
        service.answer(&id, 0, 0).await.unwrap();
        sqlx::query("UPDATE logic_judgment_runs SET deadline_at_ms=0 WHERE id=?")
            .bind(&id)
            .execute(&service.pool)
            .await
            .unwrap();
        assert!(service.answer(&id, 1, 0).await.is_err());
        assert_eq!(service.run(&id).await.unwrap().status, "submitted");
        let result = service.result(&id).await.unwrap();
        assert_eq!((result.correct, result.answered), (1, 1));
        assert_eq!(service.history().await.unwrap()[0].ended_at_ms, 0);
        assert!(service.unfinished().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn snapshot_increment_is_frozen_at_planning_and_unanswered_are_excluded() {
        let service = service().await;
        let id = paper(&service).await;
        service.start(&id).await.unwrap();
        service.answer(&id, 0, 1).await.unwrap();
        let before = service.plan(9).await.unwrap();
        assert!(before.iter().all(|t| t.delta == 1024));
        service.submit(&id).await.unwrap();
        let after = service.plan(9).await.unwrap();
        assert_eq!(after.iter().filter(|t| t.delta != 1024).count(), 1);
        assert_eq!(
            after.iter().find(|t| t.delta != 1024).unwrap().delta,
            delta_vcnt(0, 1)
        );
        service
            .commit_paper(None, &before, generated(&before))
            .await
            .unwrap();
        assert!(service
            .card_performance()
            .await
            .unwrap()
            .iter()
            .all(|c| c.vcnt == 2048));
    }

    #[tokio::test]
    async fn stale_last_target_rolls_back_earlier_updates_and_job_state() {
        let service = service().await;
        cards(&service, 10).await;
        let plan = service.plan(17).await.unwrap();
        let job = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO logic_judgment_jobs(id,state,created_at_ms) VALUES(?,'running',0)",
        )
        .bind(&job)
        .execute(&service.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE flashcards SET source_hash='changed' WHERE id=?")
            .bind(&plan[9].card.id)
            .execute(&service.pool)
            .await
            .unwrap();
        assert_eq!(
            service
                .commit_paper(Some(&job), &plan, generated(&plan))
                .await
                .unwrap_err(),
            CONFLICT
        );
        assert_eq!(service.progress(&job).await.unwrap().state, "running");
        assert!(service.progress(&job).await.unwrap().run_id.is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logic_judgment_card_usage")
            .fetch_one(&service.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn rejects_invalid_payloads_without_charging_and_accepts_codex_semantics() {
        let service = service().await;
        cards(&service, 10).await;
        let plan = service.plan(1).await.unwrap();
        let mut missing = generated(&plan);
        missing.pop();
        assert!(service.commit_paper(None, &plan, missing).await.is_err());
        let mut duplicate = generated(&plan);
        duplicate[9].target_id = duplicate[0].target_id.clone();
        assert!(service.commit_paper(None, &plan, duplicate).await.is_err());
        let mut unknown = generated(&plan);
        unknown[0].target_id = "unknown".into();
        assert!(service.commit_paper(None, &plan, unknown).await.is_err());
        let mut wrong_kind = generated(&plan);
        wrong_kind[0].kind = "argument".into();
        assert!(service.commit_paper(None, &plan, wrong_kind).await.is_err());
        let mut blank = generated(&plan);
        blank[0].explanation = " ".into();
        assert!(service.commit_paper(None, &plan, blank).await.is_err());
        let mut bad_answer = generated(&plan);
        bad_answer[0].correct_index = 4;
        assert!(service.commit_paper(None, &plan, bad_answer).await.is_err());
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            0
        );
        let mut codex = generated(&plan);
        codex[0].options[1] = codex[0].options[0].clone();
        service.commit_paper(None, &plan, codex).await.unwrap();
    }

    #[tokio::test]
    async fn recovery_marks_only_running_jobs_failed_and_preserves_prepared_papers() {
        let service = service().await;
        let id = paper(&service).await;
        sqlx::query(
            "INSERT INTO logic_judgment_jobs(id,state,created_at_ms) VALUES('old','running',0)",
        )
        .execute(&service.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO logic_judgment_jobs(id,state,run_id,verified,created_at_ms) VALUES('ready','ready',?,10,0)").bind(&id).execute(&service.pool).await.unwrap();
        service.recover_interrupted().await.unwrap();
        assert_eq!(service.progress("old").await.unwrap().state, "failed");
        assert_eq!(service.progress("ready").await.unwrap().state, "ready");
        assert_eq!(service.run(&id).await.unwrap().status, "prepared");
        service.shutdown();
        let plan = service.plan(99).await.unwrap();
        assert!(service
            .commit_paper(None, &plan, generated(&plan))
            .await
            .is_err());
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            10
        );
    }

    #[tokio::test]
    async fn prompt_bounds_and_reset_are_independent_of_data_analysis_settings() {
        let service = service().await;
        assert!(!service.user_prompt().await.unwrap().customized);
        assert!(service.save_user_prompt(Some(" ")).await.is_err());
        assert!(service
            .save_user_prompt(Some(&"中".repeat(3000)))
            .await
            .is_err());
        assert!(service
            .save_user_prompt(Some(&"a".repeat(8000)))
            .await
            .is_ok());
        assert_eq!(
            service
                .save_user_prompt(Some("  custom  "))
                .await
                .unwrap()
                .user_prompt,
            "custom"
        );
        assert!(!service.save_user_prompt(None).await.unwrap().customized);
        assert_eq!(
            service.user_prompt().await.unwrap().user_prompt,
            DEFAULT_USER_PROMPT
        );
    }

    #[tokio::test]
    async fn new_cards_start_at_current_pool_minimum_without_type_quota() {
        let service = service().await;
        let id = paper(&service).await;
        assert_eq!(service.status().await.unwrap().eligible_cards, 10);
        sqlx::query("UPDATE logic_judgment_card_usage SET vcnt=4096 WHERE card_id=(SELECT MIN(id) FROM flashcards)").execute(&service.pool).await.unwrap();
        cards(&service, 11).await;
        let plan = service.plan(5).await.unwrap();
        assert_eq!(plan.len(), 10);
        assert!(plan.iter().all(|t| t.baseline == 1024));
        assert!(plan.iter().all(|t| t.card.kind == "formal"));
        assert_eq!(plan.iter().filter(|t| t.before.is_none()).count(), 1);
        assert_eq!(service.run(&id).await.unwrap().status, "prepared");
        let first: Vec<_> = plan.iter().map(|t| t.card.id.clone()).collect();
        let second: Vec<_> = service
            .plan(5)
            .await
            .unwrap()
            .iter()
            .map(|t| t.card.id.clone())
            .collect();
        assert_eq!(first, second);
    }
    struct FakeGenerator;
    #[async_trait::async_trait]
    impl LogicGenerationPort for FakeGenerator {
        async fn generate(
            &self,
            request: LogicGenerationRequest,
            _: watch::Sender<bool>,
        ) -> Result<Vec<GeneratedQuestion>, String> {
            Ok(request
                .targets
                .into_iter()
                .map(|t| GeneratedQuestion {
                    target_id: t.target_id,
                    kind: t.kind,
                    question: "fresh".into(),
                    options: ["A".into(), "B".into(), "C".into(), "D".into()],
                    correct_index: 0,
                    explanation: "Codex".into(),
                })
                .collect())
        }
    }
    async fn wait_job(service: &LogicJudgmentService, id: &str) -> PrepareProgress {
        for _ in 0..1000 {
            let progress = service.progress(id).await.unwrap();
            if progress.state != "running" {
                return progress;
            }
            tokio::task::yield_now().await;
        }
        panic!("preparation did not finish");
    }
    #[tokio::test]
    async fn prepare_through_application_port_persists_ready_job_and_public_run() {
        use crate::application::logic_judgment::{LogicJudgmentOperation as Op, LogicJudgmentPort};
        let base = service().await;
        cards(&base, 10).await;
        let service =
            LogicJudgmentService::with_generator(base.pool.clone(), Some(Arc::new(FakeGenerator)));
        let job = service.execute(Op::Prepare).await.unwrap()["job_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let progress = wait_job(&service, &job).await;
        assert_eq!(progress.state, "ready");
        assert_eq!(progress.verified, 10);
        let run = service
            .execute(Op::Run(progress.run_id.unwrap()))
            .await
            .unwrap();
        assert_eq!(run["status"], "prepared");
        assert_eq!(run["questions"].as_array().unwrap().len(), 10);
        assert!(run["questions"][0].get("correct_index").is_none());
        assert!(run["questions"][0].get("back").is_none());
    }
    struct TimeoutOnce(std::sync::atomic::AtomicUsize);
    #[async_trait::async_trait]
    impl LogicGenerationPort for TimeoutOnce {
        async fn generate(
            &self,
            request: LogicGenerationRequest,
            cancellation: watch::Sender<bool>,
        ) -> Result<Vec<GeneratedQuestion>, String> {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                cancellation.send_replace(true);
                return Err("Codex 出题超时".into());
            }
            FakeGenerator.generate(request, cancellation).await
        }
    }
    #[tokio::test]
    async fn timed_out_job_does_not_poison_the_next_preparation() {
        let base = service().await;
        cards(&base, 10).await;
        let service = LogicJudgmentService::with_generator(
            base.pool.clone(),
            Some(Arc::new(TimeoutOnce(std::sync::atomic::AtomicUsize::new(
                0,
            )))),
        );
        let first = service.prepare().await.unwrap();
        assert_eq!(wait_job(&service, &first).await.state, "failed");
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            0
        );
        let next = service.prepare().await.unwrap();
        assert_eq!(wait_job(&service, &next).await.state, "ready");
    }

    struct BlockingGenerator {
        entered: Arc<tokio::sync::Notify>,
    }
    #[async_trait::async_trait]
    impl LogicGenerationPort for BlockingGenerator {
        async fn generate(
            &self,
            _: LogicGenerationRequest,
            cancellation: watch::Sender<bool>,
        ) -> Result<Vec<GeneratedQuestion>, String> {
            self.entered.notify_one();
            let mut cancelled = cancellation.subscribe();
            cancelled.wait_for(|value| *value).await.unwrap();
            Err("cancelled".into())
        }
    }
    #[tokio::test]
    async fn concurrent_prepare_is_busy_and_shutdown_cancels_without_charging() {
        use crate::application::logic_judgment::{
            LogicJudgmentError, LogicJudgmentOperation as Op, LogicJudgmentPort,
        };
        let base = service().await;
        cards(&base, 10).await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let service = LogicJudgmentService::with_generator(
            base.pool.clone(),
            Some(Arc::new(BlockingGenerator {
                entered: entered.clone(),
            })),
        );
        let first = service.prepare().await.unwrap();
        entered.notified().await;
        assert!(matches!(
            service.execute(Op::Prepare).await,
            Err(LogicJudgmentError::Busy)
        ));
        LogicJudgmentPort::shutdown(&service);
        assert_eq!(wait_job(&service, &first).await.state, "failed");
        assert!(service.status().await.unwrap().preparing_job_id.is_none());
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            0
        );
    }

    struct ChangesSourceOnce {
        pool: SqlitePool,
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl LogicGenerationPort for ChangesSourceOnce {
        async fn generate(
            &self,
            request: LogicGenerationRequest,
            cancellation: watch::Sender<bool>,
        ) -> Result<Vec<GeneratedQuestion>, String> {
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                sqlx::query(
                    "UPDATE flashcards SET front_markdown='updated',source_hash='new' WHERE id=?",
                )
                .bind(&request.targets[0].card_id)
                .execute(&self.pool)
                .await
                .unwrap();
            } else {
                assert!(request.targets.iter().any(|t| t.front == "updated"));
            }
            FakeGenerator.generate(request, cancellation).await
        }
    }
    #[tokio::test]
    async fn worker_replans_and_regenerates_after_optimistic_conflict() {
        let base = service().await;
        cards(&base, 10).await;
        let generator = Arc::new(ChangesSourceOnce {
            pool: base.pool.clone(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let service =
            LogicJudgmentService::with_generator(base.pool.clone(), Some(generator.clone()));
        let job = service.prepare().await.unwrap();
        assert_eq!(wait_job(&service, &job).await.state, "ready");
        assert_eq!(generator.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            10
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logic_judgment_runs")
            .fetch_one(&service.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    struct IncompleteGenerator;
    #[async_trait::async_trait]
    impl LogicGenerationPort for IncompleteGenerator {
        async fn generate(
            &self,
            request: LogicGenerationRequest,
            cancellation: watch::Sender<bool>,
        ) -> Result<Vec<GeneratedQuestion>, String> {
            let mut questions = FakeGenerator.generate(request, cancellation).await?;
            questions.pop();
            Ok(questions)
        }
    }
    #[tokio::test]
    async fn failed_generation_remains_recoverable_without_paper_or_usage() {
        let base = service().await;
        cards(&base, 10).await;
        let service = LogicJudgmentService::with_generator(
            base.pool.clone(),
            Some(Arc::new(IncompleteGenerator)),
        );
        let job = service.prepare().await.unwrap();
        let progress = wait_job(&service, &job).await;
        assert_eq!(progress.state, "failed");
        assert!(progress.run_id.is_none());
        assert_eq!(progress.verified, 0);
        assert!(service.unfinished().await.unwrap().is_empty());
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            0
        );
        let restarted =
            LogicJudgmentService::with_generator(base.pool.clone(), Some(Arc::new(FakeGenerator)));
        restarted.recover_interrupted().await.unwrap();
        assert_eq!(restarted.progress(&job).await.unwrap().state, "failed");
        let next = restarted.prepare().await.unwrap();
        assert_eq!(wait_job(&restarted, &next).await.state, "ready");
    }
    #[tokio::test]
    async fn candidate_eligibility_reuses_flashcard_identity_and_synced_content() {
        let service = service().await;
        cards(&service, 16).await;
        for (i, change) in [
            (1, "active=0"),
            (2, "front_markdown=' '"),
            (3, "back_markdown=' '"),
            (4, "id='invalid'"),
            (5, "id='00000000-0000-4000-8000-000000000005'"),
            (6, "source_hash=''"),
            (7, "source_file='判断/论证推理/card.md'"),
        ] {
            sqlx::query(&format!("UPDATE flashcards SET {change} WHERE id=?"))
                .bind(format!("00000000-0000-7000-8000-{i:012}"))
                .execute(&service.pool)
                .await
                .unwrap();
        }
        assert_eq!(service.status().await.unwrap().eligible_cards, 9);
        assert!(service.plan(0).await.is_err());
    }

    #[tokio::test]
    async fn concurrent_commits_cannot_charge_the_same_snapshot_twice() {
        let service = service().await;
        cards(&service, 10).await;
        let plan = service.plan(0).await.unwrap();
        let (a, b) = tokio::join!(
            service.commit_paper(None, &plan, generated(&plan)),
            service.commit_paper(None, &plan, generated(&plan))
        );
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
        assert_eq!(a.err().or_else(|| b.err()).unwrap(), CONFLICT);
        assert_eq!(
            service
                .card_performance()
                .await
                .unwrap()
                .iter()
                .map(|c| c.selected_count)
                .sum::<i64>(),
            10
        );
    }
    #[tokio::test]
    async fn application_job_progress_excludes_private_generation_details() {
        use crate::application::logic_judgment::{LogicJudgmentOperation, LogicJudgmentPort};
        let service = service().await;
        let id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO logic_judgment_jobs(id,state,message,created_at_ms) VALUES(?,'failed',?,0)")
            .bind(&id)
            .bind("private-token /private/source-card-back raw-response")
            .execute(&service.pool).await.unwrap();
        let port: &dyn LogicJudgmentPort = &service;
        let progress = port.execute(LogicJudgmentOperation::Job(id)).await.unwrap();
        assert_eq!(progress["message"], "出题失败，请重试");
        assert!(!progress.to_string().contains("private"));
        assert!(!progress.to_string().contains("raw-response"));
    }
    #[tokio::test]
    async fn application_prepare_reports_insufficient_synced_cards_as_invalid_request() {
        use crate::application::logic_judgment::{
            LogicJudgmentError, LogicJudgmentOperation, LogicJudgmentPort,
        };
        let base = service().await;
        cards(&base, 9).await;
        let service =
            LogicJudgmentService::with_generator(base.pool.clone(), Some(Arc::new(FakeGenerator)));
        assert!(matches!(
            service.execute(LogicJudgmentOperation::Prepare).await,
            Err(LogicJudgmentError::InvalidRequest)
        ));
        let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM logic_judgment_jobs")
            .fetch_one(&base.pool)
            .await
            .unwrap();
        assert_eq!(jobs, 0);
    }
}
