//! 申论真题只读入口：与行测 A–Z 答题及客观评分彻底隔离。
//! 来源参考答案只供服务端后续评分流程读取，浏览器使用现有无答案的 PaperView。
use super::{BankError, QuestionBankApplication};
use crate::domain::question_bank::{CreateShenlunAttempt, PaperSummary, PaperView, ShenlunAttempt};

fn is_shenlun_scope(subject: &str, role: &str, kind: &str) -> bool {
    subject == "shenlun" && role == "primary" && kind == "exam"
}

impl QuestionBankApplication {
    async fn shenlun_question_hash(
        &self,
        paper_id: &str,
        question_id: &str,
    ) -> Result<String, BankError> {
        let paper = self.shenlun_paper(paper_id).await?;
        if !paper.questions.iter().any(|q| q.id == question_id) {
            return Err(BankError::NotFound);
        }
        Ok(self.repository.paper_content(paper_id).await?.hash)
    }

    /// Create with a client UUID, retrying the same request never duplicates an attempt.
    pub async fn create_shenlun_attempt(
        &self,
        request: CreateShenlunAttempt,
    ) -> Result<ShenlunAttempt, BankError> {
        if uuid::Uuid::parse_str(&request.request_id).is_err() {
            return Err(BankError::Invalid);
        }
        let hash = self
            .shenlun_question_hash(&request.paper_id, &request.question_id)
            .await?;
        match self.repository.shenlun_get_attempt(&request.request_id).await {
            Ok(record) => {
                return if record.paper_id == request.paper_id
                    && record.question_id == request.question_id
                    && record.source_hash == hash
                {
                    Ok(record)
                } else {
                    Err(BankError::Conflict)
                };
            }
            Err(BankError::NotFound) => {}
            Err(error) => return Err(error),
        }
        let now = current_millis();
        self.repository
            .shenlun_create_attempt(ShenlunAttempt {
                id: request.request_id,
                paper_id: request.paper_id,
                question_id: request.question_id,
                source_hash: hash,
                text: String::new(),
                source_kind: "typed".into(),
                status: "draft".into(),
                revision: 0,
                created_at_ms: now,
                updated_at_ms: now,
                submitted_at_ms: None,
            })
            .await
    }

    pub async fn shenlun_attempt(&self, id: &str) -> Result<ShenlunAttempt, BankError> {
        check_attempt_id(id)?;
        self.repository.shenlun_get_attempt(id).await
    }

    pub async fn shenlun_attempts(
        &self,
        paper_id: &str,
        question_id: &str,
    ) -> Result<Vec<ShenlunAttempt>, BankError> {
        self.shenlun_question_hash(paper_id, question_id).await?;
        Ok(self
            .repository
            .shenlun_list_attempts(question_id)
            .await?
            .into_iter()
            .filter(|attempt| attempt.paper_id == paper_id)
            .collect())
    }

    pub async fn save_shenlun_draft(
        &self,
        id: &str,
        expected_revision: i64,
        text: &str,
    ) -> Result<ShenlunAttempt, BankError> {
        check_attempt_id(id)?;
        if !valid_answer_text(text) || expected_revision < 0 {
            return Err(BankError::Invalid);
        }
        let current = self.shenlun_attempt(id).await?;
        if current.status != "draft" {
            return Err(BankError::Conflict);
        }
        if self.shenlun_question_hash(&current.paper_id, &current.question_id).await?
            != current.source_hash
        {
            return Err(BankError::Stale);
        }
        self.repository
            .shenlun_save_draft(id, expected_revision, text)
            .await
    }

    pub async fn submit_shenlun_attempt(&self, id: &str) -> Result<ShenlunAttempt, BankError> {
        let current = self.shenlun_attempt(id).await?;
        if current.status == "submitted" {
            return Ok(current);
        }
        if current.text.trim().is_empty() {
            return Err(BankError::Invalid);
        }
        if self.shenlun_question_hash(&current.paper_id, &current.question_id).await?
            != current.source_hash
        {
            return Err(BankError::Stale);
        }
        self.repository.shenlun_submit_attempt(id).await
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
        paper.questions
            .retain(|question| question.answer_type == "subjective");
        if paper.questions.is_empty() {
            return Err(BankError::NotFound);
        }
        Ok(paper)
    }
}


fn check_attempt_id(id: &str) -> Result<(), BankError> {
    if uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id) {
        Ok(())
    } else {
        Err(BankError::Invalid)
    }
}

fn valid_answer_text(text: &str) -> bool {
    text.len() <= 200_000 && text.chars().count() <= 50_000 && !text.contains('\0')
}

fn current_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shenlun_scope_never_accepts_xingce_or_drills() {
        assert!(is_shenlun_scope("shenlun", "primary", "exam"));
        assert!(valid_answer_text("我的申论答案。"));
        assert!(!valid_answer_text("未确认\0片段"));
        assert!(!valid_answer_text(&"中".repeat(50_001)));
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
