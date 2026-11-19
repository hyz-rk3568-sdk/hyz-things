use crate::{
    application::ai_practice::{AiPracticeError, AiPracticeFavoritesPort},
    domain::ai_practice::{AiPracticeModule, AiPracticeQuestionId},
};
use sqlx::{Row, SqlitePool};

pub struct SqliteAiPracticeFavorites {
    pool: SqlitePool,
}
impl SqliteAiPracticeFavorites {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

pub(super) fn module_key(module: AiPracticeModule) -> &'static str {
    match module {
        AiPracticeModule::DataAnalysis => "data_analysis",
        AiPracticeModule::LogicJudgment => "logic_judgment",
        AiPracticeModule::QuantitativeRelations => "quantitative_relations",
    }
}

#[async_trait::async_trait]
impl AiPracticeFavoritesPort for SqliteAiPracticeFavorites {
    async fn set(&self, id: &AiPracticeQuestionId, favorite: bool) -> Result<(), AiPracticeError> {
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|_| AiPracticeError::Unavailable)?;
        if favorite {
            let sql = match id.module {
                AiPracticeModule::DataAnalysis => "SELECT groups_json FROM data_analysis_runs WHERE id=? AND status='submitted'",
                AiPracticeModule::LogicJudgment => "SELECT questions_json FROM logic_judgment_runs WHERE id=? AND status='submitted'",
                AiPracticeModule::QuantitativeRelations => "SELECT questions_json FROM quantitative_relations_runs WHERE id=? AND status='submitted'",
            };
            let text: Option<String> = sqlx::query_scalar(sql)
                .bind(&id.session_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|_| AiPracticeError::Unavailable)?;
            let value: serde_json::Value =
                serde_json::from_str(text.as_deref().ok_or(AiPracticeError::NotFound)?)
                    .map_err(|_| AiPracticeError::Unavailable)?;
            let exists = match id.module {
                AiPracticeModule::DataAnalysis => value
                    .get(id.question / 5)
                    .and_then(|group| group.get("questions"))
                    .and_then(|questions| questions.get(id.question % 5))
                    .is_some(),
                _ => value.get(id.question).is_some(),
            };
            if !exists {
                return Err(AiPracticeError::NotFound);
            }
            sqlx::query("INSERT INTO ai_practice_favorites(module,session_id,question_no,created_at_ms) VALUES(?,?,?,?) ON CONFLICT(module,session_id,question_no) DO NOTHING")
                .bind(module_key(id.module)).bind(&id.session_id).bind(id.question as i64).bind(chrono::Utc::now().timestamp_millis()).execute(&mut *tx).await.map_err(|_| AiPracticeError::Unavailable)?;
        } else {
            sqlx::query("DELETE FROM ai_practice_favorites WHERE module=? AND session_id=? AND question_no=?")
                .bind(module_key(id.module)).bind(&id.session_id).bind(id.question as i64).execute(&mut *tx).await.map_err(|_| AiPracticeError::Unavailable)?;
        }
        tx.commit().await.map_err(|_| AiPracticeError::Unavailable)
    }

    async fn list(&self) -> Result<Vec<AiPracticeQuestionId>, AiPracticeError> {
        let rows = sqlx::query("SELECT module,session_id,question_no FROM ai_practice_favorites ORDER BY created_at_ms DESC, module, session_id, question_no")
            .fetch_all(&self.pool).await.map_err(|_| AiPracticeError::Unavailable)?;
        rows.into_iter()
            .map(|row| {
                let module = match row
                    .try_get::<String, _>("module")
                    .map_err(|_| AiPracticeError::Unavailable)?
                    .as_str()
                {
                    "data_analysis" => AiPracticeModule::DataAnalysis,
                    "logic_judgment" => AiPracticeModule::LogicJudgment,
                    "quantitative_relations" => AiPracticeModule::QuantitativeRelations,
                    _ => return Err(AiPracticeError::Unavailable),
                };
                let question = row
                    .try_get::<i64, _>("question_no")
                    .map_err(|_| AiPracticeError::Unavailable)?;
                let id = AiPracticeQuestionId {
                    module,
                    session_id: row
                        .try_get("session_id")
                        .map_err(|_| AiPracticeError::Unavailable)?,
                    question: usize::try_from(question)
                        .map_err(|_| AiPracticeError::Unavailable)?,
                };
                if !id.is_valid() {
                    return Err(AiPracticeError::Unavailable);
                }
                Ok(id)
            })
            .collect()
    }
}
