pub mod document;
pub mod images;
pub mod practice;

use crate::domain::question_bank::{
    BankSyncStatus, BankSyncSummary, PaperSummary, PaperView, PracticeMode, PracticeSession,
};
use async_trait::async_trait;
use document::{valid_id, Document};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

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
}
impl IndexedPaper {
    pub fn new(path: String, hash: String, doc: Document) -> Self {
        Self {
            summary: doc.summary(),
            path,
            hash,
            ids: doc.ids(),
            question_ids: doc.question_ids(),
            material_ids: doc.material_ids(),
            question_numbers: doc.view().questions.iter().map(|q| q.number).collect(),
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
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError>;
    async fn apply(&self, papers: Vec<IndexedPaper>) -> Result<(), BankError>;
    async fn known(&self) -> Result<Vec<IndexedPaper>, BankError> {
        self.list().await
    }
    async fn practice_create(&self, _session: PracticeSession) -> Result<(), BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_load(&self, _id: &str) -> Result<PracticeSession, BankError> {
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
    async fn practice_candidates(&self, _mode: PracticeMode) -> Result<Vec<String>, BankError> {
        Err(BankError::Unavailable)
    }
    async fn practice_favorite(&self, _question_id: &str, _favorite: bool) -> Result<(), BankError> {
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
                return if status.running { Ok(status) } else { Err(BankError::Busy) };
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
        Ok(self
            .repository
            .list()
            .await?
            .into_iter()
            .filter(|p| p.summary.role == "primary")
            .map(|p| p.summary)
            .collect())
    }
    pub async fn paper(&self, id: &str) -> Result<PaperView, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let indexed = self
            .repository
            .list()
            .await?
            .into_iter()
            .find(|p| p.summary.id == id && p.summary.role == "primary")
            .ok_or(BankError::NotFound)?;
        let doc = self.source.load(indexed).await?;
        if doc.summary().id != id {
            return Err(BankError::Conflict);
        }
        Ok(doc.view())
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
