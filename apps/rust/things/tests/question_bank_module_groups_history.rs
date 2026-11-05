#![cfg(feature = "native")]

use async_trait::async_trait;
use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
use hyz_things::application::question_bank::{
    document::{parse, Document},
    BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankApplication,
    QuestionBankSourcePort,
};
use hyz_things::domain::question_bank::{
    CreatePracticeRequest, ModulePracticeFilters, PracticeMode,
};
use std::sync::Arc;

struct Source(Vec<IndexedPaper>);
#[async_trait]
impl QuestionBankSourcePort for Source {
    async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: self.0.clone(),
            generated_ids: 0,
        })
    }
    async fn load(&self, _: IndexedPaper) -> Result<Document, BankError> {
        Err(BankError::NotFound)
    }
    async fn asset(&self, _: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

fn paper(n: u32, region: &str, stems: &[&str]) -> IndexedPaper {
    let mut text = format!("---\nschema: hyz-paper/v1\ntitle: 测试卷\nyear: 2019\nexam: 省考\nregion: {region}\nvariant: 统考\nstatus: unverified\n---\n<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-{n:012x} -->\n# 测试卷\n");
    for (i, stem) in stems.iter().enumerate() {
        let id = n * 10 + i as u32;
        text.push_str(&format!("## Question {}\n<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-{id:012x} -->\n```yaml\nmodule: 常识判断\ncategory: 常识\nmaterials: []\n```\n### Stem\n{stem}\n### Options\n- A. 甲\n- B. 乙\n### Answer\nA\n### Explanation\n解析\n", i + 1));
    }
    IndexedPaper::new(
        format!("papers/{region}.md"),
        "hash".into(),
        parse(&text).unwrap(),
    )
}

async fn application_with_stems(stems: &[&str]) -> QuestionBankApplication {
    let source = Arc::new(Source(vec![
        paper(1, "湖南", stems),
        paper(2, "江西", &["第一题"]),
    ]));
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source, repo);
    app.sync().await.unwrap();
    app
}

fn module_request(region: &str, count: usize) -> CreatePracticeRequest {
    CreatePracticeRequest {
        mode: PracticeMode::Module,
        paper_id: None,
        module: Some("常识判断".into()),
        category: None,
        filters: ModulePracticeFilters {
            regions: vec![region.into()],
            count: Some(count),
            ..Default::default()
        },
    }
}

async fn answer_and_submit(app: &QuestionBankApplication, region: &str, answer: &str) {
    let session = app
        .create_practice(module_request(region, 1))
        .await
        .unwrap();
    app.save_practice_answer(
        &session.id,
        &session.items[0].question_id,
        vec![answer.into()],
    )
    .await
    .unwrap();
    app.submit_practice(&session.id).await.unwrap();
}

#[tokio::test]
async fn served_history_is_shared_across_filtered_versions() {
    let app = application_with_stems(&["第一题", "第二题"]).await;
    app.create_practice(module_request("江西", 1))
        .await
        .unwrap();
    let session = app
        .create_practice(module_request("湖南", 1))
        .await
        .unwrap();
    assert_eq!(
        session.items[0].question_id,
        "019a3d8e-1b32-7a60-9c83-00000000000b"
    );
}

#[tokio::test]
async fn wrong_history_is_shared_across_filtered_versions() {
    let app = application_with_stems(&["第二题", "第一题"]).await;
    let target = app
        .create_practice(module_request("湖南", 2))
        .await
        .unwrap();
    app.save_practice_answer(&target.id, &target.items[0].question_id, vec!["A".into()])
        .await
        .unwrap();
    app.submit_practice(&target.id).await.unwrap();
    answer_and_submit(&app, "江西", "B").await;
    let selected = app
        .create_practice(module_request("湖南", 1))
        .await
        .unwrap();
    assert_eq!(
        selected.items[0].question_id,
        "019a3d8e-1b32-7a60-9c83-00000000000b"
    );
}

#[tokio::test]
async fn correct_history_offsets_wrong_answers_across_versions() {
    let app = application_with_stems(&["第一题", "第二题"]).await;
    let first = "019a3d8e-1b32-7a60-9c83-00000000000a";
    let second = "019a3d8e-1b32-7a60-9c83-00000000000b";
    for round in 0..2 {
        let session = app
            .create_practice(module_request("湖南", 2))
            .await
            .unwrap();
        app.save_practice_answer(&session.id, first, vec!["B".into()])
            .await
            .unwrap();
        if round == 0 {
            app.save_practice_answer(&session.id, second, vec!["B".into()])
                .await
                .unwrap();
        }
        app.submit_practice(&session.id).await.unwrap();
    }
    answer_and_submit(&app, "江西", "A").await;
    let selected = app
        .create_practice(module_request("湖南", 1))
        .await
        .unwrap();
    assert_eq!(selected.items[0].question_id, second);
}

#[tokio::test]
async fn answered_history_outside_region_deprioritizes_equivalent_question() {
    let app = application_with_stems(&["第一题", "第二题"]).await;
    answer_and_submit(&app, "江西", "A").await;
    let session = app
        .create_practice(module_request("湖南", 1))
        .await
        .unwrap();
    assert_eq!(
        session.items[0].question_id,
        "019a3d8e-1b32-7a60-9c83-00000000000b"
    );
}
