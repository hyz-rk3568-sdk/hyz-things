#![cfg(feature = "native")]

use async_trait::async_trait;
use hyz_things::adapters::outbound::question_bank_sqlite::SqliteQuestionBankRepository;
use hyz_things::application::question_bank::{
    document::{parse, Document},
    BankAsset, BankError, BankSnapshot, IndexedPaper, QuestionBankApplication,
    QuestionBankRepositoryPort, QuestionBankSourcePort,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

struct Source {
    papers: Vec<IndexedPaper>,
    assets: BTreeMap<String, Vec<u8>>,
}

#[async_trait]
impl QuestionBankSourcePort for Source {
    async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: self.papers.clone(),
            generated_ids: 0,
        })
    }
    async fn load(&self, _: IndexedPaper) -> Result<Document, BankError> {
        Err(BankError::NotFound)
    }
    async fn asset(&self, name: String) -> Result<BankAsset, BankError> {
        Ok(BankAsset {
            contents: self.assets.get(&name).ok_or(BankError::NotFound)?.clone(),
            content_type: "image/png",
        })
    }
}

fn paper(n: u32, stem: &str, material: &str, option: &str) -> IndexedPaper {
    let text = format!(
        r#"---
schema: hyz-paper/v1
title: test
year: 2020
region: test
variant: test
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-{n:012x} -->
## Material M1
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-{n:012x} -->
placeholder
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-{n:012x} -->
```yaml
module: 资料分析
category: 比重
materials: [M1]
```
### Stem
placeholder
### Options
- A. {option}
- B. other
### Answer
A
"#
    );
    let mut paper = IndexedPaper::new(
        format!("papers/{n}.md"),
        "hash".into(),
        parse(&text).unwrap(),
    );
    paper.content.questions[0].stem = stem.into();
    paper.content.materials[0].markdown = material.into();
    paper
}

async fn catalog_count(papers: Vec<IndexedPaper>, assets: BTreeMap<String, Vec<u8>>) -> usize {
    let repo = Arc::new(
        SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    let app = QuestionBankApplication::new(Arc::new(Source { papers, assets }), repo);
    app.sync().await.unwrap();
    app.practice_catalog(&Default::default())
        .await
        .unwrap()
        .modules[0]
        .count
}

#[tokio::test]
async fn insignificant_line_whitespace_does_not_split_catalog_identity() {
    let count = catalog_count(
        vec![
            paper(1, "stem\nnext", "material\nnext", "one"),
            paper(
                2,
                "\r\nstem \r\nnext\r\n\r\n",
                "\r\nmaterial \r\nnext\r\n",
                "two",
            ),
        ],
        BTreeMap::new(),
    )
    .await;
    assert_eq!(count, 1);
}

#[tokio::test]
async fn escaped_trailing_space_does_not_become_markdown_hard_break() {
    let count = catalog_count(
        vec![
            paper(1, "stem\\ \nnext", "material", "a"),
            paper(2, "stem\\\nnext", "material", "a"),
        ],
        BTreeMap::new(),
    )
    .await;
    assert_eq!(count, 2);
}

#[tokio::test]
async fn different_options_do_not_split_identical_questions() {
    assert_eq!(
        catalog_count(
            vec![
                paper(1, "stem", "material", "one"),
                paper(2, "stem", "material", "two")
            ],
            BTreeMap::new()
        )
        .await,
        1
    );
}

#[tokio::test]
async fn different_materials_or_stems_are_distinct_questions() {
    assert_eq!(
        catalog_count(
            vec![
                paper(1, "stem", "one", "a"),
                paper(2, "stem", "two", "a"),
                paper(3, "different", "one", "a")
            ],
            BTreeMap::new()
        )
        .await,
        3
    );
}

#[tokio::test]
async fn image_contents_are_part_of_identity_and_missing_images_do_not_merge() {
    use hyz_things::application::question_bank::source_hash;
    let one = source_hash(b"one");
    let two = source_hash(b"two");
    let missing = source_hash(b"missing");
    let mut assets = BTreeMap::new();
    assets.insert(format!("{one}.png"), b"one".to_vec());
    assets.insert(format!("{two}.png"), b"two".to_vec());
    let count = catalog_count(
        vec![
            paper(
                1,
                &format!("stem ![](../assets/{one}.png)"),
                "material",
                "a",
            ),
            paper(
                2,
                &format!("stem ![](../assets/{one}.png)"),
                "material",
                "a",
            ),
            paper(
                3,
                &format!("stem ![](../assets/{two}.png)"),
                "material",
                "a",
            ),
            paper(
                4,
                &format!("stem ![](../assets/{missing}.png)"),
                "material",
                "a",
            ),
            paper(
                5,
                &format!("stem ![](../assets/{missing}.png)"),
                "material",
                "a",
            ),
        ],
        assets,
    )
    .await;
    assert_eq!(count, 4);
}

#[derive(Default)]
struct RecordingRepository {
    papers: Mutex<Vec<IndexedPaper>>,
}

#[async_trait]
impl QuestionBankRepositoryPort for RecordingRepository {
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
        Ok(self.papers.lock().unwrap().clone())
    }

    async fn apply(&self, papers: Vec<IndexedPaper>) -> Result<(), BankError> {
        *self.papers.lock().unwrap() = papers;
        Ok(())
    }
}

struct IntermittentSource {
    paper: IndexedPaper,
    unavailable: AtomicBool,
}

#[async_trait]
impl QuestionBankSourcePort for IntermittentSource {
    async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        Ok(BankSnapshot {
            papers: vec![self.paper.clone()],
            generated_ids: 0,
        })
    }

    async fn load(&self, _: IndexedPaper) -> Result<Document, BankError> {
        Err(BankError::NotFound)
    }

    async fn asset(&self, _: String) -> Result<BankAsset, BankError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(BankError::Unavailable);
        }
        Ok(BankAsset {
            contents: b"one".to_vec(),
            content_type: "image/png",
        })
    }
}

#[tokio::test]
async fn unavailable_image_preserves_previously_synced_fingerprints() {
    let hash = hyz_things::application::question_bank::source_hash(b"one");
    let source = Arc::new(IntermittentSource {
        paper: paper(
            1,
            &format!("stem ![](../assets/{hash}.png)"),
            "material",
            "a",
        ),
        unavailable: AtomicBool::new(false),
    });
    let repository = Arc::new(RecordingRepository::default());
    let app = QuestionBankApplication::new(source.clone(), repository.clone());
    app.sync().await.unwrap();
    let before = repository.list().await.unwrap();
    assert_eq!(before[0].content_fingerprints.len(), 1);

    source.unavailable.store(true, Ordering::SeqCst);
    assert_eq!(app.sync().await, Err(BankError::Unavailable));
    let after = repository.list().await.unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(
        after[0].content_fingerprints,
        before[0].content_fingerprints
    );
}
