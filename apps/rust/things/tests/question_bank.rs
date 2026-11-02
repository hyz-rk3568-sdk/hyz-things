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
issues: []
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
        Ok(parse(PAPER).unwrap())
    }
    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}
#[async_trait]
impl QuestionBankRepositoryPort for MemoryBank {
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
        Ok(self.papers.lock().unwrap().clone())
    }
    async fn apply(&self, papers: Vec<IndexedPaper>) -> Result<(), BankError> {
        *self.papers.lock().unwrap() = papers;
        Ok(())
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
        adapters::inbound::http::{app_with_admin_control_at_address, with_question_bank},
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
        self.starts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        Ok(BankSnapshot {
            papers: vec![IndexedPaper::new(
                "papers/a.md".into(), "hash".into(), parse(PAPER).unwrap(),
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
    }).await.unwrap();
    assert_eq!(status.stage, "completed");
    assert_eq!(status.summary.unwrap().papers, 1);
    assert_eq!(source.starts.load(Ordering::SeqCst), 1);
}
