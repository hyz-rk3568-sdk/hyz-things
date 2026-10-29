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

#[derive(Clone, serde::Serialize)]
pub struct LogicGenerationTarget {
    pub target_id: String,
    pub card_id: String,
    pub card_path: String,
    pub front: String,
    pub back: String,
    pub kind: String,
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
}
