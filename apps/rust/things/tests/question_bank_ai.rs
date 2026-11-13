#![cfg(feature = "native")]

use async_trait::async_trait;
use hyz_things::adapters::outbound::question_bank_ai::{
    AiRetryCommand, AiReviewError, AiReviewScope, QuestionBankAiService,
};
use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
use hyz_things::application::question_bank::document::parse;
use hyz_things::application::question_bank::document::Document;
use hyz_things::application::question_bank::practice::{
    AiAttachmentRelay, AiReviewBackend, AiReviewEventSink, AiReviewProgress, AiReviewTurn,
};
use hyz_things::application::question_bank::{
    BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankApplication,
    QuestionBankSourcePort,
};
use hyz_things::domain::question_bank::{CreatePracticeRequest, PracticeMode};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::{mpsc, Mutex, Semaphore};

const PAPER: &str = r#"---
schema: hyz-paper/v1
title: AI 复盘示例卷
year: 2026
region: 国考
variant: 统考
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a2 -->
# AI 复盘示例卷
## Material M1
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-7c54e259b610 -->
共享材料。
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->
```yaml
module: 资料分析
category: 比重
materials: [M1]
```
### Stem
题干。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
秘密解析。
"#;

const PAPER_ID: &str = "019a3d8c-7f23-7b6e-927d-3c951851d3a2";
const QUESTION_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e210";
const OTHER_QUESTION_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e299";

const SHENLUN_PAPER_ID: &str = "019a3d8c-7f23-7b6e-927d-3c951851d3b2";
const SHENLUN_QUESTION_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e220";
const SHENLUN_PAPER: &str = r#"---
schema: hyz-paper/v2
title: AI 申论复盘示例卷
year: 2026
region: 国考
variant: 统考
subject: shenlun
role: primary
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3b2 -->
# AI 申论复盘示例卷
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e220 -->
```yaml
answer_type: subjective
question_type: summary
grading_mode: ai
max_score: 10
materials: []
```
### Stem
请概括城市管理中的主要做法。
### Options
### Answer
### Reference Answer
重视协同治理。
### Explanation
解析。
"#;

struct FixtureBank;

#[async_trait]
impl QuestionBankSourcePort for FixtureBank {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: vec![IndexedPaper::new(
                "papers/ai.md".into(),
                "hash-ai".into(),
                parse(PAPER).unwrap(),
            )],
            generated_ids: 0,
        })
    }

    async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
        parse(PAPER).map_err(|_| BankError::Invalid)
    }

    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

struct ShenlunFixtureBank;

#[async_trait]
impl QuestionBankSourcePort for ShenlunFixtureBank {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: vec![IndexedPaper::new(
                "papers/shenlun.md".into(),
                "hash-shenlun".into(),
                parse(SHENLUN_PAPER).unwrap(),
            )],
            generated_ids: 0,
        })
    }

    async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
        parse(SHENLUN_PAPER).map_err(|_| BankError::Invalid)
    }

    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

#[derive(Clone, Debug)]
struct StartedTurn {
    thread_id: String,
    prompt: String,
}

#[derive(Default)]
struct FixtureAttachmentRelay {
    next_path: AtomicUsize,
}

#[async_trait]
impl AiAttachmentRelay for FixtureAttachmentRelay {
    async fn upload(&self, _media_type: &str, _bytes: &[u8]) -> Result<String, String> {
        Ok(format!(
            "/test-uploads/{}.png",
            self.next_path.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

fn test_png() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes.extend_from_slice(&[0; 4]);
    bytes
}

struct ControlledBackend {
    next_thread: AtomicUsize,
    connections: Mutex<Vec<Option<String>>>,
    started_tx: mpsc::UnboundedSender<StartedTurn>,
    started_rx: Mutex<mpsc::UnboundedReceiver<StartedTurn>>,
    release_turns: Arc<Semaphore>,
}

impl ControlledBackend {
    fn new() -> Self {
        let (started_tx, started_rx) = mpsc::unbounded_channel();
        Self {
            next_thread: AtomicUsize::new(1),
            connections: Mutex::new(Vec::new()),
            started_tx,
            started_rx: Mutex::new(started_rx),
            release_turns: Arc::new(Semaphore::new(0)),
        }
    }

    async fn next_started(&self) -> StartedTurn {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            self.started_rx.lock().await.recv().await
        })
        .await
        .expect("AI turn should reach the controlled backend")
        .expect("backend event channel should stay open")
    }

    async fn connections(&self) -> Vec<Option<String>> {
        self.connections.lock().await.clone()
    }
}

struct ControlledTurn {
    thread_id: String,
    started_tx: mpsc::UnboundedSender<StartedTurn>,
    release_turns: Arc<Semaphore>,
}

impl ControlledTurn {
    async fn run(
        &mut self,
        prompt: &str,
        cancellation: tokio::sync::watch::Sender<bool>,
        sink: AiReviewEventSink,
    ) -> Result<String, String> {
        (sink)(AiReviewProgress::TextDelta {
            item_id: self.thread_id.clone(),
            text: format!("delta-{}", self.thread_id),
        })
        .await?;
        self.started_tx
            .send(StartedTurn {
                thread_id: self.thread_id.clone(),
                prompt: prompt.to_owned(),
            })
            .map_err(|_| "test backend event channel closed".to_owned())?;
        let mut cancelled = cancellation.subscribe();
        let permit = tokio::select! {
            permit = self.release_turns.acquire() => permit.map_err(|_| "test turn gate closed")?,
            _ = cancelled.wait_for(|stopped| *stopped) => return Err("cancelled".into()),
        };
        permit.forget();
        Ok(format!("answer-{}", self.thread_id))
    }
}

#[async_trait]
impl AiReviewBackend for ControlledBackend {
    async fn connect(
        &self,
        previous_thread: Option<&str>,
    ) -> Result<Box<dyn AiReviewTurn>, String> {
        let previous = previous_thread.map(str::to_owned);
        self.connections.lock().await.push(previous.clone());
        let thread_id = previous.unwrap_or_else(|| {
            format!(
                "test-thread-{}",
                self.next_thread.fetch_add(1, Ordering::Relaxed)
            )
        });
        Ok(Box::new(ControlledTurn {
            thread_id,
            started_tx: self.started_tx.clone(),
            release_turns: self.release_turns.clone(),
        }))
    }
}

#[async_trait]
impl AiReviewTurn for ControlledTurn {
    fn thread_id(&self) -> &str {
        &self.thread_id
    }

    async fn turn(
        &mut self,
        prompt: &str,
        _images: &[String],
        cancellation: tokio::sync::watch::Sender<bool>,
    ) -> Result<String, String> {
        let sink: AiReviewEventSink = Arc::new(|_| Box::pin(async { Ok(()) }));
        self.run(prompt, cancellation, sink).await
    }

    async fn turn_stream(
        &mut self,
        prompt: &str,
        _images: &[String],
        cancellation: tokio::sync::watch::Sender<bool>,
        sink: AiReviewEventSink,
    ) -> Result<String, String> {
        self.run(prompt, cancellation, sink).await
    }
}

async fn shenlun_service(
    backend: Arc<ControlledBackend>,
) -> (Arc<QuestionBankAiService>, sqlx::SqlitePool, String) {
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let pool = repository.pool().clone();
    let bank = Arc::new(QuestionBankApplication::new(
        Arc::new(ShenlunFixtureBank),
        repository,
    ));
    bank.sync().await.unwrap();
    let locator = bank.question_ai_locator(SHENLUN_QUESTION_ID).await.unwrap();
    assert_eq!(locator.paper_id, SHENLUN_PAPER_ID);
    let legacy_fingerprint = locator.stem_fingerprint.clone();
    let ai_service = QuestionBankAiService::with_backend(bank, pool.clone(), Some(backend))
        .with_attachment_relay(
            Some(Arc::new(FixtureAttachmentRelay::default())),
            Some("/test-uploads".into()),
        );
    (Arc::new(ai_service), pool, legacy_fingerprint)
}

async fn wait_for_status(service: &QuestionBankAiService, scope: AiReviewScope, expected: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if service
                .state_in_scope(SHENLUN_QUESTION_ID, scope)
                .await
                .is_ok_and(|state| state.status == expected)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("AI review did not reach {expected} for {scope:?}"));
}

async fn submitted_practice() -> (Arc<QuestionBankAiService>, String, sqlx::SqlitePool) {
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let pool = repository.pool().clone();
    let bank = Arc::new(QuestionBankApplication::new(
        Arc::new(FixtureBank),
        repository,
    ));
    bank.sync().await.unwrap();
    let session = bank
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::PaperModule,
            paper_id: Some(PAPER_ID.into()),
            module: Some("资料分析".into()),
            category: None,
            collection_id: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    bank.save_practice_answer(&session.id, QUESTION_ID, vec!["A".into()])
        .await
        .unwrap();
    bank.submit_practice(&session.id).await.unwrap();
    (
        Arc::new(QuestionBankAiService::new(bank.clone(), pool.clone(), None)),
        bank.question_ai_locator(QUESTION_ID)
            .await
            .unwrap()
            .stem_fingerprint,
        pool,
    )
}

#[tokio::test]
async fn unrelated_active_practice_does_not_block_question_scoped_ai_review() {
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let pool = repository.pool().clone();
    let bank = Arc::new(QuestionBankApplication::new(
        Arc::new(FixtureBank),
        repository,
    ));
    bank.sync().await.unwrap();
    let session = bank
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::PaperModule,
            paper_id: Some(PAPER_ID.into()),
            module: Some("资料分析".into()),
            category: None,
            collection_id: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    let service = QuestionBankAiService::new(bank.clone(), pool, None);
    assert_eq!(service.state(QUESTION_ID).await.unwrap().status, "idle");
    assert!(service.stream_identity(QUESTION_ID).await.is_ok());
    assert!(matches!(
        bank.practice_review(&session.id).await,
        Err(BankError::Conflict)
    ));
    bank.submit_practice(&session.id).await.unwrap();
    assert_eq!(service.state(QUESTION_ID).await.unwrap().status, "idle");
}

#[tokio::test]
async fn idle_state_is_accessible_for_a_question() {
    let (service, _fingerprint, _pool) = submitted_practice().await;
    let state = service.state(QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "idle");
    assert!(state.messages.is_empty());
    assert_eq!(state.question_id, QUESTION_ID);
    assert!(matches!(
        service.state(OTHER_QUESTION_ID).await,
        Err(AiReviewError::Bank(BankError::NotFound))
    ));
}

#[tokio::test]
async fn start_without_codex_configuration_never_writes_or_consumes_quota() {
    let (service, _fingerprint, _pool) = submitted_practice().await;
    assert!(matches!(
        service.start(QUESTION_ID, None, Vec::new()).await,
        Err(AiReviewError::Unavailable)
    ));
    let state = service.state(QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "idle");
    assert!(state.messages.is_empty());
}

#[tokio::test]
async fn follow_up_rejects_blank_control_and_oversized_messages_before_any_task() {
    let (service, _fingerprint, _pool) = submitted_practice().await;
    for message in [
        String::new(),
        "   ".into(),
        "line\rbreak".into(),
        "x".repeat(2001),
        "好".repeat(2001),
    ] {
        assert!(matches!(
            service.follow_up(QUESTION_ID, message, Vec::new()).await,
            Err(AiReviewError::Invalid)
        ));
    }
    // 700 Chinese characters exceed 2000 bytes but remain within the character limit.
    assert!(matches!(
        service
            .follow_up(QUESTION_ID, "好".repeat(700), Vec::new())
            .await,
        Err(AiReviewError::Unavailable)
    ));
    let state = service.state(QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "idle");
    assert!(state.messages.is_empty());
}

#[tokio::test]
async fn restart_marks_running_review_failed_and_keeps_history() {
    let (service, fingerprint, pool) = submitted_practice().await;
    sqlx::query("INSERT INTO question_ai_reviews(question_id,stem_fingerprint,thread_id,status,error,updated_at) VALUES(?,?,?, 'running',NULL,1)")
        .bind(QUESTION_ID)
        .bind(&fingerprint)
        .bind("thread-before-restart")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO question_ai_messages(question_id,stem_fingerprint,role,content,created_at) VALUES(?,?,'assistant','已有解析',2)")
        .bind(QUESTION_ID)
        .bind(&fingerprint)
        .execute(&pool)
        .await
        .unwrap();
    service.recover_interrupted().await.unwrap();
    let state = service.state(QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "failed");
    assert!(state
        .error
        .as_deref()
        .is_some_and(|error| error.contains("设备服务重启")));
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].content, "已有解析");
}

#[tokio::test]
async fn request_identity_survives_recovery_and_replay_does_not_start_a_new_turn() {
    use sha2::{Digest, Sha256};
    let (service, fingerprint, pool) = submitted_practice().await;
    let request_id = "019a3d8e-1b32-7a60-9c83-f34217a6e211";
    let run_id = "019a3d8e-1b32-7a60-9c83-f34217a6e212";
    let hash: String = Sha256::digest(br#"{"kind":"initial","instruction":null,"attachments":[]}"#)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    sqlx::query("INSERT INTO question_ai_reviews(question_id,stem_fingerprint,status,updated_at) VALUES(?,?,'running',1)")
        .bind(QUESTION_ID).bind(&fingerprint).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO question_ai_requests(question_id,stem_fingerprint,request_id,payload_hash,run_id,intent,attachments,status,created_at,updated_at) VALUES(?,?,?,?,?,'initial','[]','running',1,1)")
        .bind(QUESTION_ID).bind(&fingerprint).bind(request_id).bind(hash).bind(run_id).execute(&pool).await.unwrap();
    service.recover_interrupted().await.unwrap();
    let info = service
        .request_status(QUESTION_ID, request_id)
        .await
        .unwrap();
    assert_eq!(info.run_id, run_id);
    assert_eq!(info.status, "unknown");
    assert!(info.remote_uncertain);
    let replay = service
        .start_request(QUESTION_ID, request_id, None, Vec::new())
        .await
        .unwrap();
    assert_eq!(replay.request.run_id, run_id);
    assert_eq!(replay.request.status, "unknown");
    assert!(matches!(
        service
            .start_request(QUESTION_ID, request_id, Some("不同".into()), Vec::new())
            .await,
        Err(AiReviewError::RequestConflict)
    ));
}

#[tokio::test]
async fn ai_locator_accepts_question_without_practice_attempt() {
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let bank = QuestionBankApplication::new(Arc::new(FixtureBank), repository);
    bank.sync().await.unwrap();
    let locator = bank.question_ai_locator(QUESTION_ID).await.unwrap();
    assert_eq!(locator.question_id, QUESTION_ID);
    assert!(!locator.stem_fingerprint.is_empty());
}

#[tokio::test]
async fn ai_schema_is_keyed_by_question_and_stem_fingerprint() {
    let repository = SqliteQuestionBankRepository::connect("sqlite::memory:")
        .await
        .unwrap();
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('question_ai_reviews')")
            .fetch_all(repository.pool())
            .await
            .unwrap();
    assert!(columns.iter().any(|name| name == "stem_fingerprint"));
    assert!(!columns.iter().any(|name| name == "session_id"));
}

#[tokio::test]
async fn migration_discards_legacy_ai_without_losing_other_data() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(r#"
        PRAGMA foreign_keys=ON;
        CREATE TABLE data_revision(id INTEGER PRIMARY KEY,revision INTEGER NOT NULL);
        INSERT INTO data_revision VALUES(1,10);
        CREATE TABLE practice_sessions(id TEXT PRIMARY KEY);
        INSERT INTO practice_sessions VALUES('attempt');
        CREATE TABLE question_ai_reviews(session_id TEXT NOT NULL REFERENCES practice_sessions(id),question_id TEXT NOT NULL,source_hash TEXT NOT NULL,thread_id TEXT,status TEXT,error TEXT,updated_at INTEGER NOT NULL,PRIMARY KEY(session_id,question_id));
        CREATE TABLE question_ai_messages(id INTEGER PRIMARY KEY,session_id TEXT NOT NULL,question_id TEXT NOT NULL,role TEXT NOT NULL,content TEXT NOT NULL,created_at INTEGER NOT NULL,FOREIGN KEY(session_id,question_id) REFERENCES question_ai_reviews(session_id,question_id));
        INSERT INTO question_ai_reviews VALUES('attempt','q','old','thread','complete',NULL,1);
        INSERT INTO question_ai_messages VALUES(1,'attempt','q','assistant','old',1);
    "#).execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/question_bank/20261106100000_question_ai.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let reviews: i64 = sqlx::query_scalar("SELECT count(*) FROM question_ai_reviews")
        .fetch_one(&pool)
        .await
        .unwrap();
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM practice_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(reviews, 0);
    assert_eq!(attempts, 1);
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM data_revision WHERE id=1")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO question_ai_reviews(question_id,stem_fingerprint,status,updated_at) VALUES('q','fingerprint','idle',1)").execute(&pool).await.unwrap();
    let next: i64 = sqlx::query_scalar("SELECT revision FROM data_revision WHERE id=1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(next, revision + 1);
}

#[tokio::test]
async fn shenlun_analysis_and_grading_keep_separate_threads_history_and_lifecycle() {
    const ANALYSIS_INITIAL_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e230";
    const GRADING_INITIAL_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e231";
    const ANALYSIS_FOLLOW_UP_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e232";
    const GRADING_FOLLOW_UP_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e233";
    const ANALYSIS_CROSS_SCOPE_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e234";
    const GRADING_CROSS_SCOPE_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e235";
    const RETRY_CROSS_SCOPE_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e236";
    const LEGACY_SHARED_REQUEST_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e237";
    const LEGACY_SHARED_RUN_ID: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e238";

    let backend = Arc::new(ControlledBackend::new());
    let (service, pool, legacy_fingerprint) = shenlun_service(backend.clone()).await;
    sqlx::query("INSERT INTO question_ai_reviews(question_id,stem_fingerprint,thread_id,status,updated_at) VALUES(?,?,'legacy-shared-thread','complete',1)")
        .bind(SHENLUN_QUESTION_ID)
        .bind(&legacy_fingerprint)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO question_ai_messages(question_id,stem_fingerprint,role,content,created_at) VALUES(?,?,'assistant','旧的申论批改与解析共用会话',1)")
        .bind(SHENLUN_QUESTION_ID)
        .bind(&legacy_fingerprint)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO question_ai_requests(question_id,stem_fingerprint,request_id,payload_hash,run_id,intent,attachments,status,created_at,updated_at) VALUES(?,?,?,?,?,'initial','[]','complete',1,1)")
        .bind(SHENLUN_QUESTION_ID)
        .bind(&legacy_fingerprint)
        .bind(LEGACY_SHARED_REQUEST_ID)
        .bind("legacy-shared-payload")
        .bind(LEGACY_SHARED_RUN_ID)
        .execute(&pool)
        .await
        .unwrap();

    for scope in [AiReviewScope::Analysis, AiReviewScope::Grading] {
        let identity = service
            .stream_identity_in_scope(SHENLUN_QUESTION_ID, scope)
            .await
            .unwrap();
        assert_ne!(identity.1, legacy_fingerprint);
        let state = service
            .state_in_scope(SHENLUN_QUESTION_ID, scope)
            .await
            .unwrap();
        assert_eq!(state.status, "idle");
        assert!(state.messages.is_empty());
        assert!(service
            .history_runs_in_scope(SHENLUN_QUESTION_ID, scope, 0, 10)
            .await
            .unwrap()
            .items
            .is_empty());
        assert!(matches!(
            service
                .request_status_in_scope(SHENLUN_QUESTION_ID, scope, LEGACY_SHARED_REQUEST_ID,)
                .await,
            Err(AiReviewError::RequestNotFound)
        ));
        assert!(matches!(
            service
                .history_events_in_scope(SHENLUN_QUESTION_ID, scope, LEGACY_SHARED_RUN_ID, 0, 10,)
                .await,
            Err(AiReviewError::RequestNotFound)
        ));
    }

    let analysis_identity = service
        .stream_identity_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis)
        .await
        .unwrap();
    let grading_identity = service
        .stream_identity_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Grading)
        .await
        .unwrap();
    assert_ne!(analysis_identity, grading_identity);

    let png = test_png();
    let ocr_attachment = service
        .attach_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Ocr, "image/png", &png)
        .await
        .unwrap();
    let analysis_attachment = service
        .attach_in_scope(
            SHENLUN_QUESTION_ID,
            AiReviewScope::Analysis,
            "image/png",
            &png,
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .start_request_in_scope(
                SHENLUN_QUESTION_ID,
                AiReviewScope::Analysis,
                ANALYSIS_CROSS_SCOPE_ID,
                Some("分析不可复用 OCR 图片".into()),
                vec![ocr_attachment],
            )
            .await,
        Err(AiReviewError::AttachmentExpired)
    ));
    assert!(matches!(
        service
            .start_request_in_scope(
                SHENLUN_QUESTION_ID,
                AiReviewScope::Grading,
                GRADING_CROSS_SCOPE_ID,
                Some("批改不可复用解析图片".into()),
                vec![analysis_attachment],
            )
            .await,
        Err(AiReviewError::AttachmentExpired)
    ));

    let analysis_initial = service
        .start_request_in_scope(
            SHENLUN_QUESTION_ID,
            AiReviewScope::Analysis,
            ANALYSIS_INITIAL_ID,
            Some("分析输入：请解释审题与要点".into()),
            Vec::new(),
        )
        .await
        .unwrap();
    let grading_initial = service
        .start_request_in_scope(
            SHENLUN_QUESTION_ID,
            AiReviewScope::Grading,
            GRADING_INITIAL_ID,
            Some("批改输入：学生作答为协同治理".into()),
            Vec::new(),
        )
        .await
        .unwrap();
    let initial_turns = [backend.next_started().await, backend.next_started().await];
    let analysis_initial_turn = initial_turns
        .iter()
        .find(|turn| turn.prompt.contains("申论真题解析"))
        .expect("analysis should use its own initial prompt")
        .clone();
    let grading_initial_turn = initial_turns
        .iter()
        .find(|turn| turn.prompt.contains("申论练习批改"))
        .expect("grading should use its own initial prompt")
        .clone();
    assert_ne!(
        analysis_initial_turn.thread_id,
        grading_initial_turn.thread_id
    );
    let analysis_persisted_thread: String = sqlx::query_scalar(
        "SELECT thread_id FROM question_ai_reviews WHERE question_id=? AND stem_fingerprint=?",
    )
    .bind(&analysis_identity.0)
    .bind(&analysis_identity.1)
    .fetch_one(&pool)
    .await
    .unwrap();
    let grading_persisted_thread: String = sqlx::query_scalar(
        "SELECT thread_id FROM question_ai_reviews WHERE question_id=? AND stem_fingerprint=?",
    )
    .bind(&grading_identity.0)
    .bind(&grading_identity.1)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(analysis_persisted_thread, analysis_initial_turn.thread_id);
    assert_eq!(grading_persisted_thread, grading_initial_turn.thread_id);

    let analysis_snapshot = service.stream_snapshot(&analysis_identity, "idle");
    let grading_snapshot = service.stream_snapshot(&grading_identity, "idle");
    assert_eq!(
        analysis_snapshot.run_id.as_deref(),
        Some(analysis_initial.request.run_id.as_str())
    );
    assert_eq!(
        grading_snapshot.run_id.as_deref(),
        Some(grading_initial.request.run_id.as_str())
    );
    assert!(analysis_snapshot
        .text
        .contains(&analysis_initial_turn.thread_id));
    assert!(!analysis_snapshot
        .text
        .contains(&grading_initial_turn.thread_id));
    assert!(grading_snapshot
        .text
        .contains(&grading_initial_turn.thread_id));
    assert!(!grading_snapshot
        .text
        .contains(&analysis_initial_turn.thread_id));

    backend.release_turns.add_permits(2);
    wait_for_status(&service, AiReviewScope::Analysis, "complete").await;
    wait_for_status(&service, AiReviewScope::Grading, "complete").await;
    let analysis_initial_events = service
        .history_events_in_scope(
            SHENLUN_QUESTION_ID,
            AiReviewScope::Analysis,
            &analysis_initial.request.run_id,
            0,
            100,
        )
        .await
        .unwrap();
    let grading_initial_events = service
        .history_events_in_scope(
            SHENLUN_QUESTION_ID,
            AiReviewScope::Grading,
            &grading_initial.request.run_id,
            0,
            100,
        )
        .await
        .unwrap();
    let analysis_delta = format!("delta-{}", analysis_initial_turn.thread_id);
    let grading_delta = format!("delta-{}", grading_initial_turn.thread_id);
    assert!(analysis_initial_events.items.iter().any(|event| {
        event.kind == "text_delta"
            && event
                .payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                == Some(analysis_delta.as_str())
    }));
    assert!(grading_initial_events.items.iter().any(|event| {
        event.kind == "text_delta"
            && event
                .payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                == Some(grading_delta.as_str())
    }));

    let analysis_follow_up = service
        .follow_up_request_in_scope(
            SHENLUN_QUESTION_ID,
            AiReviewScope::Analysis,
            ANALYSIS_FOLLOW_UP_ID,
            "分析追问：概括作答结构".into(),
            Vec::new(),
        )
        .await
        .unwrap();
    let grading_follow_up = service
        .follow_up_request_in_scope(
            SHENLUN_QUESTION_ID,
            AiReviewScope::Grading,
            GRADING_FOLLOW_UP_ID,
            "批改追问：如何补充材料依据".into(),
            Vec::new(),
        )
        .await
        .unwrap();
    let follow_up_turns = [backend.next_started().await, backend.next_started().await];
    let analysis_follow_up_turn = follow_up_turns
        .iter()
        .find(|turn| turn.prompt == "分析追问：概括作答结构")
        .expect("analysis follow-up should reach its own session")
        .clone();
    let grading_follow_up_turn = follow_up_turns
        .iter()
        .find(|turn| turn.prompt == "批改追问：如何补充材料依据")
        .expect("grading follow-up should reach its own session")
        .clone();
    assert_eq!(
        analysis_follow_up_turn.thread_id,
        analysis_initial_turn.thread_id
    );
    assert_eq!(
        grading_follow_up_turn.thread_id,
        grading_initial_turn.thread_id
    );
    let connections = backend.connections().await;
    assert!(connections.contains(&Some(analysis_initial_turn.thread_id.clone())));
    assert!(connections.contains(&Some(grading_initial_turn.thread_id.clone())));

    let stopped_analysis = service
        .stop_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis)
        .await
        .unwrap();
    assert_eq!(stopped_analysis.status, "stopped");
    assert_eq!(
        service
            .state_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Grading)
            .await
            .unwrap()
            .status,
        "running"
    );
    backend.release_turns.add_permits(1);
    wait_for_status(&service, AiReviewScope::Grading, "complete").await;
    assert!(matches!(
        service
            .retry_request_in_scope(
                SHENLUN_QUESTION_ID,
                AiRetryCommand {
                    scope: AiReviewScope::Grading,
                    source_request_id: ANALYSIS_FOLLOW_UP_ID.into(),
                    request_id: RETRY_CROSS_SCOPE_ID.into(),
                    confirm_uncertain: true,
                    confirm_new_thread: false,
                    replacement_attachments: None,
                },
            )
            .await,
        Err(AiReviewError::RequestNotFound)
    ));

    let analysis_state = service
        .state_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis)
        .await
        .unwrap();
    let grading_state = service
        .state_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Grading)
        .await
        .unwrap();
    assert!(analysis_state
        .messages
        .iter()
        .any(|message| message.content.contains("分析输入")));
    assert!(analysis_state
        .messages
        .iter()
        .any(|message| message.content.contains("分析追问")));
    assert!(!analysis_state.messages.iter().any(
        |message| message.content.contains("批改输入") || message.content.contains("批改追问")
    ));
    assert!(grading_state
        .messages
        .iter()
        .any(|message| message.content.contains("批改输入")));
    assert!(grading_state
        .messages
        .iter()
        .any(|message| message.content.contains("批改追问")));
    assert!(!grading_state.messages.iter().any(
        |message| message.content.contains("分析输入") || message.content.contains("分析追问")
    ));

    let analysis_runs = service
        .history_runs_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis, 0, 10)
        .await
        .unwrap();
    let grading_runs = service
        .history_runs_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Grading, 0, 10)
        .await
        .unwrap();
    assert_eq!(analysis_runs.items.len(), 2);
    assert_eq!(grading_runs.items.len(), 2);
    assert_eq!(
        service
            .request_status_in_scope(
                SHENLUN_QUESTION_ID,
                AiReviewScope::Analysis,
                ANALYSIS_FOLLOW_UP_ID,
            )
            .await
            .unwrap()
            .run_id,
        analysis_follow_up.request.run_id
    );
    assert_eq!(
        service
            .request_status_in_scope(
                SHENLUN_QUESTION_ID,
                AiReviewScope::Grading,
                GRADING_FOLLOW_UP_ID,
            )
            .await
            .unwrap()
            .run_id,
        grading_follow_up.request.run_id
    );
    assert!(matches!(
        service
            .request_status_in_scope(
                SHENLUN_QUESTION_ID,
                AiReviewScope::Grading,
                ANALYSIS_INITIAL_ID,
            )
            .await,
        Err(AiReviewError::RequestNotFound)
    ));
    assert!(matches!(
        service
            .history_events_in_scope(
                SHENLUN_QUESTION_ID,
                AiReviewScope::Grading,
                &analysis_initial.request.run_id,
                0,
                100,
            )
            .await,
        Err(AiReviewError::RequestNotFound)
    ));

    let deleted_grading = service
        .delete_with_confirmation_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Grading, false)
        .await
        .unwrap();
    assert_eq!(deleted_grading.status, "idle");
    assert_eq!(
        service
            .history_runs_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Grading, 0, 10)
            .await
            .unwrap()
            .items
            .len(),
        0
    );
    assert_eq!(
        service
            .state_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis)
            .await
            .unwrap()
            .status,
        "stopped"
    );
    assert_eq!(
        service
            .history_runs_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis, 0, 10)
            .await
            .unwrap()
            .items
            .len(),
        2
    );
    assert!(matches!(
        service
            .delete_with_confirmation_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis, false)
            .await,
        Err(AiReviewError::ConfirmationRequired)
    ));
    assert_eq!(
        service
            .delete_with_confirmation_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis, true)
            .await
            .unwrap()
            .status,
        "idle"
    );
    assert!(service
        .history_runs_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Analysis, 0, 10)
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(service
        .history_runs_in_scope(SHENLUN_QUESTION_ID, AiReviewScope::Grading, 0, 10)
        .await
        .unwrap()
        .items
        .is_empty());
}
