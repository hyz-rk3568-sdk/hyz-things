#![cfg(feature = "native")]
use async_trait::async_trait;
use hyz_things::{
    application::{
        favorite_flashcard_ports::{
            FavoriteFlashcardWriterPort, FavoriteWriteResult, NewFavoriteCard,
        },
        favorite_flashcards::{
            CreateFavoriteFlashcard, FavoriteFlashcardApplication, FavoriteQuestion,
            FavoriteQuestionPort,
        },
        flashcards::{
            FlashcardApplication, FlashcardError, FlashcardRepositoryPort, FlashcardSourcePort,
        },
        ports::ClockPort,
        question_bank::BankError,
    },
    domain::{favorite_flashcard::FlashcardCreationMode, flashcards::*},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
const QUESTION: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e210";
const CARD: &str = "019a31b7-8c42-7c16-a6d1-0c1f0f34c821";
#[derive(Default)]
struct Fake {
    written: Mutex<Vec<NewFavoriteCard>>,
    indexed: Mutex<Vec<SourceCard>>,
    fail_sync: AtomicBool,
    deny_favorite: AtomicBool,
    coarse_category: AtomicBool,
    block_scan: AtomicBool,
    scan_entered: tokio::sync::Notify,
    scan_release: tokio::sync::Notify,
}
#[async_trait]
impl FavoriteQuestionPort for Fake {
    async fn load(&self, id: &str) -> Result<FavoriteQuestion, BankError> {
        if self.deny_favorite.load(Ordering::SeqCst) {
            return Err(BankError::NotFound);
        }
        Ok(FavoriteQuestion {
            question_id: id.into(),
            module: Some("数量关系".into()),
            category: Some(
                if self.coarse_category.load(Ordering::SeqCst) {
                    "数学运算"
                } else {
                    "工程效率"
                }
                .into(),
            ),
            front: "材料\n\n题干\n- A. 甲\n- B. 乙".into(),
            correct_answer: vec!["B".into()],
            source_revision: "v1".into(),
            gradable: true,
        })
    }
    async fn asset(&self, _: String) -> Result<Vec<u8>, BankError> {
        Err(BankError::NotFound)
    }
}
#[async_trait]
impl FavoriteFlashcardWriterPort for Fake {
    async fn append(&self, card: NewFavoriteCard) -> Result<FavoriteWriteResult, FlashcardError> {
        let mut cards = self.written.lock().unwrap();
        let appended = !cards.iter().any(|existing| {
            existing.question_id == card.question_id && existing.target_deck == card.target_deck
        });
        let relative_path = format!("{}/cards.md", card.target_deck);
        if appended {
            cards.push(card);
        }
        Ok(FavoriteWriteResult {
            relative_path,
            card_id: None,
            appended,
        })
    }
}
#[async_trait]
impl FlashcardSourcePort for Fake {
    async fn scan(&self) -> Result<SourceSnapshot, FlashcardError> {
        if self.block_scan.load(Ordering::SeqCst) {
            self.scan_entered.notify_one();
            self.scan_release.notified().await;
        }
        if self.fail_sync.load(Ordering::SeqCst) {
            return Err(FlashcardError::SourceUnavailable("offline".into()));
        }
        let cards: Vec<_> = self
            .written
            .lock()
            .unwrap()
            .iter()
            .map(|card| SourceCard {
                id: FlashcardId::new(CARD).unwrap(),
                source_question_id: Some(card.question_id.clone()),
                correct_answer: card.correct_answer.clone(),
                source_file: format!("{}/cards.md", card.target_deck),
                source_order: 0,
                deck_path: card.target_deck.clone(),
                front_markdown: card.front.clone(),
                back_markdown: card.back.clone(),
                tags: vec![],
                source_hash: "hash".into(),
                generated_id: true,
            })
            .collect();
        let successful_markdown_files = cards.iter().map(|card| card.source_file.clone()).collect();
        Ok(SourceSnapshot {
            cards,
            complete: true,
            successful_markdown_files,
            ..Default::default()
        })
    }
}
impl ClockPort for Fake {
    fn unix_time_millis(&self) -> u64 {
        1_800_000_000_000
    }
}
#[async_trait]
impl FlashcardRepositoryPort for Fake {
    async fn apply_sync(
        &self,
        snapshot: SourceSnapshot,
        _: u64,
    ) -> Result<SyncSummary, FlashcardError> {
        *self.indexed.lock().unwrap() = snapshot.cards;
        Ok(SyncSummary::default())
    }
    async fn list_cards(
        &self,
        _: FlashcardFilter,
        _: u64,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
        Ok(vec![])
    }
    async fn list_review_queue(
        &self,
        _: FlashcardFilter,
        _: u64,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
        Ok(vec![])
    }
    async fn list_decks(&self) -> Result<Vec<DeckSummary>, FlashcardError> {
        Ok(vec![])
    }
    async fn list_tags(&self) -> Result<Vec<TagSummary>, FlashcardError> {
        Ok(vec![])
    }
    async fn summary(&self, _: u64) -> Result<StudySummary, FlashcardError> {
        Ok(StudySummary::default())
    }
    async fn review_target(
        &self,
        id: &FlashcardId,
        _: u64,
    ) -> Result<Option<ReviewTarget>, FlashcardError> {
        Ok(self
            .indexed
            .lock()
            .unwrap()
            .iter()
            .find(|card| card.id == *id)
            .map(|card| ReviewTarget {
                card: FlashcardSummary {
                    id: card.id.clone(),
                    source_file: card.source_file.clone(),
                    source_order: 0,
                    deck_path: card.deck_path.clone(),
                    front_markdown: card.front_markdown.clone(),
                    back_markdown: card.back_markdown.clone(),
                    tags: vec![],
                    active: true,
                    due_at_ms: None,
                    last_reviewed_at_ms: None,
                    state: ReviewStateKind::New,
                    review_count: 0,
                    lapse_count: 0,
                    is_new: true,
                },
                state: None,
            }))
    }
    async fn record_review(
        &self,
        _: &FlashcardId,
        _: ReviewState,
        _: ReviewLog,
    ) -> Result<(FlashcardSummary, ReviewLog), FlashcardError> {
        Err(FlashcardError::NotFound)
    }
}
fn setup() -> (
    FavoriteFlashcardApplication,
    Arc<Fake>,
    Arc<FlashcardApplication>,
) {
    let fake = Arc::new(Fake::default());
    let cards = Arc::new(FlashcardApplication::new(
        fake.clone(),
        fake.clone(),
        fake.clone(),
    ));
    (
        FavoriteFlashcardApplication::new(fake.clone(), cards.clone(), fake.clone()),
        fake,
        cards,
    )
}
fn request() -> CreateFavoriteFlashcard {
    CreateFavoriteFlashcard {
        target_deck: "真题练习/数量关系/工程效率/错题".into(),
        creation_mode: Some(FlashcardCreationMode::Practice),
        back: "".into(),
        source_revision: "v1".into(),
        type_confirmed: true,
    }
}
#[tokio::test]
async fn creates_blank_back_as_dash_and_syncs_source_generated_id() {
    let (app, fake, _) = setup();
    let result = app.create(QUESTION, request()).await.unwrap();
    assert_eq!(result.status, "created_synced");
    assert_eq!(result.card_id.as_deref(), Some(CARD));
    let written = fake.written.lock().unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].back, "-");
    assert_eq!(written[0].correct_answer, vec!["B"]);
}

use hyz_things::application::ports::{
    AdminCredentialStorePort, AdminRandomPort, PlatformError, PortalControlError,
    PortalControlHandler,
};
use hyz_things::domain::admin::{
    AdminCredential, AdminLoginRequest, AdminPasswordChangeRequest, SecretString,
};
#[derive(Default)]
struct AuthPorts {
    credential: Mutex<Option<AdminCredential>>,
    sequence: std::sync::atomic::AtomicUsize,
}
impl AdminCredentialStorePort for AuthPorts {
    fn load_admin_credential(&self) -> Result<Option<AdminCredential>, PlatformError> {
        Ok(self.credential.lock().unwrap().clone())
    }
    fn save_admin_credential(&self, c: &AdminCredential) -> Result<(), PlatformError> {
        *self.credential.lock().unwrap() = Some(c.clone());
        Ok(())
    }
}
impl AdminRandomPort for AuthPorts {
    fn fill_random(&self, b: &mut [u8]) -> Result<(), PlatformError> {
        let n = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (i, b) in b.iter_mut().enumerate() {
            *b = n.wrapping_add(i) as u8;
        }
        Ok(())
    }
}
impl ClockPort for AuthPorts {
    fn unix_time_millis(&self) -> u64 {
        100
    }
}
#[async_trait]
impl PortalControlHandler for AuthPorts {
    async fn handle(
        &self,
        _op: hyz_contract::router::ControlOperation,
    ) -> Result<hyz_contract::router::ControlResult, PortalControlError> {
        Err(PortalControlError::Transport("offline".into()))
    }
}

async fn server() -> (tokio::task::JoinHandle<()>, String, String, Arc<Fake>) {
    use hyz_things::{
        adapters::inbound::http::{app_with_admin_control_at_address, with_favorite_flashcards},
        application::{admin::AdminApplication, status::PortalStatus},
    };
    let auth = Arc::new(AuthPorts::default());
    let admin =
        Arc::new(AdminApplication::initialize(auth.clone(), auth.clone(), auth.clone()).unwrap());
    let session = admin
        .login(&AdminLoginRequest {
            password: SecretString::new("admin"),
        })
        .unwrap()
        .token;
    admin
        .change_password(
            &session,
            &AdminPasswordChangeRequest {
                current_password: SecretString::new("admin"),
                new_password: SecretString::new("Favorite-flashcard-test-2026"),
            },
        )
        .unwrap();
    let (app, fake, _) = setup();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = with_favorite_flashcards(
        app_with_admin_control_at_address(
            PortalStatus::new(auth.clone()),
            auth,
            admin,
            "fc-csrf".into(),
            std::net::Ipv4Addr::LOCALHOST,
            addr.port(),
        ),
        Some(Arc::new(app)),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (
        task,
        format!("http://{addr}"),
        format!("hyz_admin_session={}", session.expose()),
        fake,
    )
}
#[tokio::test]
async fn favorite_flashcard_http_requires_admin_origin_csrf_and_typed_body() {
    let (task, base, cookie, fake) = server().await;
    let client = reqwest::Client::new();
    let root = format!("{base}/api/v1/question-bank/practice/questions/{QUESTION}");
    let response = client
        .get(format!("{root}/flashcard-preview"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let preview = client
        .get(format!("{root}/flashcard-preview"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(preview.status(), 200);
    let value: serde_json::Value = preview.json().await.unwrap();
    assert_eq!(value["correct_answer"], serde_json::json!(["B"]));
    for (origin, csrf, body, expected) in [
        (
            base.as_str(),
            None,
            serde_json::to_string(&request()).unwrap(),
            403,
        ),
        (
            "https://foreign.example",
            Some("fc-csrf"),
            serde_json::to_string(&request()).unwrap(),
            403,
        ),
        (
            base.as_str(),
            Some("fc-csrf"),
            "{\"path\":\"/tmp/escape\"}".into(),
            400,
        ),
        (
            base.as_str(),
            Some("fc-csrf"),
            serde_json::to_string(&request()).unwrap(),
            200,
        ),
    ] {
        let mut call = client
            .post(format!("{root}/flashcard"))
            .header("cookie", &cookie)
            .header("origin", origin)
            .header("content-type", "application/json")
            .body(body);
        if let Some(csrf) = csrf {
            call = call.header("x-hyz-csrf", csrf);
        }
        assert_eq!(call.send().await.unwrap().status(), expected);
    }
    assert_eq!(fake.written.lock().unwrap().len(), 1);
    task.abort();
}

const PAPER: &str = r#"---
schema: hyz-paper/v1
title: 示例卷
year: 2001
exam: 国考
region: 国考
variant: 统考
status: unverified
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a2 -->
# 示例卷
## Material M1
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-7c54e259b610 -->
共享材料。
## Question 26
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->
```yaml
module: 资料分析
category: 比重
topics: [现期比重]
materials: [M1]
```
### Stem
题干。
### Options
- A. 甲
- B. 乙
- C. 丙
- D. 丁
### Answer
B
### Explanation
秘密解析。
"#;

use hyz_things::application::question_bank::{
    document::{parse, Document},
    BankAsset, BankSnapshot, IndexedPaper, PaperContent, PracticeContent, QuestionBankApplication,
    QuestionBankRepositoryPort, QuestionBankSourcePort,
};
struct BankFake {
    favorite: AtomicBool,
    stale: AtomicBool,
}
impl BankFake {
    fn indexed(&self) -> IndexedPaper {
        IndexedPaper::new("papers/test.md".into(), "v1".into(), parse(PAPER).unwrap())
    }
}
#[async_trait]
impl QuestionBankSourcePort for BankFake {
    async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        unreachable!()
    }
    async fn load(&self, _: IndexedPaper) -> Result<Document, BankError> {
        if self.stale.load(Ordering::SeqCst) {
            Err(BankError::Stale)
        } else {
            Ok(parse(PAPER).unwrap())
        }
    }
    async fn asset(&self, _: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}
#[async_trait]
impl QuestionBankRepositoryPort for BankFake {
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
        Ok(vec![self.indexed()])
    }
    async fn apply(&self, _: Vec<IndexedPaper>) -> Result<(), BankError> {
        unreachable!()
    }
    async fn indexed_paper(&self, _: &str) -> Result<IndexedPaper, BankError> {
        Ok(self.indexed())
    }
    async fn practice_is_favorite(&self, _: &str) -> Result<bool, BankError> {
        Ok(self.favorite.load(Ordering::SeqCst))
    }
    async fn question_contents(&self, _: &[String]) -> Result<Vec<PracticeContent>, BankError> {
        let indexed = self.indexed();
        let content = PaperContent::from_document("v1", &parse(PAPER).unwrap());
        Ok(vec![PracticeContent {
            paper_id: indexed.summary.id,
            paper_title: indexed.summary.title,
            source_hash: "v1".into(),
            active: true,
            question: content.questions[0].clone(),
            materials: content.materials,
        }])
    }
}
#[tokio::test]
async fn favorite_content_loads_material_options_and_server_answer_without_submission_gate() {
    let fake = Arc::new(BankFake {
        favorite: AtomicBool::new(true),
        stale: AtomicBool::new(false),
    });
    let bank = QuestionBankApplication::new(fake.clone(), fake.clone());
    let q = FavoriteQuestionPort::load(&bank, QUESTION).await.unwrap();
    assert!(q.front.contains("共享材料。"));
    assert!(q.front.contains("题干。"));
    assert!(q.front.contains("- D. 丁"));
    assert!(!q.front.contains("秘密解析"));
    assert_eq!(q.correct_answer, vec!["B"]);
    fake.favorite.store(false, Ordering::SeqCst);
    // Cards can be created from any indexed question without first favoriting it.
    assert!(FavoriteQuestionPort::load(&bank, QUESTION).await.is_ok());
    fake.stale.store(true, Ordering::SeqCst);
    assert!(matches!(
        FavoriteQuestionPort::load(&bank, QUESTION).await,
        Err(BankError::Stale)
    ));
}

#[tokio::test]
async fn ai_and_default_requests_validate_against_their_own_target_sets() {
    let (app, fake, _) = setup();
    let mut default = request();
    default.target_deck = "真题练习/数量关系/工程效率/错题".into();
    default.creation_mode = Some(FlashcardCreationMode::Practice);
    let result = app.create(QUESTION, default).await.unwrap();
    assert_eq!(result.status, "created_synced");
    let mut wrong_mode = request();
    wrong_mode.target_deck = "真题练习/数量关系/工程效率/错题".into();
    wrong_mode.creation_mode = Some(FlashcardCreationMode::UserOrganized);
    assert!(app.create(QUESTION, wrong_mode).await.is_err());
    assert_eq!(fake.written.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn web_can_save_directly_to_user_organized_as_an_ai_source() {
    let (app, fake, _) = setup();
    let mut request = request();
    request.target_deck = "用户整理/数量关系/工程效率/错题".into();
    request.creation_mode = Some(FlashcardCreationMode::UserOrganized);
    let result = app.create(QUESTION, request).await.unwrap();
    assert_eq!(result.status, "created_synced");
    assert_eq!(
        fake.written.lock().unwrap()[0].target_deck,
        "用户整理/数量关系/工程效率/错题"
    );
}

#[tokio::test]
async fn sync_failure_keeps_markdown_and_retry_reuses_the_source_question() {
    let (app, fake, _) = setup();
    fake.fail_sync.store(true, Ordering::SeqCst);
    let pending = app.create(QUESTION, request()).await.unwrap();
    assert_eq!(pending.status, "created_sync_pending");
    assert!(pending.retryable);
    assert_eq!(fake.written.lock().unwrap().len(), 1);
    fake.fail_sync.store(false, Ordering::SeqCst);
    let existing = app.create(QUESTION, request()).await.unwrap();
    assert_eq!(existing.status, "already_exists");
    assert!(!existing.retryable);
    assert_eq!(existing.card_id.as_deref(), Some(CARD));
    assert_eq!(fake.written.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn stale_preview_and_cross_module_target_do_not_write() {
    let (app, fake, _) = setup();
    let mut stale = request();
    stale.source_revision = "old".into();
    assert!(app.create(QUESTION, stale).await.is_err());
    let mut foreign = request();
    foreign.target_deck = "用户整理/判断推理/形式推理/直言命题/错题".into();
    assert!(app.create(QUESTION, foreign).await.is_err());
    assert!(fake.written.lock().unwrap().is_empty());
    fake.deny_favorite.store(true, Ordering::SeqCst);
    assert!(app.create(QUESTION, request()).await.is_err());
    assert!(fake.written.lock().unwrap().is_empty());
}

#[tokio::test]
async fn coarse_category_preserves_question_category_in_both_destinations() {
    let (app, fake, _) = setup();
    fake.coarse_category.store(true, Ordering::SeqCst);
    let preview = app.preview(QUESTION).await.unwrap();
    assert!(!preview.eligibility.requires_type_confirmation);
    assert_eq!(preview.eligibility.allowed_targets.len(), 2);
    assert_eq!(
        preview.eligibility.recommended_target.as_deref(),
        Some("真题练习/数量关系/数学运算/错题")
    );
    assert!(app.create(QUESTION, request()).await.is_err());
    assert!(fake.written.lock().unwrap().is_empty());
    let mut confirmed = request();
    confirmed.target_deck = "真题练习/数量关系/数学运算/错题".into();
    assert_eq!(
        app.create(QUESTION, confirmed).await.unwrap().status,
        "created_synced"
    );
}

#[tokio::test]
async fn creation_and_manual_sync_share_one_write_gate() {
    let (app, fake, cards) = setup();
    let app = Arc::new(app);
    fake.block_scan.store(true, Ordering::SeqCst);
    let worker = tokio::spawn({
        let app = app.clone();
        async move { app.create(QUESTION, request()).await }
    });
    fake.scan_entered.notified().await;
    assert!(matches!(cards.sync().await, Err(FlashcardError::Busy)));
    assert!(app.create(QUESTION, request()).await.is_err());
    fake.scan_release.notify_one();
    assert_eq!(worker.await.unwrap().unwrap().status, "created_synced");
    assert_eq!(fake.written.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancelled_creation_keeps_write_gate_until_its_worker_finishes() {
    let (app, fake, cards) = setup();
    let app = Arc::new(app);
    fake.block_scan.store(true, Ordering::SeqCst);
    let caller = tokio::spawn({
        let app = app.clone();
        async move { app.create(QUESTION, request()).await }
    });
    fake.scan_entered.notified().await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    // The scan models a filesystem operation that can outlive an HTTP request.
    let is_busy = tokio::select! {
        biased;
        result = cards.sync() => matches!(result, Err(FlashcardError::Busy)),
        _ = tokio::task::yield_now() => false,
    };
    fake.scan_release.notify_one();
    assert!(
        is_busy,
        "request cancellation released the filesystem write gate"
    );
}

#[tokio::test]
async fn cancelled_manual_sync_keeps_write_gate_until_its_worker_finishes() {
    let (_, fake, cards) = setup();
    fake.block_scan.store(true, Ordering::SeqCst);
    let caller = tokio::spawn({
        let cards = cards.clone();
        async move { cards.sync().await }
    });
    fake.scan_entered.notified().await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let is_busy = tokio::select! {
        biased;
        result = cards.sync() => matches!(result, Err(FlashcardError::Busy)),
        _ = tokio::task::yield_now() => false,
    };
    fake.scan_release.notify_one();
    assert!(is_busy, "cancelled sync released its filesystem write gate");
}
