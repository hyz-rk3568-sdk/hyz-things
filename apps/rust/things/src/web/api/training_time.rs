use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const TRAINING_TIME_ROOT: &str = "/api/v1/study/training-time";
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
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct TrainingDay {
    pub date: String,
    pub total_ms: i64,
    pub modules: BTreeMap<String, i64>,
    pub phases: BTreeMap<String, i64>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct TrainingTimeStats {
    pub days: Vec<TrainingDay>,
}
