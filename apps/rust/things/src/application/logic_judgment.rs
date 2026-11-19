//! Typed operations exposed to the logic judgment driving adapters.
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum LogicJudgmentOperation {
    Status,
    Prepare,
    Job(String),
    Run(String),
    Start(String),
    Pause(String),
    Answer {
        id: String,
        question: usize,
        choice: usize,
    },
    Submit(String),
    Result(String),
    Retry(String),
    History,
    Unfinished,
    CardPerformance,
    UserPrompt,
    SaveUserPrompt(Option<String>),
}

#[derive(Clone, Copy, Debug)]
pub enum LogicJudgmentError {
    InvalidRequest,
    NotFound,
    Busy,
    Unavailable,
}

#[async_trait::async_trait]
pub trait LogicJudgmentPort: Send + Sync {
    fn shutdown(&self) {}

    async fn execute(&self, operation: LogicJudgmentOperation)
        -> Result<Value, LogicJudgmentError>;
}

#[derive(Clone)]
pub struct LogicGenerationTarget {
    pub target_id: String,
    pub card_id: String,
    pub card_path: String,
    pub front: String,
    pub back: String,
    pub correct_answer: Vec<String>,
    pub kind: String,
}

impl serde::Serialize for LogicGenerationTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.card_id)
    }
}

#[derive(Clone)]
pub struct LogicGenerationRequest {
    pub targets: Vec<LogicGenerationTarget>,
    pub user_prompt: String,
}

#[async_trait::async_trait]
pub trait LogicGenerationPort: Send + Sync {
    async fn generate(
        &self,
        request: LogicGenerationRequest,
        cancellation: tokio::sync::watch::Sender<bool>,
    ) -> Result<Vec<crate::domain::logic_judgment::GeneratedQuestion>, String>;
    /// Default preserves mock generators: only the Codex adapter publishes
    /// actual tool notifications, never fabricated progress.
    async fn generate_stream(
        &self,
        request: LogicGenerationRequest,
        cancellation: tokio::sync::watch::Sender<bool>,
        _sink: crate::application::question_bank::practice::AiReviewEventSink,
    ) -> Result<Vec<crate::domain::logic_judgment::GeneratedQuestion>, String> {
        self.generate(request, cancellation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_target_exposes_only_card_uuid_to_codex() {
        let target = LogicGenerationTarget {
            target_id: "card-1".into(),
            card_id: "card-1".into(),
            card_path: "secret/path".into(),
            front: "private front".into(),
            back: "private back".into(),
            correct_answer: vec!["A".into()],
            kind: "formal".into(),
        };
        assert_eq!(
            serde_json::to_value(target).unwrap(),
            serde_json::json!("card-1")
        );
    }
}
