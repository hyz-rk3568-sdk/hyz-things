use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const TRAINING_TIME_ROOT: &str = "/api/v1/study/training-time";
pub(crate) const STUDY_PLAN_ROOT: &str = "/api/v1/study/plan";
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct TrainingTimeSnapshot {
    pub now_ms: i64,
    pub today_ms: i64,
    pub running: bool,
    pub module: String,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct TrainingTimeEventRequest {
    pub event_id: String,
    pub action: String,
    pub module: Option<String>,
    pub phase: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(crate) struct QuestionBankDayBreakdown {
    #[serde(default)]
    pub modules: BTreeMap<String, BTreeMap<String, i64>>,
    #[serde(default)]
    pub papers: BTreeMap<String, i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct TrainingDay {
    pub date: String,
    pub total_ms: i64,
    #[serde(default)]
    pub answered_count: i64,
    #[serde(default)]
    pub answered_modules: BTreeMap<String, i64>,
    #[serde(default)]
    pub question_bank: QuestionBankDayBreakdown,
    pub modules: BTreeMap<String, i64>,
    pub phases: BTreeMap<String, i64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub(crate) struct StudyGoals {
    pub daily_questions: u32,
    pub daily_study_ms: i64,
}

impl Default for StudyGoals {
    fn default() -> Self {
        Self {
            daily_questions: 400,
            daily_study_ms: 12 * 3_600_000,
        }
    }
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct StudyGoalStatus {
    pub goals: StudyGoals,
    pub today_questions: i64,
    pub today_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StudyPlanItem {
    pub module: String,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StudyPlan {
    #[serde(default = "prediction_enabled")]
    pub prediction_enabled: bool,
    #[serde(default)]
    pub items: Vec<StudyPlanItem>,
}

fn prediction_enabled() -> bool {
    true
}

impl Default for StudyPlan {
    fn default() -> Self {
        Self {
            prediction_enabled: true,
            items: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct StudyPlanForecastItem {
    pub module: String,
    pub category: Option<String>,
    pub total: i64,
    pub done: i64,
    pub remaining: i64,
    pub answered_yesterday: i64,
    pub answered_7: i64,
    pub days_yesterday: Option<i64>,
    pub days_7: Option<i64>,
    pub finish_yesterday: Option<String>,
    pub finish_7: Option<String>,
    pub low_confidence: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct StudyPlanForecast {
    pub prediction_enabled: bool,
    pub items: Vec<StudyPlanForecastItem>,
    pub total_remaining: i64,
    pub days_yesterday: Option<i64>,
    pub days_7: Option<i64>,
    pub finish_yesterday: Option<String>,
    pub finish_7: Option<String>,
    pub filter_label: String,
    pub low_confidence: bool,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct TrainingTimeStats {
    pub days: Vec<TrainingDay>,
}
