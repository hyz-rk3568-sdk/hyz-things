//! Indexed original-question content for Flashcard creation, independent of favorites.
use super::{document::valid_id, BankError, PaperContent, QuestionBankApplication};
use crate::application::favorite_flashcards::{FavoriteQuestion, FavoriteQuestionPort};
use async_trait::async_trait;

#[async_trait]
impl FavoriteQuestionPort for QuestionBankApplication {
    async fn load(&self, question_id: &str) -> Result<FavoriteQuestion, BankError> {
        if !valid_id(question_id) {
            return Err(BankError::Invalid);
        }
        let contents = self
            .repository
            .question_contents(&[question_id.into()])
            .await?;
        let indexed_question = contents
            .iter()
            .find(|content| content.question.id == question_id && content.active)
            .ok_or(BankError::NotFound)?;
        let indexed = self.primary_indexed(&indexed_question.paper_id).await?;
        if indexed.hash != indexed_question.source_hash {
            return Err(BankError::Stale);
        }
        let document = self.source.load(indexed.clone()).await?;
        let content = PaperContent::from_document(&indexed.hash, &document);
        if content.paper_id != indexed.summary.id {
            return Err(BankError::Conflict);
        }
        let question = content.question(question_id).ok_or(BankError::NotFound)?;
        let mut front = Vec::new();
        for id in &question.materials {
            let material = content
                .materials
                .iter()
                .find(|material| material.id == *id)
                .ok_or(BankError::Invalid)?;
            if material.markdown.trim().is_empty() {
                return Err(BankError::Invalid);
            }
            front.push(material.markdown.clone());
        }
        front.push(question.stem.clone());
        front.push(question.options_markdown.clone());
        Ok(FavoriteQuestion {
            question_id: question.id.clone(),
            module: question.module.clone(),
            category: question.category.clone(),
            front: front.join("\n\n"),
            correct_answer: question.correct_answer.clone(),
            source_revision: indexed.hash,
            gradable: question.gradable,
            allow_manual_answer: indexed.summary.kind == "drill"
                && question.correct_answer.is_empty()
                && !question.stem.trim().is_empty(),
        })
    }

    async fn asset(&self, name: String) -> Result<Vec<u8>, BankError> {
        Ok(self.source.asset(name).await?.contents)
    }
}
