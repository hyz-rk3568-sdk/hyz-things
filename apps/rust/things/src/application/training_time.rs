//! Study time and submitted-answer statistics across independent repositories.
use super::question_bank::{BankError, QuestionBankApplication};
use async_trait::async_trait;
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DAY_MS: i64 = 86_400_000;
pub const ZONE_OFFSET_MS: i64 = 8 * 3_600_000;

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
    pub answered_count: i64,
    pub answered_modules: BTreeMap<String, i64>,
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

pub struct StudyDateRange {
    pub first: NaiveDate,
    pub length: i64,
    pub start_ms: i64,
    pub end_ms: i64,
}

impl StudyDateRange {
    pub fn parse(from: &str, to: &str) -> Result<Self, String> {
        let first = NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|_| "invalid from date")?;
        let last = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| "invalid to date")?;
        let length = last.signed_duration_since(first).num_days();
        if !(0..=366).contains(&length) {
            return Err("invalid date range".into());
        }
        let midnight = |date: NaiveDate| {
            date.and_hms_opt(0, 0, 0)
                .map(|time| time.and_utc().timestamp_millis() - ZONE_OFFSET_MS)
                .ok_or_else(|| "invalid date".to_owned())
        };
        let next = last
            .checked_add_signed(Duration::days(1))
            .ok_or("invalid date")?;
        Ok(Self {
            first,
            length,
            start_ms: midnight(first)?,
            end_ms: midnight(next)?,
        })
    }
}

#[async_trait]
pub trait TrainingTimePort: Send + Sync {
    async fn current(&self) -> Result<ClockSnapshot, String>;
    async fn event(&self, event: StudyTimeEvent) -> Result<ClockSnapshot, String>;
    async fn statistics(&self, from: &str, to: &str) -> Result<TimeStatistics, String>;
}

#[derive(Debug)]
pub enum StatisticsError {
    Time(String),
    QuestionBank(BankError),
}

pub async fn statistics(
    time: &dyn TrainingTimePort,
    bank: &QuestionBankApplication,
    from: &str,
    to: &str,
) -> Result<TimeStatistics, StatisticsError> {
    let range = StudyDateRange::parse(from, to).map_err(StatisticsError::Time)?;
    let mut result = time
        .statistics(from, to)
        .await
        .map_err(StatisticsError::Time)?;
    let counts: BTreeMap<_, _> = bank
        .daily_answered_counts(range.start_ms, range.end_ms)
        .await
        .map_err(StatisticsError::QuestionBank)?
        .into_iter()
        .collect();
    for day in &mut result.days {
        if let Some(&count) = counts.get(&day.date).filter(|&&count| count > 0) {
            day.answered_count += count;
            *day.answered_modules
                .entry("question_bank".into())
                .or_default() += count;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::question_bank::{
        document::Document, BankAsset, BankSnapshot, IndexedPaper, QuestionBankRepositoryPort,
        QuestionBankSourcePort,
    };
    use std::sync::Arc;

    struct FixedTime;
    #[async_trait]
    impl TrainingTimePort for FixedTime {
        async fn current(&self) -> Result<ClockSnapshot, String> {
            Err("unused".into())
        }
        async fn event(&self, _event: StudyTimeEvent) -> Result<ClockSnapshot, String> {
            Err("unused".into())
        }
        async fn statistics(&self, _from: &str, _to: &str) -> Result<TimeStatistics, String> {
            Ok(TimeStatistics {
                days: vec![DaySummary {
                    date: "2026-01-02".into(),
                    total_ms: 1_000,
                    modules: BTreeMap::from([("logic".to_owned(), 1_000)]),
                    phases: BTreeMap::new(),
                    answered_count: 2,
                    answered_modules: BTreeMap::from([("logic".to_owned(), 2)]),
                }],
            })
        }
    }

    struct EmptySource;
    #[async_trait]
    impl QuestionBankSourcePort for EmptySource {
        async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
            Err(BankError::Unavailable)
        }
        async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
            Err(BankError::Unavailable)
        }
        async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
            Err(BankError::Unavailable)
        }
    }

    struct FixedBank;
    #[async_trait]
    impl QuestionBankRepositoryPort for FixedBank {
        async fn daily_answered_counts(
            &self,
            _start_ms: i64,
            _end_ms: i64,
        ) -> Result<Vec<(String, i64)>, BankError> {
            Ok(vec![("2026-01-02".into(), 3)])
        }
        async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
            Ok(Vec::new())
        }
        async fn apply(&self, _papers: Vec<IndexedPaper>) -> Result<(), BankError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn statistics_merge_question_bank_counts_into_days() {
        let bank = QuestionBankApplication::new(Arc::new(EmptySource), Arc::new(FixedBank));
        let result = statistics(&FixedTime, &bank, "2026-01-02", "2026-01-02")
            .await
            .unwrap();
        assert_eq!(result.days[0].answered_count, 5);
        assert_eq!(result.days[0].answered_modules["logic"], 2);
        assert_eq!(result.days[0].answered_modules["question_bank"], 3);
    }
}
