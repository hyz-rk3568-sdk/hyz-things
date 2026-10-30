//! Logic judgment wire views exclude grading data until submission.
use serde::{Deserialize, Serialize};
pub(crate) const LOGIC_ROOT: &str = "/api/v1/study/logic-judgment";
pub(crate) const QUANT_ROOT: &str = "/api/v1/study/quantitative-relations";
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

pub(crate) async fn enter_drill_run(
    root: &str,
    seconds: u64,
    id: &str,
    csrf: &str,
) -> Result<LogicRun, String> {
    use super::{fetch_json, post_json_response, EmptyRequest};
    let mut run: LogicRun = fetch_json(&format!("{root}/runs/{id}"), "试卷").await?;
    if run.status == "prepared" {
        run = post_json_response(
            &format!("{root}/runs/{id}/start"),
            csrf,
            &EmptyRequest {},
            "开始训练",
        )
        .await?;
    }
    if !matches!(run.status.as_str(), "active" | "submitted")
        || run.questions.len() != 10
        || run.answers.len() != 10
        || run.duration_seconds != seconds
        || (run.status == "active"
            && run.deadline_at_ms.is_none()
            && run.paused_remaining_ms.is_none())
        || run.answers.iter().flatten().any(|choice| *choice >= 4)
    {
        return Err("服务器试卷不符合题量、计时或答案安全性约定".into());
    }
    Ok(run)
}

pub(crate) async fn read_drill_result(root: &str, id: &str) -> Result<LogicResult, String> {
    super::fetch_json(&format!("{root}/runs/{id}/result"), "答卷结果").await
}

#[cfg(test)]
mod tests {
    use super::LogicResult;

    #[test]
    fn result_view_deserializes_without_unused_card_metadata() {
        let result: LogicResult = serde_json::from_value(serde_json::json!({
            "id": "run-1",
            "correct": 1,
            "total": 1,
            "questions": [{
                "question": "题目",
                "options": ["A", "B", "C", "D"],
                "correct_index": 0,
                "explanation": "解析",
                "card_path": "cards/example.md",
                "front": "题干",
                "back": "答案",
                "chosen": 0
            }]
        }))
        .expect("the review page should decode the fields it displays");

        assert_eq!(result.total, 1);
        assert_eq!(result.questions[0].question, "题目");
        assert_eq!(result.questions[0].correct_index, 0);
        assert_eq!(result.questions[0].explanation, "解析");
        assert_eq!(result.questions[0].card_path, "cards/example.md");
        assert_eq!(result.questions[0].front, "题干");
        assert_eq!(result.questions[0].back, "答案");
        assert_eq!(result.questions[0].chosen, Some(0));
    }
}
