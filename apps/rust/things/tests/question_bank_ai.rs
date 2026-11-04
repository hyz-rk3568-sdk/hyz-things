#![cfg(feature = "native")]

use async_trait::async_trait;
use hyz_things::adapters::outbound::question_bank_ai::{AiReviewError, QuestionBankAiService};
use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
use hyz_things::application::question_bank::document::parse;
use hyz_things::application::question_bank::document::Document;
use hyz_things::application::question_bank::{
    BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankApplication,
    QuestionBankSourcePort,
};
use hyz_things::domain::question_bank::{CreatePracticeRequest, PracticeMode};
use std::sync::Arc;

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
            filters: Default::default(),
        })
        .await
        .unwrap();
    bank.save_practice_answer(&session.id, QUESTION_ID, vec!["A".into()])
        .await
        .unwrap();
    bank.submit_practice(&session.id).await.unwrap();
    (
        Arc::new(QuestionBankAiService::new(bank, pool.clone(), None)),
        session.id,
        pool,
    )
}

#[tokio::test]
async fn idle_state_is_private_to_a_completed_practice_question() {
    let (service, session, _pool) = submitted_practice().await;
    let state = service.state(&session, QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "idle");
    assert!(state.messages.is_empty());
    assert_eq!(state.session_id, session);
    assert_eq!(state.question_id, QUESTION_ID);
    assert!(matches!(
        service.state(&session, OTHER_QUESTION_ID).await,
        Err(AiReviewError::Bank(BankError::NotFound))
    ));
}

#[tokio::test]
async fn start_without_codex_configuration_never_writes_or_consumes_quota() {
    let (service, session, _pool) = submitted_practice().await;
    assert!(matches!(
        service.start(&session, QUESTION_ID).await,
        Err(AiReviewError::Unavailable)
    ));
    let state = service.state(&session, QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "idle");
    assert!(state.messages.is_empty());
}

#[tokio::test]
async fn follow_up_rejects_blank_control_and_oversized_messages_before_any_task() {
    let (service, session, _pool) = submitted_practice().await;
    for message in [
        String::new(),
        "   ".into(),
        "line\rbreak".into(),
        "x".repeat(2001),
        "好".repeat(2001),
    ] {
        assert!(matches!(
            service.follow_up(&session, QUESTION_ID, message).await,
            Err(AiReviewError::Invalid)
        ));
    }
    // 700 Chinese characters exceed 2000 bytes but remain within the character limit.
    assert!(matches!(
        service
            .follow_up(&session, QUESTION_ID, "好".repeat(700))
            .await,
        Err(AiReviewError::Unavailable)
    ));
    let state = service.state(&session, QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "idle");
    assert!(state.messages.is_empty());
}

#[tokio::test]
async fn restart_marks_running_review_failed_and_keeps_history() {
    let (service, session, pool) = submitted_practice().await;
    sqlx::query("INSERT INTO question_ai_reviews(session_id,question_id,source_hash,thread_id,status,error,updated_at) VALUES(?,?,?,?, 'running',NULL,1)")
        .bind(&session)
        .bind(QUESTION_ID)
        .bind("hash-ai")
        .bind("thread-before-restart")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO question_ai_messages(session_id,question_id,role,content,created_at) VALUES(?,?,'assistant','已有解析',2)")
        .bind(&session)
        .bind(QUESTION_ID)
        .execute(&pool)
        .await
        .unwrap();
    service.recover_interrupted().await.unwrap();
    let state = service.state(&session, QUESTION_ID).await.unwrap();
    assert_eq!(state.status, "failed");
    assert!(state
        .error
        .as_deref()
        .is_some_and(|error| error.contains("设备服务重启")));
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].content, "已有解析");
}
