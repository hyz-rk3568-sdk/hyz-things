//! Metadata-only index in its own database; Markdown remains the content source.
use crate::application::question_bank::{BankError, IndexedPaper, QuestionBankRepositoryPort};
use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::str::FromStr;

pub struct SqliteQuestionBankRepository {
    pool: SqlitePool,
}
impl SqliteQuestionBankRepository {
    pub async fn connect(path: &str) -> Result<Self, BankError> {
        let options = SqliteConnectOptions::from_str(path)
            .map_err(|_| BankError::Storage)?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|_| BankError::Storage)?;
        sqlx::query("CREATE TABLE IF NOT EXISTS papers(id TEXT PRIMARY KEY, metadata TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 1)").execute(&pool).await.map_err(|_|BankError::Storage)?;
        sqlx::query("CREATE TABLE IF NOT EXISTS identities(id TEXT PRIMARY KEY, paper_id TEXT NOT NULL REFERENCES papers(id))").execute(&pool).await.map_err(|_|BankError::Storage)?;
        Ok(Self { pool })
    }
}
#[async_trait]
impl QuestionBankRepositoryPort for SqliteQuestionBankRepository {
    async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
        let rows = sqlx::query("SELECT metadata FROM papers WHERE active=1 ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|_| BankError::Storage)?;
        rows.iter()
            .map(|row| serde_json::from_str(row.get::<&str, _>(0)).map_err(|_| BankError::Storage))
            .collect()
    }
    async fn known(&self) -> Result<Vec<IndexedPaper>, BankError> {
        let rows = sqlx::query("SELECT metadata FROM papers ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|_| BankError::Storage)?;
        rows.iter()
            .map(|row| serde_json::from_str(row.get::<&str, _>(0)).map_err(|_| BankError::Storage))
            .collect()
    }
    async fn apply(&self, papers: Vec<IndexedPaper>) -> Result<(), BankError> {
        let mut tx = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        sqlx::query("UPDATE papers SET active=0")
            .execute(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
        for paper in papers {
            let metadata = serde_json::to_string(&paper).map_err(|_| BankError::Storage)?;
            sqlx::query("INSERT INTO papers(id,metadata,active) VALUES(?,?,1) ON CONFLICT(id) DO UPDATE SET metadata=excluded.metadata,active=1").bind(&paper.summary.id).bind(metadata).execute(&mut *tx).await.map_err(|_|BankError::Storage)?;
            for id in paper.ids {
                let owner: Option<String> =
                    sqlx::query_scalar("SELECT paper_id FROM identities WHERE id=?")
                        .bind(&id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|_| BankError::Storage)?;
                if owner
                    .as_ref()
                    .is_some_and(|owner| owner != &paper.summary.id)
                {
                    return Err(BankError::Conflict);
                }
                sqlx::query("INSERT OR IGNORE INTO identities(id,paper_id) VALUES(?,?)")
                    .bind(id)
                    .bind(&paper.summary.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| BankError::Storage)?;
            }
        }
        tx.commit().await.map_err(|_| BankError::Storage)?;
        Ok(())
    }
}
