#![cfg(feature = "native")]

use async_trait::async_trait;
use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
use hyz_things::application::question_bank::document::{parse, Document};
use hyz_things::application::question_bank::{
    BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankApplication,
    QuestionBankSourcePort,
};
use hyz_things::domain::question_bank::{
    CreatePracticeRequest, ModulePracticeFilters, PracticeMode,
};
use std::sync::Arc;

const PAPER_A: &str = r#"---
schema: hyz-paper/v1
title: 第一份模块练习卷
year: 2008
region: 国考
variant: 统考
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a2 -->
# 第一份
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->
```yaml
module: 资料分析
category: 比重
```
### Stem
第一份第一题。
### Options
- A. 甲
- B. 乙
### Answer
A
### Explanation
解析一。
## Question 2
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e211 -->
```yaml
module: 资料分析
category: 比重
```
### Stem
第一份第二题。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
解析二。
"#;

const PAPER_B: &str = r#"---
schema: hyz-paper/v1
title: 第二份模块练习卷
year: 2018
region: 广东
variant: 省考
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a3 -->
# 第二份
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e212 -->
```yaml
module: 资料分析
category: 比重
```
### Stem
第二份第一题。
### Options
- A. 甲
- B. 乙
### Answer
A
### Explanation
解析三。
## Question 2
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e213 -->
```yaml
module: 资料分析
category: 比重
```
### Stem
第二份第二题。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
解析四。
"#;

const GROUPED_PAPER: &str = r#"---
schema: hyz-paper/v1
title: 材料题组练习卷
year: 2020
region: 国考
variant: 统考
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a4 -->
# 材料题组
## Material M1
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-7c54e259b611 -->
材料一。
## Material M2
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-7c54e259b612 -->
材料二。
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e214 -->
```yaml
module: 资料分析
category: 增长
materials: [M1]
```
### Stem
材料一第一题。
### Options
- A. 甲
- B. 乙
### Answer
A
### Explanation
解析。
## Question 2
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e215 -->
```yaml
module: 资料分析
category: 比重
materials: [M1]
```
### Stem
材料一第二题。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
解析。
## Question 3
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e216 -->
```yaml
module: 资料分析
category: 增长
materials: [M2]
```
### Stem
材料二题目。
### Options
- A. 甲
- B. 乙
### Answer
A
### Explanation
解析。
## Question 4
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e217 -->
```yaml
module: 资料分析
category: 增长
materials: []
```
### Stem
答案缺失的题目。
### Options
- A. 甲
- B. 乙
### Answer

### Explanation
无答案。
"#;

const PASSAGE_PAPER: &str = r#"---
schema: hyz-paper/v1
title: 篇章阅读练习卷
year: 2021
region: 国考
variant: 统考
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a5 -->
# 篇章阅读
## Material M1
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-7c54e259b613 -->
篇章材料。
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e218 -->
```yaml
module: 言语理解与表达
category: 篇章阅读
materials: [M1]
```
### Stem
篇章第一题。
### Options
- A. 甲
- B. 乙
### Answer
A
### Explanation
解析。
## Question 2
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e219 -->
```yaml
module: 言语理解与表达
category: 篇章阅读
materials: [M1]
```
### Stem
篇章第二题。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
解析。
## Question 3
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e21a -->
```yaml
module: 言语理解与表达
category: 片段阅读
materials: [M1]
```
### Stem
片段阅读第一题。
### Options
- A. 甲
- B. 乙
### Answer
A
### Explanation
解析。
## Question 4
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e21b -->
```yaml
module: 言语理解与表达
category: 片段阅读
materials: [M1]
```
### Stem
片段阅读第二题。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
解析。
"#;

struct TwoPaperBank;

fn paper(source: &str, path: &str, hash: &str) -> IndexedPaper {
    IndexedPaper::new(path.into(), hash.into(), parse(source).unwrap())
}

#[async_trait]
impl QuestionBankSourcePort for TwoPaperBank {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: vec![
                paper(PAPER_A, "papers/a.md", "hash-a"),
                paper(PAPER_B, "papers/b.md", "hash-b"),
            ],
            generated_ids: 0,
        })
    }
    async fn load(&self, paper: IndexedPaper) -> Result<Document, BankError> {
        let source = if paper.summary.id == "019a3d8c-7f23-7b6e-927d-3c951851d3a3" {
            PAPER_B
        } else {
            PAPER_A
        };
        parse(source).map_err(|_| BankError::Invalid)
    }
    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

async fn application() -> QuestionBankApplication {
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(Arc::new(TwoPaperBank), repository);
    app.sync().await.unwrap();
    app
}

struct GroupedBank;

#[async_trait]
impl QuestionBankSourcePort for GroupedBank {
    async fn scan(&self, _known: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: vec![
                paper(GROUPED_PAPER, "papers/grouped.md", "hash-grouped"),
                paper(PASSAGE_PAPER, "papers/passage.md", "hash-passage"),
            ],
            generated_ids: 0,
        })
    }
    async fn load(&self, paper: IndexedPaper) -> Result<Document, BankError> {
        let source = if paper.summary.id == "019a3d8c-7f23-7b6e-927d-3c951851d3a5" {
            PASSAGE_PAPER
        } else {
            GROUPED_PAPER
        };
        parse(source).map_err(|_| BankError::Invalid)
    }
    async fn asset(&self, _name: String) -> Result<BankAsset, BankError> {
        Err(BankError::NotFound)
    }
}

async fn grouped_application() -> QuestionBankApplication {
    let repository = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(Arc::new(GroupedBank), repository);
    app.sync().await.unwrap();
    app
}

#[tokio::test]
async fn module_selection_interleaves_papers_and_randomizes_within_each_position() {
    let app = application().await;
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            filters: ModulePracticeFilters::default(),
        })
        .await
        .unwrap();
    let papers = session
        .items
        .iter()
        .map(|item| item.paper_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(papers.len(), 4);
    assert_ne!(papers[0], papers[1]);
    assert_ne!(papers[2], papers[3]);
    // Each position draws one question from every eligible paper; the order
    // inside each position is random, so compare the pairs as sets.
    let mut first = [papers[0], papers[1]];
    first.sort_unstable();
    let mut second = [papers[2], papers[3]];
    second.sort_unstable();
    assert_eq!(first, second);
}

#[tokio::test]
async fn module_groups_keep_shared_material_questions_together_and_skip_missing_answers() {
    let app = grouped_application().await;
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            filters: ModulePracticeFilters {
                count: Some(2),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let ids = session
        .items
        .iter()
        .map(|item| item.question_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "019a3d8e-1b32-7a60-9c83-f34217a6e214",
            "019a3d8e-1b32-7a60-9c83-f34217a6e215",
        ]
    );
}

#[tokio::test]
async fn module_category_drill_keeps_the_whole_material_group() {
    let app = grouped_application().await;
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: Some("比重".into()),
            filters: ModulePracticeFilters {
                count: Some(2),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let ids = session
        .items
        .iter()
        .map(|item| item.question_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "019a3d8e-1b32-7a60-9c83-f34217a6e214",
            "019a3d8e-1b32-7a60-9c83-f34217a6e215",
        ]
    );
}

#[tokio::test]
async fn paper_practice_skips_questions_without_an_answer() {
    let app = grouped_application().await;
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Paper,
            paper_id: Some("019a3d8c-7f23-7b6e-927d-3c951851d3a4".into()),
            module: None,
            category: None,
            filters: ModulePracticeFilters::default(),
        })
        .await
        .unwrap();
    let ids = session
        .items
        .iter()
        .map(|item| item.question_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "019a3d8e-1b32-7a60-9c83-f34217a6e214",
            "019a3d8e-1b32-7a60-9c83-f34217a6e215",
            "019a3d8e-1b32-7a60-9c83-f34217a6e216",
        ]
    );
}

#[tokio::test]
async fn passage_groups_exclude_non_passage_questions_sharing_section_material() {
    let app = grouped_application().await;
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("言语理解与表达".into()),
            category: None,
            filters: ModulePracticeFilters {
                count: Some(2),
                ..Default::default()
            },
        })
        .await
        .unwrap();
    let ids = session
        .items
        .iter()
        .map(|item| item.question_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "019a3d8e-1b32-7a60-9c83-f34217a6e218",
            "019a3d8e-1b32-7a60-9c83-f34217a6e219",
        ]
    );
}

#[tokio::test]
async fn unavailable_questions_are_excluded_from_catalog_and_paper_module_counts() {
    let app = grouped_application().await;
    let catalog = app
        .practice_catalog(&ModulePracticeFilters::default())
        .await
        .unwrap();
    let data_analysis = catalog
        .modules
        .iter()
        .find(|module| module.name == "资料分析")
        .unwrap();
    assert_eq!(data_analysis.count, 3);

    let modules = app
        .modules("019a3d8c-7f23-7b6e-927d-3c951851d3a4")
        .await
        .unwrap();
    let data_analysis = modules
        .modules
        .iter()
        .find(|module| module.name == "资料分析")
        .unwrap();
    assert_eq!(data_analysis.count, 3);
    assert_eq!(data_analysis.ungradable, 1);
}

#[tokio::test]
async fn repeat_practice_copies_the_same_question_set_and_clears_answers() {
    let app = application().await;
    let session = app
        .create_practice(CreatePracticeRequest {
            mode: PracticeMode::Module,
            paper_id: None,
            module: Some("资料分析".into()),
            category: None,
            filters: ModulePracticeFilters::default(),
        })
        .await
        .unwrap();
    let ids = session
        .items
        .iter()
        .map(|item| item.question_id.clone())
        .collect::<Vec<_>>();
    app.save_practice_answer(&session.id, &ids[0], vec!["A".into()])
        .await
        .unwrap();
    let completed = app.submit_practice(&session.id).await.unwrap();
    let repeated = app.repeat_practice(&completed.id).await.unwrap();
    assert_ne!(repeated.id, completed.id);
    assert_eq!(repeated.status, "active");
    assert_eq!(repeated.elapsed_seconds, 0);
    assert!(!repeated.paused);
    assert_eq!(
        repeated
            .items
            .iter()
            .map(|item| item.question_id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    assert!(repeated
        .items
        .iter()
        .all(|item| item.answer.is_empty() && item.grade.is_none()));
}
