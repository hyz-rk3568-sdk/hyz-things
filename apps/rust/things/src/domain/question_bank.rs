//! Browser-safe question-bank values. Answers and explanations are never in these DTOs.
use serde::{Deserialize, Serialize};

/// Only one-way, unambiguous mappings are allowed; don't infer from category/topics.
pub fn primary_module(name: Option<&str>) -> Option<&'static str> {
    match name?.trim() {
        "政治理论" => Some("政治理论"),
        "常识判断" => Some("常识判断"),
        "言语理解与表达" | "言语理解" | "言语" => Some("言语理解与表达"),
        "判断推理" | "图形推理" | "定义判断" | "类比推理" | "逻辑判断" | "形式推理" => {
            Some("判断推理")
        }
        "数量关系" | "数量" => Some("数量关系"),
        "资料分析" | "资料" => Some("资料分析"),
        _ => None,
    }
}

pub const PAPER_KIND_EXAM: &str = "exam";
pub const PAPER_KIND_DRILL: &str = "drill";
pub const SUBJECT_XINGCE: &str = "xingce";
pub const SUBJECT_SHENLUN: &str = "shenlun";

pub fn exam_kind() -> String {
    PAPER_KIND_EXAM.into()
}
pub fn default_subject() -> String {
    SUBJECT_XINGCE.into()
}
pub fn default_answer_type() -> String {
    "objective".into()
}
pub fn default_grading_mode() -> String {
    "exact".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaperSummary {
    pub id: String,
    pub title: String,
    pub year: i32,
    pub region: String,
    pub variant: String,
    #[serde(default = "default_subject")]
    pub subject: String,
    pub role: String,
    #[serde(default = "exam_kind")]
    pub kind: String,
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub collection_title: Option<String>,
    pub questions: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuestionView {
    pub id: String,
    pub number: u32,
    pub stem: String,
    pub options_markdown: String,
    pub materials: Vec<String>,
    pub module: Option<String>,
    pub category: Option<String>,
    pub topics: Vec<String>,
    #[serde(default = "default_answer_type")]
    pub answer_type: String,
    #[serde(default)]
    pub question_type: Option<String>,
    #[serde(default = "default_grading_mode")]
    pub grading_mode: String,
    #[serde(default)]
    pub max_score: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialView {
    pub id: String,
    pub name: String,
    pub markdown: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaperView {
    pub id: String,
    pub title: String,
    pub year: i32,
    pub region: String,
    pub variant: String,
    #[serde(default = "default_subject")]
    pub subject: String,
    pub role: String,
    #[serde(default = "exam_kind")]
    pub kind: String,
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub collection_title: Option<String>,
    pub questions: Vec<QuestionView>,
    pub materials: Vec<MaterialView>,
}

/// One private asynchronous transcription, grading, or answer-generation request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShenlunJobRequest {
    pub request_id: String,
    pub kind: String,
    #[serde(default)]
    pub attachments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShenlunJob {
    pub id: String,
    pub attempt_id: String,
    pub kind: String,
    pub status: String,
    pub error: Option<String>,
    pub result: Option<serde_json::Value>,
    pub source_hash: String,
    pub attempt_revision: i64,
    pub skill_version: String,
    pub model: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmShenlunTranscription {
    pub job_id: String,
    pub expected_revision: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShenlunSourceAnswer {
    pub text: String,
    pub provenance: String,
    pub source_hash: String,
}

/// Subjective 申论 work is independent of the A–Z objective answer contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShenlunAttempt {
    /// Client-generated UUID: retries of the same creation are idempotent.
    pub request_id: String,
    pub paper_id: String,
    pub question_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveShenlunDraft {
    pub expected_revision: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShenlunAttempt {
    pub id: String,
    pub paper_id: String,
    pub question_id: String,
    pub source_hash: String,
    pub text: String,
    pub source_kind: String,
    pub status: String,
    pub revision: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub submitted_at_ms: Option<i64>,
    #[serde(default)]
    pub reference_viewed_at_ms: Option<i64>,
    #[serde(default)]
    pub after_reference: bool,
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
    Drill,
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
    pub collection_id: Option<String>,
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
pub struct DrillPaper {
    pub paper_id: String,
    pub title: String,
    pub questions: usize,
    pub gradable: usize,
    pub done: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrillCollection {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub module: Option<String>,
    pub year: i32,
    pub questions: usize,
    pub gradable: usize,
    pub done: usize,
    pub papers: Vec<DrillPaper>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudyPlanItem {
    pub module: String,
    /// `None` plans the whole primary module; `Some` plans one secondary category.
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudyPlan {
    #[serde(default = "prediction_enabled_default")]
    pub prediction_enabled: bool,
    #[serde(default)]
    pub items: Vec<StudyPlanItem>,
}

fn prediction_enabled_default() -> bool {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudyPlanForecastItem {
    pub module: String,
    pub category: Option<String>,
    pub total: i64,
    pub done: i64,
    pub remaining: i64,
    pub answered_today: i64,
    pub answered_yesterday: i64,
    pub answered_7: i64,
    pub days_yesterday: Option<i64>,
    pub days_7: Option<i64>,
    pub finish_yesterday: Option<String>,
    pub finish_7: Option<String>,
    pub low_confidence: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudyPlanForecast {
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

/// Browsable metadata for saved wrong and favorite Question Bank items.
/// No answers or explanations are returned by this read-only index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PracticeBrowseItem {
    pub question_id: String,
    pub paper_id: String,
    pub paper_title: String,
    pub year: i32,
    pub region: String,
    pub number: u32,
    pub module: Option<String>,
    pub category: Option<String>,
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
pub struct CodexLimitWindow {
    pub remaining_percent: u8,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexLimits {
    pub five_hour: Option<CodexLimitWindow>,
    pub week: Option<CodexLimitWindow>,
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
    #[serde(default)]
    pub collection_id: Option<String>,
    #[serde(default)]
    pub collection_title: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub elapsed_seconds: u64,
    pub paused: bool,
    pub revision: u64,
    pub items: Vec<PracticeItem>,
    pub score: Option<PracticeScore>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PracticeQuestion {
    pub item: PracticeItem,
    pub paper_title: String,
    pub question: Option<QuestionView>,
    pub materials: Vec<MaterialView>,
    pub content_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PracticeWorkspace {
    pub session: PracticeSession,
    pub questions: Vec<PracticeQuestion>,
}

/// Read-only review of one saved question. It never depends on a practice
/// session, so wrong and favorite items can be reviewed without re-answering.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PracticeQuestionDetail {
    pub item: PracticeBrowseItem,
    pub question: QuestionView,
    pub materials: Vec<MaterialView>,
    pub correct_answer: Vec<String>,
    pub explanation: String,
    /// Cumulative completed submissions for this question across all sessions.
    #[serde(default)]
    pub correct_count: usize,
    #[serde(default)]
    pub wrong_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewQuestion {
    pub question: PracticeQuestion,
    pub correct_answer: Vec<String>,
    pub explanation: String,
    /// Cumulative completed submissions for this question across all sessions.
    #[serde(default)]
    pub correct_count: usize,
    #[serde(default)]
    pub wrong_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiReviewMessage {
    #[serde(default)]
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiReviewState {
    pub question_id: String,
    /// idle, running, complete, stopped or failed
    pub status: String,
    pub error: Option<String>,
    pub messages: Vec<AiReviewMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiReviewFollowUp {
    pub message: String,
    /// Opaque IDs returned by the temporary image upload endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
}
