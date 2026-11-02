//! Browser-safe question-bank values. Answers and explanations are never in these DTOs.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperSummary {
    pub id: String,
    pub title: String,
    pub year: i32,
    pub region: String,
    pub variant: String,
    pub role: String,
    pub questions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionView {
    pub id: String,
    pub number: u32,
    pub stem: String,
    pub options_markdown: String,
    pub materials: Vec<String>,
    pub module: Option<String>,
    pub category: Option<String>,
    pub topics: Vec<String>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialView {
    pub id: String,
    pub name: String,
    pub markdown: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperView {
    pub id: String,
    pub title: String,
    pub year: i32,
    pub region: String,
    pub variant: String,
    pub role: String,
    pub questions: Vec<QuestionView>,
    pub materials: Vec<MaterialView>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankSyncSummary {
    pub papers: usize,
    pub questions: usize,
    pub materials: usize,
    pub generated_ids: usize,
}
