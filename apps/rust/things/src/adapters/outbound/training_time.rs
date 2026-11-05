//! SQLite-backed, server-authoritative effective learning time.
//! One active lease for the portal's administrator, independent of exam clocks.
use crate::application::training_time::{
    ClockSnapshot, DaySummary, StudyDateRange, StudyGoals, StudyTimeEvent, TimeStatistics,
    TrainingTimePort, DAY_MS, ZONE_OFFSET_MS,
};
use async_trait::async_trait;
use chrono::Duration;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

const GRACE_MS: i64 = 45_000;
const MODULES: [&str; 6] = [
    "calculation",
    "flashcards",
    "comprehensive",
    "logic",
    "quantitative",
    "question_bank",
];

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn day_start(ms: i64) -> i64 {
    (ms + ZONE_OFFSET_MS).div_euclid(DAY_MS) * DAY_MS - ZONE_OFFSET_MS
}

struct ClockState {
    status: String,
    module: String,
    phase: String,
    started_at_ms: Option<i64>,
    last_seen_at_ms: Option<i64>,
}
impl ClockState {
    fn from_row(row: sqlx::sqlite::SqliteRow) -> Self {
        Self {
            status: row.get("status"),
            module: row.get("module"),
            phase: row.get("phase"),
            started_at_ms: row.get("started_at_ms"),
            last_seen_at_ms: row.get("last_seen_at_ms"),
        }
    }
    fn credited_end(&self, now: i64) -> i64 {
        now.min(self.last_seen_at_ms.unwrap_or(now).saturating_add(GRACE_MS))
    }
}

#[derive(Clone)]
pub struct TrainingTimeService {
    pool: SqlitePool,
    gate: Arc<Mutex<()>>,
}

impl TrainingTimeService {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            gate: Arc::new(Mutex::new(())),
        }
    }

    async fn load(&self) -> Result<ClockState, String> {
        let row = sqlx::query("SELECT status,module,phase,started_at_ms,last_seen_at_ms FROM study_time_state WHERE id=1")
            .fetch_one(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(ClockState::from_row(row))
    }

    async fn close(
        tx: &mut Transaction<'_, Sqlite>,
        state: &ClockState,
        now: i64,
    ) -> Result<(), String> {
        if state.status == "running" {
            if let Some(start) = state.started_at_ms {
                let end = state.credited_end(now).max(start);
                if end > start {
                    sqlx::query("INSERT INTO study_time_segments(module,phase,started_at_ms,ended_at_ms,effective_ms) VALUES(?,?,?,?,?)")
                        .bind(&state.module).bind(&state.phase).bind(start).bind(end).bind(end-start)
                        .execute(&mut **tx).await.map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    async fn update_state(
        tx: &mut Transaction<'_, Sqlite>,
        status: &str,
        module: &str,
        phase: &str,
        started: Option<i64>,
        seen: Option<i64>,
    ) -> Result<(), String> {
        sqlx::query("UPDATE study_time_state SET status=?,module=?,phase=?,started_at_ms=?,last_seen_at_ms=?,revision=revision+1 WHERE id=1")
            .bind(status).bind(module).bind(phase).bind(started).bind(seen)
            .execute(&mut **tx).await.map_err(|e|e.to_string())?;
        Ok(())
    }

    pub async fn event(&self, event: StudyTimeEvent) -> Result<ClockSnapshot, String> {
        if event.event_id.is_empty()
            || event.event_id.len() > 128
            || !event
                .event_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("invalid event id".into());
        }
        if !["enter", "switch", "leave", "heartbeat"].contains(&event.action.as_str()) {
            return Err("invalid action".into());
        }
        if let Some(ref module) = event.module {
            if !MODULES.contains(&module.as_str()) {
                return Err("invalid module".into());
            }
        }
        if let Some(ref phase) = event.phase {
            if phase != "answer" && phase != "review" {
                return Err("invalid phase".into());
            }
        }
        if ["enter", "switch"].contains(&event.action.as_str()) && event.module.is_none() {
            return Err("missing module".into());
        }
        let _guard = self.gate.lock().await;
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        let now = now_ms();
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO study_time_events(event_id,created_at_ms) VALUES(?,?)",
        )
        .bind(&event.event_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        if inserted.rows_affected() != 0 {
            let row = sqlx::query("SELECT status,module,phase,started_at_ms,last_seen_at_ms FROM study_time_state WHERE id=1")
                .fetch_one(&mut *tx).await.map_err(|e|e.to_string())?;
            let state = ClockState::from_row(row);
            let phase = event.phase.as_deref().unwrap_or("answer");
            match event.action.as_str() {
                "enter" | "switch" => {
                    let module = event.module.as_deref().unwrap_or_default();
                    if state.status == "running"
                        && state.module == module
                        && state.phase == phase
                        && state.credited_end(now) == now
                    {
                        Self::update_state(
                            &mut tx,
                            "running",
                            module,
                            phase,
                            state.started_at_ms,
                            Some(now),
                        )
                        .await?;
                    } else {
                        Self::close(&mut tx, &state, now).await?;
                        Self::update_state(&mut tx, "running", module, phase, Some(now), Some(now))
                            .await?;
                    }
                }
                "leave" => {
                    Self::close(&mut tx, &state, now).await?;
                    Self::update_state(&mut tx, "idle", &state.module, &state.phase, None, None)
                        .await?;
                }
                "heartbeat" => {
                    if state.status == "running" {
                        if state.credited_end(now) < now {
                            Self::close(&mut tx, &state, now).await?;
                            Self::update_state(
                                &mut tx,
                                "running",
                                &state.module,
                                &state.phase,
                                Some(now),
                                Some(now),
                            )
                            .await?;
                        } else {
                            Self::update_state(
                                &mut tx,
                                "running",
                                &state.module,
                                &state.phase,
                                state.started_at_ms,
                                Some(now),
                            )
                            .await?;
                        }
                    }
                }
                _ => {}
            }
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        self.snapshot_at(now).await
    }

    async fn credited_segments(
        &self,
        start: i64,
        end: i64,
        now: i64,
    ) -> Result<Vec<(String, String, i64, i64)>, String> {
        let rows = sqlx::query("SELECT module,phase,started_at_ms,ended_at_ms FROM study_time_segments WHERE started_at_ms < ? AND ended_at_ms > ?")
            .bind(end).bind(start).fetch_all(&self.pool).await.map_err(|e|e.to_string())?;
        let mut result = rows
            .into_iter()
            .map(|row| {
                (
                    row.get("module"),
                    row.get("phase"),
                    row.get("started_at_ms"),
                    row.get("ended_at_ms"),
                )
            })
            .collect::<Vec<(String, String, i64, i64)>>();
        let active = self.load().await?;
        if active.status == "running" {
            if let Some(s) = active.started_at_ms {
                let e = active.credited_end(now).max(s);
                if s < end && e > start {
                    result.push((active.module, active.phase, s, e));
                }
            }
        }
        Ok(result)
    }

    async fn snapshot_at(&self, now: i64) -> Result<ClockSnapshot, String> {
        let active = self.load().await?;
        let start = day_start(now);
        let rows = self
            .credited_segments(start, now.saturating_add(1), now)
            .await?;
        let today_ms = rows
            .iter()
            .map(|(_, _, s, e)| ((*e).min(now) - (*s).max(start)).max(0))
            .sum();
        Ok(ClockSnapshot {
            now_ms: now,
            today_ms,
            running: active.status == "running" && active.credited_end(now) == now,
            module: active.module,
        })
    }
    pub async fn current(&self) -> Result<ClockSnapshot, String> {
        self.snapshot_at(now_ms()).await
    }

    async fn daily_answered_counts(
        &self,
        start: i64,
        end: i64,
    ) -> Result<BTreeMap<String, BTreeMap<String, i64>>, String> {
        let mut days: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        // Read the immutable submitted answers, including historical runs without result flags.
        for (table, module) in [
            ("data_analysis_runs", "comprehensive"),
            ("logic_judgment_runs", "logic"),
            ("quantitative_relations_runs", "quantitative"),
        ] {
            let query = format!(
                "SELECT date(run.ended_at_ms / 1000.0, 'unixepoch', '+8 hours') AS day,
                        COUNT(*) AS answered
                 FROM {table} run, json_each(run.answers_json) answer
                 WHERE run.status='submitted' AND run.ended_at_ms>=? AND run.ended_at_ms<?
                   AND json_type(run.answers_json)='array' AND answer.type='integer'
                 GROUP BY day"
            );
            let rows = sqlx::query(&query)
                .bind(start)
                .bind(end)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
            for row in rows {
                days.entry(row.get("day"))
                    .or_default()
                    .insert(module.to_owned(), row.get("answered"));
            }
        }
        Ok(days)
    }

    pub async fn goals(&self) -> Result<StudyGoals, String> {
        let row = sqlx::query("SELECT daily_questions,daily_study_ms FROM study_goals WHERE id=1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row
            .map(|row| StudyGoals {
                daily_questions: row.get::<i64, _>("daily_questions").clamp(1, 2_000) as u32,
                daily_study_ms: row.get("daily_study_ms"),
            })
            .unwrap_or_default())
    }

    pub async fn save_goals(&self, goals: StudyGoals) -> Result<StudyGoals, String> {
        goals.validate()?;
        sqlx::query(
            "INSERT INTO study_goals(id,daily_questions,daily_study_ms) VALUES(1,?,?) \
             ON CONFLICT(id) DO UPDATE SET daily_questions=excluded.daily_questions, \
             daily_study_ms=excluded.daily_study_ms",
        )
        .bind(i64::from(goals.daily_questions))
        .bind(goals.daily_study_ms)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(goals)
    }

    pub async fn statistics(&self, from: &str, to: &str) -> Result<TimeStatistics, String> {
        let StudyDateRange {
            first,
            length,
            start_ms: start,
            end_ms: end,
        } = StudyDateRange::parse(from, to)?;
        let now = now_ms();
        let rows = self.credited_segments(start, end, now).await?;
        let answered = self.daily_answered_counts(start, end).await?;
        let mut days = Vec::with_capacity((length + 1) as usize);
        for i in 0..=length {
            let date = first + Duration::days(i);
            let ds = start + i * DAY_MS;
            let de = ds + DAY_MS;
            let mut day = DaySummary {
                date: date.to_string(),
                total_ms: 0,
                modules: BTreeMap::new(),
                phases: BTreeMap::new(),
                answered_count: 0,
                answered_modules: BTreeMap::new(),
            };
            for (module, phase, s, e) in &rows {
                let duration = (e.min(&de) - s.max(&ds)).max(0);
                if duration > 0 {
                    day.total_ms += duration;
                    *day.modules.entry(module.clone()).or_default() += duration;
                    *day.phases.entry(phase.clone()).or_default() += duration;
                }
            }
            if let Some(modules) = answered.get(&day.date) {
                day.answered_modules = modules.clone();
                day.answered_count = modules.values().sum();
            }
            days.push(day);
        }
        Ok(TimeStatistics { days })
    }
}

#[async_trait]
impl TrainingTimePort for TrainingTimeService {
    async fn current(&self) -> Result<ClockSnapshot, String> {
        TrainingTimeService::current(self).await
    }
    async fn event(&self, event: StudyTimeEvent) -> Result<ClockSnapshot, String> {
        TrainingTimeService::event(self, event).await
    }
    async fn statistics(&self, from: &str, to: &str) -> Result<TimeStatistics, String> {
        TrainingTimeService::statistics(self, from, to).await
    }
    async fn goals(&self) -> Result<StudyGoals, String> {
        TrainingTimeService::goals(self).await
    }
    async fn save_goals(&self, goals: StudyGoals) -> Result<StudyGoals, String> {
        TrainingTimeService::save_goals(self, goals).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn day_boundaries_are_beijing_midnight() {
        assert_eq!(day_start(0), -28_800_000);
        assert_eq!(day_start(86_400_000 - 28_800_000), 86_400_000 - 28_800_000);
    }
    #[tokio::test]
    async fn schema_removes_manual_pause_state() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('study_time_state')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!columns.iter().any(|column| column == "manual_paused"));
        let table_sql: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='study_time_state'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!table_sql.contains("paused"));
    }

    #[tokio::test]
    async fn daily_goals_default_persist_and_reject_out_of_range_values() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let service = TrainingTimeService::new(pool.clone());
        assert_eq!(service.goals().await.unwrap(), StudyGoals::default());
        let custom = StudyGoals {
            daily_questions: 250,
            daily_study_ms: 10 * 3_600_000,
        };
        assert_eq!(service.save_goals(custom.clone()).await.unwrap(), custom);
        assert_eq!(
            TrainingTimeService::new(pool).goals().await.unwrap(),
            custom
        );
        assert!(service
            .save_goals(StudyGoals {
                daily_questions: 0,
                ..custom.clone()
            })
            .await
            .is_err());
        assert!(service
            .save_goals(StudyGoals {
                daily_study_ms: 25 * 3_600_000,
                ..custom.clone()
            })
            .await
            .is_err());
        assert_eq!(service.goals().await.unwrap(), custom);
    }

    #[tokio::test]
    async fn statistics_count_submitted_answers_by_beijing_day() {
        use chrono::NaiveDate;
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 1, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
            - ZONE_OFFSET_MS;
        for (id, answers, status, ended) in [
            ("comprehensive", "[0,1,null,null]", "submitted", day + 1),
            ("comprehensive-blank", "[null,null]", "submitted", day + 2),
            ("logic", "[2,3,4]", "submitted", day + 3),
            ("quantitative-prepared", "[0,1]", "prepared", day + 4),
            ("comprehensive-active", "[0]", "active", day + 5),
        ] {
            let table = if id.starts_with("logic") {
                "logic_judgment_runs"
            } else if id.starts_with("quantitative") {
                "quantitative_relations_runs"
            } else {
                "data_analysis_runs"
            };
            let questions_column = if table == "data_analysis_runs" {
                "groups_json"
            } else {
                "questions_json"
            };
            let query = format!(
                "INSERT INTO {table}
                 (id,{questions_column},answers_json,status,created_at_ms,ended_at_ms)
                 VALUES(?,?,?,?,?,?)"
            );
            sqlx::query(&query)
                .bind(id)
                .bind("[]")
                .bind(answers)
                .bind(status)
                .bind(day - 600_000)
                .bind(ended)
                .execute(&pool)
                .await
                .unwrap();
        }
        let service = TrainingTimeService::new(pool);
        let statistics = service
            .statistics("2026-01-02", "2026-01-02")
            .await
            .unwrap();
        assert_eq!(statistics.days.len(), 1);
        assert_eq!(statistics.days[0].answered_count, 5);
        assert_eq!(
            statistics.days[0].answered_modules,
            BTreeMap::from([("comprehensive".to_owned(), 2), ("logic".to_owned(), 3)])
        );
        let empty = service
            .statistics("2026-01-03", "2026-01-03")
            .await
            .unwrap();
        assert_eq!(empty.days[0].answered_count, 0);
        assert!(empty.days[0].answered_modules.is_empty());
    }

    #[tokio::test]
    async fn study_clock_runs_until_leave_and_rejects_toggle() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let service = TrainingTimeService::new(pool);
        let enter = StudyTimeEvent {
            event_id: "one".into(),
            action: "enter".into(),
            module: Some("logic".into()),
            phase: None,
        };
        assert!(service.event(enter).await.unwrap().running);
        let duplicate = StudyTimeEvent {
            event_id: "one".into(),
            action: "enter".into(),
            module: Some("logic".into()),
            phase: None,
        };
        assert!(service.event(duplicate).await.unwrap().running);
        let toggle = StudyTimeEvent {
            event_id: "two".into(),
            action: "toggle".into(),
            module: None,
            phase: None,
        };
        assert!(service.event(toggle).await.is_err());
        let leave = StudyTimeEvent {
            event_id: "three".into(),
            action: "leave".into(),
            module: None,
            phase: None,
        };
        assert!(!service.event(leave).await.unwrap().running);
    }
}
