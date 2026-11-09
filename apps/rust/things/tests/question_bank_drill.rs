#![cfg(feature = "native")]
use async_trait::async_trait;
use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
use hyz_things::application::question_bank::document::{complete_ids, parse, Document};
use hyz_things::application::question_bank::{
    BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankApplication,
    QuestionBankRepositoryPort, QuestionBankSourcePort,
};
use hyz_things::domain::question_bank::{CreatePracticeRequest, PracticeMode};
use std::sync::Arc;

const DRILL: &str = r#"---
schema: hyz-paper/v1
title: 方程关系
year: 2026
region: 小P讲义
variant: 数量关系
role: primary
kind: drill
collection_id: xiaop-2026-shuliang
collection_title: 小P 2026 数量关系讲义
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a3 -->
# 方程关系
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e220 -->
```yaml
module: 数量关系
category: 方程关系
```
### Stem
有答案的题。
### Options
- A. 甲
- B. 乙
- C. 丙
- D. 丁
### Answer
B
### Explanation
解析。
## Question 2
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e221 -->
```yaml
module: 数量关系
category: 方程关系
```
### Stem
暂无可靠答案的题。
### Options
- A. 甲
- B. 乙
- C. 丙
- D. 丁
### Answer

"#;

struct Source;

#[async_trait]
impl QuestionBankSourcePort for Source {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: vec![drill_paper()],
            generated_ids: 0,
        })
    }
    async fn load(&self, _paper: IndexedPaper) -> Result<Document, BankError> {
        parse(DRILL).map_err(|_| BankError::Invalid)
    }
    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

fn drill_paper() -> IndexedPaper {
    let document = parse(DRILL).unwrap();
    assert_eq!(document.view().kind, "drill");
    assert_eq!(
        document.view().collection_id.as_deref(),
        Some("xiaop-2026-shuliang")
    );
    IndexedPaper::new(
        "collections/xiaop-2026-shuliang/01-方程关系.md".into(),
        "hash".into(),
        document,
    )
}

async fn application() -> QuestionBankApplication {
    let repository = SqliteQuestionBankRepository::connect("sqlite::memory:")
        .await
        .unwrap();
    repository.apply(vec![drill_paper()]).await.unwrap();
    QuestionBankApplication::new(Arc::new(Source), Arc::new(repository))
}

#[tokio::test]
async fn drill_practice_keeps_ungradable_questions_and_scores_only_answers() {
    let app = application().await;
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Drill,
            paper_id: None,
            module: None,
            category: None,
            collection_id: Some("xiaop-2026-shuliang".into()),
            filters: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(session.mode, PracticeMode::Drill);
    assert_eq!(
        session.collection_id.as_deref(),
        Some("xiaop-2026-shuliang")
    );
    assert_eq!(
        session.collection_title.as_deref(),
        Some("小P 2026 数量关系讲义")
    );
    assert_eq!(session.items.len(), 2);
    assert_eq!(session.items.iter().filter(|item| item.gradable).count(), 1);

    let question = session.items[0].question_id.clone();
    app.save_practice_answer(&session.id, &question, vec!["B".into()])
        .await
        .unwrap();
    let completed = app.submit_practice(&session.id).await.unwrap();
    let score = completed.score.unwrap();
    assert_eq!(score.total, 2);
    assert_eq!(score.correct, 1);
    assert_eq!(score.ungradable, 1);
    assert_eq!(score.wrong, 0);
    assert_eq!(score.unanswered, 0);

    let days = app.daily_answered_breakdown(0, i64::MAX).await.unwrap();
    let day = days.values().next().unwrap();
    assert_eq!(day.collections["小P 2026 数量关系讲义"]["方程关系"], 1);
    assert!(day.papers.is_empty());
    assert!(day.modules.is_empty());
}

#[tokio::test]
async fn drill_collection_progress_is_reported_separately_from_exam_forecast() {
    let app = application().await;
    let collections = app.drill_collections().await.unwrap();
    assert_eq!(collections.len(), 1);
    let collection = &collections[0];
    assert_eq!(collection.title, "小P 2026 数量关系讲义");
    assert_eq!(collection.questions, 2);
    assert_eq!(collection.gradable, 1);
    assert_eq!(collection.done, 0);
    assert_eq!(collection.papers.len(), 1);

    let forecast = app.study_plan_forecast(&Default::default()).await.unwrap();
    assert_eq!(forecast.total_remaining, 0);
}

#[test]
fn lecture_collection_parses_with_all_522_questions_and_images() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../question_bank/collections/xiaop-2026-shuliang");
    let mut files = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files.len(), 19);
    let mut total = 0;
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let mut sequence = 0_u64;
        let (text, _) = complete_ids(&text, || {
            sequence += 1;
            format!("019a3d8c-7f23-7b6e-927d-{sequence:012x}")
        })
        .unwrap();
        let document = parse(&text).unwrap();
        assert_eq!(document.view().kind, "drill");
        assert_eq!(
            document.view().collection_id.as_deref(),
            Some("xiaop-2026-shuliang")
        );
        assert!(
            document
                .view()
                .questions
                .iter()
                .all(|question| !question.stem.trim().is_empty()),
            "empty stem in {}",
            path.display()
        );
        assert!(
            document
                .view()
                .questions
                .iter()
                .all(|question| !question.options_markdown.contains("assets/")),
            "figure left in options in {}",
            path.display()
        );
        assert!(
            document.view().questions.iter().all(|question| {
                !question.options_markdown.contains('$')
                    && !question.options_markdown.contains("\\frac")
                    && !question.options_markdown.contains("\\sqrt")
            }),
            "LaTeX left in options in {}",
            path.display()
        );
        total += document.view().questions.len();
        for reference in text.lines().filter_map(|line| {
            line.split("](../../assets/")
                .nth(1)
                .and_then(|rest| rest.split(')').next())
        }) {
            assert!(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../question_bank/assets")
                    .join(reference)
                    .is_file(),
                "missing lecture asset {reference}"
            );
        }
    }
    assert_eq!(total, 522);
}

#[test]
fn solution_lecture_is_separate_from_travel_lecture_without_renumbering() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../question_bank/collections/xiaop-2026-shuliang");
    let travel = std::fs::read_to_string(root.join("06-行程问题（二）.md")).unwrap();
    let solution = std::fs::read_to_string(root.join("07-溶液问题.md")).unwrap();
    assert!(travel.contains("## Question 161"));
    assert!(!travel.contains("## Question 162"));
    assert!(solution.contains("## Question 162"));
    assert!(solution.contains("## Question 188"));
    assert!(!solution.contains("## Question 161"));
    assert!(solution.contains("category: \"蒸发加水问题\""));
    assert_eq!(travel.matches("## Question ").count(), 24);
    assert_eq!(solution.matches("## Question ").count(), 27);
}

#[test]
fn prepared_sync_stores_canonical_asset_tokens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../question_bank/collections/xiaop-2026-shuliang");
    let path = "collections/xiaop-2026-shuliang/06-行程问题（二）.md";
    let text = std::fs::read_to_string(root.join("06-行程问题（二）.md")).unwrap();
    let plan = hyz_things::application::question_bank::prepare_sync(
        vec![hyz_things::application::question_bank::SourceFile {
            path: path.into(),
            text,
        }],
        Vec::new(),
        || unreachable!("all ids are present"),
    )
    .unwrap();
    let stem = &plan.snapshot.papers[0]
        .content
        .questions
        .iter()
        .find(|question| question.number == 146)
        .unwrap()
        .stem;
    assert!(stem.contains("hyz-bank-asset:"));
    assert!(!stem.contains("../../assets/"));
}

#[test]
fn lecture_image_references_rewrite_from_the_nested_collection_path() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../question_bank/collections/xiaop-2026-shuliang");
    let path = root.join("06-行程问题（二）.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let (rewritten, references) = hyz_things::application::question_bank::images::rewrite_assets(
        &text,
        "collections/xiaop-2026-shuliang/06-行程问题（二）.md",
    )
    .unwrap();
    assert_eq!(references.len(), 1);
    assert!([".jpg", ".jpeg", ".png"]
        .iter()
        .any(|extension| references[0].ends_with(extension)));
    assert!(rewritten.contains(&format!("hyz-bank-asset:{}", references[0])));
}

#[test]
fn drill_header_requires_a_collection() {
    let invalid = DRILL.replace("collection_id: xiaop-2026-shuliang\n", "");
    assert!(parse(&invalid).is_err());
}
