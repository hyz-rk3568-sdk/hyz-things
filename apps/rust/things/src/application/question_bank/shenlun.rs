//! 申论真题只读入口：与行测 A–Z 答题及客观评分彻底隔离。
//! 来源参考答案只供服务端后续评分流程读取，浏览器使用现有无答案的 PaperView。
use super::{BankError, QuestionBankApplication};
use crate::domain::question_bank::{PaperSummary, PaperView};

fn is_shenlun_scope(subject: &str, role: &str, kind: &str) -> bool {
    subject == "shenlun" && role == "primary" && kind == "exam"
}

/// Server-only source snapshot. Never serialize this type into pre-grading HTTP responses.
#[derive(Clone, Debug)]
pub struct ShenlunSourceContext {
    pub paper_id: String,
    pub question_id: String,
    pub paper_title: String,
    pub source_hash: String,
    pub stem: String,
    pub question_type: Option<String>,
    pub max_score: Option<f64>,
    pub materials: Vec<crate::domain::question_bank::MaterialView>,
    pub reference_answer: String,
}

impl QuestionBankApplication {
    /// Re-check the subject and stored source hash at the application boundary.
    /// Reference answers only leave this function to the protected native grading service.
    pub async fn shenlun_source_context(
        &self,
        paper_id: &str,
        question_id: &str,
    ) -> Result<ShenlunSourceContext, BankError> {
        let paper = self.shenlun_paper(paper_id).await?;
        if !paper
            .questions
            .iter()
            .any(|question| question.id == question_id)
        {
            return Err(BankError::NotFound);
        }
        let source = self.repository.paper_content(paper_id).await?;
        let question = source
            .questions
            .into_iter()
            .find(|question| question.id == question_id && question.answer_type == "subjective")
            .ok_or(BankError::NotFound)?;
        let materials = source
            .materials
            .into_iter()
            .filter(|material| {
                question.materials.contains(&material.name)
                    || question.materials.contains(&material.id)
            })
            .collect();
        Ok(ShenlunSourceContext {
            paper_id: paper.id,
            question_id: question.id,
            paper_title: paper.title,
            source_hash: source.hash,
            stem: question.stem,
            question_type: question.question_type,
            max_score: question.max_score,
            materials,
            reference_answer: question.reference_answer,
        })
    }

    /// Only primary exam papers for 申论, never drill collections or xingce.
    pub async fn shenlun_papers(&self) -> Result<Vec<PaperSummary>, BankError> {
        Ok(self
            .papers()
            .await?
            .into_iter()
            .filter(|paper| is_shenlun_scope(&paper.subject, &paper.role, &paper.kind))
            .collect())
    }

    /// The returned QuestionView never includes the stored reference_answer.
    /// Reject cross-subject lookups even when the caller knows the paper UUID.
    pub async fn shenlun_paper(&self, id: &str) -> Result<PaperView, BankError> {
        let mut paper = self.paper(id).await?;
        if !is_shenlun_scope(&paper.subject, &paper.role, &paper.kind) {
            return Err(BankError::NotFound);
        }
        paper
            .questions
            .retain(|question| question.answer_type == "subjective");
        if paper.questions.is_empty() {
            return Err(BankError::NotFound);
        }
        Ok(paper)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shenlun_scope_never_accepts_xingce_or_drills() {
        assert!(is_shenlun_scope("shenlun", "primary", "exam"));
        for (subject, role, kind) in [
            ("xingce", "primary", "exam"),
            ("shenlun", "primary", "drill"),
            ("shenlun", "derived", "exam"),
            ("", "primary", "exam"),
        ] {
            assert!(!is_shenlun_scope(subject, role, kind));
        }
    }
}
