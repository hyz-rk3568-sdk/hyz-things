#![cfg(feature = "native")]
use async_trait::async_trait;
use hyz_things::application::question_bank::{
    document::Document, BankAsset, BankError, BankSnapshot, IndexedPaper, PracticeContent,
    QuestionBankApplication, QuestionBankRepositoryPort, QuestionBankSourcePort, StoredQuestion,
};
use hyz_things::domain::question_bank::{PracticeItem, PracticeMode, PracticeSession};
use std::sync::{Arc, Mutex};

const SESSION: &str = "019a3d8c-7f23-7b6e-927d-3c951851d3a1";
const PAPER: &str = "019a3d8c-7f23-7b6e-927d-3c951851d3a2";
const QUESTION: &str = "019a3d8c-7f23-7b6e-927d-3c951851d3a3";

struct ReadPorts {
    reads: Mutex<Vec<Vec<String>>>,
    content_error: Mutex<Option<BankError>>,
    content_hash: Mutex<Option<String>>,
    session: Mutex<PracticeSession>,
}
impl Default for ReadPorts {
    fn default() -> Self {
        Self {
            reads: Mutex::new(Vec::new()),
            content_error: Mutex::new(None),
            content_hash: Mutex::new(None),
            session: Mutex::new(PracticeSession {
                id: SESSION.into(),
                mode: PracticeMode::Module,
                paper_id: None,
                module: Some("资料分析".into()),
                category: None,
                collection_id: None,
                collection_title: None,
                status: "active".into(),
                created_at: 1,
                updated_at: 1,
                elapsed_seconds: 0,
                paused: false,
                revision: 0,
                score: None,
                items: vec![PracticeItem {
                    question_id: QUESTION.into(),
                    paper_id: PAPER.into(),
                    source_hash: "hash".into(),
                    number: 26,
                    answer: vec![],
                    grade: None,
                    gradable: true,
                }],
            }),
        }
    }
}
#[async_trait]
impl QuestionBankSourcePort for ReadPorts {
    async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        panic!("runtime must not scan files")
    }
    async fn load(&self, _: IndexedPaper) -> Result<Document, BankError> {
        panic!("runtime must not open Markdown")
    }
    async fn asset(&self, _: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}
#[async_trait]
impl QuestionBankRepositoryPort for ReadPorts {
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
        panic!("single practice must not scan the catalog")
    }
    async fn apply(&self, _: Vec<IndexedPaper>) -> Result<(), BankError> {
        panic!("runtime must not sync")
    }
    async fn practice_load(&self, _: &str) -> Result<PracticeSession, BankError> {
        Ok(self.session.lock().unwrap().clone())
    }
    async fn practice_module_groups(
        &self,
        _: &str,
        _: Option<&str>,
        _: &hyz_things::domain::question_bank::ModulePracticeFilters,
        _: usize,
    ) -> Result<Vec<Vec<String>>, BankError> {
        Ok(vec![vec![QUESTION.into()]])
    }
    async fn practice_create(&self, session: PracticeSession) -> Result<(), BankError> {
        *self.session.lock().unwrap() = session;
        Ok(())
    }
    async fn practice_update(
        &self,
        session: PracticeSession,
        previous_revision: u64,
    ) -> Result<(), BankError> {
        assert_eq!(previous_revision, self.session.lock().unwrap().revision);
        *self.session.lock().unwrap() = session;
        Ok(())
    }
    async fn practice_save_answer(
        &self,
        session: PracticeSession,
        question_id: &str,
        previous_revision: u64,
    ) -> Result<(), BankError> {
        assert_eq!(question_id, QUESTION);
        assert_eq!(previous_revision, self.session.lock().unwrap().revision);
        *self.session.lock().unwrap() = session;
        Ok(())
    }
    async fn question_contents(&self, ids: &[String]) -> Result<Vec<PracticeContent>, BankError> {
        self.reads.lock().unwrap().push(ids.to_vec());
        if let Some(error) = self.content_error.lock().unwrap().clone() {
            return Err(error);
        }
        assert_eq!(ids, &[QUESTION.to_string()]);
        Ok(vec![PracticeContent {
            paper_id: PAPER.into(),
            paper_title: "原卷".into(),
            source_hash: self
                .content_hash
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "hash".into()),
            active: true,
            question: StoredQuestion {
                id: QUESTION.into(),
                number: 26,
                stem: "题干".into(),
                options_markdown: "- A. 甲\n- B. 乙".into(),
                module: Some("资料分析".into()),
                gradable: true,
                correct_answer: vec!["B".into()],
                explanation: "秘密解析".into(),
                ..Default::default()
            },
            materials: vec![],
        }])
    }
}

#[tokio::test]
async fn workspace_reads_only_its_question_ids_once() {
    let ports = Arc::new(ReadPorts::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    let workspace = app.practice_workspace(SESSION).await.unwrap();
    assert_eq!(
        workspace.questions[0].question.as_ref().unwrap().stem,
        "题干"
    );
    assert_eq!(
        *ports.reads.lock().unwrap(),
        vec![vec![QUESTION.to_string()]]
    );
    let json = serde_json::to_string(&workspace).unwrap();
    assert!(!json.contains("秘密解析"));
    assert!(!json.contains("correct_answer"));
}

#[tokio::test]
async fn module_start_reads_only_selected_question_ids() {
    use hyz_things::domain::question_bank::CreatePracticeRequest;
    let ports = Arc::new(ReadPorts::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            collection_id: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(session.items.len(), 1);
    assert_eq!(session.items[0].question_id, QUESTION);
    assert_eq!(
        *ports.reads.lock().unwrap(),
        vec![vec![QUESTION.to_string()]]
    );
}

#[tokio::test]
async fn saving_an_answer_reads_only_that_question() {
    let ports = Arc::new(ReadPorts::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    let session = app
        .save_practice_answer(SESSION, QUESTION, vec!["B".into()])
        .await
        .unwrap();
    assert_eq!(session.items[0].answer, vec!["B"]);
    assert_eq!(
        *ports.reads.lock().unwrap(),
        vec![vec![QUESTION.to_string()]]
    );
}

#[tokio::test]
async fn review_reads_each_selected_question_once() {
    let ports = Arc::new(ReadPorts::default());
    ports.session.lock().unwrap().status = "completed".into();
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    let review = app.practice_review(SESSION).await.unwrap();
    assert_eq!(review.questions[0].correct_answer, vec!["B"]);
    assert_eq!(review.questions[0].explanation, "秘密解析");
    assert_eq!(
        *ports.reads.lock().unwrap(),
        vec![vec![QUESTION.to_string()]]
    );
}

#[tokio::test]
async fn submission_reads_only_session_question_ids() {
    let ports = Arc::new(ReadPorts::default());
    ports.session.lock().unwrap().items[0].answer = vec!["B".into()];
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    let session = app.submit_practice(SESSION).await.unwrap();
    assert_eq!(session.score.unwrap().correct, 1);
    assert_eq!(
        *ports.reads.lock().unwrap(),
        vec![vec![QUESTION.to_string()]]
    );
}

#[tokio::test]
async fn storage_error_does_not_silently_submit_a_session_as_ungradable() {
    let ports = Arc::new(ReadPorts::default());
    *ports.content_error.lock().unwrap() = Some(BankError::Storage);
    let before = ports.session.lock().unwrap().clone();
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    assert_eq!(app.submit_practice(SESSION).await, Err(BankError::Storage));
    assert_eq!(*ports.session.lock().unwrap(), before);
}

#[tokio::test]
async fn changed_sqlite_content_cannot_accept_an_answer() {
    let ports = Arc::new(ReadPorts::default());
    *ports.content_hash.lock().unwrap() = Some("changed".into());
    let before = ports.session.lock().unwrap().clone();
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    assert_eq!(
        app.save_practice_answer(SESSION, QUESTION, vec!["B".into()])
            .await,
        Err(BankError::Stale)
    );
    assert_eq!(*ports.session.lock().unwrap(), before);
}

#[tokio::test]
async fn changed_sqlite_content_never_exposes_current_answer_keys_in_old_review() {
    let ports = Arc::new(ReadPorts::default());
    ports.session.lock().unwrap().status = "completed".into();
    *ports.content_hash.lock().unwrap() = Some("changed".into());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    let review = app.practice_review(SESSION).await.unwrap();
    assert!(review.questions[0].question.content_changed);
    assert!(review.questions[0].question.question.is_none());
    assert!(review.questions[0].correct_answer.is_empty());
    assert!(review.questions[0].explanation.is_empty());
}
