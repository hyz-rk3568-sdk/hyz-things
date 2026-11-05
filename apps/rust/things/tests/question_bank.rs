#![cfg(feature = "native")]
use hyz_things::application::question_bank::document::parse;

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

#[test]
fn whole_paper_load_preserves_number_and_material_without_answers() {
    let paper = parse(PAPER).unwrap();
    let view = paper.view();
    assert_eq!(view.questions[0].number, 26);
    assert_eq!(
        view.questions[0].materials,
        vec![view.materials[0].id.clone()]
    );
    assert_eq!(view.materials[0].markdown, "共享材料。");
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("秘密解析"));
    assert!(!json.contains("answer"));
    assert_eq!(view.year, 2001);
}

#[test]
fn first_sync_adds_three_ids_and_second_sync_is_unchanged() {
    let without = PAPER
        .lines()
        .filter(|line| !line.starts_with("<!-- hyz-"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut sequence = 0;
    let (written, count) =
        hyz_things::application::question_bank::document::complete_ids(&without, || {
            sequence += 1;
            format!("019a3d8c-7f23-7b6e-927d-{sequence:012x}")
        })
        .unwrap();
    assert_eq!(count, 3);
    assert_eq!(parse(&written).unwrap().ids().len(), 3);
    let (again, count) =
        hyz_things::application::question_bank::document::complete_ids(&written, || {
            panic!("no new UUID allowed")
        })
        .unwrap();
    assert_eq!(again, written);
    assert_eq!(count, 0);
}

use async_trait::async_trait;
use hyz_things::application::question_bank::document::Document;
use hyz_things::application::question_bank::{
    BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankApplication,
    QuestionBankRepositoryPort, QuestionBankSourcePort,
};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct MemoryBank {
    papers: Mutex<Vec<IndexedPaper>>,
    fail_source: std::sync::atomic::AtomicBool,
    loads: std::sync::atomic::AtomicUsize,
    content_error: Mutex<Option<BankError>>,
    list_error: Mutex<Option<BankError>>,
}
#[async_trait]
impl QuestionBankSourcePort for MemoryBank {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        if self.fail_source.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(BankError::Unavailable);
        }
        Ok(BankSnapshot {
            papers: vec![IndexedPaper::new(
                "papers/a.md".into(),
                "hash".into(),
                parse(PAPER).unwrap(),
            )],
            generated_ids: 0,
        })
    }
    async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
        self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(parse(PAPER).unwrap())
    }
    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}
#[async_trait]
impl QuestionBankRepositoryPort for MemoryBank {
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
        if let Some(error) = self.list_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(self.papers.lock().unwrap().clone())
    }
    async fn paper_summaries(
        &self,
    ) -> Result<Vec<hyz_things::domain::question_bank::PaperSummary>, BankError> {
        Ok(self
            .papers
            .lock()
            .unwrap()
            .iter()
            .filter(|paper| paper.summary.role == "primary")
            .map(|paper| paper.summary.clone())
            .collect())
    }
    async fn indexed_paper(&self, id: &str) -> Result<IndexedPaper, BankError> {
        self.papers
            .lock()
            .unwrap()
            .iter()
            .find(|paper| paper.summary.id == id)
            .cloned()
            .ok_or(BankError::NotFound)
    }
    async fn apply(&self, papers: Vec<IndexedPaper>) -> Result<(), BankError> {
        *self.papers.lock().unwrap() = papers;
        Ok(())
    }
    async fn paper_content(
        &self,
        paper_id: &str,
    ) -> Result<hyz_things::application::question_bank::PaperContent, BankError> {
        if let Some(error) = self.content_error.lock().unwrap().clone() {
            return Err(error);
        }
        self.papers
            .lock()
            .unwrap()
            .iter()
            .find(|paper| paper.summary.id == paper_id)
            .map(|paper| paper.content.clone())
            .ok_or(BankError::NotFound)
    }
}
#[tokio::test]
async fn failed_scan_preserves_last_synced_index() {
    let ports = Arc::new(MemoryBank::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    assert_eq!(app.sync().await.unwrap().questions, 1);
    ports
        .fail_source
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(app.sync().await, Err(BankError::Unavailable));
    assert_eq!(app.papers().await.unwrap().len(), 1);
    let paper = app
        .paper("019a3d8c-7f23-7b6e-927d-3c951851d3a2")
        .await
        .unwrap();
    assert_eq!(paper.questions[0].number, 26);
}

#[test]
fn lost_recorded_uuid_rejects_entire_write_plan() {
    use hyz_things::application::question_bank::{prepare_sync, SourceFile};
    let indexed = IndexedPaper::new("papers/a.md".into(), "hash".into(), parse(PAPER).unwrap());
    let lost = PAPER.replace(
        "<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->",
        "",
    );
    assert!(matches!(
        prepare_sync(
            vec![SourceFile {
                path: "papers/a.md".into(),
                text: lost
            }],
            vec![indexed],
            || "019a3d8e-1b32-7a60-9c83-f34217a6e211".into()
        ),
        Err(BankError::Conflict)
    ));
}

use hyz_things::application::ports::{
    AdminCredentialStorePort, AdminRandomPort, ClockPort, PlatformError, PortalControlError,
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
async fn bank_server() -> (tokio::task::JoinHandle<()>, String, String, Arc<MemoryBank>) {
    use hyz_things::{
        adapters::inbound::http::{
            app_with_admin_control_at_address, with_question_bank, with_question_bank_ai,
        },
        adapters::outbound::question_bank_ai::QuestionBankAiService,
        adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository,
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
                new_password: SecretString::new("Question-bank-test-password-2026"),
            },
        )
        .unwrap();
    let ports = Arc::new(MemoryBank::default());
    let app = Arc::new(QuestionBankApplication::new(ports.clone(), ports.clone()));
    let ai_repository = SqliteQuestionBankRepository::connect("sqlite::memory:")
        .await
        .unwrap();
    let ai = Arc::new(QuestionBankAiService::new(
        app.clone(),
        ai_repository.pool().clone(),
        None,
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = with_question_bank(
        app_with_admin_control_at_address(
            PortalStatus::new(auth.clone()),
            auth,
            admin,
            "qb-csrf".into(),
            addr.ip().to_string().parse().unwrap(),
            addr.port(),
        ),
        Some(app),
    );
    let router = with_question_bank_ai(router, Some(ai));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (
        server,
        format!("http://{addr}"),
        format!("hyz_admin_session={}", session.expose()),
        ports,
    )
}
#[tokio::test]
async fn question_bank_requires_admin_for_reads() {
    let (server, base, _cookie, _ports) = bank_server().await;
    let client = reqwest::Client::new();
    for path in [
        "papers",
        "assets/secret.png",
        "papers/019a3d8c-7f23-7b6e-927d-3c951851d3a2",
    ] {
        let response = client
            .get(format!("{base}/api/v1/question-bank/{path}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    server.abort();
}

#[tokio::test]
async fn ai_review_routes_require_admin_origin_csrf_and_typed_bounded_bodies() {
    let (server, base, cookie, _ports) = bank_server().await;
    let client = reqwest::Client::new();
    let session = "019a3d8c-7f23-7b6e-927d-3c951851d3a2";
    let question = "019a3d8e-1b32-7a60-9c83-f34217a6e210";
    let endpoint =
        format!("{base}/api/v1/question-bank/practice/sessions/{session}/questions/{question}/ai");

    let response = client.get(&endpoint).send().await.unwrap();
    assert_eq!(response.status(), 401);
    assert_eq!(response.headers()["cache-control"], "no-store");

    for (origin, csrf, body, expected) in [
        (Some(base.as_str()), Some("qb-csrf"), "{}", 401),
        (Some(base.as_str()), None, "{}", 403),
        (Some("https://other.example"), Some("qb-csrf"), "{}", 403),
        (Some("http://127.0.0.1"), Some("qb-csrf"), "{}", 403),
        (
            Some(base.as_str()),
            Some("qb-csrf"),
            "{\"unexpected\":true}",
            400,
        ),
        (Some(base.as_str()), Some("qb-csrf"), "{}", 503),
    ] {
        let mut request = client
            .post(&endpoint)
            .header("content-type", "application/json");
        if expected != 401 {
            request = request.header("cookie", &cookie);
        }
        if let Some(origin) = origin {
            request = request.header("origin", origin);
        }
        if let Some(csrf) = csrf {
            request = request.header("x-hyz-csrf", csrf);
        }
        assert_eq!(request.body(body).send().await.unwrap().status(), expected);
    }

    let response = client
        .post(format!("{endpoint}/messages"))
        .header("cookie", &cookie)
        .header("origin", &base)
        .header("x-hyz-csrf", "qb-csrf")
        .header("content-type", "application/json")
        .body(" ".repeat(9000) + "{\"message\":\"x\"}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);

    let response = client
        .get(&endpoint)
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
    server.abort();
}

#[tokio::test]
async fn sync_and_whole_paper_routes_preserve_typed_security_boundaries() {
    let (server, base, cookie, _ports) = bank_server().await;
    let client = reqwest::Client::new();
    let endpoint = format!("{base}/api/v1/control/question-bank/sync");
    for (origin, csrf, body, expected) in [
        (base.as_str(), None, "{}", 403),
        ("https://other.example", Some("qb-csrf"), "{}", 403),
        (
            base.as_str(),
            Some("qb-csrf"),
            "{\"path\":\"/etc/shadow\"}",
            400,
        ),
        (base.as_str(), Some("qb-csrf"), "{}", 202),
    ] {
        let mut request = client
            .post(&endpoint)
            .header("cookie", &cookie)
            .header("origin", origin)
            .header("content-type", "application/json")
            .body(body);
        if let Some(csrf) = csrf {
            request = request.header("x-hyz-csrf", csrf);
        }
        assert_eq!(request.send().await.unwrap().status(), expected);
    }
    let response = client
        .post(&endpoint)
        .header("cookie", &cookie)
        .header("origin", &base)
        .header("x-hyz-csrf", "qb-csrf")
        .header("content-type", "application/json")
        .body(" ".repeat(5000) + "{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
    let status_url = format!("{base}/api/v1/question-bank/sync/status");
    for _ in 0..100 {
        let status: hyz_things::domain::question_bank::BankSyncStatus = client
            .get(&status_url)
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if !status.running {
            assert_eq!(status.stage, "completed");
            break;
        }
        tokio::task::yield_now().await;
    }
    let papers: Vec<hyz_things::domain::question_bank::PaperSummary> = client
        .get(format!("{base}/api/v1/question-bank/papers"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(papers.len(), 1);
    let response = client
        .get(format!(
            "{base}/api/v1/question-bank/papers/{}",
            papers[0].id
        ))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let text = response.text().await.unwrap();
    assert!(text.contains("共享材料"));
    assert!(!text.contains("秘密解析"));
    assert!(!text.contains("answer"));
    assert_eq!(
        client
            .get(&endpoint)
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    assert_eq!(
        client
            .get(format!("{base}/api/v1/question-bank/no-such-route"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    server.abort();
}

#[test]
fn html_and_markdown_images_use_only_bank_assets() {
    use hyz_things::application::question_bank::images::rewrite_assets;
    let name = format!("{}.png", "a".repeat(64));
    let markdown = format!(
        "<img width=\"20\" src=\"../../../assets/{name}\" />\n![图](../../../assets/{name})"
    );
    let (result, references) = rewrite_assets(&markdown, "papers/国考/2001/a.md").unwrap();
    assert_eq!(references, vec![name.clone(), name.clone()]);
    assert_eq!(result.matches(&format!("hyz-bank-asset:{name}")).count(), 2);
    assert!(rewrite_assets("![](../../../../etc/shadow)", "papers/国考/2001/a.md").is_err());
    assert!(rewrite_assets("![](https://example.com/a.png)", "papers/a.md").is_err());
}

#[test]
fn misplaced_extra_uuid_is_rejected_instead_of_ignored() {
    let extra =
        PAPER.to_owned() + "\n<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e211 -->\n";
    assert!(parse(&extra).is_err());
}

#[test]
fn stable_ids_cannot_be_reused_for_a_different_unit_kind() {
    use hyz_things::application::question_bank::{prepare_sync, SourceFile};
    let old = IndexedPaper::new("papers/a.md".into(), "hash".into(), parse(PAPER).unwrap());
    let material = "019a3d8d-4a21-739a-8b15-7c54e259b610";
    let question = "019a3d8e-1b32-7a60-9c83-f34217a6e210";
    let swapped = PAPER
        .replace(material, "TEMP")
        .replace(question, material)
        .replace("TEMP", question);
    assert!(matches!(
        prepare_sync(
            vec![SourceFile {
                path: "papers/a.md".into(),
                text: swapped
            }],
            vec![old],
            || panic!("all ids exist")
        ),
        Err(BankError::Conflict)
    ));
}

#[test]
fn imported_bmp_images_remain_usable_without_transcoding() {
    let name = format!("{}.bmp", "b".repeat(64));
    let (markdown, refs) = hyz_things::application::question_bank::images::rewrite_assets(
        &format!("![](../../../assets/{name})"),
        "papers/国考/2001/a.md",
    )
    .unwrap();
    assert!(markdown.contains(&format!("hyz-bank-asset:{name}")));
    assert_eq!(refs, vec![name]);
}

#[derive(Default)]
struct SlowScan {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    starts: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl QuestionBankSourcePort for SlowScan {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        self.starts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        Ok(BankSnapshot {
            papers: vec![IndexedPaper::new(
                "papers/a.md".into(),
                "hash".into(),
                parse(PAPER).unwrap(),
            )],
            generated_ids: 0,
        })
    }
    async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
        Ok(parse(PAPER).unwrap())
    }
    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

#[tokio::test]
async fn detached_sync_survives_caller_exit_and_deduplicates_concurrent_starts() {
    use std::sync::atomic::Ordering;
    let source = Arc::new(SlowScan::default());
    let repo = Arc::new(MemoryBank::default());
    let app = Arc::new(QuestionBankApplication::new(source.clone(), repo));
    let entered = source.entered.notified();
    let first = app.start_sync().unwrap();
    assert!(first.running);
    entered.await;
    // The first request is gone; no outstanding HTTP future owns the permit.
    let second = app.start_sync().unwrap();
    assert_eq!(first.task_id, second.task_id);
    assert_eq!(source.starts.load(Ordering::SeqCst), 1);
    source.release.notify_one();
    let status = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let status = app.sync_status();
            if !status.running {
                break status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status.stage, "completed");
    assert_eq!(status.summary.unwrap().papers, 1);
    assert_eq!(source.starts.load(Ordering::SeqCst), 1);
}

#[test]
fn answer_keys_stay_server_only_and_legacy_issue_metadata_is_ignored() {
    let doc = parse(PAPER).unwrap();
    let id = doc.view().questions[0].id.clone();
    let key = doc.key(&id).unwrap();
    assert_eq!(key.choices, vec!["B"]);
    assert_eq!(key.explanation, "秘密解析。");
    assert!(key.gradable);
    assert!(!serde_json::to_string(&doc.view())
        .unwrap()
        .contains("秘密解析"));

    // Older papers may still carry the removed `issues` metadata. It must be
    // ignored without changing parsing or grading.
    let legacy = PAPER.replace(
        "topics: [现期比重]",
        "issues: [order_unverified]\ntopics: [现期比重]",
    );
    let doc = parse(&legacy).unwrap();
    assert!(doc.key(&id).unwrap().gradable);
    let content = hyz_things::application::question_bank::PaperContent::from_document("hash", &doc);
    assert!(content.questions[0].gradable);
}

#[test]
fn one_paper_module_aliases_do_not_merge_political_theory_with_common_knowledge() {
    use hyz_things::domain::question_bank::primary_module;
    assert_eq!(primary_module(Some("图形推理")), Some("判断推理"));
    assert_eq!(primary_module(Some("逻辑判断")), Some("判断推理"));
    assert_eq!(primary_module(Some("政治理论")), Some("政治理论"));
    assert_eq!(primary_module(Some("常识判断")), Some("常识判断"));
    assert_eq!(primary_module(Some("未知一级模块")), None);
}

#[tokio::test]
async fn practice_from_original_paper_is_persistent_private_and_idempotent() {
    use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
    use hyz_things::domain::question_bank::{CreatePracticeRequest, PracticeMode};
    let source = Arc::new(MemoryBank::default());
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source.clone(), repository);
    app.sync().await.unwrap();
    let paper_id = "019a3d8c-7f23-7b6e-927d-3c951851d3a2";
    let question_id = "019a3d8e-1b32-7a60-9c83-f34217a6e210";
    let modules = app.modules(paper_id).await.unwrap();
    assert_eq!(modules.modules[0].name, "资料分析");
    assert_eq!(modules.modules[0].count, 1);
    assert_eq!(modules.modules[0].ungradable, 0);
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::PaperModule,
            paper_id: Some(paper_id.into()),
            module: Some("资料分析".into()),
            category: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(session.items.len(), 1);
    assert_eq!(session.items[0].number, 26);
    let workspace = app.practice_workspace(&session.id).await.unwrap();
    let before = serde_json::to_string(&workspace).unwrap();
    assert!(!before.contains("秘密解析"));
    assert!(!before.contains("correct_answer"));
    assert!(!before.contains(r#""choices""#));
    assert!(matches!(
        app.practice_review(&session.id).await,
        Err(BankError::Conflict)
    ));
    app.save_practice_answer(&session.id, question_id, vec!["A".into()])
        .await
        .unwrap();
    let saved = app.practice_workspace(&session.id).await.unwrap();
    assert_eq!(saved.session.items[0].answer, vec!["A"]);
    let result = app.submit_practice(&session.id).await.unwrap();
    assert_eq!(result.score.unwrap().wrong, 1);
    assert_eq!(
        app.submit_practice(&session.id).await.unwrap().revision,
        result.revision
    );
    let review = app.practice_review(&session.id).await.unwrap();
    assert_eq!(review.questions[0].correct_answer, vec!["B"]);
    assert!(review.questions[0].explanation.contains("秘密解析"));
    assert!(matches!(
        app.save_practice_answer(&session.id, question_id, vec!["B".into()])
            .await,
        Err(BankError::Conflict)
    ));
    let wrong = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Wrong,
            paper_id: None,
            module: None,
            category: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(wrong.items[0].question_id, question_id);
    app.save_practice_answer(&wrong.id, question_id, vec!["B".into()])
        .await
        .unwrap();
    assert_eq!(
        app.submit_practice(&wrong.id)
            .await
            .unwrap()
            .score
            .unwrap()
            .correct,
        1
    );
    assert!(matches!(
        app.create_practice(CreatePracticeRequest {
            mode: PracticeMode::Wrong,
            paper_id: None,
            module: None,
            category: None,
            filters: Default::default(),
        })
        .await,
        Err(BankError::NotFound)
    ));
    assert_eq!(app.practice_history().await.unwrap().len(), 2);
    app.set_favorite(question_id, true).await.unwrap();
    assert!(app.favorite(question_id).await.unwrap().favorite);
    let favorites = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Favorite,
            paper_id: None,
            module: None,
            category: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(favorites.items[0].question_id, question_id);
    app.set_favorite(question_id, false).await.unwrap();
    assert!(!app.favorite(question_id).await.unwrap().favorite);
}

#[test]
fn module_aliases_are_deterministic_and_keep_politics_separate() {
    use hyz_things::domain::question_bank::primary_module;
    assert_eq!(primary_module(Some("图形推理")), Some("判断推理"));
    assert_eq!(primary_module(Some("政治理论")), Some("政治理论"));
    assert_eq!(primary_module(Some("常识判断")), Some("常识判断"));
    assert_eq!(primary_module(Some("未知模块")), None);
    assert_eq!(primary_module(None), None);
}

#[tokio::test]
async fn detached_sync_does_not_start_overlapping_scan() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct SlowSource {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        scans: AtomicUsize,
    }
    #[async_trait]
    impl QuestionBankSourcePort for SlowSource {
        async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
            self.scans.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            Ok(BankSnapshot {
                papers: vec![IndexedPaper::new(
                    "papers/a.md".into(),
                    "hash".into(),
                    parse(PAPER).unwrap(),
                )],
                generated_ids: 0,
            })
        }
        async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
            Ok(parse(PAPER).unwrap())
        }
        async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
            Err(BankError::NotFound)
        }
    }
    let source = Arc::new(SlowSource {
        entered: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
        scans: AtomicUsize::new(0),
    });
    let repo = Arc::new(MemoryBank::default());
    let app = Arc::new(QuestionBankApplication::new(source.clone(), repo));
    let first = app.start_sync().unwrap();
    assert!(first.running);
    source.entered.notified().await;
    let second = app.start_sync().unwrap();
    assert_eq!(first.task_id, second.task_id);
    assert_eq!(source.scans.load(Ordering::SeqCst), 1);
    source.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if !app.sync_status().running {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(app.sync_status().stage, "completed");
    assert_eq!(app.sync_status().summary.unwrap().questions, 1);
}

#[tokio::test]
async fn indexed_module_and_category_drills_keep_original_sources() {
    use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
    use hyz_things::domain::question_bank::{CreatePracticeRequest, PracticeMode};

    let source = Arc::new(MemoryBank::default());
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source.clone(), repo);
    app.sync().await.unwrap();

    let catalog = app.practice_catalog(&Default::default()).await.unwrap();
    let module = catalog
        .modules
        .iter()
        .find(|m| m.name == "资料分析")
        .unwrap();
    assert_eq!(module.count, 1);
    assert_eq!(module.done, 0);
    assert_eq!(module.uncategorized, 0);
    assert_eq!(module.categories[0].name, "比重");
    assert_eq!(module.categories[0].count, 1);
    assert_eq!(module.categories[0].done, 0);
    let mut filtered = hyz_things::domain::question_bank::ModulePracticeFilters {
        year_min: Some(2000),
        year_max: Some(2002),
        regions: vec!["国考".into()],
        exclude_regions: false,
        count: Some(1),
    };
    assert_eq!(
        app.practice_catalog(&filtered).await.unwrap().modules[0].count,
        1
    );
    filtered.exclude_regions = true;
    assert!(app
        .practice_catalog(&filtered)
        .await
        .unwrap()
        .modules
        .is_empty());
    filtered.exclude_regions = false;
    filtered.year_min = Some(2020);
    filtered.year_max = Some(2020);
    assert!(app
        .practice_catalog(&filtered)
        .await
        .unwrap()
        .modules
        .is_empty());

    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: Some("比重".into()),
            filters: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(session.items.len(), 1);
    assert_eq!(session.items[0].number, 26);
    assert_eq!(session.category.as_deref(), Some("比重"));
    // A draft answer does not advance progress; only a submitted answer counts.
    app.save_practice_answer(&session.id, &session.items[0].question_id, vec!["A".into()])
        .await
        .unwrap();
    app.save_practice_answer(&session.id, &session.items[0].question_id, vec!["B".into()])
        .await
        .unwrap();
    let draft = app.practice_catalog(&Default::default()).await.unwrap();
    assert_eq!(draft.modules[0].done, 0);
    app.submit_practice(&session.id).await.unwrap();
    let updated = app.practice_catalog(&Default::default()).await.unwrap();
    assert_eq!(updated.modules[0].done, 1);
    assert_eq!(updated.modules[0].categories[0].done, 1);
    assert_eq!(
        session.items[0].paper_id,
        "019a3d8c-7f23-7b6e-927d-3c951851d3a2"
    );
    assert_eq!(
        app.practice_workspace(&session.id)
            .await
            .unwrap()
            .questions
            .len(),
        1
    );

    assert!(matches!(
        app.create_practice(CreatePracticeRequest {
            mode: PracticeMode::PaperModule,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            filters: Default::default(),
        })
        .await,
        Err(BankError::Invalid)
    ));
    assert!(matches!(
        app.create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: Some("不存在的题型".into()),
            filters: Default::default(),
        })
        .await,
        Err(BankError::NotFound)
    ));
    assert!(matches!(
        app.create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: Some("019a3d8c-7f23-7b6e-927d-3c951851d3a2".into()),
            module: Some("资料分析".into()),
            category: None,
            filters: Default::default(),
        })
        .await,
        Err(BankError::Invalid)
    ));
}

#[derive(Default)]
struct CountingSource {
    loads: std::sync::atomic::AtomicUsize,
}
#[async_trait]
impl QuestionBankSourcePort for CountingSource {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: vec![IndexedPaper::new(
                "papers/a.md".into(),
                "hash".into(),
                parse(PAPER).unwrap(),
            )],
            generated_ids: 0,
        })
    }
    async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
        self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(parse(PAPER).unwrap())
    }
    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

#[tokio::test]
async fn synced_question_content_is_served_from_sqlite_without_markdown_reloads() {
    use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
    use hyz_things::domain::question_bank::{CreatePracticeRequest, PracticeMode};

    let source = Arc::new(CountingSource::default());
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source.clone(), repository);
    app.sync().await.unwrap();

    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Paper,
            paper_id: Some("019a3d8c-7f23-7b6e-927d-3c951851d3a2".into()),
            module: None,
            category: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    let workspace = app.practice_workspace(&session.id).await.unwrap();
    let question = workspace.questions[0].question.as_ref().unwrap();
    assert_eq!(question.stem, "题干。");
    assert_eq!(
        question.options_markdown,
        "- A. 甲\n- B. 乙\n- C. 丙\n- D. 丁"
    );
    assert_eq!(workspace.questions[0].materials[0].markdown, "共享材料。");

    app.save_practice_answer(&session.id, &session.items[0].question_id, vec!["B".into()])
        .await
        .unwrap();
    app.submit_practice(&session.id).await.unwrap();
    let review = app.practice_review(&session.id).await.unwrap();
    assert_eq!(review.questions[0].explanation, "秘密解析。");
    assert_eq!(review.questions[0].correct_answer, vec!["B"]);
    assert_eq!(
        source.loads.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "practice must read synced content from SQLite"
    );
}

#[tokio::test]
async fn sqlite_content_failure_is_reported_without_opening_markdown() {
    let ports = Arc::new(MemoryBank::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    app.sync().await.unwrap();
    *ports.content_error.lock().unwrap() = Some(BankError::Storage);
    assert_eq!(
        app.modules("019a3d8c-7f23-7b6e-927d-3c951851d3a2").await,
        Err(BankError::Storage)
    );
    assert_eq!(ports.loads.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn whole_paper_reads_synced_content_without_opening_markdown() {
    let ports = Arc::new(MemoryBank::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    app.sync().await.unwrap();
    let paper = app
        .paper("019a3d8c-7f23-7b6e-927d-3c951851d3a2")
        .await
        .unwrap();
    assert_eq!(paper.questions[0].stem, "题干。");
    assert_eq!(paper.materials[0].markdown, "共享材料。");
    assert_eq!(ports.loads.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn reading_one_paper_does_not_load_the_entire_catalog() {
    let ports = Arc::new(MemoryBank::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    app.sync().await.unwrap();
    *ports.list_error.lock().unwrap() = Some(BankError::Storage);
    let paper = app
        .paper("019a3d8c-7f23-7b6e-927d-3c951851d3a2")
        .await
        .unwrap();
    assert_eq!(paper.questions.len(), 1);
}

#[tokio::test]
async fn incomplete_sqlite_content_requires_sync_without_a_source_reload() {
    let ports = Arc::new(MemoryBank::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    app.sync().await.unwrap();
    ports.papers.lock().unwrap()[0].content.questions.clear();
    assert_eq!(
        app.paper("019a3d8c-7f23-7b6e-927d-3c951851d3a2").await,
        Err(BankError::Unavailable)
    );
    assert_eq!(ports.loads.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn paper_list_uses_only_the_summary_projection() {
    let ports = Arc::new(MemoryBank::default());
    let app = QuestionBankApplication::new(ports.clone(), ports.clone());
    app.sync().await.unwrap();
    *ports.list_error.lock().unwrap() = Some(BankError::Storage);
    let summaries = app.papers().await.unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].title, "示例卷");
}

#[tokio::test]
async fn practice_question_detail_reviews_any_question_without_a_session() {
    use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
    let source = Arc::new(MemoryBank::default());
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source, repository);
    app.sync().await.unwrap();
    let detail = app
        .practice_question_detail("019a3d8e-1b32-7a60-9c83-f34217a6e210")
        .await
        .unwrap();
    assert_eq!(detail.item.paper_title, "示例卷");
    assert_eq!(detail.item.year, 2001);
    assert_eq!(detail.item.region, "国考");
    assert_eq!(detail.item.number, 26);
    assert_eq!(detail.item.module.as_deref(), Some("资料分析"));
    assert_eq!(detail.question.stem, "题干。");
    assert_eq!(detail.correct_answer, vec!["B"]);
    assert_eq!(detail.explanation, "秘密解析。");
    assert_eq!(detail.materials[0].markdown, "共享材料。");
    assert!(detail.question.topics.contains(&"现期比重".to_string()));
}

#[tokio::test]
async fn practice_question_detail_rejects_invalid_and_unknown_ids() {
    use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
    let source = Arc::new(MemoryBank::default());
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source, repository);
    app.sync().await.unwrap();
    assert_eq!(
        app.practice_question_detail("bad").await,
        Err(BankError::Invalid)
    );
    assert_eq!(
        app.practice_question_detail("019a3d8e-1b32-7a60-9c83-f34217a6e2ff")
            .await,
        Err(BankError::NotFound)
    );
}
