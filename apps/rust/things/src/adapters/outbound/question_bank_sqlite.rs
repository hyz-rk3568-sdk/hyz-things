//! Runtime question content in SQLite, populated by explicit Markdown import.
use crate::application::question_bank::document::structural_gradable;
use crate::application::question_bank::{
    BankError, DailyAnsweredBreakdownRow, DrillPaperRow, IndexedPaper, PaperContent,
    PracticeContent, QuestionBankRepositoryPort, StoredQuestion, StudyAnswerDay,
};
use crate::domain::question_bank::primary_module;
use crate::domain::question_bank::{
    MaterialView, ModulePracticeFilters, PaperSummary, PracticeBrowseItem, PracticeCatalog,
    PracticeCategory, PracticeMode, PracticeModule, PracticeSession,
};
use crate::domain::question_bank::{StudyPlan, StudyPlanItem};
use async_trait::async_trait;
use futures_util::TryStreamExt;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

// This migrator is intentionally separate from the portal and Flashcard databases.
static QUESTION_BANK_MIGRATOR: sqlx::migrate::Migrator =
    sqlx::migrate!("./migrations/question_bank");

/// Indexes written by binaries before structural gradability was detached
/// from issue verification contain stale `gradable=0` rows. The repair is a
/// one-time data migration guarded by `PRAGMA user_version`.
const QUESTION_BANK_GRADABLE_REPAIR_VERSION: i64 = 1;

async fn initialize_schema(pool: &SqlitePool) -> Result<(), BankError> {
    QUESTION_BANK_MIGRATOR
        .run(pool)
        .await
        .map_err(|_| BankError::Storage)?;
    repair_gradable(pool).await
}

async fn repair_gradable(pool: &SqlitePool) -> Result<(), BankError> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await
        .map_err(|_| BankError::Storage)?;
    if version >= QUESTION_BANK_GRADABLE_REPAIR_VERSION {
        return Ok(());
    }
    let mut tx = pool.begin().await.map_err(|_| BankError::Storage)?;
    let mut last_rowid = 0_i64;
    loop {
        let rows = sqlx::query(
            "SELECT rowid AS rowid,id,COALESCE(stem,'') AS stem,COALESCE(options_markdown,'') AS options_markdown,correct_answer FROM questions WHERE rowid>? ORDER BY rowid LIMIT 1000",
        )
        .bind(last_rowid)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| BankError::Storage)?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let rowid: i64 = row.get("rowid");
            last_rowid = rowid;
            let answer: Vec<String> = row
                .try_get::<Option<&str>, _>("correct_answer")
                .map_err(|_| BankError::Storage)?
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or_default();
            let gradable = i64::from(structural_gradable(
                row.get("stem"),
                row.get("options_markdown"),
                &answer,
            ));
            sqlx::query("UPDATE questions SET gradable=? WHERE rowid=? AND gradable<>?")
                .bind(gradable)
                .bind(rowid)
                .bind(gradable)
                .execute(&mut *tx)
                .await
                .map_err(|_| BankError::Storage)?;
        }
    }
    sqlx::query("PRAGMA user_version=1")
        .execute(&mut *tx)
        .await
        .map_err(|_| BankError::Storage)?;
    tx.commit().await.map_err(|_| BankError::Storage)
}

pub struct SqliteQuestionBankRepository {
    pool: SqlitePool,
}
impl SqliteQuestionBankRepository {
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

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
        initialize_schema(&pool).await?;
        Ok(Self { pool })
    }
}
#[async_trait]
impl QuestionBankRepositoryPort for SqliteQuestionBankRepository {
    async fn data_revision(&self) -> Result<i64, BankError> {
        sqlx::query_scalar("SELECT revision FROM data_revision WHERE id = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|_| BankError::Storage)
    }

    async fn daily_answered_counts(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<(String, i64)>, BankError> {
        sqlx::query_as(
            "SELECT date(a.submitted_at / 1000.0, 'unixepoch', '+8 hours') AS day, COUNT(*)
             FROM practice_attempts a
             JOIN practice_sessions s ON s.id=a.session_id
             WHERE s.status='completed' AND a.submitted_at>=? AND a.submitted_at<?
               AND json_type(a.answer)='array'
               AND EXISTS (SELECT 1 FROM json_each(a.answer) choice
                 WHERE choice.type='text'
                   AND length(trim(choice.value, char(9)||char(10)||char(13)||' '))>0)
             GROUP BY day ORDER BY day",
        )
        .bind(start_ms)
        .bind(end_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| BankError::Storage)
    }

    async fn daily_answered_breakdown(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<DailyAnsweredBreakdownRow>, BankError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                i64,
            ),
        >(
            "SELECT date(a.submitted_at / 1000.0, 'unixepoch', '+8 hours') AS day, \
             CASE \
               WHEN json_extract(s.data,'$.mode')='module' THEN 1 \
               WHEN json_extract(s.data,'$.paper_id') IS NOT NULL THEN 0 \
               WHEN json_extract(s.data,'$.module') IS NOT NULL THEN 1 \
               ELSE 0 \
             END AS is_module, \
             COALESCE(p.kind,'exam') AS kind, \
             p.collection_title AS collection_title, \
             q.module AS module, \
             q.category AS category, \
             json_extract(p.metadata,'$.summary.title') AS paper_title, \
             COUNT(*) AS answered \
             FROM practice_attempts a \
             JOIN practice_sessions s ON s.id=a.session_id \
             LEFT JOIN questions q ON q.id=a.question_id \
             LEFT JOIN papers p ON p.id=q.paper_id \
             WHERE s.status='completed' AND a.submitted_at>=? AND a.submitted_at<? \
               AND json_type(a.answer)='array' \
               AND EXISTS (SELECT 1 FROM json_each(a.answer) choice \
                 WHERE choice.type='text' \
                   AND length(trim(choice.value, char(9)||char(10)||char(13)||' '))>0) \
             GROUP BY day,is_module,kind,collection_title,module,category,paper_title \
             ORDER BY day",
        )
        .bind(start_ms)
        .bind(end_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| BankError::Storage)?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    day,
                    is_module,
                    kind,
                    collection_title,
                    module,
                    category,
                    paper_title,
                    answered,
                )| DailyAnsweredBreakdownRow {
                    day,
                    is_module: is_module != 0,
                    kind,
                    collection_title,
                    module,
                    category,
                    paper_title,
                    answered,
                },
            )
            .collect())
    }

    async fn paper_summaries(&self) -> Result<Vec<PaperSummary>, BankError> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT json_extract(metadata,'$.summary') FROM papers WHERE active=1 AND COALESCE(kind,'exam')='exam' AND json_extract(metadata,'$.summary.role')='primary' ORDER BY id")
            .fetch_all(&self.pool).await.map_err(|_| BankError::Storage)?;
        rows.into_iter()
            .map(|summary| serde_json::from_str(&summary).map_err(|_| BankError::Storage))
            .collect()
    }

    async fn indexed_paper(&self, id: &str) -> Result<IndexedPaper, BankError> {
        let metadata: Option<String> =
            sqlx::query_scalar("SELECT metadata FROM papers WHERE id=? AND active=1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| BankError::Storage)?;
        serde_json::from_str(&metadata.ok_or(BankError::NotFound)?).map_err(|_| BankError::Storage)
    }

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
        let snapshot_papers: BTreeSet<String> = papers
            .iter()
            .map(|paper| paper.summary.id.clone())
            .collect();
        let mut question_owners: BTreeMap<String, String> = BTreeMap::new();
        for paper in &papers {
            for id in &paper.question_ids {
                if question_owners
                    .insert(id.clone(), paper.summary.id.clone())
                    .is_some()
                {
                    return Err(BankError::Conflict);
                }
            }
        }
        let mut tx = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        sqlx::query("UPDATE papers SET active=0")
            .execute(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
        for paper in papers {
            // Read the previous hash before the metadata upsert below.
            let previous_hash: Option<String> =
                sqlx::query_scalar("SELECT json_extract(metadata,'$.hash') FROM papers WHERE id=?")
                    .bind(&paper.summary.id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|_| BankError::Storage)?;
            let cached_questions: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM questions WHERE paper_id=? AND stem IS NOT NULL AND source_hash=?",
            )
            .bind(&paper.summary.id)
            .bind(&paper.hash)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
            let cached_materials: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM paper_materials WHERE paper_id=?")
                    .bind(&paper.summary.id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| BankError::Storage)?;
            let metadata = serde_json::to_string(&paper).map_err(|_| BankError::Storage)?;
            sqlx::query("INSERT INTO papers(id,metadata,active,kind,subject,collection_id,collection_title) VALUES(?,?,1,?,?,?,?) ON CONFLICT(id) DO UPDATE SET metadata=excluded.metadata,active=1,kind=excluded.kind,subject=excluded.subject,collection_id=excluded.collection_id,collection_title=excluded.collection_title")
                .bind(&paper.summary.id)
                .bind(metadata)
                .bind(&paper.summary.kind)
                .bind(&paper.summary.subject)
                .bind(&paper.summary.collection_id)
                .bind(&paper.summary.collection_title)
                .execute(&mut *tx)
                .await
                .map_err(|_| BankError::Storage)?;
            // Rewrite parsed content only when the source hash changed or the
            // cache is incomplete. Re-running sync keeps complete unchanged rows.
            let content_changed = previous_hash.as_deref() != Some(paper.hash.as_str())
                || cached_questions as usize != paper.summary.questions
                || cached_materials as usize != paper.material_ids.len();
            if content_changed && !paper.content.questions.is_empty() {
                sqlx::query("DELETE FROM questions WHERE paper_id=?")
                    .bind(&paper.summary.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| BankError::Storage)?;
                sqlx::query("DELETE FROM paper_materials WHERE paper_id=?")
                    .bind(&paper.summary.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|_| BankError::Storage)?;
                for question in &paper.content.questions {
                    let topics =
                        serde_json::to_string(&question.topics).map_err(|_| BankError::Storage)?;
                    let materials = serde_json::to_string(&question.materials)
                        .map_err(|_| BankError::Storage)?;
                    let correct = serde_json::to_string(&question.correct_answer)
                        .map_err(|_| BankError::Storage)?;
                    let module = question
                        .module
                        .as_deref()
                        .and_then(|value| primary_module(Some(value)));
                    sqlx::query("INSERT INTO questions(id,paper_id,number,source_hash,module,category,stem,options_markdown,topics,materials,answer_type,question_type,grading_mode,max_score,gradable,correct_answer,explanation,reference_answer) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET paper_id=excluded.paper_id,number=excluded.number,source_hash=excluded.source_hash,module=excluded.module,category=excluded.category,stem=excluded.stem,options_markdown=excluded.options_markdown,topics=excluded.topics,materials=excluded.materials,answer_type=excluded.answer_type,question_type=excluded.question_type,grading_mode=excluded.grading_mode,max_score=excluded.max_score,gradable=excluded.gradable,correct_answer=excluded.correct_answer,explanation=excluded.explanation,reference_answer=excluded.reference_answer")
                        .bind(&question.id).bind(&paper.summary.id).bind(question.number as i64)
                        .bind(&paper.hash).bind(module).bind(&question.category)
                        .bind(&question.stem).bind(&question.options_markdown)
                        .bind(topics).bind(materials)
                        .bind(&question.answer_type).bind(&question.question_type)
                        .bind(&question.grading_mode).bind(question.max_score)
                        .bind(i64::from(question.gradable)).bind(correct).bind(&question.explanation)
                        .bind(&question.reference_answer)
                        .execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
                }
                for (ordinal, material) in paper.content.materials.iter().enumerate() {
                    sqlx::query("INSERT INTO paper_materials(paper_id,material_id,ordinal,name,markdown) VALUES(?,?,?,?,?)")
                        .bind(&paper.summary.id).bind(&material.id).bind(ordinal as i64)
                        .bind(&material.name).bind(&material.markdown)
                        .execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
                }
            } else if content_changed {
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
            }
            for id in &paper.question_ids {
                let fingerprint = paper.content_fingerprints.get(id);
                sqlx::query("UPDATE questions SET content_fingerprint=? WHERE paper_id=? AND id=? AND content_fingerprint IS NOT ?")
                    .bind(fingerprint).bind(&paper.summary.id).bind(id).bind(fingerprint)
                    .execute(&mut *tx).await.map_err(|_| BankError::Storage)?;
            }
            for id in paper.ids {
                let owner: Option<String> =
                    sqlx::query_scalar("SELECT paper_id FROM identities WHERE id=?")
                        .bind(&id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|_| BankError::Storage)?;
                if let Some(owner) = owner {
                    if owner != paper.summary.id {
                        // Only a question may move, and only while its previous
                        // paper is still part of this snapshot and has already
                        // dropped it. Materials and paper identities stay fixed.
                        let movable = question_owners
                            .get(&id)
                            .is_some_and(|new_owner| new_owner == &paper.summary.id)
                            && snapshot_papers.contains(&owner);
                        if !movable {
                            return Err(BankError::Conflict);
                        }
                        sqlx::query("UPDATE identities SET paper_id=? WHERE id=? AND paper_id=?")
                            .bind(&paper.summary.id)
                            .bind(&id)
                            .bind(&owner)
                            .execute(&mut *tx)
                            .await
                            .map_err(|_| BankError::Storage)?;
                    }
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

    async fn paper_module_counts(
        &self,
        paper_id: &str,
        source_hash: &str,
    ) -> Result<Vec<(Option<String>, usize, usize)>, BankError> {
        let cached_hash: Option<String> =
            sqlx::query_scalar("SELECT source_hash FROM questions WHERE paper_id=? LIMIT 1")
                .bind(paper_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| BankError::Storage)?;
        if cached_hash.as_deref() != Some(source_hash) {
            return Err(BankError::Stale);
        }
        let rows = sqlx::query(
            "SELECT module,\
             SUM(CASE WHEN gradable=1 THEN 1 ELSE 0 END),\
             SUM(CASE WHEN gradable=1 THEN 0 ELSE 1 END)\
             FROM questions WHERE paper_id=? AND source_hash=? GROUP BY module",
        )
        .bind(paper_id)
        .bind(source_hash)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| BankError::Storage)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<Option<String>, _>(0),
                    row.get::<i64, _>(1) as usize,
                    row.get::<i64, _>(2) as usize,
                )
            })
            .collect())
    }

    async fn paper_content(&self, paper_id: &str) -> Result<PaperContent, BankError> {
        let mut tx = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        let metadata: Option<String> = sqlx::query_scalar("SELECT metadata FROM papers WHERE id=?")
            .bind(paper_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
        let paper: IndexedPaper = serde_json::from_str(&metadata.ok_or(BankError::NotFound)?)
            .map_err(|_| BankError::Storage)?;
        let rows = sqlx::query(
            "SELECT * FROM questions WHERE paper_id=? AND stem IS NOT NULL ORDER BY number",
        )
        .bind(paper_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| BankError::Storage)?;
        if rows.len() != paper.summary.questions {
            return Err(BankError::Unavailable);
        }
        let questions = rows
            .iter()
            .map(|row| {
                let hash: String = row.try_get("source_hash").map_err(|_| BankError::Storage)?;
                if hash != paper.hash {
                    return Err(BankError::Stale);
                }
                decode_question(row)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let materials = sqlx::query(
            "SELECT material_id,name,markdown FROM paper_materials WHERE paper_id=? ORDER BY ordinal",
        )
        .bind(paper_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| BankError::Storage)?
        .into_iter()
        .map(|row| MaterialView {
            id: row.get(0),
            name: row.get(1),
            markdown: row.get(2),
        })
        .collect::<Vec<_>>();
        let ids = materials
            .iter()
            .map(|material: &MaterialView| material.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if questions.iter().any(|question| {
            question
                .materials
                .iter()
                .any(|id| !ids.contains(id.as_str()))
        }) {
            return Err(BankError::Unavailable);
        }
        tx.commit().await.map_err(|_| BankError::Storage)?;
        Ok(PaperContent {
            paper_id: paper.summary.id,
            hash: paper.hash,
            questions,
            materials,
        })
    }
    async fn question_contents(&self, ids: &[String]) -> Result<Vec<PracticeContent>, BankError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let requested = serde_json::to_string(ids).map_err(|_| BankError::Storage)?;
        let mut tx = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        let rows = sqlx::query(
            "SELECT q.*,json_extract(p.metadata,'$.summary.title') AS paper_title,              (p.active=1 AND json_extract(p.metadata,'$.summary.role')='primary') AS active              FROM json_each(?) selected JOIN questions q ON q.id=selected.value              JOIN papers p ON p.id=q.paper_id              WHERE q.stem IS NOT NULL AND q.source_hash=json_extract(p.metadata,'$.hash')"
        ).bind(requested).fetch_all(&mut *tx).await.map_err(|_| BankError::Storage)?;
        let mut contents = rows
            .iter()
            .map(|row| {
                Ok(PracticeContent {
                    paper_id: row.try_get("paper_id").map_err(|_| BankError::Storage)?,
                    paper_title: row.try_get("paper_title").map_err(|_| BankError::Storage)?,
                    source_hash: row.try_get("source_hash").map_err(|_| BankError::Storage)?,
                    active: row
                        .try_get::<i64, _>("active")
                        .map_err(|_| BankError::Storage)?
                        != 0,
                    question: decode_question(row)?,
                    materials: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>, BankError>>()?;
        let materials = contents
            .iter()
            .flat_map(|content| {
                content
                    .question
                    .materials
                    .iter()
                    .map(|id| (content.paper_id.clone(), id.clone()))
            })
            .collect::<std::collections::BTreeSet<_>>();
        let requested = serde_json::to_string(&materials).map_err(|_| BankError::Storage)?;
        let rows = sqlx::query(
            "SELECT m.paper_id,m.material_id,m.name,m.markdown              FROM json_each(?) selected JOIN paper_materials m              ON m.paper_id=json_extract(selected.value,'$[0]')              AND m.material_id=json_extract(selected.value,'$[1]')"
        ).bind(requested).fetch_all(&mut *tx).await.map_err(|_| BankError::Storage)?;
        let materials = rows
            .into_iter()
            .map(|row| {
                (
                    (
                        row.get::<String, _>("paper_id"),
                        row.get::<String, _>("material_id"),
                    ),
                    MaterialView {
                        id: row.get("material_id"),
                        name: row.get("name"),
                        markdown: row.get("markdown"),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        for content in &mut contents {
            content.materials = content
                .question
                .materials
                .iter()
                .map(|id| {
                    materials
                        .get(&(content.paper_id.clone(), id.clone()))
                        .cloned()
                        .ok_or(BankError::Unavailable)
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
        tx.commit().await.map_err(|_| BankError::Storage)?;
        Ok(contents)
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

    async fn practice_save_answer(
        &self,
        session: PracticeSession,
        question_id: &str,
        previous_revision: u64,
    ) -> Result<(), BankError> {
        if session.status != "active"
            || session.paused
            || previous_revision.checked_add(1) != Some(session.revision)
        {
            return Err(BankError::Conflict);
        }
        let mut matching = session
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.question_id == question_id);
        let (ordinal, item) = matching.next().ok_or(BankError::NotFound)?;
        if matching.next().is_some() {
            return Err(BankError::Conflict);
        }
        let revision = i64::try_from(session.revision).map_err(|_| BankError::Invalid)?;
        let previous_revision = i64::try_from(previous_revision).map_err(|_| BankError::Invalid)?;
        let ordinal = i64::try_from(ordinal).map_err(|_| BankError::Invalid)?;
        let answer = serde_json::to_string(&item.answer).map_err(|_| BankError::Storage)?;
        let answer_path = format!("$.items[{ordinal}].answer");
        let question_path = format!("$.items[{ordinal}].question_id");
        let mut tx = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        let result = async {
            let updated = sqlx::query(
                "UPDATE practice_sessions SET revision=?,data=json_set(data,?,json(?),'$.revision',?,'$.updated_at',?) \
                 WHERE id=? AND status='active' AND revision=? \
                 AND json_extract(data,'$.status')='active' \
                 AND json_extract(data,'$.revision')=? AND json_extract(data,'$.paused')=0 \
                 AND json_extract(data,?)=?",
            )
            .bind(revision)
            .bind(&answer_path)
            .bind(&answer)
            .bind(revision)
            .bind(session.updated_at)
            .bind(&session.id)
            .bind(previous_revision)
            .bind(previous_revision)
            .bind(&question_path)
            .bind(question_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
            if updated.rows_affected() != 1 {
                return Err(BankError::Conflict);
            }
            let updated = sqlx::query(
                "UPDATE practice_items SET answer=? WHERE session_id=? AND ordinal=? AND question_id=?",
            )
            .bind(&answer)
            .bind(&session.id)
            .bind(ordinal)
            .bind(question_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| BankError::Storage)?;
            if updated.rows_affected() != 1 {
                return Err(BankError::Conflict);
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            tx.rollback().await.map_err(|_| BankError::Storage)?;
            return Err(error);
        }
        tx.commit().await.map_err(|_| BankError::Storage)
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
                // Only chosen answers count as done; skipped questions stay
                // ahead for the next module drill.
                if item.answer.is_empty() {
                    continue;
                }
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

    async fn practice_attempt_counts(
        &self,
        ids: &[String],
    ) -> Result<Vec<(String, usize, usize)>, BankError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let requested = serde_json::to_string(ids).map_err(|_| BankError::Storage)?;
        let rows = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT question_id, \
             SUM(CASE WHEN grade='correct' THEN 1 ELSE 0 END), \
             SUM(CASE WHEN grade='wrong' THEN 1 ELSE 0 END) \
             FROM practice_attempts \
             WHERE question_id IN (SELECT value FROM json_each(?)) \
             GROUP BY question_id",
        )
        .bind(requested)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| BankError::Storage)?;
        Ok(rows
            .into_iter()
            .map(|(id, correct, wrong)| (id, correct.max(0) as usize, wrong.max(0) as usize))
            .collect())
    }

    async fn practice_catalog(
        &self,
        filters: &ModulePracticeFilters,
    ) -> Result<PracticeCatalog, BankError> {
        let regions = serde_json::to_string(&filters.regions).map_err(|_| BankError::Storage)?;
        // LIMIT -1 prevents older SQLite versions from flattening the paper filter.
        let rows = sqlx::query(
            "WITH eligible_papers AS MATERIALIZED (\
             SELECT p.id FROM papers p \
             WHERE p.active=1 AND p.subject='xingce' AND COALESCE(p.kind,'exam')='exam' AND json_extract(p.metadata,'$.summary.role')='primary' \
             AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)>=?) \
             AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)<=?) \
             AND (?=0 OR (EXISTS (SELECT 1 FROM json_each(?) \
                  WHERE value=json_extract(p.metadata,'$.summary.region'))) != (?)) \
             LIMIT -1) \
             , answered_ids AS MATERIALIZED (\
             SELECT DISTINCT COALESCE('fp:'||q.content_fingerprint,'id:'||q.id) AS identity \
             FROM practice_attempts a JOIN questions q ON q.id=a.question_id \
             WHERE a.answer NOT IN ('[]','')), \
             candidates AS MATERIALIZED (\
             SELECT q.module,q.category,\
               COALESCE('fp:'||q.content_fingerprint,'id:'||q.id) AS identity,\
               (answered.identity IS NOT NULL) AS answered \
             FROM eligible_papers p JOIN questions q ON q.paper_id=p.id \
             LEFT JOIN answered_ids answered ON answered.identity=\
               COALESCE('fp:'||q.content_fingerprint,'id:'||q.id) \
             WHERE q.gradable=1), \
             unique_modules AS (SELECT module,identity,MAX(answered) AS answered \
               FROM candidates GROUP BY module,identity), \
             unique_categories AS (SELECT module,category,identity,MAX(answered) AS answered \
               FROM candidates WHERE module IS NOT NULL GROUP BY module,category,identity) \
             SELECT module,NULL AS category,COUNT(*) AS total,SUM(answered) AS done,0 AS scope \
               FROM unique_modules GROUP BY module \
             UNION ALL \
             SELECT module,category,COUNT(*) AS total,SUM(answered) AS done,1 AS scope \
               FROM unique_categories GROUP BY module,category \
             ORDER BY module,scope,category",
        )
        .bind(filters.year_min)
        .bind(filters.year_min)
        .bind(filters.year_max)
        .bind(filters.year_max)
        .bind(i64::from(!filters.regions.is_empty()))
        .bind(regions)
        .bind(i64::from(filters.exclude_regions))
        .fetch_all(&self.pool)
        .await
        .map_err(|_| BankError::Storage)?;
        let mut modules: BTreeMap<String, (usize, usize, usize, BTreeMap<String, (usize, usize)>)> =
            BTreeMap::new();
        let mut unclassified = 0;
        for row in rows {
            let module: Option<String> = row.get(0);
            let category: Option<String> = row.get(1);
            let count = row.get::<i64, _>(2) as usize;
            let done = row.get::<i64, _>(3) as usize;
            if let Some(module) = module {
                let entry = modules.entry(module).or_default();
                if row.get::<i64, _>(4) == 0 {
                    entry.0 = count;
                    entry.1 = done;
                } else if let Some(category) = category.filter(|c| !c.trim().is_empty()) {
                    entry.3.insert(category, (count, done));
                } else {
                    entry.2 += count;
                }
            } else {
                unclassified += count;
            }
        }
        Ok(PracticeCatalog {
            modules: modules
                .into_iter()
                .map(
                    |(name, (count, done, uncategorized, categories))| PracticeModule {
                        name,
                        count,
                        done,
                        uncategorized,
                        categories: categories
                            .into_iter()
                            .map(|(name, (count, done))| PracticeCategory { name, count, done })
                            .collect(),
                    },
                )
                .collect(),
            unclassified,
        })
    }

    async fn drill_paper_stats(&self) -> Result<Vec<DrillPaperRow>, BankError> {
        let rows =
            sqlx::query_as::<_, (String, String, String, String, String, i64, i64, i64, i64)>(
                "SELECT p.id, \
             json_extract(p.metadata,'$.summary.title'), \
             p.collection_id, p.collection_title, \
             COALESCE(MAX(q.module),''), \
             CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER), \
             COUNT(q.id), COALESCE(SUM(q.gradable),0), \
             COALESCE(SUM(CASE WHEN EXISTS (\
               SELECT 1 FROM practice_attempts a WHERE a.question_id=q.id \
                 AND json_type(a.answer)='array' \
                 AND EXISTS (SELECT 1 FROM json_each(a.answer) choice \
                   WHERE choice.type='text' \
                     AND length(trim(choice.value, char(9)||char(10)||char(13)||' '))>0)) \
               THEN 1 ELSE 0 END),0) \
             FROM papers p LEFT JOIN questions q ON q.paper_id=p.id \
             WHERE p.active=1 AND COALESCE(p.kind,'exam')='drill' \
               AND p.collection_id IS NOT NULL AND p.collection_title IS NOT NULL \
             GROUP BY p.id \
             ORDER BY p.collection_title, p.id",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|_| BankError::Storage)?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    paper_id,
                    title,
                    collection_id,
                    collection_title,
                    module,
                    year,
                    questions,
                    gradable,
                    done,
                )| DrillPaperRow {
                    paper_id,
                    title,
                    collection_id,
                    collection_title,
                    module: (!module.trim().is_empty()).then_some(module),
                    year: year as i32,
                    questions: questions.max(0) as usize,
                    gradable: gradable.max(0) as usize,
                    done: done.max(0) as usize,
                },
            )
            .collect())
    }

    async fn drill_question_ids(
        &self,
        collection_id: &str,
        paper_id: Option<&str>,
    ) -> Result<Vec<String>, BankError> {
        sqlx::query_scalar(
            "SELECT q.id FROM questions q JOIN papers p ON p.id=q.paper_id \
             WHERE p.active=1 AND COALESCE(p.kind,'exam')='drill' \
               AND p.collection_id=? AND (? IS NULL OR p.id=?) \
             ORDER BY p.id,q.number",
        )
        .bind(collection_id)
        .bind(paper_id)
        .bind(paper_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| BankError::Storage)
    }

    async fn module_settings(&self) -> Result<ModulePracticeFilters, BankError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT filters FROM module_practice_settings WHERE id=1")
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| BankError::Storage)?;
        match value {
            Some(value) => serde_json::from_str(&value).map_err(|_| BankError::Storage),
            None => Ok(ModulePracticeFilters::default()),
        }
    }

    async fn save_module_settings(&self, filters: &ModulePracticeFilters) -> Result<(), BankError> {
        let value = serde_json::to_string(filters).map_err(|_| BankError::Storage)?;
        sqlx::query("INSERT INTO module_practice_settings(id, filters) VALUES(1, ?) ON CONFLICT(id) DO UPDATE SET filters=excluded.filters")
            .bind(value).execute(&self.pool).await.map_err(|_| BankError::Storage)?;
        Ok(())
    }

    async fn study_plan(&self) -> Result<StudyPlan, BankError> {
        let enabled: Option<i64> =
            sqlx::query_scalar("SELECT prediction_enabled FROM study_plan_settings WHERE id=1")
                .fetch_optional(&self.pool)
                .await
                .map_err(|_| BankError::Storage)?;
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT module,category FROM study_plan_items ORDER BY module,category")
                .fetch_all(&self.pool)
                .await
                .map_err(|_| BankError::Storage)?;
        Ok(StudyPlan {
            prediction_enabled: enabled.unwrap_or(1) != 0,
            items: rows
                .into_iter()
                .map(|(module, category)| StudyPlanItem {
                    module,
                    category: (!category.is_empty()).then_some(category),
                })
                .collect(),
        })
    }

    async fn save_study_plan(&self, plan: &StudyPlan) -> Result<(), BankError> {
        let mut transaction = self.pool.begin().await.map_err(|_| BankError::Storage)?;
        sqlx::query("UPDATE study_plan_settings SET prediction_enabled=? WHERE id=1")
            .bind(i64::from(plan.prediction_enabled))
            .execute(&mut *transaction)
            .await
            .map_err(|_| BankError::Storage)?;
        sqlx::query("DELETE FROM study_plan_items")
            .execute(&mut *transaction)
            .await
            .map_err(|_| BankError::Storage)?;
        for item in &plan.items {
            sqlx::query("INSERT INTO study_plan_items(module,category) VALUES(?,?)")
                .bind(&item.module)
                .bind(item.category.as_deref().unwrap_or(""))
                .execute(&mut *transaction)
                .await
                .map_err(|_| BankError::Storage)?;
        }
        transaction.commit().await.map_err(|_| BankError::Storage)
    }

    async fn study_answered_days(
        &self,
        filters: &ModulePracticeFilters,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<StudyAnswerDay>, BankError> {
        let regions = serde_json::to_string(&filters.regions).map_err(|_| BankError::Storage)?;
        let rows = sqlx::query_as::<_, (String, String, String, i64)>(
            "WITH eligible_papers AS MATERIALIZED (\
             SELECT p.id FROM papers p \
             WHERE p.active=1 AND p.subject='xingce' AND COALESCE(p.kind,'exam')='exam' AND json_extract(p.metadata,'$.summary.role')='primary' \
             AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)>=?) \
             AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)<=?) \
             AND (?=0 OR (EXISTS (SELECT 1 FROM json_each(?) \
                  WHERE value=json_extract(p.metadata,'$.summary.region'))) != (?)) \
             LIMIT -1) \
             SELECT date(a.submitted_at/1000.0,'unixepoch','+8 hours') AS day, \
                    q.module, COALESCE(q.category,'') AS category, COUNT(*) AS answered \
             FROM eligible_papers p \
             JOIN questions q ON q.paper_id=p.id \
             JOIN practice_attempts a ON a.question_id=q.id \
             WHERE q.gradable=1 AND q.module IS NOT NULL \
               AND a.submitted_at>=? AND a.submitted_at<? \
               AND json_type(a.answer)='array' \
               AND EXISTS (SELECT 1 FROM json_each(a.answer) choice \
                 WHERE choice.type='text' \
                   AND length(trim(choice.value, char(9)||char(10)||char(13)||' '))>0) \
             GROUP BY day,q.module,category ORDER BY day",
        )
        .bind(filters.year_min)
        .bind(filters.year_min)
        .bind(filters.year_max)
        .bind(filters.year_max)
        .bind(i64::from(!filters.regions.is_empty()))
        .bind(regions)
        .bind(i64::from(filters.exclude_regions))
        .bind(start_ms)
        .bind(end_ms)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| BankError::Storage)?;
        Ok(rows
            .into_iter()
            .map(|(day, module, category, answered)| StudyAnswerDay {
                day,
                module,
                category,
                answered,
            })
            .collect())
    }

    async fn practice_module_groups(
        &self,
        module: &str,
        category: Option<&str>,
        filters: &ModulePracticeFilters,
        group_limit: usize,
    ) -> Result<Vec<Vec<String>>, BankError> {
        if group_limit == 0 || group_limit > 100 {
            return Err(BankError::Invalid);
        }
        let regions = serde_json::to_string(&filters.regions).map_err(|_| BankError::Storage)?;
        // LIMIT -1 keeps the paper metadata filter materialized on older SQLite.
        let papers = "WITH eligible_papers AS MATERIALIZED (\
            SELECT p.id,CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER) AS year \
            FROM papers p \
            WHERE p.active=1 AND p.subject='xingce' AND COALESCE(p.kind,'exam')='exam' AND json_extract(p.metadata,'$.summary.role')='primary' \
            AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)>=?) \
            AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)<=?) \
            AND (?=0 OR (EXISTS (SELECT 1 FROM json_each(?) \
                 WHERE value=json_extract(p.metadata,'$.summary.region'))) != (?)) \
            LIMIT -1) ";
        // A material-linked group is atomic. Passage-based 言语 categories
        // outside 篇章阅读 are single questions even when the source attaches
        // a section-level material to them.
        let sql = format!(
            "{papers}, filtered AS (\
             SELECT q.id,q.paper_id,q.number,q.content_fingerprint,\
             COALESCE('fp:'||q.content_fingerprint,'id:'||q.id) AS identity,\
             (? IS NULL OR q.category=?) AS matches,\
             CASE \
               WHEN q.materials IS NULL OR q.materials IN ('[]','') THEN 'solo:'||q.id \
               WHEN q.module='言语理解与表达' AND COALESCE(q.category,'')<>'篇章阅读' \
                 THEN 'solo:'||q.id \
               ELSE 'group:'||q.paper_id||':'||q.materials \
             END AS group_key \
             FROM eligible_papers p JOIN questions q ON q.paper_id=p.id \
             WHERE q.module=? AND q.gradable=1),\
             attempt_history AS MATERIALIZED (\
               SELECT COALESCE('fp:'||q.content_fingerprint,'id:'||q.id) AS identity,\
                      MAX(a.answer NOT IN ('[]','')) AS answered,\
                      SUM(a.grade='wrong') AS wrong_count,\
                      SUM(a.grade='correct') AS correct_count \
               FROM practice_attempts a JOIN questions q ON q.id=a.question_id \
               GROUP BY identity),\
             served_history AS MATERIALIZED (\
               SELECT COALESCE('fp:'||q.content_fingerprint,'id:'||q.id) AS identity,\
                      COUNT(*) AS served \
               FROM practice_items i JOIN questions q ON q.id=i.question_id \
               GROUP BY identity),\
             blocks AS (\
               SELECT group_key,paper_id,MIN(number) AS first_number,\
                      MIN(COALESCE(attempt_history.answered,0)) AS answered,MIN(COALESCE(served_history.served,0)) AS served,MAX(matches) AS matched,\
                      SUM(MAX(COALESCE(wrong_count,0)-COALESCE(correct_count,0),0)) AS net_errors \
               FROM filtered \
               LEFT JOIN attempt_history ON attempt_history.identity=filtered.identity \
               LEFT JOIN served_history ON served_history.identity=filtered.identity \
               GROUP BY group_key,paper_id),\
             eligible_blocks AS (SELECT * FROM blocks WHERE matched=1),\
             ranked AS (\
               SELECT group_key,ROW_NUMBER() OVER (\
                 ORDER BY answered,net_errors DESC,served,position,RANDOM(),group_key) AS rn \
               FROM (SELECT group_key,answered,net_errors,served,\
                     ROW_NUMBER() OVER (PARTITION BY paper_id ORDER BY first_number) AS position \
                     FROM eligible_blocks)),\
             selected AS (SELECT group_key,rn FROM ranked) \
             SELECT selected.rn,filtered.id,filtered.identity FROM selected \
             JOIN filtered ON filtered.group_key=selected.group_key \
             ORDER BY selected.rn,filtered.number"
        );
        let mut rows = sqlx::query_as::<_, (i64, String, String)>(&sql)
            .bind(filters.year_min)
            .bind(filters.year_min)
            .bind(filters.year_max)
            .bind(filters.year_max)
            .bind(i64::from(!filters.regions.is_empty()))
            .bind(regions)
            .bind(i64::from(filters.exclude_regions))
            .bind(category)
            .bind(category)
            .bind(module)
            .fetch(&self.pool);
        let mut groups = Vec::new();
        let mut group = Vec::new();
        let mut current = i64::MIN;
        let mut seen = std::collections::HashSet::new();
        while let Some((rank, id, identity)) =
            rows.try_next().await.map_err(|_| BankError::Storage)?
        {
            if rank != current {
                if !group.is_empty() {
                    groups.push(std::mem::take(&mut group));
                    if groups.len() == group_limit {
                        return Ok(groups);
                    }
                }
                current = rank;
            }
            if seen.insert(identity) {
                group.push(id);
            }
        }
        if !group.is_empty() && groups.len() < group_limit {
            groups.push(group);
        }
        Ok(groups)
    }

    async fn practice_browse(
        &self,
        mode: PracticeMode,
        paper_id: Option<&str>,
        module: Option<&str>,
        category: Option<&str>,
        filters: &ModulePracticeFilters,
    ) -> Result<Vec<PracticeBrowseItem>, BankError> {
        let (source, condition) = match mode {
            PracticeMode::Paper | PracticeMode::Module => ("questions q", "1=1"),
            PracticeMode::Wrong => (
                "question_progress progress JOIN questions q ON q.id=progress.question_id",
                "progress.pending=1",
            ),
            PracticeMode::Favorite => (
                "favorites saved JOIN questions q ON q.id=saved.question_id",
                "saved.favorite=1",
            ),
            PracticeMode::PaperModule | PracticeMode::Drill => return Err(BankError::Invalid),
        };
        let kind_scope = match mode {
            PracticeMode::Paper | PracticeMode::Module => "COALESCE(p.kind,'exam')='exam'",
            _ => "COALESCE(p.kind,'exam') IN ('exam','drill')",
        };
        let gradable_scope = match mode {
            PracticeMode::Paper | PracticeMode::Module => "q.gradable=1",
            _ => "1=1",
        };
        let regions = serde_json::to_string(&filters.regions).map_err(|_| BankError::Storage)?;
        let sql = format!(
            "SELECT q.id,q.paper_id,json_extract(p.metadata,'$.summary.title'),\
             CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER),\
             json_extract(p.metadata,'$.summary.region'),q.number,q.module,q.category \
             FROM {source} JOIN papers p ON p.id=q.paper_id \
             WHERE p.active=1 AND p.subject='xingce' AND {kind_scope} AND json_extract(p.metadata,'$.summary.role')='primary' \
             AND {gradable_scope} \
             AND {condition} \
             AND (? IS NULL OR q.paper_id=?) \
             AND (? IS NULL OR q.module=?) \
             AND (? IS NULL OR q.category=?) \
             AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)>=?) \
             AND (? IS NULL OR CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER)<=?) \
             AND (?=0 OR (EXISTS (SELECT 1 FROM json_each(?) \
                  WHERE value=json_extract(p.metadata,'$.summary.region'))) != (?)) \
             ORDER BY CAST(json_extract(p.metadata,'$.summary.year') AS INTEGER) DESC,p.id,q.number"
        );
        let rows = sqlx::query(&sql)
            .bind(paper_id)
            .bind(paper_id)
            .bind(module)
            .bind(module)
            .bind(category)
            .bind(category)
            .bind(filters.year_min)
            .bind(filters.year_min)
            .bind(filters.year_max)
            .bind(filters.year_max)
            .bind(i64::from(!filters.regions.is_empty()))
            .bind(regions)
            .bind(i64::from(filters.exclude_regions))
            .fetch_all(&self.pool)
            .await
            .map_err(|_| BankError::Storage)?;
        Ok(rows
            .into_iter()
            .map(|row| PracticeBrowseItem {
                question_id: row.get(0),
                paper_id: row.get(1),
                paper_title: row.get(2),
                year: row.get::<i64, _>(3) as i32,
                region: row.get(4),
                number: row.get::<i64, _>(5) as u32,
                module: row.get(6),
                category: row.get(7),
            })
            .collect())
    }

    async fn practice_candidates(&self, mode: PracticeMode) -> Result<Vec<String>, BankError> {
        let sql = match mode {
            PracticeMode::Wrong => "SELECT q.id FROM questions q JOIN papers p ON p.id=q.paper_id JOIN question_progress s ON s.question_id=q.id WHERE p.active=1 AND p.subject='xingce' AND COALESCE(p.kind,'exam') IN ('exam','drill') AND json_extract(p.metadata,'$.summary.role')='primary' AND q.gradable=1 AND s.pending=1 ORDER BY p.id,q.number",
            PracticeMode::Favorite => "SELECT q.id FROM questions q JOIN papers p ON p.id=q.paper_id JOIN favorites s ON s.question_id=q.id WHERE p.active=1 AND p.subject='xingce' AND COALESCE(p.kind,'exam') IN ('exam','drill') AND json_extract(p.metadata,'$.summary.role')='primary' AND s.favorite=1 ORDER BY p.id,q.number",
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

fn decode_question(row: &sqlx::sqlite::SqliteRow) -> Result<StoredQuestion, BankError> {
    fn text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<String, BankError> {
        row.try_get(column).map_err(|_| BankError::Storage)
    }
    fn values(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<Vec<String>, BankError> {
        serde_json::from_str(&text(row, column)?).map_err(|_| BankError::Storage)
    }
    Ok(StoredQuestion {
        id: text(row, "id")?,
        number: u32::try_from(
            row.try_get::<i64, _>("number")
                .map_err(|_| BankError::Storage)?,
        )
        .map_err(|_| BankError::Storage)?,
        module: row.try_get("module").map_err(|_| BankError::Storage)?,
        category: row.try_get("category").map_err(|_| BankError::Storage)?,
        topics: values(row, "topics")?,
        materials: values(row, "materials")?,
        stem: text(row, "stem")?,
        options_markdown: text(row, "options_markdown")?,
        answer_type: text(row, "answer_type")?,
        question_type: row
            .try_get("question_type")
            .map_err(|_| BankError::Storage)?,
        grading_mode: text(row, "grading_mode")?,
        max_score: row.try_get("max_score").map_err(|_| BankError::Storage)?,
        gradable: row
            .try_get::<i64, _>("gradable")
            .map_err(|_| BankError::Storage)?
            != 0,
        correct_answer: values(row, "correct_answer")?,
        explanation: text(row, "explanation")?,
        reference_answer: row
            .try_get::<Option<String>, _>("reference_answer")
            .map_err(|_| BankError::Storage)?
            .unwrap_or_default(),
    })
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
    async fn resync_repairs_missing_materials_even_when_the_source_hash_is_unchanged() {
        let repository = SqliteQuestionBankRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        let markdown = r#"---
schema: hyz-paper/v1
title: 缓存修复样卷
year: 2001
exam: 国考
region: 国考
variant: 统考
status: unverified
---
<!-- hyz-paper-id: 019a3d8c-7f23-7b6e-927d-3c951851d3a2 -->
## Material M1
<!-- hyz-material-id: 019a3d8d-4a21-739a-8b15-7c54e259b610 -->
共享材料。
## Question 1
<!-- hyz-question-id: 019a3d8e-1b32-7a60-9c83-f34217a6e210 -->
```yaml
materials: [M1]
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
        let paper = IndexedPaper::new(
            "papers/a.md".into(),
            "hash".into(),
            crate::application::question_bank::document::parse(markdown).unwrap(),
        );
        let id = paper.summary.id.clone();
        let mut incomplete = paper.clone();
        incomplete.content.materials.clear();
        repository.apply(vec![incomplete]).await.unwrap();
        assert_eq!(
            repository.paper_content(&id).await,
            Err(BankError::Unavailable)
        );
        repository.apply(vec![paper]).await.unwrap();
        assert_eq!(
            repository.paper_content(&id).await.unwrap().materials[0].markdown,
            "共享材料。"
        );
    }

    #[tokio::test]
    async fn stale_gradable_flags_are_repaired_from_stored_question_content() {
        let pool = memory_pool().await;
        initialize_schema(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers(id,metadata,active) VALUES('paper-1','{}',1)")
            .execute(&pool)
            .await
            .unwrap();
        // An older binary marked complete order-unverified questions as
        // ungradable. Repair must derive the flag from stem/options/answer.
        for (id, stem, options, answer, issues) in [
            (
                "q-ok",
                "题干",
                "- A. 甲\n- B. 乙",
                r#"["B"]"#,
                r#"["order_unverified"]"#,
            ),
            ("q-no-answer", "题干", "- A. 甲\n- B. 乙", "[]", "[]"),
            ("q-bad-label", "题干", "- A. 甲\n- B. 乙", r#"["C"]"#, "[]"),
            ("q-no-stem", "", "- A. 甲\n- B. 乙", r#"["A"]"#, "[]"),
        ] {
            sqlx::query("INSERT INTO questions(id,paper_id,number,source_hash,stem,options_markdown,correct_answer,issues,gradable) VALUES(?, 'paper-1', 1, 'hash', ?, ?, ?, ?, 0)")
                .bind(id)
                .bind(stem)
                .bind(options)
                .bind(answer)
                .bind(issues)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("PRAGMA user_version=0")
            .execute(&pool)
            .await
            .unwrap();

        initialize_schema(&pool).await.unwrap();
        initialize_schema(&pool).await.unwrap();

        let flags: Vec<(String, i64)> =
            sqlx::query_as("SELECT id,gradable FROM questions ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            flags,
            vec![
                ("q-bad-label".into(), 0),
                ("q-no-answer".into(), 0),
                ("q-no-stem".into(), 0),
                ("q-ok".into(), 1),
            ]
        );
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(version, 1);
    }

    #[tokio::test]
    async fn catalog_projection_uses_a_covering_index() {
        let pool = memory_pool().await;
        initialize_schema(&pool).await.unwrap();
        let plan = sqlx::query("EXPLAIN QUERY PLAN SELECT q.module,q.category,q.id,q.number FROM questions q WHERE q.paper_id=?")
            .bind("paper-1").fetch_all(&pool).await.unwrap();
        assert!(
            plan.iter().any(|row| row
                .get::<String, _>("detail")
                .contains("USING COVERING INDEX")),
            "catalog reads must not fetch large question content rows: {:?}",
            plan.iter()
                .map(|row| row.get::<String, _>("detail"))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn fresh_question_bank_has_all_tables() {
        let pool = memory_pool().await;
        initialize_schema(&pool).await.unwrap();
        for table in [
            "papers",
            "identities",
            "questions",
            "paper_materials",
            "practice_sessions",
            "practice_items",
            "practice_attempts",
            "question_progress",
            "favorites",
            "question_ai_reviews",
            "question_ai_messages",
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

        initialize_schema(&pool).await.unwrap();
        sqlx::query("INSERT INTO favorites(question_id,favorite) VALUES('question-1',1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO question_progress(question_id,pending,updated_at) VALUES('question-1',1,123)")
            .execute(&pool)
            .await
            .unwrap();

        // Reopening the same database is safe and never rebuilds user state.
        initialize_schema(&pool).await.unwrap();
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
        assert_eq!(
            applied,
            QUESTION_BANK_MIGRATOR.iter().count() as i64,
            "each version is applied exactly once"
        );
    }
}
