//! Independent logic judgment persistence and question preparation.

use super::data_analysis::CodexConfig;
use crate::application::quantitative_relations::{
    QuantitativeGenerationPort, QuantitativeGenerationRequest, QuantitativeGenerationTarget,
};

use crate::domain::quantitative_relations::{
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
pub struct QuantitativeRelationsService {
    pool: SqlitePool,
    generator: Option<Arc<dyn QuantitativeGenerationPort>>,
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

impl QuantitativeRelationsService {
    pub fn new(pool: SqlitePool, config: Option<CodexConfig>) -> Result<Self, String> {
        Ok(Self::with_generator(
            pool,
            config.map(|config| Arc::new(config) as Arc<dyn QuantitativeGenerationPort>),
        ))
    }

    pub fn with_generator(
        pool: SqlitePool,
        generator: Option<Arc<dyn QuantitativeGenerationPort>>,
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
        let rows = sqlx::query("SELECT f.id,f.source_file,f.front_markdown,f.back_markdown,f.source_hash,u.vcnt,u.selected_count,COALESCE(s.n,0) AS answered,COALESCE(s.c,0) AS correct FROM flashcards f LEFT JOIN quantitative_relations_card_usage u ON u.card_id=f.id LEFT JOIN (SELECT r.card_id,COUNT(*) AS n,SUM(r.is_correct) AS c FROM quantitative_relations_results r JOIN quantitative_relations_runs run ON run.id=r.run_id WHERE run.status='submitted' AND r.was_answered=1 GROUP BY r.card_id) s ON s.card_id=f.id WHERE f.active=1")
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
        "数量/经济利润/cards.md"
        | "数量/工程效率/cards.md"
        | "数量/行程/相对运动/cards.md"
        | "数量/行程/综合应用/cards.md"
        | "数量/溶液/cards.md"
        | "数量/最值/cards.md"
        | "数量/排列组合/基础/cards.md"
        | "数量/排列组合/插空与同素分堆/cards.md"
        | "数量/排列组合/特殊套路/cards.md"
        | "数量/概率/cards.md"
        | "数量/周期/cards.md" => Some("quantitative"),
        _ => None,
    }
}

pub const DEFAULT_USER_PROMPT: &str = "题目接近国考、省考数量关系风格，难度中等偏上，条件与数字自然；干扰选项合理，优先考查方法迁移和快速计算，避免机械换数字，解析简洁。";
const INVALID: &str = "训练请求或状态无效";
const INSUFFICIENT_CARDS: &str = "数量关系至少需要10张有效且已同步的不同卡片";
const CONFLICT: &str = "数量关系调度快照冲突，请重选";
const BUSY: &str = "已有数量关系出题任务正在执行";
const NOT_FOUND: &str = "数量关系记录不存在";

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
            return Err("数量关系存储的试卷结构损坏".into());
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
            deadline_at_ms: if self.paused_at.is_some() {
                None
            } else {
                self.deadline
            },
            paused_remaining_ms: self
                .paused_at
                .zip(self.deadline)
                .map(|(paused, deadline)| (deadline - paused).max(0)),
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

#[allow(dead_code)]
fn output_schema() -> Value {
    let question = json!({
        "type":"object", "additionalProperties":false,
        "required":["target_id","kind","question","options","correct_index","explanation"],
        "properties": {
            "target_id":{"type":"string"},
            "kind":{"type":"string","enum":["quantitative"]},
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

impl QuantitativeRelationsService {
    /// Call once at startup before accepting requests; prepared and active runs survive.
    pub async fn recover_interrupted(&self) -> Result<(), String> {
        sqlx::query("UPDATE quantitative_relations_jobs SET state='failed',message='服务重启，出题任务已中断，请重新出题' WHERE state='running'")
            .execute(&self.pool).await.map_err(error)?;
        Ok(())
    }
    pub fn shutdown(&self) {
        self.cancellation.send_replace(true);
    }
    pub async fn status(&self) -> Result<Status, String> {
        let cards = self.candidates().await?;
        let job =
            sqlx::query_scalar("SELECT id FROM quantitative_relations_jobs WHERE state='running'")
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
        let prompt: Option<String> = sqlx::query_scalar(
            "SELECT user_prompt FROM quantitative_relations_settings WHERE id=1",
        )
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
            sqlx::query("INSERT INTO quantitative_relations_settings(id,user_prompt) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET user_prompt=excluded.user_prompt").bind(prompt.trim()).execute(&self.pool).await.map_err(error)?;
        } else {
            sqlx::query("DELETE FROM quantitative_relations_settings WHERE id=1")
                .execute(&self.pool)
                .await
                .map_err(error)?;
        }
        self.user_prompt().await
    }
    pub async fn progress(&self, id: &str) -> Result<PrepareProgress, String> {
        let row =
            sqlx::query("SELECT state,verified,run_id FROM quantitative_relations_jobs WHERE id=?")
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
            state,
            verified: row.try_get::<i64, _>("verified").map_err(error)? as usize,
            message: message.into(),
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
            "INSERT INTO quantitative_relations_jobs(id,state,created_at_ms) VALUES(?,'running',?)",
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
                let _=sqlx::query("UPDATE quantitative_relations_jobs SET state='failed',message=? WHERE id=? AND state='running'")
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
        let request = QuantitativeGenerationRequest {
            targets: plan
                .iter()
                .map(|t| QuantitativeGenerationTarget {
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
            let updated=sqlx::query("UPDATE quantitative_relations_jobs SET state='ready',verified=10,message='',run_id=? WHERE id=? AND state='running'").bind(&id).bind(job).execute(&mut *tx).await.map_err(error)?;
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
                Some((vcnt,count))=>sqlx::query("UPDATE quantitative_relations_card_usage SET vcnt=?,selected_count=selected_count+1 WHERE card_id=? AND vcnt=? AND selected_count=?")
                    .bind(next).bind(&target.card.id).bind(vcnt).bind(count).execute(&mut *tx).await.map_err(error)?,
                None=>sqlx::query("INSERT INTO quantitative_relations_card_usage(card_id,vcnt,selected_count) SELECT ?,?,1 WHERE NOT EXISTS(SELECT 1 FROM quantitative_relations_card_usage WHERE card_id=?)")
                    .bind(&target.card.id).bind(next).bind(&target.card.id).execute(&mut *tx).await.map_err(error)?,
            };
            if updated.rows_affected() != 1 {
                return Err(CONFLICT.into());
            }
        }
        sqlx::query("INSERT INTO quantitative_relations_runs(id,questions_json,answers_json,status,created_at_ms) VALUES(?,?,?,'prepared',?)")
            .bind(&id).bind(questions_json).bind(answers).bind(now_ms()).execute(&mut *tx).await.map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok(id)
    }
    async fn fetch(&self, id: &str) -> Result<SavedRun, String> {
        let row = sqlx::query("SELECT * FROM quantitative_relations_runs WHERE id=?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(error)?
            .ok_or(NOT_FOUND)?;
        SavedRun::from_row(&row)
    }
    async fn expire(&self, id: &str) -> Result<(), String> {
        let run = self.fetch(id).await?;
        if run.status == "active"
            && run.paused_at.is_none()
            && run.deadline.is_some_and(|d| d <= now_ms())
        {
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
        sqlx::query("UPDATE quantitative_relations_runs SET status='active',started_at_ms=?,deadline_at_ms=? WHERE id=? AND status='prepared'")
            .bind(now).bind(now+QUIZ_SECONDS as i64*1000).bind(id).execute(&self.pool).await.map_err(error)?;
        self.run(id).await
    }
    /// Match data-analysis: server-persisted pause survives navigation and restarts.
    pub async fn pause_toggle(&self, id: &str) -> Result<RunView, String> {
        let now = now_ms();
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(error)?;
        let row = sqlx::query(
            "SELECT status,deadline_at_ms,paused_at_ms FROM quantitative_relations_runs WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(error)?
        .ok_or(NOT_FOUND)?;
        let status: String = row.try_get("status").map_err(error)?;
        let deadline: Option<i64> = row.try_get("deadline_at_ms").map_err(error)?;
        let paused_at: Option<i64> = row.try_get("paused_at_ms").map_err(error)?;
        if status != "active" {
            return Err("只有进行中的训练可以暂停".into());
        }
        let deadline = deadline.ok_or(INVALID)?;
        if let Some(paused) = paused_at {
            let extra = (now - paused).max(0);
            sqlx::query("UPDATE quantitative_relations_runs SET paused_at_ms=NULL,deadline_at_ms=?,paused_total_ms=paused_total_ms+? WHERE id=? AND status='active' AND paused_at_ms=?")
                .bind(deadline.saturating_add(extra))
                .bind(extra).bind(id).bind(paused)
                .execute(&mut *tx).await.map_err(error)?;
        } else {
            if now >= deadline {
                return Err("训练计时已结束，不能暂停".into());
            }
            sqlx::query("UPDATE quantitative_relations_runs SET paused_at_ms=? WHERE id=? AND status='active' AND paused_at_ms IS NULL")
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
        let row = sqlx::query("SELECT * FROM quantitative_relations_runs WHERE id=?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(error)?
            .ok_or(NOT_FOUND)?;
        let mut run = SavedRun::from_row(&row)?;
        if run.status != "active"
            || run.paused_at.is_some()
            || run.deadline.is_none_or(|d| d <= now_ms())
        {
            return Err(INVALID.into());
        }
        run.answers[question] = Some(choice);
        let updated=sqlx::query("UPDATE quantitative_relations_runs SET answers_json=? WHERE id=? AND status='active' AND paused_at_ms IS NULL AND deadline_at_ms>?")
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
        let row = sqlx::query("SELECT * FROM quantitative_relations_runs WHERE id=?")
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
                sqlx::query("INSERT INTO quantitative_relations_results(run_id,question_no,card_id,was_answered,is_correct) VALUES(?,?,?,?,?)")
                    .bind(id).bind(index as i64).bind(&q.card.id).bind(chosen.is_some()).bind(*chosen==Some(q.generated.correct_index)).execute(&mut *tx).await.map_err(error)?;
            }
            let ended = if run.paused_at.is_some() {
                now_ms()
            } else {
                run.deadline.map(|d| d.min(now_ms())).unwrap_or_else(now_ms)
            };
            sqlx::query("UPDATE quantitative_relations_runs SET status='submitted',ended_at_ms=? WHERE id=? AND status='active'").bind(ended).bind(id).execute(&mut *tx).await.map_err(error)?;
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
        let row = sqlx::query("SELECT * FROM quantitative_relations_runs WHERE id=?")
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
        sqlx::query("INSERT INTO quantitative_relations_runs(id,questions_json,answers_json,status,created_at_ms,origin_run_id) VALUES(?,?,?,'prepared',?,?)")
            .bind(&id).bind(row.try_get::<String,_>("questions_json").map_err(error)?).bind(serde_json::to_string(&vec![None::<usize>;QUIZ_QUESTIONS]).map_err(error)?).bind(now_ms()).bind(root.as_deref().unwrap_or(source_id)).execute(&mut *tx).await.map_err(error)?;
        tx.commit().await.map_err(error)?;
        Ok(id)
    }
    async fn expire_all(&self) -> Result<(), String> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM quantitative_relations_runs WHERE status='active' AND paused_at_ms IS NULL AND deadline_at_ms<=?",
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
        let rows=sqlx::query("SELECT * FROM quantitative_relations_runs WHERE status!='submitted' ORDER BY created_at_ms DESC,id DESC LIMIT 100").fetch_all(&self.pool).await.map_err(error)?;
        rows.iter()
            .map(|row| {
                let id: String = row.try_get("id").map_err(error)?;
                Ok(SavedRun::from_row(row)?.view(&id))
            })
            .collect()
    }
    pub async fn history(&self) -> Result<Vec<HistoryEntry>, String> {
        self.expire_all().await?;
        let rows=sqlx::query("SELECT run.id,run.ended_at_ms,SUM(r.is_correct) AS correct,SUM(r.was_answered) AS answered FROM quantitative_relations_runs run JOIN quantitative_relations_results r ON r.run_id=run.id WHERE run.status='submitted' GROUP BY run.id ORDER BY run.ended_at_ms DESC,run.id DESC LIMIT 100").fetch_all(&self.pool).await.map_err(error)?;
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
                category: t
                    .card
                    .card_path
                    .split('/')
                    .take(2)
                    .collect::<Vec<_>>()
                    .join("/"),
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
impl QuantitativeGenerationPort for CodexConfig {
    async fn generate(
        &self,
        request: QuantitativeGenerationRequest,
        cancellation: watch::Sender<bool>,
    ) -> Result<Vec<GeneratedQuestion>, String> {
        // Follow the verified data-analysis generation contract: the server fixes
        // all target cards and checks shape/identity, while Codex checks mathematics.
        let targets_text = request
            .targets
            .iter()
            .map(|target| format!(
                "【目标 {id}】\n题型：{kind}\n卡片ID：{card_id}\n路径：{path}\n### Front\n{front}\n### Back\n{back}",
                id = target.target_id,
                kind = target.kind,
                card_id = target.card_id,
                path = target.card_path,
                front = target.front,
                back = target.back,
            ))
            .collect::<Vec<_>>()
            .join("\n\n");
        let prompt = format!(
            r#"【固定规则 / 应用端约束】
你是 hyz 行测数量关系命题 Agent。先阅读仓库 .agents/skills/行测做题/SKILL.md、references/数量关系.md 和 references/小P-数量关系-解题方法.md。
根据后端指定的已同步 Flashcard 命题，生成恰好10道原创四选一独立题，每张卡片恰好对应一道，target_id 不可更换和复用，kind 固定 quantitative。
只涵盖经济利润、工程效率、行程、溶液、最值、排列组合、概率、周期；排除几何、特殊题型、数字推理与共享材料题。题干只陈述独立完整的已知条件，不泄露思路、公式、答案。
遵循资料分析已验证的命题责任分工：应用只检验结构、题量、选项、目标唯一性，数学正确性和语义唯一性由 Codex 自检。

【本轮动态上下文】
所选10张卡由 SQLite 依据本模块 VCNT 排序产生，有方法卡也可有真实错题卡。每张必须生成一题；先识别相关方法再构思不同的完整题目，避免简单替换旧题数字。
Codex 自行验算每题数值、边界、单位、正确选项唯一性、解析与 correct_index 一致性；发现不确定必须先修正后输出。

【本轮固定 Flashcard】
{targets_text}

【用户出题要求 / 自定义】
{user_prompt}

只输出合法 JSON：{{"questions":[{{"target_id":"...","kind":"quantitative","question":"...","options":["...","...","...","..."],"correct_index":0,"explanation":"..."}}]}}，题目恰好10道，correct_index 从0到3，不要前言。"#,
            targets_text = targets_text,
            user_prompt = request.user_prompt,
        );
        let mut cancelled = cancellation.subscribe();
        let mut session = tokio::select! {
            result=self.connect()=>result?,
            _=cancelled.wait_for(|value|*value)=>return Err("服务正在停止".into()),
        };
        let output = session.turn_cancellable(&prompt, cancellation).await?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Batch {
            questions: Vec<GeneratedQuestion>,
        }
        let batch: Batch = serde_json::from_str(&output)
            .map_err(|_| "Codex 返回的数量关系 JSON 格式错误".to_owned())?;
        Ok(batch.questions)
    }
}

#[async_trait::async_trait]
impl crate::application::quantitative_relations::QuantitativeRelationsPort
    for QuantitativeRelationsService
{
    fn shutdown(&self) {
        QuantitativeRelationsService::shutdown(self);
    }
    async fn execute(
        &self,
        operation: crate::application::quantitative_relations::QuantitativeRelationsOperation,
    ) -> Result<Value, crate::application::quantitative_relations::QuantitativeRelationsError> {
        use crate::application::quantitative_relations::{
            QuantitativeRelationsError as E, QuantitativeRelationsOperation as Op,
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
mod quantitative_tests {
    use super::*;
    #[test]
    fn quantitative_pool_is_exactly_eleven_method_decks() {
        for path in [
            "数量/经济利润/cards.md",
            "数量/工程效率/cards.md",
            "数量/行程/相对运动/cards.md",
            "数量/行程/综合应用/cards.md",
            "数量/溶液/cards.md",
            "数量/最值/cards.md",
            "数量/排列组合/基础/cards.md",
            "数量/排列组合/插空与同素分堆/cards.md",
            "数量/排列组合/特殊套路/cards.md",
            "数量/概率/cards.md",
            "数量/周期/cards.md",
        ] {
            assert_eq!(eligible_path(path), Some("quantitative"));
        }
        for path in [
            "判断/形式推理/直言命题/cards.md",
            "数量/几何/cards.md",
            "数量/错题/cards.md",
        ] {
            assert_eq!(eligible_path(path), None);
        }
    }
}
