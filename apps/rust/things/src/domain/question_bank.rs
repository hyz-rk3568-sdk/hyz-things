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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankSyncStatus {
    pub task_id: Option<String>,
    pub stage: String,
    pub running: bool,
    pub summary: Option<BankSyncSummary>,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PracticeMode {
    Paper,
    PaperModule,
    Module,
    Wrong,
    Favorite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePracticeRequest {
    pub mode: PracticeMode,
    pub paper_id: Option<String>,
    pub module: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub filters: ModulePracticeFilters,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModulePracticeFilters {
    #[serde(default)]
    pub year_min: Option<i32>,
    #[serde(default)]
    pub year_max: Option<i32>,
    #[serde(default)]
    pub regions: Vec<String>,
    #[serde(default)]
    pub exclude_regions: bool,
    #[serde(default)]
    pub count: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperModule {
    pub name: String,
    pub count: usize,
    pub ungradable: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperModules {
    pub modules: Vec<PaperModule>,
    pub unclassified: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeCategory {
    pub name: String,
    pub count: usize,
    #[serde(default)]
    pub done: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeModule {
    pub name: String,
    pub count: usize,
    #[serde(default)]
    pub done: usize,
    pub uncategorized: usize,
    pub categories: Vec<PracticeCategory>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeCatalog {
    pub modules: Vec<PracticeModule>,
    pub unclassified: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeItem {
    pub question_id: String,
    pub paper_id: String,
    pub source_hash: String,
    pub number: u32,
    pub answer: Vec<String>,
    pub grade: Option<String>,
    pub gradable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeScore {
    pub total: usize,
    pub correct: usize,
    pub wrong: usize,
    pub unanswered: usize,
    pub ungradable: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeSession {
    pub id: String,
    pub mode: PracticeMode,
    pub paper_id: Option<String>,
    pub module: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub elapsed_seconds: u64,
    pub paused: bool,
    pub revision: u64,
    pub items: Vec<PracticeItem>,
    pub score: Option<PracticeScore>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeQuestion {
    pub item: PracticeItem,
    pub paper_title: String,
    pub question: Option<QuestionView>,
    pub materials: Vec<MaterialView>,
    pub content_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeWorkspace {
    pub session: PracticeSession,
    pub questions: Vec<PracticeQuestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQuestion {
    pub question: PracticeQuestion,
    pub correct_answer: Vec<String>,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeReview {
    pub session: PracticeSession,
    pub questions: Vec<ReviewQuestion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavePracticeAnswer {
    pub answer: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPracticePaused {
    pub paused: bool,
    pub elapsed_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetFavorite {
    pub favorite: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteState {
    pub question_id: String,
    pub favorite: bool,
}
