#![cfg(feature = "native")]

use std::sync::Arc;

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

fn paper(paper_id: &str, question_id: &str, region: &str, stem: &str) -> IndexedPaper {
    let text = format!(
        r#"---
schema: hyz-paper/v1
title: 测试卷
year: 2019
exam: 省考
region: {region}
variant: 统考
status: unverified
---
<!-- hyz-paper-id: {paper_id} -->
# 测试卷
## Question 1
<!-- hyz-question-id: {question_id} -->
```yaml
module: 常识判断
category: 常识
topics: []
materials: []
```
### Stem
{stem}
### Options
- A. 甲
- B. 乙
### Answer
A
### Explanation
解析
"#
    );
    IndexedPaper::new(
        format!("papers/{region}.md"),
        "hash".into(),
        parse(&text).unwrap(),
    )
}

fn source() -> Arc<Source> {
    Arc::new(Source(vec![
        paper(
            "019a3d8c-7f23-7b6e-927d-3c951851d3a2",
            "019a3d8e-1b32-7a60-9c83-f34217a6e210",
            "湖南",
            "相同的完整题干。",
        ),
        paper(
            "019a3d8c-7f23-7b6e-927d-3c951851d3a3",
            "019a3d8e-1b32-7a60-9c83-f34217a6e211",
            "江西",
            "相同的完整题干。",
        ),
    ]))
}

#[tokio::test]
async fn module_catalog_counts_identical_questions_once() {
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source(), repo);
    app.sync().await.unwrap();
    let catalog = app
        .practice_catalog(&ModulePracticeFilters::default())
        .await
        .unwrap();
    assert_eq!(catalog.modules[0].count, 1);
}

#[tokio::test]
async fn module_progress_shares_answers_across_filtered_paper_versions() {
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source(), repo);
    app.sync().await.unwrap();
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Paper,
            paper_id: Some("019a3d8c-7f23-7b6e-927d-3c951851d3a3".into()),
            module: None,
            category: None,
            collection_id: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    app.save_practice_answer(&session.id, &session.items[0].question_id, vec!["A".into()])
        .await
        .unwrap();
    app.submit_practice(&session.id).await.unwrap();
    let filters = ModulePracticeFilters {
        regions: vec!["湖南".into()],
        ..Default::default()
    };
    let catalog = app.practice_catalog(&filters).await.unwrap();
    assert_eq!((catalog.modules[0].count, catalog.modules[0].done), (1, 1));
}

#[tokio::test]
async fn module_practice_selects_one_version_of_identical_questions() {
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source(), repo);
    app.sync().await.unwrap();
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("常识判断".into()),
            category: None,
            collection_id: None,
            filters: ModulePracticeFilters {
                count: Some(10),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(session.items.len(), 1);
}

fn material_paper(n: u32, region: &str) -> IndexedPaper {
    let text = format!(
        r#"---
schema: hyz-paper/v1
title: 材料卷
year: 2019
exam: 省考
region: {region}
variant: 统考
status: unverified
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-{n:012x} -->
## Material M1
<!-- hyz-material-id: 019a3d8d-7f23-7b6e-927d-{n:012x} -->
共享材料。
## Question 1
<!-- hyz-question-id: 019a3d8e-7f23-7b6e-927d-{n:012x} -->
```yaml
module: 资料分析
category: 比重
materials: [M1]
```
### Stem
第一道题？
### Options
- A. 甲
- B. 乙
### Answer
A
## Question 2
<!-- hyz-question-id: 019a3d8f-7f23-7b6e-927d-{n:012x} -->
```yaml
module: 资料分析
category: 比重
materials: [M1]
```
### Stem
第二道题？
### Options
- A. 甲
- B. 乙
### Answer
A
"#
    );
    IndexedPaper::new(
        format!("papers/{region}.md"),
        "hash".into(),
        parse(&text).unwrap(),
    )
}

#[tokio::test]
async fn duplicate_material_blocks_keep_one_complete_paper_group() {
    let source = Arc::new(Source(vec![
        material_paper(1, "湖南"),
        material_paper(2, "江西"),
    ]));
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source, repo);
    app.sync().await.unwrap();
    let catalog = app.practice_catalog(&Default::default()).await.unwrap();
    assert_eq!(catalog.modules[0].count, 2);
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            collection_id: None,
            filters: ModulePracticeFilters {
                count: Some(10),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(session.items.len(), 2);
    assert_eq!(session.items[0].paper_id, session.items[1].paper_id);
}

#[tokio::test]
async fn repeated_questions_within_material_group_are_not_all_discarded() {
    let mut paper = material_paper(1, "湖南");
    paper.content.questions[1].stem = paper.content.questions[0].stem.clone();
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(Arc::new(Source(vec![paper])), repo);
    app.sync().await.unwrap();
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            collection_id: None,
            filters: ModulePracticeFilters {
                count: Some(10),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(session.items.len(), 1);
}

#[tokio::test]
async fn resync_keeps_progress_and_paper_history() {
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source(), repo);
    app.sync().await.unwrap();
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Paper,
            paper_id: Some("019a3d8c-7f23-7b6e-927d-3c951851d3a2".into()),
            module: None,
            category: None,
            collection_id: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    let id = &session.items[0].question_id;
    app.save_practice_answer(&session.id, id, vec!["A".into()])
        .await
        .unwrap();
    app.submit_practice(&session.id).await.unwrap();
    app.set_favorite(id, true).await.unwrap();
    app.sync().await.unwrap();
    let catalog = app.practice_catalog(&Default::default()).await.unwrap();
    assert_eq!((catalog.modules[0].count, catalog.modules[0].done), (1, 1));
    assert!(app.favorite(id).await.unwrap().favorite);
    assert_eq!(app.practice_history().await.unwrap().len(), 1);
}

#[tokio::test]
async fn module_total_deduplicates_across_different_categories() {
    let mut other = paper(
        "019a3d8c-7f23-7b6e-927d-3c951851d3a3",
        "019a3d8e-1b32-7a60-9c83-f34217a6e211",
        "江西",
        "相同的完整题干。",
    );
    other.content.questions[0].category = Some("其他".into());
    other.question_categories[0] = Some("其他".into());
    let first = source().0[0].clone();
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(Arc::new(Source(vec![first, other])), repo);
    app.sync().await.unwrap();
    let catalog = app.practice_catalog(&Default::default()).await.unwrap();
    assert_eq!(catalog.modules[0].count, 1);
    assert_eq!(catalog.modules[0].categories.len(), 2);
    assert!(catalog.modules[0].categories.iter().all(|c| c.count == 1));
}

#[tokio::test]
async fn partially_overlapping_material_groups_keep_unique_questions() {
    let first = material_paper(1, "湖南");
    let mut second = material_paper(2, "江西");
    second.content.questions[0].stem = first.content.questions[1].stem.clone();
    second.content.questions[1].stem = "第三道题？".into();
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(Arc::new(Source(vec![first, second])), repo);
    app.sync().await.unwrap();
    assert_eq!(
        app.practice_catalog(&Default::default())
            .await
            .unwrap()
            .modules[0]
            .count,
        3
    );
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            collection_id: None,
            filters: ModulePracticeFilters {
                count: Some(10),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(session.items.len(), 3);
}

#[tokio::test]
async fn module_progress_includes_history_from_inactive_equivalent_papers() {
    struct ChangingSource(std::sync::Mutex<Vec<IndexedPaper>>);
    #[async_trait]
    impl QuestionBankSourcePort for ChangingSource {
        async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
            Ok(BankSnapshot {
                papers: self.0.lock().unwrap().clone(),
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
    let source = Arc::new(ChangingSource(std::sync::Mutex::new(source().0.clone())));
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(source.clone(), repo);
    app.sync().await.unwrap();
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Paper,
            paper_id: Some("019a3d8c-7f23-7b6e-927d-3c951851d3a3".into()),
            module: None,
            category: None,
            collection_id: None,
            filters: Default::default(),
        })
        .await
        .unwrap();
    app.save_practice_answer(&session.id, &session.items[0].question_id, vec!["A".into()])
        .await
        .unwrap();
    app.submit_practice(&session.id).await.unwrap();
    source
        .0
        .lock()
        .unwrap()
        .retain(|p| p.summary.region == "湖南");
    app.sync().await.unwrap();
    let catalog = app.practice_catalog(&Default::default()).await.unwrap();
    assert_eq!((catalog.modules[0].count, catalog.modules[0].done), (1, 1));
}
