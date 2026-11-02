//! Metadata-only index in its own database; Markdown remains the content source.
use crate::application::question_bank::practice::primary_module;
use crate::application::question_bank::{BankError, IndexedPaper, QuestionBankRepositoryPort};
use crate::domain::question_bank::{
    PracticeCatalog, PracticeCategory, PracticeMode, PracticeModule, PracticeSession,
};
use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::collections::BTreeMap;
use std::str::FromStr;

// This migrator is intentionally separate from the portal and Flashcard databases.
static QUESTION_BANK_MIGRATOR: sqlx::migrate::Migrator =
    sqlx::migrate!("./migrations/question_bank");

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
        QUESTION_BANK_MIGRATOR
            .run(&pool)
            .await
            .map_err(|_| BankError::Storage)?;
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
            for (ordinal, id) in paper.question_ids.iter().enumerate() {
                let number = paper
                    .question_numbers
                    .get(ordinal)
                    .copied()
                    .unwrap_or((ordinal + 1) as u32);
                let module = paper
                    .question_modules
                    .get(ordinal)
                    .and_then(|value| value.as_deref())
                    .and_then(|value| primary_module(Some(value)));
                let category = paper
                    .question_categories
                    .get(ordinal)
                    .and_then(|value| value.as_deref());
                sqlx::query("INSERT INTO questions(id,paper_id,number,source_hash,module,category) VALUES(?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET paper_id=excluded.paper_id,number=excluded.number,source_hash=excluded.source_hash,module=excluded.module,category=excluded.category")
                    .bind(id).bind(&paper.summary.id).bind(number as i64).bind(&paper.hash)
                    .bind(module).bind(category)
                    .execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
            }
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
    async fn practice_create(&self, session: PracticeSession) -> Result<(), BankError> {
        let mut tx = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        let data = serde_json::to_string(&session).map_err(|_| BankError::Storage)?;
        sqlx::query("INSERT INTO practice_sessions(id,status,revision,data) VALUES(?,?,?,?)")
            .bind(&session.id)
            .bind(&session.status)
            .bind(session.revision as i64)
            .bind(data)
            .execute(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
        for (ordinal, item) in session.items.iter().enumerate() {
            sqlx::query("INSERT INTO practice_items(session_id,ordinal,question_id,paper_id,number,source_hash,answer,grade) VALUES(?,?,?,?,?,?,?,NULL)")
                .bind(&session.id).bind(ordinal as i64).bind(&item.question_id)
                .bind(&item.paper_id).bind(item.number as i64).bind(&item.source_hash)
                .bind("[]").execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
        }
        tx.commit().await.map_err(|_| BankError::Storage)
    }

    async fn practice_load(&self, id: &str) -> Result<PracticeSession, BankError> {
        let data: Option<String> =
            sqlx::query_scalar("SELECT data FROM practice_sessions WHERE id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| BankError::Storage)?;
        serde_json::from_str(&data.ok_or(BankError::NotFound)?).map_err(|_| BankError::Storage)
    }

    async fn practice_update(
        &self,
        session: PracticeSession,
        previous_revision: u64,
    ) -> Result<(), BankError> {
        let mut tx = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        let data = serde_json::to_string(&session).map_err(|_| BankError::Storage)?;
        let updated = sqlx::query(
            "UPDATE practice_sessions SET status=?,revision=?,data=? WHERE id=? AND status='active' AND revision=?"
        ).bind(&session.status).bind(session.revision as i64).bind(data).bind(&session.id)
            .bind(previous_revision as i64).execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
        if updated.rows_affected() != 1 {
            return Err(BankError::Conflict);
        }
        for (ordinal, item) in session.items.iter().enumerate() {
            sqlx::query(
                "UPDATE practice_items SET answer=?,grade=? WHERE session_id=? AND ordinal=?",
            )
            .bind(serde_json::to_string(&item.answer).map_err(|_| BankError::Storage)?)
            .bind(&item.grade)
            .bind(&session.id)
            .bind(ordinal as i64)
            .execute(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
        }
        if session.status == "completed" {
            for item in &session.items {
                let grade = item.grade.as_deref().unwrap_or("ungradable");
                sqlx::query("INSERT OR IGNORE INTO practice_attempts(session_id,question_id,answer,grade,submitted_at) VALUES(?,?,?,?,?)")
                    .bind(&session.id).bind(&item.question_id)
                    .bind(serde_json::to_string(&item.answer).map_err(|_| BankError::Storage)?)
                    .bind(grade).bind(session.updated_at)
                    .execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
                if grade == "wrong" || grade == "correct" {
                    sqlx::query("INSERT INTO question_progress(question_id,pending,updated_at) VALUES(?,?,?) ON CONFLICT(question_id) DO UPDATE SET pending=excluded.pending,updated_at=excluded.updated_at")
                        .bind(&item.question_id).bind(i64::from(grade == "wrong")).bind(session.updated_at)
                        .execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
                }
            }
        }
        tx.commit().await.map_err(|_| BankError::Storage)
    }

    async fn practice_history(&self) -> Result<Vec<PracticeSession>, BankError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT data FROM practice_sessions ORDER BY json_extract(data,'$.created_at') DESC LIMIT 100"
        ).fetch_all(&self.pool).await.map_err(|_| BankError::Storage)?;
        rows.iter()
            .map(|json| serde_json::from_str(json).map_err(|_| BankError::Storage))
            .collect()
    }

    async fn practice_catalog(&self) -> Result<PracticeCatalog, BankError> {
        let rows = sqlx::query(
            "SELECT q.module,q.category,COUNT(*) AS total FROM questions q JOIN papers p ON p.id=q.paper_id WHERE p.active=1 AND json_extract(p.metadata,'$.summary.role')='primary' GROUP BY q.module,q.category ORDER BY q.module,q.category"
        ).fetch_all(&self.pool).await.map_err(|_| BankError::Storage)?;
        let mut modules: BTreeMap<String, (usize, usize, BTreeMap<String, usize>)> =
            BTreeMap::new();
        let mut unclassified = 0;
        for row in rows {
            let module: Option<String> = row.get(0);
            let category: Option<String> = row.get(1);
            let count: i64 = row.get(2);
            let count = count as usize;
            if let Some(module) = module {
                let entry = modules.entry(module).or_default();
                entry.0 += count;
                if let Some(category) = category.filter(|c| !c.trim().is_empty()) {
                    *entry.2.entry(category).or_default() += count;
                } else {
                    entry.1 += count;
                }
            } else {
                unclassified += count;
            }
        }
        Ok(PracticeCatalog {
            modules: modules
                .into_iter()
                .map(
                    |(name, (count, uncategorized, categories))| PracticeModule {
                        name,
                        count,
                        uncategorized,
                        categories: categories
                            .into_iter()
                            .map(|(name, count)| PracticeCategory { name, count })
                            .collect(),
                    },
                )
                .collect(),
            unclassified,
        })
    }

    async fn practice_module_candidates(
        &self,
        module: &str,
        category: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>, BankError> {
        if limit == 0 || limit > 100 {
            return Err(BankError::Invalid);
        }
        // SQLite's integer sort puts never-practiced questions first. A stable source
        // ordering keeps a created session fully reproducible on every resume.
        let base = "SELECT q.id FROM questions q JOIN papers p ON p.id=q.paper_id WHERE p.active=1 AND json_extract(p.metadata,'$.summary.role')='primary' AND q.module=? ";
        let suffix = " ORDER BY EXISTS(SELECT 1 FROM practice_attempts a WHERE a.question_id=q.id), CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER) DESC, p.id, q.number LIMIT ?";
        let sql = if category.is_some() {
            format!("{base}AND q.category=?{suffix}")
        } else {
            format!("{base}{suffix}")
        };
        let mut query = sqlx::query_scalar::<_, String>(&sql).bind(module);
        if let Some(category) = category {
            query = query.bind(category);
        }
        query
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|_| BankError::Storage)
    }

    async fn practice_candidates(&self, mode: PracticeMode) -> Result<Vec<String>, BankError> {
        let sql = match mode {
            PracticeMode::Wrong => "SELECT q.id FROM questions q JOIN papers p ON p.id=q.paper_id JOIN question_progress s ON s.question_id=q.id WHERE p.active=1 AND json_extract(p.metadata,'$.summary.role')='primary' AND s.pending=1 ORDER BY p.id,q.number",
            PracticeMode::Favorite => "SELECT q.id FROM questions q JOIN papers p ON p.id=q.paper_id JOIN favorites s ON s.question_id=q.id WHERE p.active=1 AND json_extract(p.metadata,'$.summary.role')='primary' AND s.favorite=1 ORDER BY p.id,q.number",
            _ => return Err(BankError::Invalid),
        };
        sqlx::query_scalar(sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|_| BankError::Storage)
    }

    async fn practice_favorite(&self, question_id: &str, favorite: bool) -> Result<(), BankError> {
        let exists: Option<String> = sqlx::query_scalar("SELECT q.id FROM questions q JOIN papers p ON p.id=q.paper_id WHERE q.id=? AND p.active=1")
            .bind(question_id).fetch_optional(&self.pool).await.map_err(|_| BankError::Storage)?;
        if exists.is_none() {
            return Err(BankError::NotFound);
        }
        sqlx::query("INSERT INTO favorites(question_id,favorite) VALUES(?,?) ON CONFLICT(question_id) DO UPDATE SET favorite=excluded.favorite")
            .bind(question_id).bind(i64::from(favorite)).execute(&self.pool)
            .await.map_err(|_| BankError::Storage)?;
        Ok(())
    }

    async fn practice_is_favorite(&self, question_id: &str) -> Result<bool, BankError> {
        let value: Option<i64> =
            sqlx::query_scalar("SELECT favorite FROM favorites WHERE question_id=?")
                .bind(question_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| BankError::Storage)?;
        Ok(value.unwrap_or(0) != 0)
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    async fn memory_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn fresh_question_bank_has_all_tables() {
        let pool = memory_pool().await;
        QUESTION_BANK_MIGRATOR.run(&pool).await.unwrap();
        for table in [
            "papers",
            "identities",
            "questions",
            "practice_sessions",
            "practice_items",
            "practice_attempts",
            "question_progress",
            "favorites",
        ] {
            let exists: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(exists, 1, "missing Question Bank table: {table}");
        }
    }

    #[tokio::test]
    async fn upgrade_preserves_legacy_index_and_repeated_migration_preserves_learning() {
        let pool = memory_pool().await;
        sqlx::query("CREATE TABLE papers(id TEXT PRIMARY KEY, metadata TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE identities(id TEXT PRIMARY KEY, paper_id TEXT NOT NULL REFERENCES papers(id))")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO papers(id,metadata,active) VALUES('paper-1','{}',1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO identities(id,paper_id) VALUES('question-1','paper-1')")
            .execute(&pool)
            .await
            .unwrap();

        QUESTION_BANK_MIGRATOR.run(&pool).await.unwrap();
        sqlx::query("INSERT INTO favorites(question_id,favorite) VALUES('question-1',1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO question_progress(question_id,pending,updated_at) VALUES('question-1',1,123)")
            .execute(&pool)
            .await
            .unwrap();

        // Reopening the same database is safe and never rebuilds user state.
        QUESTION_BANK_MIGRATOR.run(&pool).await.unwrap();
        for (table, expected) in [
            ("papers", 1_i64),
            ("identities", 1),
            ("favorites", 1),
            ("question_progress", 1),
        ] {
            let query = format!("SELECT count(*) FROM {table}");
            let count: i64 = sqlx::query_scalar(&query).fetch_one(&pool).await.unwrap();
            assert_eq!(count, expected, "{table} was unexpectedly cleared");
        }
        let applied: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success=1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(applied, 2, "each version is applied exactly once");
    }
}
