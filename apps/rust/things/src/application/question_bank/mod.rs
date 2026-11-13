pub mod attachments;
pub mod document;
mod favorite_flashcards;
pub mod images;
pub mod practice;
pub mod shenlun;
pub mod shenlun_grading;

use crate::domain::question_bank::{
    BankSyncStatus, BankSyncSummary, PaperSummary, PaperView, PracticeCatalog, PracticeMode,
    PracticeSession, ShenlunAttempt, StudyPlan,
};
use async_trait::async_trait;
use document::{valid_id, Document};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

/// One submitted-answer group for the study statistics screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DailyAnsweredBreakdownRow {
    pub day: String,
    pub is_module: bool,
    pub kind: String,
    pub collection_title: Option<String>,
    pub module: Option<String>,
    pub category: Option<String>,
    pub paper_title: Option<String>,
    pub answered: i64,
}

/// Submitted answers split by module drill, original paper or drill collection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionBankDayBreakdown {
    pub modules: BTreeMap<String, BTreeMap<String, i64>>,
    pub papers: BTreeMap<String, i64>,
    #[serde(default)]
    pub collections: BTreeMap<String, BTreeMap<String, i64>>,
}

/// One indexed drill topic paper; the application groups these by collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillPaperRow {
    pub paper_id: String,
    pub title: String,
    pub collection_id: String,
    pub collection_title: String,
    pub module: Option<String>,
    pub year: i32,
    pub questions: usize,
    pub gradable: usize,
    pub done: usize,
}

/// One Beijing calendar day of submitted answers for a module category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudyAnswerDay {
    pub day: String,
    pub module: String,
    pub category: String,
    pub answered: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BankError {
    Busy,
    Unavailable,
    Storage,
    Invalid,
    Conflict,
    NotFound,
    Stale,
}
impl std::fmt::Display for BankError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "question bank {self:?}")
    }
}
impl std::error::Error for BankError {}

/// Parsed question content cached by the SQLite index so practice does not
/// rescan Markdown. Answer keys stay on the server and are never serialized
/// into the browser-facing workspace DTOs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredQuestion {
    pub id: String,
    pub number: u32,
    pub module: Option<String>,
    pub category: Option<String>,
    pub topics: Vec<String>,
    pub materials: Vec<String>,
    pub stem: String,
    pub options_markdown: String,
    #[serde(default = "crate::domain::question_bank::default_answer_type")]
    pub answer_type: String,
    #[serde(default)]
    pub question_type: Option<String>,
    #[serde(default = "crate::domain::question_bank::default_grading_mode")]
    pub grading_mode: String,
    #[serde(default)]
    pub max_score: Option<f64>,
    pub gradable: bool,
    pub correct_answer: Vec<String>,
    pub explanation: String,
    #[serde(default)]
    pub reference_answer: String,
}

impl StoredQuestion {
    pub fn view(&self) -> crate::domain::question_bank::QuestionView {
        crate::domain::question_bank::QuestionView {
            id: self.id.clone(),
            number: self.number,
            stem: self.stem.clone(),
            options_markdown: self.options_markdown.clone(),
            materials: self.materials.clone(),
            module: self.module.clone(),
            category: self.category.clone(),
            topics: self.topics.clone(),
            answer_type: self.answer_type.clone(),
            question_type: self.question_type.clone(),
            grading_mode: self.grading_mode.clone(),
            max_score: self.max_score,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaperContent {
    pub paper_id: String,
    pub hash: String,
    pub questions: Vec<StoredQuestion>,
    pub materials: Vec<crate::domain::question_bank::MaterialView>,
}

impl PaperContent {
    pub fn from_document(hash: &str, doc: &Document) -> Self {
        let view = doc.view();
        let questions = view
            .questions
            .into_iter()
            .map(|question| {
                let key = doc.key(&question.id);
                StoredQuestion {
                    id: question.id,
                    number: question.number,
                    module: question.module,
                    category: question.category,
                    topics: question.topics,
                    materials: question.materials,
                    stem: question.stem,
                    options_markdown: question.options_markdown,
                    answer_type: question.answer_type,
                    question_type: question.question_type,
                    grading_mode: question.grading_mode,
                    max_score: question.max_score,
                    gradable: key.is_some_and(|key| key.gradable),
                    correct_answer: key.map(|key| key.choices.clone()).unwrap_or_default(),
                    explanation: key.map(|key| key.explanation.clone()).unwrap_or_default(),
                    reference_answer: key
                        .map(|key| key.reference_answer.clone())
                        .unwrap_or_default(),
                }
            })
            .collect();
        Self {
            paper_id: view.id,
            hash: hash.into(),
            questions,
            materials: view.materials,
        }
    }

    pub fn question(&self, id: &str) -> Option<&StoredQuestion> {
        self.questions.iter().find(|question| question.id == id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PracticeContent {
    pub paper_id: String,
    pub paper_title: String,
    pub source_hash: String,
    pub active: bool,
    pub question: StoredQuestion,
    pub materials: Vec<crate::domain::question_bank::MaterialView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedPaper {
    pub summary: PaperSummary,
    pub path: String,
    pub hash: String,
    pub ids: Vec<String>,
    #[serde(default)]
    pub question_ids: Vec<String>,
    #[serde(default)]
    pub material_ids: Vec<String>,
    #[serde(default)]
    pub question_numbers: Vec<u32>,
    #[serde(default)]
    pub question_modules: Vec<Option<String>>,
    #[serde(default)]
    pub question_categories: Vec<Option<String>>,
    /// Indexed content is written to SQLite and skipped in the metadata JSON.
    #[serde(skip)]
    pub content: PaperContent,
    #[serde(skip)]
    pub content_fingerprints: BTreeMap<String, String>,
}
impl IndexedPaper {
    pub fn new(path: String, hash: String, doc: Document) -> Self {
        let content = PaperContent::from_document(&hash, &doc);
        Self {
            summary: doc.summary(),
            path,
            hash,
            ids: doc.ids(),
            question_ids: doc.question_ids(),
            material_ids: doc.material_ids(),
            question_numbers: doc.view().questions.iter().map(|q| q.number).collect(),
            question_modules: doc
                .view()
                .questions
                .iter()
                .map(|q| q.module.clone())
                .collect(),
            question_categories: doc
                .view()
                .questions
                .iter()
                .map(|q| q.category.clone())
                .collect(),
            content,
            content_fingerprints: BTreeMap::new(),
        }
    }
}
pub struct BankSnapshot {
    pub papers: Vec<IndexedPaper>,
    pub generated_ids: usize,
}
pub struct BankAsset {
    pub contents: Vec<u8>,
    pub content_type: &'static str,
}
#[async_trait]
pub trait QuestionBankSourcePort: Send + Sync {
    async fn scan(&self, known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError>;
    async fn load(&self, paper: IndexedPaper) -> Result<Document, BankError>;
    async fn asset(&self, name: String) -> Result<BankAsset, BankError>;
}
#[async_trait]
pub trait QuestionBankRepositoryPort: Send + Sync {
    /// Counts nonempty answers in completed submissions within UTC millisecond bounds.
    async fn daily_answered_counts(
        &self,
        _start_ms: i64,
        _end_ms: i64,
    ) -> Result<Vec<(String, i64)>, BankError> {
        Err(BankError::Unavailable)
    }
    /// Groups submitted answers by practice scope for the same UTC bounds.
    async fn daily_answered_breakdown(
        &self,
        _start_ms: i64,
        _end_ms: i64,
    ) -> Result<Vec<DailyAnsweredBreakdownRow>, BankError> {
        Ok(Vec::new())
    }
    async fn shenlun_create_attempt(
        &self,
        _attempt: ShenlunAttempt,
    ) -> Result<ShenlunAttempt, BankError> {
        Err(BankError::Unavailable)
    }
    async fn shenlun_get_attempt(&self, _id: &str) -> Result<ShenlunAttempt, BankError> {
        Err(BankError::Unavailable)
    }
    async fn shenlun_list_attempts(
        &self,
        _question_id: &str,
    ) -> Result<Vec<ShenlunAttempt>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn shenlun_save_draft(
        &self,
        _id: &str,
        _expected_revision: i64,
        _text: &str,
    ) -> Result<ShenlunAttempt, BankError> {
        Err(BankError::Unavailable)
    }
    async fn shenlun_submit_attempt(&self, _id: &str) -> Result<ShenlunAttempt, BankError> {
        Err(BankError::Unavailable)
    }
    async fn data_revision(&self) -> Result<i64, BankError> {
        Err(BankError::Unavailable)
    }
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError>;
    async fn paper_summaries(&self) -> Result<Vec<PaperSummary>, BankError> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .filter(|paper| paper.summary.role == "primary")
            .map(|paper| paper.summary)
            .collect())
    }
    async fn indexed_paper(&self, _id: &str) -> Result<IndexedPaper, BankError> {
        Err(BankError::Unavailable)
    }
    async fn apply(&self, papers: Vec<IndexedPaper>) -> Result<(), BankError>;
    async fn known(&self) -> Result<Vec<IndexedPaper>, BankError> {
        self.list().await
    }
    /// Reads the synced Markdown-derived question content from the index.
    async fn paper_content(&self, _paper_id: &str) -> Result<PaperContent, BankError> {
        Err(BankError::Unavailable)
    }
    /// Aggregates one paper's module counts without loading question content.
    /// SQLite overrides this with a `GROUP BY`; the default keeps simple test
    /// doubles working from their cached content.
    async fn paper_module_counts(
        &self,
        paper_id: &str,
        source_hash: &str,
    ) -> Result<Vec<(Option<String>, usize, usize)>, BankError> {
        let content = self.paper_content(paper_id).await?;
        if content.hash != source_hash {
            return Err(BankError::Stale);
        }
        let mut counts: std::collections::BTreeMap<Option<String>, (usize, usize)> =
            std::collections::BTreeMap::new();
        for question in content.questions {
            let entry = counts.entry(question.module).or_default();
            if question.gradable {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
        Ok(counts
            .into_iter()
            .map(|(module, (gradable, ungradable))| (module, gradable, ungradable))
            .collect())
    }
    async fn question_contents(&self, _ids: &[String]) -> Result<Vec<PracticeContent>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_create(&self, _session: PracticeSession) -> Result<(), BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_load(&self, _id: &str) -> Result<PracticeSession, BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_save_answer(
        &self,
        _session: PracticeSession,
        _question_id: &str,
        _previous_revision: u64,
    ) -> Result<(), BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_update(
        &self,
        _session: PracticeSession,
        _previous_revision: u64,
    ) -> Result<(), BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_history(&self) -> Result<Vec<PracticeSession>, BankError> {
        Err(BankError::Unavailable)
    }
    /// Returns `(question_id, correct, wrong)` for completed submissions.
    async fn practice_attempt_counts(
        &self,
        _ids: &[String],
    ) -> Result<Vec<(String, usize, usize)>, BankError> {
        Ok(Vec::new())
    }
    async fn practice_browse(
        &self,
        _mode: PracticeMode,
        _paper_id: Option<&str>,
        _module: Option<&str>,
        _category: Option<&str>,
        _filters: &crate::domain::question_bank::ModulePracticeFilters,
    ) -> Result<Vec<crate::domain::question_bank::PracticeBrowseItem>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_candidates(&self, _mode: PracticeMode) -> Result<Vec<String>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_catalog(
        &self,
        _filters: &crate::domain::question_bank::ModulePracticeFilters,
    ) -> Result<PracticeCatalog, BankError> {
        Err(BankError::Unavailable)
    }
    /// Drill collections grouped from indexed `kind=drill` papers. Each row is
    /// one topic paper; the application aggregates rows into collections.
    async fn drill_paper_stats(&self) -> Result<Vec<DrillPaperRow>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn drill_question_ids(
        &self,
        _collection_id: &str,
        _paper_id: Option<&str>,
    ) -> Result<Vec<String>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn module_settings(
        &self,
    ) -> Result<crate::domain::question_bank::ModulePracticeFilters, BankError> {
        Err(BankError::Unavailable)
    }
    async fn save_module_settings(
        &self,
        _filters: &crate::domain::question_bank::ModulePracticeFilters,
    ) -> Result<(), BankError> {
        Err(BankError::Unavailable)
    }
    async fn study_plan(&self) -> Result<StudyPlan, BankError> {
        Ok(StudyPlan::default())
    }
    async fn save_study_plan(&self, _plan: &StudyPlan) -> Result<(), BankError> {
        Err(BankError::Unavailable)
    }
    async fn study_answered_days(
        &self,
        _filters: &crate::domain::question_bank::ModulePracticeFilters,
        _start_ms: i64,
        _end_ms: i64,
    ) -> Result<Vec<StudyAnswerDay>, BankError> {
        Ok(Vec::new())
    }
    /// Returns complete practice groups in draw order. A group contains every
    /// question that shares one source material, so callers never split a
    /// material block while sampling.
    async fn practice_module_groups(
        &self,
        _module: &str,
        _category: Option<&str>,
        _filters: &crate::domain::question_bank::ModulePracticeFilters,
        _group_limit: usize,
    ) -> Result<Vec<Vec<String>>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_favorite(
        &self,
        _question_id: &str,
        _favorite: bool,
    ) -> Result<(), BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_is_favorite(&self, _question_id: &str) -> Result<bool, BankError> {
        Err(BankError::Unavailable)
    }
}

pub struct QuestionBankApplication {
    source: Arc<dyn QuestionBankSourcePort>,
    repository: Arc<dyn QuestionBankRepositoryPort>,
    gate: Arc<Semaphore>,
    sync_status: Arc<Mutex<BankSyncStatus>>,
    data_epoch: uuid::Uuid,
}
impl QuestionBankApplication {
    /// Fingerprint only the stem, including verified image contents. Invalid or missing
    /// images and empty stems have no fingerprint; source failures are propagated.
    pub async fn stem_fingerprint(
        &self,
        paper_path: &str,
        stem: &str,
    ) -> Result<Option<String>, BankError> {
        if stem.trim().is_empty() {
            return Ok(None);
        }
        fingerprint_parts(&*self.source, paper_path, &[stem], &mut BTreeMap::new()).await
    }

    pub async fn daily_answered_counts(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<(String, i64)>, BankError> {
        self.repository
            .daily_answered_counts(start_ms, end_ms)
            .await
    }

    pub async fn daily_answered_breakdown(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<BTreeMap<String, QuestionBankDayBreakdown>, BankError> {
        let rows = self
            .repository
            .daily_answered_breakdown(start_ms, end_ms)
            .await?;
        let mut days = BTreeMap::new();
        for row in rows {
            let day = days
                .entry(row.day)
                .or_insert_with(QuestionBankDayBreakdown::default);
            if row.kind == "drill" {
                let collection = row
                    .collection_title
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "专项合集".into());
                let paper = row
                    .paper_title
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "未知专题".into());
                *day.collections
                    .entry(collection)
                    .or_default()
                    .entry(paper)
                    .or_default() += row.answered;
            } else if row.is_module {
                let module = row
                    .module
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "未分类".into());
                let category = row
                    .category
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "未分类".into());
                *day.modules
                    .entry(module)
                    .or_default()
                    .entry(category)
                    .or_default() += row.answered;
            } else {
                let paper = row
                    .paper_title
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "未知试卷".into());
                *day.papers.entry(paper).or_default() += row.answered;
            }
        }
        Ok(days)
    }

    pub fn new(
        source: Arc<dyn QuestionBankSourcePort>,
        repository: Arc<dyn QuestionBankRepositoryPort>,
    ) -> Self {
        Self {
            source,
            repository,
            gate: Arc::new(Semaphore::new(1)),
            sync_status: Arc::new(Mutex::new(BankSyncStatus::default())),
            data_epoch: uuid::Uuid::now_v7(),
        }
    }
    pub async fn data_version(&self) -> Result<String, BankError> {
        let revision = self.repository.data_revision().await?;
        Ok(format!("{}-{revision}", self.data_epoch))
    }

    pub fn sync_status(&self) -> BankSyncStatus {
        self.sync_status.lock().unwrap().clone()
    }

    /// Detached task owns the permit until scanning and indexing have exited.
    pub fn start_sync(self: &Arc<Self>) -> Result<BankSyncStatus, BankError> {
        let permit = match self.gate.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                let status = self.sync_status();
                return if status.running {
                    Ok(status)
                } else {
                    Err(BankError::Busy)
                };
            }
        };
        let status = BankSyncStatus {
            task_id: Some(uuid::Uuid::now_v7().to_string()),
            stage: "scanning".into(),
            running: true,
            summary: None,
            error_code: None,
        };
        *self.sync_status.lock().unwrap() = status.clone();
        let app = self.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = app.run_sync().await;
            let mut status = app.sync_status.lock().unwrap();
            status.running = false;
            match result {
                Ok(summary) => {
                    status.stage = "completed".into();
                    status.summary = Some(summary);
                }
                Err(error) => {
                    status.stage = "failed".into();
                    status.error_code = Some(error.code().into());
                }
            }
        });
        Ok(status)
    }

    /// Synchronous entrypoint retained for application-level tests.
    pub async fn sync(&self) -> Result<BankSyncSummary, BankError> {
        let _permit = self.gate.try_acquire().map_err(|_| BankError::Busy)?;
        self.run_sync().await
    }

    fn stage(&self, stage: &str) {
        let mut status = self.sync_status.lock().unwrap();
        if status.running {
            status.stage = stage.into();
        }
    }

    async fn run_sync(&self) -> Result<BankSyncSummary, BankError> {
        self.stage("scanning");
        let known = self.repository.known().await?;
        let mut snapshot = self.source.scan(known).await?;
        self.stage("validating");
        if snapshot.papers.is_empty() {
            return Err(BankError::Unavailable);
        }
        let mut all = std::collections::BTreeSet::new();
        for paper in &snapshot.papers {
            if paper.ids.iter().any(|id| !valid_id(id) || !all.insert(id)) {
                return Err(BankError::Conflict);
            }
        }
        populate_fingerprints(&*self.source, &mut snapshot.papers).await?;
        let questions = snapshot.papers.iter().map(|p| p.summary.questions).sum();
        let materials = snapshot
            .papers
            .iter()
            .map(|p| p.ids.len().saturating_sub(p.summary.questions + 1))
            .sum();
        let summary = BankSyncSummary {
            papers: snapshot.papers.len(),
            questions,
            materials,
            generated_ids: snapshot.generated_ids,
        };
        self.stage("indexing");
        self.repository.apply(snapshot.papers).await?;
        Ok(summary)
    }
    pub async fn papers(&self) -> Result<Vec<PaperSummary>, BankError> {
        self.repository.paper_summaries().await
    }
    pub async fn paper(&self, id: &str) -> Result<PaperView, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let indexed = self.primary_indexed(id).await?;
        let content = self.indexed_content(&indexed).await?;
        let summary = indexed.summary;
        Ok(PaperView {
            id: summary.id,
            title: summary.title,
            year: summary.year,
            region: summary.region,
            variant: summary.variant,
            subject: summary.subject,
            role: summary.role,
            kind: summary.kind,
            collection_id: summary.collection_id,
            collection_title: summary.collection_title,
            questions: content.questions.iter().map(StoredQuestion::view).collect(),
            materials: content.materials,
        })
    }
    pub async fn asset(&self, name: String) -> Result<BankAsset, BankError> {
        self.source.asset(name).await
    }
}

impl BankError {
    fn code(&self) -> &'static str {
        match self {
            Self::Busy => "question_bank_busy",
            Self::Unavailable => "question_bank_unavailable",
            Self::Storage => "question_bank_storage",
            Self::Invalid => "question_bank_invalid",
            Self::Conflict => "question_bank_conflict",
            Self::NotFound => "question_bank_not_found",
            Self::Stale => "question_bank_stale",
        }
    }
}

pub struct SourceFile {
    pub path: String,
    pub text: String,
}
pub struct PreparedFile {
    pub path: String,
    pub before: String,
    pub after: String,
}
pub struct SyncPlan {
    pub writes: Vec<PreparedFile>,
    pub snapshot: BankSnapshot,
}

fn rewrite_content_assets(paper: &mut IndexedPaper) -> Result<(), BankError> {
    let path = paper.path.clone();
    for question in &mut paper.content.questions {
        question.stem = images::rewrite_assets(&question.stem, &path)?.0;
        question.options_markdown = images::rewrite_assets(&question.options_markdown, &path)?.0;
    }
    for material in &mut paper.content.materials {
        material.markdown = images::rewrite_assets(&material.markdown, &path)?.0;
    }
    Ok(())
}

pub fn prepare_sync(
    files: Vec<SourceFile>,
    known: Vec<IndexedPaper>,
    mut next: impl FnMut() -> String,
) -> Result<SyncPlan, BankError> {
    let mut all = std::collections::BTreeSet::new();
    let mut writes = Vec::new();
    let mut papers = Vec::new();
    let mut generated_ids = 0;
    for file in files {
        let (after, count) =
            document::complete_ids(&file.text, &mut next).map_err(|_| BankError::Invalid)?;
        let doc = document::parse(&after).map_err(|_| BankError::Invalid)?;
        let mut indexed = IndexedPaper::new(file.path.clone(), source_hash(after.as_bytes()), doc);
        rewrite_content_assets(&mut indexed)?;
        if indexed.ids.iter().any(|id| !all.insert(id.clone())) {
            return Err(BankError::Conflict);
        }
        papers.push(indexed);
        generated_ids += count;
        if count > 0 {
            writes.push(PreparedFile {
                path: file.path,
                before: file.text,
                after,
            });
        }
    }
    if papers.is_empty() {
        return Err(BankError::Unavailable);
    }
    // A question may move to another paper while keeping its stable ID and
    // practice history. The old paper may drop it only when this snapshot
    // assigns it to exactly one different paper.
    let mut question_owners: BTreeMap<&str, &str> = BTreeMap::new();
    for paper in &papers {
        for id in &paper.question_ids {
            question_owners.insert(id.as_str(), paper.summary.id.as_str());
        }
    }
    for indexed in &papers {
        if let Some(old) = known
            .iter()
            .find(|old| old.path == indexed.path || old.summary.id == indexed.summary.id)
        {
            if !preserves_identity(old, indexed, &question_owners) {
                return Err(BankError::Conflict);
            }
        }
    }
    Ok(SyncPlan {
        writes,
        snapshot: BankSnapshot {
            papers,
            generated_ids,
        },
    })
}

async fn populate_fingerprints(
    source: &dyn QuestionBankSourcePort,
    papers: &mut [IndexedPaper],
) -> Result<(), BankError> {
    let mut verified_assets = BTreeMap::new();
    for paper in papers {
        paper.content_fingerprints.clear();
        for question in &paper.content.questions {
            if question.stem.trim().is_empty() {
                continue;
            }
            let mut parts = vec![question.stem.as_str()];
            for id in &question.materials {
                let Some(material) = paper.content.materials.iter().find(|item| &item.id == id)
                else {
                    parts.clear();
                    break;
                };
                parts.push(&material.markdown);
            }
            if parts.is_empty() {
                continue;
            }
            if let Some(fingerprint) =
                fingerprint_parts(source, &paper.path, &parts, &mut verified_assets).await?
            {
                paper
                    .content_fingerprints
                    .insert(question.id.clone(), fingerprint);
            }
        }
    }
    Ok(())
}

async fn fingerprint_parts(
    source: &dyn QuestionBankSourcePort,
    paper_path: &str,
    parts: &[&str],
    verified_assets: &mut BTreeMap<String, bool>,
) -> Result<Option<String>, BankError> {
    let mut canonical = Vec::new();
    for part in parts {
        let Ok((mut text, references)) = images::rewrite_assets(part, paper_path) else {
            return Ok(None);
        };
        for name in references {
            let expected = name.split('.').next().unwrap_or("");
            let verified = if let Some(verified) = verified_assets.get(&name) {
                *verified
            } else {
                let verified = match source.asset(name.clone()).await {
                    Ok(asset) => source_hash(&asset.contents) == expected,
                    Err(BankError::NotFound) => false,
                    Err(error) => return Err(error),
                };
                verified_assets.insert(name.clone(), verified);
                verified
            };
            if !verified {
                return Ok(None);
            }
            text = text.replace(
                &format!("hyz-bank-asset:{name}"),
                &format!("hyz-bank-asset:{expected}"),
            );
        }
        canonical.push(normalize_fingerprint_text(&text));
    }
    Ok(serde_json::to_vec(&("question-stem-v1", canonical))
        .ok()
        .map(|encoded| source_hash(&encoded)))
}

fn normalize_fingerprint_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let mut lines = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut math = false;
    for line in text.split('\n') {
        let unindented = line.trim_start_matches(' ');
        let marker = unindented.chars().next().filter(|c| *c == '`' || *c == '~');
        let marker_len = marker.map_or(0, |c| unindented.chars().take_while(|v| *v == c).count());
        let fence_line = line.len() - unindented.len() <= 3 && marker_len >= 3;
        let special = fence.is_some()
            || fence_line
            || math
            || line.contains('$')
            || line.contains("\\[")
            || line.contains("\\]")
            || line.contains("\\(")
            || line.contains("\\)")
            || line.contains("\\begin{")
            || line.contains("\\end{")
            || line.contains('|')
            || line.starts_with("    ")
            || line.starts_with('\t')
            || line.ends_with("  ")
            || line.ends_with("\\ ")
            || line.ends_with('\t');
        lines.push(if special {
            line
        } else {
            line.trim_end_matches(' ')
        });
        if fence_line {
            if let Some((ch, count)) = fence {
                if marker == Some(ch)
                    && marker_len >= count
                    && unindented[marker_len..].trim().is_empty()
                {
                    fence = None;
                }
            } else if let Some(ch) = marker {
                fence = Some((ch, marker_len));
            }
        } else if fence.is_none()
            && (line.matches("$$").count() % 2 == 1
                || line.contains("\\[")
                || line.contains("\\]")
                || line.contains("\\begin{")
                || line.contains("\\end{"))
        {
            math = !math;
        }
    }
    let first = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    let last = if fence.is_some() || math {
        lines.len()
    } else {
        lines
            .iter()
            .rposition(|line| !line.trim().is_empty())
            .map_or(first, |i| i + 1)
    };
    lines[first..last].join("\n")
}

pub fn source_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn preserves_identity(
    old: &IndexedPaper,
    new: &IndexedPaper,
    question_owners: &BTreeMap<&str, &str>,
) -> bool {
    old.summary.id == new.summary.id
        && old
            .material_ids
            .iter()
            .all(|id| new.material_ids.contains(id))
        && old.question_ids.iter().all(|id| {
            new.question_ids.contains(id)
                || question_owners
                    .get(id.as_str())
                    .is_some_and(|owner| *owner != old.summary.id.as_str())
        })
}
