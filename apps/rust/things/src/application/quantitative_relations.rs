//! Typed operations exposed to the logic judgment driving adapters.
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum QuantitativeRelationsOperation {
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
pub enum QuantitativeRelationsError {
    InvalidRequest,
    NotFound,
    Busy,
    Unavailable,
}

#[async_trait::async_trait]
pub trait QuantitativeRelationsPort: Send + Sync {
    fn shutdown(&self) {}

    async fn execute(
        &self,
        operation: QuantitativeRelationsOperation,
    ) -> Result<Value, QuantitativeRelationsError>;
}

#[derive(Clone, serde::Serialize)]
pub struct QuantitativeGenerationTarget {
    pub target_id: String,
    pub card_id: String,
    pub card_path: String,
    pub front: String,
    pub back: String,
    pub kind: String,
}

#[derive(Clone)]
pub struct QuantitativeGenerationRequest {
    pub targets: Vec<QuantitativeGenerationTarget>,
    pub user_prompt: String,
}

#[async_trait::async_trait]
pub trait QuantitativeGenerationPort: Send + Sync {
    async fn generate(
        &self,
        request: QuantitativeGenerationRequest,
        cancellation: tokio::sync::watch::Sender<bool>,
    ) -> Result<Vec<crate::domain::quantitative_relations::GeneratedQuestion>, String>;
}
