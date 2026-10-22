//! User-facing DTOs. Secrets and correct indices never enter an active run.
use hyz_things::domain::data_analysis::Material;
use serde::{Deserialize, Serialize};
pub(crate) const ANALYSIS_ROOT: &str = "/api/v1/study/data-analysis";
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnalysisStatus {
    pub configured: bool,
    pub model: Option<String>,
    pub cached_sources: i64,
    pub verified_groups: i64,
    pub available_skills: Vec<String>,
    pub preparing_job_id: Option<String>,
}
#[derive(Debug, Deserialize)]
pub(crate) struct PrepareTask {
    pub job_id: String,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PrepareProgress {
    pub state: String,
    pub verified: usize,
    pub message: String,
    pub run_id: Option<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ViewQuestion {
    pub question: String,
    pub options: [String; 4],
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ViewGroup {
    pub material: Material,
    pub questions: Vec<ViewQuestion>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct AnalysisRun {
    pub id: String,
    pub status: String,
    pub started_at_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
    pub paused_remaining_ms: Option<i64>,
    pub duration_seconds: u64,
    pub groups: Vec<ViewGroup>,
    pub answers: Vec<Option<usize>>,
    #[serde(default)]
    pub current_question: usize,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ClaimedRun {
    pub run: AnalysisRun,
    pub control_token: String,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct UnfinishedRun {
    pub id: String,
    pub status: String,
    pub answered: usize,
    pub current_question: usize,
    pub deadline_at_ms: Option<i64>,
    pub paused_remaining_ms: Option<i64>,
    pub created_at_ms: i64,
}
#[derive(Serialize)]
pub(crate) struct ControlRequest {
    pub control_token: String,
}
#[derive(Serialize)]
pub(crate) struct PositionRequest {
    pub control_token: String,
    pub question: usize,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GradedQuestion {
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
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnalysisResult {
    pub id: String,
    pub correct: usize,
    pub total: usize,
    pub questions: Vec<GradedQuestion>,
    pub materials: Vec<Material>,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ComprehensiveHistoryEntry {
    pub id: String,
    #[serde(default = "first_attempt")]
    pub attempt_no: i64,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub elapsed_ms: i64,
    pub correct: usize,
    pub total: usize,
    pub topics: Vec<String>,
}
fn first_attempt() -> i64 {
    1
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RetryRun {
    pub run_id: String,
}

#[derive(Serialize)]
pub(crate) struct SaveAnswer {
    pub choice: usize,
    pub control_token: String,
}
