//! Logic judgment wire views exclude grading data until submission.
use serde::{Deserialize, Serialize};
pub(crate) const LOGIC_ROOT: &str = "/api/v1/study/logic-judgment";
#[derive(Clone, Deserialize)]
pub(crate) struct LogicStatus {
    pub configured: bool,
    pub eligible_cards: usize,
    pub preparing_job_id: Option<String>,
}
#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct LogicQuestion {
    pub question: String,
    pub options: [String; 4],
}
#[derive(Clone, Deserialize, PartialEq)]
pub(crate) struct LogicRun {
    pub id: String,
    pub status: String,
    pub deadline_at_ms: Option<i64>,
    pub paused_remaining_ms: Option<i64>,
    pub server_now_ms: i64,
    pub duration_seconds: u64,
    pub questions: Vec<LogicQuestion>,
    pub answers: Vec<Option<usize>>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LogicGrade {
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
#[derive(Clone, Deserialize)]
pub(crate) struct LogicResult {
    pub id: String,
    pub correct: usize,
    pub total: usize,
    pub answered: usize,
    pub questions: Vec<LogicGrade>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LogicHistory {
    pub id: String,
    pub correct: usize,
    pub answered: usize,
    pub total: usize,
    pub ended_at_ms: i64,
}
#[derive(Serialize)]
pub(crate) struct LogicAnswer {
    pub question: usize,
    pub choice: usize,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LogicRetry {
    pub run_id: String,
}
#[derive(Clone, Deserialize)]
pub(crate) struct LogicPerformance {
    pub card_id: String,
    pub category: String,
    pub card_path: String,
    pub front: String,
    pub back: String,
    pub answered_count: u64,
    pub correct_count: u64,
}

pub(crate) async fn enter_logic_run(id: &str, csrf: &str) -> Result<LogicRun, String> {
    use super::{fetch_json, post_json_response, EmptyRequest};
    let mut run: LogicRun = fetch_json(&format!("{LOGIC_ROOT}/runs/{id}"), "试卷").await?;
    if run.status == "prepared" {
        run = post_json_response(
            &format!("{LOGIC_ROOT}/runs/{id}/start"),
            csrf,
            &EmptyRequest {},
            "开始训练",
        )
        .await?;
    }
    if !matches!(run.status.as_str(), "active" | "submitted")
        || run.questions.len() != 10
        || run.answers.len() != 10
        || run.duration_seconds != 600
        || (run.status == "active"
            && run.deadline_at_ms.is_none()
            && run.paused_remaining_ms.is_none())
        || run.answers.iter().flatten().any(|choice| *choice >= 4)
    {
        return Err("服务器试卷不符合10题10分钟约定".into());
    }
    Ok(run)
}

pub(crate) async fn read_logic_result(id: &str) -> Result<LogicResult, String> {
    super::fetch_json(&format!("{LOGIC_ROOT}/runs/{id}/result"), "答卷结果").await
}
