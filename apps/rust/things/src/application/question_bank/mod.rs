pub mod document;
mod favorite_flashcards;
pub mod images;
pub mod practice;

use crate::domain::question_bank::{
    BankSyncStatus, BankSyncSummary, PaperSummary, PaperView, PracticeCatalog, PracticeMode,
    PracticeSession,
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
    pub module: Option<String>,
    pub category: Option<String>,
    pub paper_title: Option<String>,
    pub answered: i64,
}

/// Submitted answers split by module drill scope or original paper.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionBankDayBreakdown {
    pub modules: BTreeMap<String, BTreeMap<String, i64>>,
    pub papers: BTreeMap<String, i64>,
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredQuestion {
    pub id: String,
    pub number: u32,
    pub module: Option<String>,
    pub category: Option<String>,
    pub topics: Vec<String>,
    pub materials: Vec<String>,
    pub stem: String,
    pub options_markdown: String,
    pub gradable: bool,
    pub correct_answer: Vec<String>,
    pub explanation: String,
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
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
                    gradable: key.is_some_and(|key| key.gradable),
                    correct_answer: key.map(|key| key.choices.clone()).unwrap_or_default(),
                    explanation: key.map(|key| key.explanation.clone()).unwrap_or_default(),
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

#[derive(Clone, Debug, PartialEq, Eq)]
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
}
impl QuestionBankApplication {
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
            if row.is_module {
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
        }
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
        let snapshot = self.source.scan(known).await?;
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
            role: summary.role,
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
        let indexed = IndexedPaper::new(file.path.clone(), source_hash(after.as_bytes()), doc);
        if let Some(old) = known
            .iter()
            .find(|old| old.path == file.path || old.summary.id == indexed.summary.id)
        {
            if !preserves_identity(old, &indexed) {
                return Err(BankError::Conflict);
            }
        }
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
    Ok(SyncPlan {
        writes,
        snapshot: BankSnapshot {
            papers,
            generated_ids,
        },
    })
}

pub fn source_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn preserves_identity(old: &IndexedPaper, new: &IndexedPaper) -> bool {
    old.summary.id == new.summary.id
        && old.ids.iter().all(|id| new.ids.contains(id))
        && old
            .question_ids
            .iter()
            .all(|id| new.question_ids.contains(id))
        && old
            .material_ids
            .iter()
            .all(|id| new.material_ids.contains(id))
}
