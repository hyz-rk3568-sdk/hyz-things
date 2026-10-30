//! SQLite-backed, server-authoritative effective learning time.
//! One active lease for the portal's administrator, independent of exam clocks.
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

const GRACE_MS: i64 = 45_000;
const DAY_MS: i64 = 86_400_000;
const ZONE_OFFSET_MS: i64 = 8 * 3_600_000;
const MODULES: [&str; 5] = [
    "calculation",
    "flashcards",
    "comprehensive",
    "logic",
    "quantitative",
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

#[derive(Clone, Serialize)]
pub struct ClockSnapshot {
    pub now_ms: i64,
    pub today_ms: i64,
    pub running: bool,
    pub module: String,
}

#[derive(Clone, Serialize)]
pub struct DaySummary {
    pub date: String,
    pub total_ms: i64,
    pub modules: BTreeMap<String, i64>,
    pub phases: BTreeMap<String, i64>,
}
#[derive(Serialize)]
pub struct TimeStatistics {
    pub days: Vec<DaySummary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudyTimeEvent {
    pub event_id: String,
    pub action: String,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
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

    pub async fn statistics(&self, from: &str, to: &str) -> Result<TimeStatistics, String> {
        let first = NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|_| "invalid from date")?;
        let last = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| "invalid to date")?;
        let length = last.signed_duration_since(first).num_days();
        if !(0..=366).contains(&length) {
            return Err("invalid date range".into());
        }
        let start = first
            .and_hms_opt(0, 0, 0)
            .ok_or("invalid date")?
            .and_utc()
            .timestamp_millis()
            - ZONE_OFFSET_MS;
        let end = (last + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .ok_or("invalid date")?
            .and_utc()
            .timestamp_millis()
            - ZONE_OFFSET_MS;
        let now = now_ms();
        let rows = self.credited_segments(start, end, now).await?;
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
            };
            for (module, phase, s, e) in &rows {
                let duration = (e.min(&de) - s.max(&ds)).max(0);
                if duration > 0 {
                    day.total_ms += duration;
                    *day.modules.entry(module.clone()).or_default() += duration;
                    *day.phases.entry(phase.clone()).or_default() += duration;
                }
            }
            days.push(day);
        }
        Ok(TimeStatistics { days })
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
