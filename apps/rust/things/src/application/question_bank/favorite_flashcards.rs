//! Indexed original-question content for Flashcard creation, independent of favorites.
use super::{document::valid_id, BankError, PaperContent, QuestionBankApplication};
use crate::application::favorite_flashcards::{FavoriteQuestion, FavoriteQuestionPort};
use async_trait::async_trait;

fn shenlun_flashcard_category(question_type: Option<&str>) -> &'static str {
    match question_type {
        Some("summary" | "generalization" | "归纳概括") => "归纳概括",
        Some("analysis" | "综合分析") => "综合分析",
        Some("measures" | "countermeasure" | "提出对策") => "提出对策",
        Some("document_writing" | "document" | "official_document" | "公文写作") => "公文写作",
        Some("essay" | "composition" | "大作文") => "大作文",
        _ => "其他",
    }
}

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
        let shenlun = indexed.summary.subject == "shenlun"
            && indexed.summary.role == "primary"
            && indexed.summary.kind == "exam"
            && question.answer_type == "subjective"
            && !question.stem.trim().is_empty();
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
            module: if shenlun {
                Some("申论".into())
            } else {
                question.module.clone()
            },
            category: if shenlun {
                Some(shenlun_flashcard_category(question.question_type.as_deref()).into())
            } else {
                question.category.clone()
            },
            front: front.join("\n\n"),
            // Shenlun reference answers stay behind the existing gated source-answer view.
            correct_answer: if shenlun {
                Vec::new()
            } else {
                question.correct_answer.clone()
            },
            source_revision: indexed.hash,
            gradable: question.gradable,
            allow_manual_answer: shenlun
                || (indexed.summary.kind == "drill"
                    && question.correct_answer.is_empty()
                    && !question.stem.trim().is_empty()),
        })
    }

    async fn asset(&self, name: String) -> Result<Vec<u8>, BankError> {
        Ok(self.source.asset(name).await?.contents)
    }
}
