use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use rs_fsrs::{Card as FsrsCard, Rating as FsrsRating, State as FsrsState, FSRS};
use std::{fmt, sync::Arc};
use tokio::sync::Semaphore;

use crate::{
    application::ports::ClockPort,
    domain::flashcards::{
        DeckSummary, FlashcardFilter, FlashcardId, FlashcardSummary, ReviewLog, ReviewOutcome,
        ReviewRating, ReviewState, ReviewStateKind, ReviewTarget, SourceSnapshot, StudySummary,
        SyncSummary, TagSummary,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlashcardError {
    Busy,
    InvalidRequest(String),
    NotFound,
    SourceUnavailable(String),
    StorageUnavailable(String),
    Storage(String),
}

impl FlashcardError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy => "study_sync_busy",
            Self::InvalidRequest(_) => "study_invalid_request",
            Self::NotFound => "study_card_not_found",
            Self::SourceUnavailable(_) => "study_source_unavailable",
            Self::StorageUnavailable(_) => "study_storage_unavailable",
            Self::Storage(_) => "study_storage_error",
        }
    }
}

impl fmt::Display for FlashcardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("a flashcard sync is already running"),
            Self::InvalidRequest(detail) => {
                write!(formatter, "invalid flashcard request: {detail}")
            }
            Self::NotFound => formatter.write_str("flashcard was not found"),
            Self::SourceUnavailable(detail) => {
                write!(formatter, "flashcard source unavailable: {detail}")
            }
            Self::StorageUnavailable(detail) => {
                write!(formatter, "flashcard storage unavailable: {detail}")
            }
            Self::Storage(detail) => write!(formatter, "flashcard storage failed: {detail}"),
        }
    }
}

impl std::error::Error for FlashcardError {}

#[async_trait]
pub trait FlashcardSourcePort: Send + Sync {
    async fn scan(&self) -> Result<SourceSnapshot, FlashcardError>;
}

#[async_trait]
pub trait FlashcardRepositoryPort: Send + Sync {
    async fn apply_sync(
        &self,
        snapshot: SourceSnapshot,
        now_ms: u64,
    ) -> Result<SyncSummary, FlashcardError>;

    async fn list_cards(
        &self,
        filter: FlashcardFilter,
        now_ms: u64,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError>;

    async fn list_review_queue(
        &self,
        filter: FlashcardFilter,
        now_ms: u64,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError>;

    async fn list_decks(&self) -> Result<Vec<DeckSummary>, FlashcardError>;

    async fn list_tags(&self) -> Result<Vec<TagSummary>, FlashcardError>;

    async fn summary(&self, now_ms: u64) -> Result<StudySummary, FlashcardError>;

    async fn review_target(
        &self,
        card_id: &FlashcardId,
        now_ms: u64,
    ) -> Result<Option<ReviewTarget>, FlashcardError>;

    async fn record_review(
        &self,
        card_id: &FlashcardId,
        state: ReviewState,
        log: ReviewLog,
    ) -> Result<(FlashcardSummary, ReviewLog), FlashcardError>;
}

pub struct FlashcardApplication {
    source: Arc<dyn FlashcardSourcePort>,
    repository: Arc<dyn FlashcardRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    sync_gate: Arc<Semaphore>,
}

impl FlashcardApplication {
    pub fn new(
        source: Arc<dyn FlashcardSourcePort>,
        repository: Arc<dyn FlashcardRepositoryPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            source,
            repository,
            clock,
            sync_gate: Arc::new(Semaphore::new(1)),
        }
    }

    pub async fn sync(&self) -> Result<SyncSummary, FlashcardError> {
        let _permit = self
            .sync_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| FlashcardError::Busy)?;
        let snapshot = self.source.scan().await?;
        let generated_ids = snapshot
            .cards
            .iter()
            .filter(|card| card.generated_id)
            .count() as u32;
        let mut summary = self
            .repository
            .apply_sync(snapshot.clone(), self.clock.unix_time_millis())
            .await?;
        summary.generated_ids = generated_ids;
        summary.errors = snapshot.errors;
        Ok(summary)
    }

    pub async fn cards(
        &self,
        filter: FlashcardFilter,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
        let filter = filter
            .normalized()
            .map_err(|error| FlashcardError::InvalidRequest(error.to_string()))?;
        self.repository
            .list_cards(filter, self.clock.unix_time_millis())
            .await
    }

    pub async fn review_queue(
        &self,
        filter: FlashcardFilter,
    ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
        let filter = filter
            .normalized()
            .map_err(|error| FlashcardError::InvalidRequest(error.to_string()))?;
        self.repository
            .list_review_queue(filter, self.clock.unix_time_millis())
            .await
    }

    pub async fn decks(&self) -> Result<Vec<DeckSummary>, FlashcardError> {
        self.repository.list_decks().await
    }

    pub async fn tags(&self) -> Result<Vec<TagSummary>, FlashcardError> {
        self.repository.list_tags().await
    }

    pub async fn summary(&self) -> Result<StudySummary, FlashcardError> {
        self.repository.summary(self.clock.unix_time_millis()).await
    }

    pub async fn review(
        &self,
        card_id: FlashcardId,
        rating: ReviewRating,
    ) -> Result<ReviewOutcome, FlashcardError> {
        let now_ms = self.clock.unix_time_millis();
        let target = self
            .repository
            .review_target(&card_id, now_ms)
            .await?
            .ok_or(FlashcardError::NotFound)?;
        if !target.card.active {
            return Err(FlashcardError::NotFound);
        }

        let fsrs_card = fsrs_card(target.state.as_ref(), now_ms);
        let fsrs_rating = match rating {
            ReviewRating::Again => FsrsRating::Again,
            ReviewRating::Hard => FsrsRating::Hard,
            ReviewRating::Good => FsrsRating::Good,
        };
        let reviewed = FSRS::default().next(fsrs_card, timestamp(now_ms), fsrs_rating);
        let next_state = ReviewState {
            due_at_ms: millis(reviewed.card.due),
            last_reviewed_at_ms: Some(now_ms),
            stability: reviewed.card.stability,
            difficulty: reviewed.card.difficulty,
            state: review_state(reviewed.card.state),
            review_count: target
                .state
                .as_ref()
                .map_or(1, |state| state.review_count.saturating_add(1)),
            lapse_count: reviewed.card.lapses.max(0) as u32,
        };
        let log = ReviewLog {
            id: 0,
            card_id: card_id.clone(),
            reviewed_at_ms: now_ms,
            rating,
            state: next_state.state,
            due_at_ms: next_state.due_at_ms,
            stability: next_state.stability,
            difficulty: next_state.difficulty,
        };
        let (card, log) = self
            .repository
            .record_review(&card_id, next_state.clone(), log)
            .await?;
        Ok(ReviewOutcome {
            card,
            state: next_state,
            log,
        })
    }
}

fn timestamp(unix_time_ms: u64) -> chrono::DateTime<Utc> {
    Utc.timestamp_millis_opt(unix_time_ms as i64)
        .single()
        .expect("clock timestamp must be representable")
}

fn millis(value: chrono::DateTime<Utc>) -> u64 {
    value.timestamp_millis().max(0) as u64
}

fn fsrs_card(state: Option<&ReviewState>, now_ms: u64) -> FsrsCard {
    let now = timestamp(now_ms);
    let Some(state) = state else {
        return FsrsCard {
            due: now,
            last_review: now,
            ..FsrsCard::default()
        };
    };
    FsrsCard {
        due: timestamp(state.due_at_ms),
        stability: state.stability,
        difficulty: state.difficulty,
        reps: state.review_count as i32,
        lapses: state.lapse_count as i32,
        state: fsrs_state(state.state),
        last_review: timestamp(state.last_reviewed_at_ms.unwrap_or(now_ms)),
        ..FsrsCard::default()
    }
}

fn fsrs_state(state: ReviewStateKind) -> FsrsState {
    match state {
        ReviewStateKind::New => FsrsState::New,
        ReviewStateKind::Learning => FsrsState::Learning,
        ReviewStateKind::Review => FsrsState::Review,
        ReviewStateKind::Relearning => FsrsState::Relearning,
    }
}

fn review_state(state: FsrsState) -> ReviewStateKind {
    match state {
        FsrsState::New => ReviewStateKind::New,
        FsrsState::Learning => ReviewStateKind::Learning,
        FsrsState::Review => ReviewStateKind::Review,
        FsrsState::Relearning => ReviewStateKind::Relearning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::flashcards::{SourceCard, SourceSnapshot, SyncSummary, TagSummary};
    use std::{collections::BTreeSet, sync::Mutex};

    struct FakeClock(u64);

    impl ClockPort for FakeClock {
        fn unix_time_millis(&self) -> u64 {
            self.0
        }
    }

    struct FakeSource {
        snapshot: Mutex<Option<SourceSnapshot>>,
    }

    #[async_trait]
    impl FlashcardSourcePort for FakeSource {
        async fn scan(&self) -> Result<SourceSnapshot, FlashcardError> {
            self.snapshot.lock().unwrap().take().ok_or_else(|| {
                FlashcardError::SourceUnavailable("missing fake snapshot".to_owned())
            })
        }
    }

    struct FakeRepository {
        applied: Mutex<Vec<SourceSnapshot>>,
    }

    #[async_trait]
    impl FlashcardRepositoryPort for FakeRepository {
        async fn apply_sync(
            &self,
            snapshot: SourceSnapshot,
            _now_ms: u64,
        ) -> Result<SyncSummary, FlashcardError> {
            self.applied.lock().unwrap().push(snapshot);
            Ok(SyncSummary::default())
        }

        async fn list_cards(
            &self,
            _filter: FlashcardFilter,
            _now_ms: u64,
        ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
            Ok(Vec::new())
        }

        async fn list_review_queue(
            &self,
            _filter: FlashcardFilter,
            _now_ms: u64,
        ) -> Result<Vec<FlashcardSummary>, FlashcardError> {
            Ok(Vec::new())
        }

        async fn list_decks(&self) -> Result<Vec<DeckSummary>, FlashcardError> {
            Ok(Vec::new())
        }

        async fn list_tags(&self) -> Result<Vec<TagSummary>, FlashcardError> {
            Ok(Vec::new())
        }

        async fn summary(&self, _now_ms: u64) -> Result<StudySummary, FlashcardError> {
            Ok(StudySummary::default())
        }

        async fn review_target(
            &self,
            _card_id: &FlashcardId,
            _now_ms: u64,
        ) -> Result<Option<ReviewTarget>, FlashcardError> {
            Ok(None)
        }

        async fn record_review(
            &self,
            _card_id: &FlashcardId,
            _state: ReviewState,
            _log: ReviewLog,
        ) -> Result<(FlashcardSummary, ReviewLog), FlashcardError> {
            Err(FlashcardError::NotFound)
        }
    }

    #[tokio::test]
    async fn sync_is_serialized_and_propagates_source_errors() {
        let id = FlashcardId::from_trusted("019a31b7-8c42-7c16-a6d1-0c1f0f34c821");
        let source = Arc::new(FakeSource {
            snapshot: Mutex::new(Some(SourceSnapshot {
                cards: vec![SourceCard {
                    id,
                    source_file: "deck/cards.md".to_owned(),
                    source_order: 0,
                    deck_path: "deck".to_owned(),
                    front_markdown: "front".to_owned(),
                    back_markdown: "back".to_owned(),
                    tags: Vec::new(),
                    source_hash: "hash".to_owned(),
                    generated_id: true,
                }],
                scanned_markdown_files: BTreeSet::new(),
                successful_markdown_files: BTreeSet::new(),
                failed_markdown_files: BTreeSet::new(),
                errors: Vec::new(),
                complete: true,
            })),
        });
        let repository = Arc::new(FakeRepository {
            applied: Mutex::new(Vec::new()),
        });
        let application = FlashcardApplication::new(
            source,
            repository.clone(),
            Arc::new(FakeClock(1_800_000_000_000)),
        );

        let summary = application.sync().await.unwrap();
        assert_eq!(summary.generated_ids, 1);
        assert_eq!(repository.applied.lock().unwrap().len(), 1);
    }
}
