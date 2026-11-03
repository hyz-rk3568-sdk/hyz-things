#![cfg(feature = "native")]

use async_trait::async_trait;
use hyz_things::application::question_bank::document::{parse, Document};
use hyz_things::application::question_bank::{
    BankAsset, BankError, BankSnapshot, IndexedPaper, PaperContent, PracticeContent,
    QuestionBankApplication, QuestionBankRepositoryPort, QuestionBankSourcePort,
};
use hyz_things::domain::question_bank::{PracticeItem, PracticeMode, PracticeSession};
use std::sync::{Arc, Mutex};

const SESSION: &str = "019a3d8c-7f23-7b6e-927d-3c951851d3a1";
const QUESTION: &str = "019a3d8e-1b32-7a60-9c83-f34217a6e210";
const PAPER: &str = r#"---
schema: hyz-paper/v1
title: 示例卷
year: 2001
exam: 国考
region: 国考
variant: 统考
status: unverified
role: primary
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a2 -->
# 示例卷
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->
```yaml
module: 资料分析
category: 比重
topics: []
materials: []
```
### Stem
题干。
### Options
- A. 甲
- B. 乙
### Answer
B
### Explanation
解析。
"#;

#[derive(Default)]
struct AnswerBank {
    legacy_writes: Mutex<usize>,
    saves: Mutex<Vec<(PracticeSession, String, u64)>>,
    error: Mutex<Option<BankError>>,
}
impl AnswerBank {
    fn application(self: &Arc<Self>) -> QuestionBankApplication {
        QuestionBankApplication::new(self.clone(), self.clone())
    }
    fn paper() -> IndexedPaper {
        IndexedPaper::new("papers/a.md".into(), "hash".into(), parse(PAPER).unwrap())
    }
    fn session() -> PracticeSession {
        PracticeSession {
            id: SESSION.into(),
            mode: PracticeMode::Paper,
            paper_id: Some(Self::paper().summary.id),
            module: None,
            category: None,
            status: "active".into(),
            created_at: 1,
            updated_at: 1,
            elapsed_seconds: 0,
            paused: false,
            revision: 7,
            items: vec![PracticeItem {
                question_id: QUESTION.into(),
                paper_id: Self::paper().summary.id,
                source_hash: "hash".into(),
                number: 1,
                answer: vec![],
                grade: None,
                gradable: true,
            }],
            score: None,
        }
    }
}
#[async_trait]
impl QuestionBankSourcePort for AnswerBank {
    async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
        panic!("answer save must not scan source")
    }
    async fn load(&self, _: IndexedPaper) -> Result<Document, BankError> {
        panic!("answer save must not load source")
    }
    async fn asset(&self, _: String) -> Result<BankAsset, BankError> {
        panic!("answer save must not load assets")
    }
}
#[async_trait]
impl QuestionBankRepositoryPort for AnswerBank {
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
        Ok(vec![Self::paper()])
    }
    async fn indexed_paper(&self, _: &str) -> Result<IndexedPaper, BankError> {
        Ok(Self::paper())
    }
    async fn apply(&self, _: Vec<IndexedPaper>) -> Result<(), BankError> {
        panic!("answer save must not sync")
    }
    async fn paper_content(&self, _: &str) -> Result<PaperContent, BankError> {
        Ok(Self::paper().content)
    }
    async fn question_contents(&self, ids: &[String]) -> Result<Vec<PracticeContent>, BankError> {
        let paper = Self::paper();
        Ok(paper
            .content
            .questions
            .iter()
            .filter(|question| ids.contains(&question.id))
            .map(|question| PracticeContent {
                paper_id: paper.summary.id.clone(),
                paper_title: paper.summary.title.clone(),
                source_hash: paper.hash.clone(),
                active: true,
                question: question.clone(),
                materials: vec![],
            })
            .collect())
    }
    async fn practice_load(&self, _: &str) -> Result<PracticeSession, BankError> {
        Ok(Self::session())
    }
    async fn practice_save_answer(
        &self,
        session: PracticeSession,
        question_id: &str,
        previous_revision: u64,
    ) -> Result<(), BankError> {
        self.saves
            .lock()
            .unwrap()
            .push((session, question_id.into(), previous_revision));
        match self.error.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    async fn practice_update(&self, _: PracticeSession, _: u64) -> Result<(), BankError> {
        *self.legacy_writes.lock().unwrap() += 1;
        Ok(())
    }
}

#[tokio::test]
async fn answer_save_uses_only_the_precise_item_port() {
    let bank = Arc::new(AnswerBank::default());
    let app = bank.application();
    let saved = app
        .save_practice_answer(SESSION, QUESTION, vec!["B".into()])
        .await
        .unwrap();
    assert_eq!(*bank.legacy_writes.lock().unwrap(), 0);
    let writes = bank.saves.lock().unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0], (saved.clone(), QUESTION.into(), 7));
    assert_eq!(saved.revision, 8);
    assert_eq!(saved.items[0].answer, vec!["B"]);
}

async fn assert_save_error(error: BankError) {
    let bank = Arc::new(AnswerBank::default());
    *bank.error.lock().unwrap() = Some(error.clone());
    assert_eq!(
        bank.application()
            .save_practice_answer(SESSION, QUESTION, vec!["B".into()])
            .await,
        Err(error)
    );
    assert_eq!(*bank.legacy_writes.lock().unwrap(), 0);
    assert_eq!(bank.saves.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn answer_save_propagates_storage_failure_without_legacy_fallback() {
    assert_save_error(BankError::Storage).await;
}

#[tokio::test]
async fn answer_save_propagates_conflict_without_retry_or_legacy_fallback() {
    assert_save_error(BankError::Conflict).await;
}

#[tokio::test]
async fn answer_save_propagates_unavailable_without_legacy_fallback() {
    assert_save_error(BankError::Unavailable).await;
}

#[tokio::test]
async fn invalid_answer_never_reaches_a_write_port() {
    let bank = Arc::new(AnswerBank::default());
    assert_eq!(
        bank.application()
            .save_practice_answer(SESSION, QUESTION, vec!["Z".into()])
            .await,
        Err(BankError::Invalid)
    );
    assert_eq!(*bank.legacy_writes.lock().unwrap(), 0);
    assert!(bank.saves.lock().unwrap().is_empty());
}
