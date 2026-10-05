use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use crate::{
    application::flashcards::{FlashcardError, FlashcardRepositoryPort},
    domain::flashcards::{
        DeckSummary, FlashcardFilter, FlashcardId, FlashcardSummary, ReviewLog, ReviewRating,
        ReviewState, ReviewStateKind, ReviewTarget, SourceSnapshot, StudySummary, SyncSummary,
        TagSummary,
    },
};

#[derive(Clone)]
pub struct SqliteFlashcardRepository {
    pool: SqlitePool,
}

impl SqliteFlashcardRepository {
    pub async fn connect(path: impl Into<PathBuf>) -> Result<Self, FlashcardError> {
        let path = path.into();
        let parent = path.parent().ok_or_else(|| {
            FlashcardError::StorageUnavailable(
                "flashcard database has no parent directory".to_owned(),
            )
        })?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| FlashcardError::StorageUnavailable(error.to_string()))?;
        set_private_directory(parent).map_err(|error| {
            FlashcardError::StorageUnavailable(format!("{}: {error}", parent.display()))
        })?;

        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|error| FlashcardError::StorageUnavailable(error.to_string()))?;
        set_private_file(&path).map_err(|error| {
            FlashcardError::StorageUnavailable(format!("{}: {error}", path.display()))
        })?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|error| FlashcardError::StorageUnavailable(error.to_string()))?;
        Ok(Self { pool })
    }

    pub async fn from_pool(pool: SqlitePool) -> Result<Self, FlashcardError> {
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .map_err(storage_error)?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|error| FlashcardError::StorageUnavailable(error.to_string()))?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl FlashcardRepositoryPort for SqliteFlashcardRepository {
    async fn apply_sync(
        &self,
        snapshot: SourceSnapshot,
        now_ms: u64,
    ) -> Result<SyncSummary, FlashcardError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let mut summary = SyncSummary::default();
        let now = to_i64(now_ms);
        let mut seen_by_file = BTreeMap::<String, BTreeSet<String>>::new();

        for card in &snapshot.cards {
            validate_source_file(&card.source_file)?;
            let existing = sqlx::query(
                "SELECT source_hash, source_file, source_order, deck_path, active, created_at_ms\n                 FROM flashcards WHERE id = ?",
            )
            .bind(card.id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
            let existing_changed = existing.as_ref().is_some_and(|row| {
                row.try_get::<String, _>("source_hash").ok().as_deref()
                    != Some(card.source_hash.as_str())
                    || row.try_get::<String, _>("source_file").ok().as_deref()
                        != Some(card.source_file.as_str())
                    || row.try_get::<i64, _>("source_order").ok()
                        != Some(i64::from(card.source_order))
                    || row.try_get::<String, _>("deck_path").ok().as_deref()
                        != Some(card.deck_path.as_str())
            });
            let was_inactive = existing
                .as_ref()
                .and_then(|row| row.try_get::<i64, _>("active").ok())
                == Some(0);
            let created_at = existing
                .as_ref()
                .and_then(|row| row.try_get::<i64, _>("created_at_ms").ok())
                .unwrap_or(now);

            sqlx::query(
                "INSERT INTO flashcards\n                 (id, source_file, source_order, deck_path, front_markdown, back_markdown, source_hash, active, created_at_ms, updated_at_ms)\n                 VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)\n                 ON CONFLICT(id) DO UPDATE SET\n                   source_file = excluded.source_file,\n                   source_order = excluded.source_order,\n                   deck_path = excluded.deck_path,\n                   front_markdown = excluded.front_markdown,\n                   back_markdown = excluded.back_markdown,\n                   source_hash = excluded.source_hash,\n                   active = 1,\n                   updated_at_ms = excluded.updated_at_ms",
            )
            .bind(card.id.as_str())
            .bind(&card.source_file)
            .bind(i64::from(card.source_order))
            .bind(&card.deck_path)
            .bind(&card.front_markdown)
            .bind(&card.back_markdown)
            .bind(&card.source_hash)
            .bind(created_at)
            .bind(if existing.is_some() && !existing_changed && !was_inactive {
                existing
                    .as_ref()
                    .and_then(|row| row.try_get::<i64, _>("created_at_ms").ok())
                    .unwrap_or(now)
            } else {
                now
            })
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;

            sqlx::query("DELETE FROM flashcard_card_tags WHERE card_id = ?")
                .bind(card.id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(storage_error)?;
            for tag in &card.tags {
                sqlx::query("INSERT INTO flashcard_card_tags (card_id, tag) VALUES (?, ?)")
                    .bind(card.id.as_str())
                    .bind(tag)
                    .execute(&mut *transaction)
                    .await
                    .map_err(storage_error)?;
            }
            seen_by_file
                .entry(card.source_file.clone())
                .or_default()
                .insert(card.id.to_string());

            if existing.is_none() {
                summary.inserted = summary.inserted.saturating_add(1);
            } else if was_inactive {
                summary.reactivated = summary.reactivated.saturating_add(1);
            } else if existing_changed {
                summary.updated = summary.updated.saturating_add(1);
            } else {
                summary.unchanged = summary.unchanged.saturating_add(1);
            }
        }

        let rows = sqlx::query("SELECT id, source_file FROM flashcards WHERE active = 1")
            .fetch_all(&mut *transaction)
            .await
            .map_err(storage_error)?;
        for row in rows {
            let id = row.try_get::<String, _>("id").map_err(storage_error)?;
            let source_file = row
                .try_get::<String, _>("source_file")
                .map_err(storage_error)?;
            let missing_from_successful_file =
                snapshot.successful_markdown_files.contains(&source_file)
                    && !seen_by_file
                        .get(&source_file)
                        .is_some_and(|ids| ids.contains(&id));
            let missing_deleted_file = snapshot.complete
                && !snapshot.scanned_markdown_files.contains(&source_file)
                && !snapshot.failed_markdown_files.contains(&source_file);
            if missing_from_successful_file || missing_deleted_file {
                sqlx::query("UPDATE flashcards SET active = 0, updated_at_ms = ? WHERE id = ?")
                    .bind(now)
                    .bind(id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(storage_error)?;
                summary.deactivated = summary.deactivated.saturating_add(1);
            }
        }

        transaction.commit().await.map_err(storage_error)?;
        Ok(summary)
    }

    async fn list_cards(
        &self,
        filter: FlashcardFilter,
        _now_ms: u64,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
        let mut cards = self.load_cards(false, 0).await?;
        cards.retain(|card| filter.matches(card));
        Ok(paginate(cards, filter.offset, filter.limit))
    }

    async fn list_review_queue(
        &self,
        filter: FlashcardFilter,
        now_ms: u64,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
        let mut cards = self.load_cards(true, now_ms).await?;
        cards.retain(|card| filter.matches(card));
        Ok(paginate(cards, filter.offset, filter.limit))
    }

    async fn list_decks(&self) -> Result<Vec<DeckSummary>, FlashcardError> {
        let rows = sqlx::query(
            "SELECT deck_path, COUNT(*) AS card_count\n             FROM flashcards WHERE active = 1 GROUP BY deck_path ORDER BY deck_path",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(DeckSummary {
                    path: row.try_get("deck_path").map_err(storage_error)?,
                    card_count: row.try_get::<i64, _>("card_count").map_err(storage_error)? as u32,
                })
            })
            .collect()
    }

    async fn list_tags(&self) -> Result<Vec<TagSummary>, FlashcardError> {
        let rows = sqlx::query(
            "SELECT t.tag, COUNT(*) AS card_count\n             FROM flashcard_card_tags t\n             JOIN flashcards f ON f.id = t.card_id\n             WHERE f.active = 1\n             GROUP BY t.tag ORDER BY t.tag",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(TagSummary {
                    tag: row.try_get("tag").map_err(storage_error)?,
                    card_count: row.try_get::<i64, _>("card_count").map_err(storage_error)? as u32,
                })
            })
            .collect()
    }

    async fn summary(&self, now_ms: u64) -> Result<StudySummary, FlashcardError> {
        let row = sqlx::query(
            "SELECT\n               COUNT(*) AS active_cards,\n               SUM(CASE WHEN s.card_id IS NULL THEN 1 ELSE 0 END) AS new_cards,\n               SUM(CASE WHEN s.card_id IS NULL OR s.due_at_ms <= ? THEN 1 ELSE 0 END) AS due_cards,\n               COUNT(DISTINCT f.deck_path) AS deck_count\n             FROM flashcards f\n             LEFT JOIN flashcard_review_state s ON s.card_id = f.id\n             WHERE f.active = 1",
        )
        .bind(to_i64(now_ms))
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        let tag_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(DISTINCT t.tag)\n             FROM flashcard_card_tags t\n             JOIN flashcards f ON f.id = t.card_id\n             WHERE f.active = 1",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(storage_error)?;
        Ok(StudySummary {
            active_cards: row
                .try_get::<i64, _>("active_cards")
                .map_err(storage_error)? as u32,
            new_cards: row
                .try_get::<Option<i64>, _>("new_cards")
                .map_err(storage_error)?
                .unwrap_or(0) as u32,
            due_cards: row
                .try_get::<Option<i64>, _>("due_cards")
                .map_err(storage_error)?
                .unwrap_or(0) as u32,
            deck_count: row.try_get::<i64, _>("deck_count").map_err(storage_error)? as u32,
            tag_count: tag_count as u32,
        })
    }

    async fn review_target(
        &self,
        card_id: &FlashcardId,
        _now_ms: u64,
    ) -> Result<Option<ReviewTarget>, FlashcardError> {
        let Some(card) = self.load_card(card_id).await? else {
            return Ok(None);
        };
        let state = sqlx::query(
            "SELECT due_at_ms, last_reviewed_at_ms, stability, difficulty, state, review_count, lapse_count\n             FROM flashcard_review_state WHERE card_id = ?",
        )
        .bind(card_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?
        .map(state_from_row)
        .transpose()?;
        Ok(Some(ReviewTarget { card, state }))
    }

    async fn record_review(
        &self,
        card_id: &FlashcardId,
        state: ReviewState,
        mut log: ReviewLog,
    ) -> Result<(FlashcardSummary, ReviewLog), FlashcardError> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        let active = sqlx::query_scalar::<_, i64>("SELECT active FROM flashcards WHERE id = ?")
            .bind(card_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
        if active != Some(1) {
            return Err(FlashcardError::NotFound);
        }
        sqlx::query(
            "INSERT INTO flashcard_review_state\n             (card_id, due_at_ms, last_reviewed_at_ms, stability, difficulty, state, review_count, lapse_count)\n             VALUES (?, ?, ?, ?, ?, ?, ?, ?)\n             ON CONFLICT(card_id) DO UPDATE SET\n               due_at_ms = excluded.due_at_ms,\n               last_reviewed_at_ms = excluded.last_reviewed_at_ms,\n               stability = excluded.stability,\n               difficulty = excluded.difficulty,\n               state = excluded.state,\n               review_count = excluded.review_count,\n               lapse_count = excluded.lapse_count",
        )
        .bind(card_id.as_str())
        .bind(to_i64(state.due_at_ms))
        .bind(state.last_reviewed_at_ms.map(to_i64))
        .bind(state.stability)
        .bind(state.difficulty)
        .bind(state_value(state.state))
        .bind(i64::from(state.review_count))
        .bind(i64::from(state.lapse_count))
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        sqlx::query(
            "INSERT INTO flashcard_review_logs\n             (card_id, reviewed_at_ms, rating, state, due_at_ms, stability, difficulty)\n             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(card_id.as_str())
        .bind(to_i64(log.reviewed_at_ms))
        .bind(rating_value(log.rating))
        .bind(state_value(log.state))
        .bind(to_i64(log.due_at_ms))
        .bind(log.stability)
        .bind(log.difficulty)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
        log.id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
            .fetch_one(&mut *transaction)
            .await
            .map_err(storage_error)?;
        transaction.commit().await.map_err(storage_error)?;
        let card = self
            .load_card(card_id)
            .await?
            .ok_or(FlashcardError::NotFound)?;
        Ok((card, log))
    }
}

impl SqliteFlashcardRepository {
    async fn load_cards(
        &self,
        due_only: bool,
        now_ms: u64,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
        let rows = sqlx::query(
            "SELECT f.id, f.source_file, f.source_order, f.deck_path, f.front_markdown, f.back_markdown,\n                    f.active, s.due_at_ms, s.last_reviewed_at_ms, s.state, s.review_count, s.lapse_count\n             FROM flashcards f\n             LEFT JOIN flashcard_review_state s ON s.card_id = f.id\n             WHERE f.active = 1\n             ORDER BY f.source_file, f.source_order, f.id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        let mut cards = Vec::with_capacity(rows.len());
        for row in rows {
            let card = self.card_from_row(&row).await?;
            if due_only && !card.is_new && card.due_at_ms.is_none_or(|due| due > now_ms) {
                continue;
            }
            cards.push(card);
        }
        Ok(cards)
    }

    async fn load_card(
        &self,
        card_id: &FlashcardId,
    ) -> Result<Option<FlashcardSummary>, FlashcardError> {
        let row = sqlx::query(
            "SELECT f.id, f.source_file, f.source_order, f.deck_path, f.front_markdown, f.back_markdown,\n                    f.active, s.due_at_ms, s.last_reviewed_at_ms, s.state, s.review_count, s.lapse_count\n             FROM flashcards f\n             LEFT JOIN flashcard_review_state s ON s.card_id = f.id\n             WHERE f.id = ?",
        )
        .bind(card_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error)?;
        match row {
            Some(row) => Ok(Some(self.card_from_row(&row).await?)),
            None => Ok(None),
        }
    }

    async fn card_from_row(
        &self,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<FlashcardSummary, FlashcardError> {
        let id = FlashcardId::new(row.try_get::<String, _>("id").map_err(storage_error)?)
            .map_err(|error| FlashcardError::Storage(error.to_string()))?;
        let tags = sqlx::query_scalar::<_, String>(
            "SELECT tag FROM flashcard_card_tags WHERE card_id = ? ORDER BY tag",
        )
        .bind(id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(storage_error)?;
        let state = row
            .try_get::<Option<i64>, _>("state")
            .map_err(storage_error)?
            .map(state_kind)
            .unwrap_or(ReviewStateKind::New);
        Ok(FlashcardSummary {
            id,
            source_file: validate_db_source_file(
                row.try_get::<String, _>("source_file")
                    .map_err(storage_error)?,
            )?,
            source_order: row
                .try_get::<i64, _>("source_order")
                .map_err(storage_error)? as u32,
            deck_path: row.try_get("deck_path").map_err(storage_error)?,
            front_markdown: row.try_get("front_markdown").map_err(storage_error)?,
            back_markdown: row.try_get("back_markdown").map_err(storage_error)?,
            tags,
            active: row.try_get::<i64, _>("active").map_err(storage_error)? != 0,
            due_at_ms: row
                .try_get::<Option<i64>, _>("due_at_ms")
                .map_err(storage_error)?
                .map(from_i64),
            last_reviewed_at_ms: row
                .try_get::<Option<i64>, _>("last_reviewed_at_ms")
                .map_err(storage_error)?
                .map(from_i64),
            state,
            review_count: row
                .try_get::<Option<i64>, _>("review_count")
                .map_err(storage_error)?
                .unwrap_or(0) as u32,
            lapse_count: row
                .try_get::<Option<i64>, _>("lapse_count")
                .map_err(storage_error)?
                .unwrap_or(0) as u32,
            is_new: row
                .try_get::<Option<i64>, _>("state")
                .map_err(storage_error)?
                .is_none(),
        })
    }
}

fn paginate<T>(items: Vec<T>, offset: u32, limit: u32) -> Vec<T> {
    items
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect()
}

fn state_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ReviewState, FlashcardError> {
    Ok(ReviewState {
        due_at_ms: from_i64(row.try_get("due_at_ms").map_err(storage_error)?),
        last_reviewed_at_ms: row
            .try_get::<Option<i64>, _>("last_reviewed_at_ms")
            .map_err(storage_error)?
            .map(from_i64),
        stability: row.try_get("stability").map_err(storage_error)?,
        difficulty: row.try_get("difficulty").map_err(storage_error)?,
        state: state_kind(row.try_get("state").map_err(storage_error)?),
        review_count: row
            .try_get::<i64, _>("review_count")
            .map_err(storage_error)? as u32,
        lapse_count: row
            .try_get::<i64, _>("lapse_count")
            .map_err(storage_error)? as u32,
    })
}

fn state_value(state: ReviewStateKind) -> i64 {
    match state {
        ReviewStateKind::New => 0,
        ReviewStateKind::Learning => 1,
        ReviewStateKind::Review => 2,
        ReviewStateKind::Relearning => 3,
    }
}

fn state_kind(state: i64) -> ReviewStateKind {
    match state {
        1 => ReviewStateKind::Learning,
        2 => ReviewStateKind::Review,
        3 => ReviewStateKind::Relearning,
        _ => ReviewStateKind::New,
    }
}

fn rating_value(rating: ReviewRating) -> &'static str {
    match rating {
        ReviewRating::Again => "again",
        ReviewRating::Hard => "hard",
        ReviewRating::Good => "good",
    }
}

fn to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn from_i64(value: i64) -> u64 {
    value.max(0) as u64
}

fn storage_error(error: sqlx::Error) -> FlashcardError {
    FlashcardError::Storage(error.to_string())
}

fn validate_source_file(value: &str) -> Result<(), FlashcardError> {
    validate_db_source_file(value.to_owned()).map(|_| ())
}

fn validate_db_source_file(value: String) -> Result<String, FlashcardError> {
    let path = Path::new(&value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || value.contains('\\')
    {
        return Err(FlashcardError::Storage(
            "database contains an unsafe source path".to_owned(),
        ));
    }
    Ok(value)
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::flashcards::FlashcardRepositoryPort;
    use crate::domain::flashcards::{SourceCard, SourceSnapshot};
    use sqlx::sqlite::SqlitePoolOptions;

    fn clock_id() -> FlashcardId {
        FlashcardId::from_trusted("019a31b7-8c42-7c16-a6d1-0c1f0f34c821")
    }

    fn snapshot(front: &str) -> SourceSnapshot {
        SourceSnapshot {
            cards: vec![SourceCard {
                id: clock_id(),
                source_file: "deck/cards.md".to_owned(),
                source_order: 0,
                deck_path: "deck".to_owned(),
                front_markdown: front.to_owned(),
                back_markdown: "back".to_owned(),
                tags: vec!["tag".to_owned()],
                source_hash: format!("hash-{front}"),
                generated_id: false,
            }],
            scanned_markdown_files: ["deck/cards.md".to_owned()].into_iter().collect(),
            successful_markdown_files: ["deck/cards.md".to_owned()].into_iter().collect(),
            complete: true,
            ..SourceSnapshot::default()
        }
    }

    async fn repository() -> SqliteFlashcardRepository {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        SqliteFlashcardRepository::from_pool(pool).await.unwrap()
    }

    #[tokio::test]
    async fn sync_filter_and_foreign_key_behaviors_are_transactional() {
        let repository = repository().await;
        let first = repository
            .apply_sync(snapshot("front"), 1_800_000_000_000)
            .await
            .unwrap();
        assert_eq!(first.inserted, 1);
        assert_eq!(repository.list_decks().await.unwrap()[0].path, "deck");
        assert_eq!(repository.list_tags().await.unwrap()[0].tag, "tag");
        let filtered = repository
            .list_cards(
                FlashcardFilter {
                    deck: Some("deck".to_owned()),
                    tags: vec!["tag".to_owned()],
                    ..FlashcardFilter::default()
                },
                0,
            )
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        let second = repository
            .apply_sync(snapshot("changed"), 1_800_000_000_100)
            .await
            .unwrap();
        assert_eq!(second.updated, 1);
    }
}
