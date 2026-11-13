//! 申论真题只读入口：与行测 A–Z 答题及客观评分彻底隔离。
//! 来源参考答案只供服务端后续评分流程读取，浏览器使用现有无答案的 PaperView。
use super::{BankError, QuestionBankApplication};
use crate::domain::question_bank::{PaperSummary, PaperView};

fn is_shenlun_scope(subject: &str, role: &str, kind: &str) -> bool {
    subject == "shenlun" && role == "primary" && kind == "exam"
}

impl QuestionBankApplication {
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
        paper.questions
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
