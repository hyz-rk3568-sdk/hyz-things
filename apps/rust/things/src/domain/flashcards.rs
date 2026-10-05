use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FlashcardId(String);

impl FlashcardId {
    pub fn new(value: impl Into<String>) -> Result<Self, FlashcardDomainError> {
        let value = value.into();
        if !is_uuid_v7_text(&value) {
            return Err(FlashcardDomainError::InvalidCardId);
        }
        Ok(Self(value))
    }

    pub fn from_trusted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FlashcardId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlashcardDomainError {
    InvalidCardId,
    InvalidFilter,
    InvalidRating,
}

impl fmt::Display for FlashcardDomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCardId => "card id must be a UUIDv7",
            Self::InvalidFilter => "flashcard filter is invalid",
            Self::InvalidRating => "review rating is invalid",
        })
    }
}

impl std::error::Error for FlashcardDomainError {}

fn is_uuid_v7_text(value: &str) -> bool {
    value.len() == 36
        && value.as_bytes().get(14).is_some_and(|byte| *byte == b'7')
        && value
            .as_bytes()
            .get(19)
            .is_some_and(|byte| matches!(*byte, b'8' | b'9' | b'a' | b'b' | b'A' | b'B'))
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flashcard {
    pub id: FlashcardId,
    pub source_file: String,
    pub source_order: u32,
    pub deck_path: String,
    pub front_markdown: String,
    pub back_markdown: String,
    pub tags: Vec<String>,
    pub source_hash: String,
    pub active: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlashcardSummary {
    pub id: FlashcardId,
    pub source_file: String,
    pub source_order: u32,
    pub deck_path: String,
    pub front_markdown: String,
    pub back_markdown: String,
    pub tags: Vec<String>,
    pub active: bool,
    pub due_at_ms: Option<u64>,
    pub last_reviewed_at_ms: Option<u64>,
    pub state: ReviewStateKind,
    pub review_count: u32,
    pub lapse_count: u32,
    pub is_new: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStateKind {
    #[default]
    New,
    Learning,
    Review,
    Relearning,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewState {
    pub due_at_ms: u64,
    pub last_reviewed_at_ms: Option<u64>,
    pub stability: f64,
    pub difficulty: f64,
    pub state: ReviewStateKind,
    pub review_count: u32,
    pub lapse_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewLog {
    pub id: i64,
    pub card_id: FlashcardId,
    pub reviewed_at_ms: u64,
    pub rating: ReviewRating,
    pub state: ReviewStateKind,
    pub due_at_ms: u64,
    pub stability: f64,
    pub difficulty: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRating {
    Again,
    Hard,
    Good,
}

impl ReviewRating {
    pub const ALL: [Self; 3] = [Self::Again, Self::Hard, Self::Good];
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlashcardFilter {
    pub deck: Option<String>,
    pub tags: Vec<String>,
    pub limit: u32,
    pub offset: u32,
}

impl Default for FlashcardFilter {
    fn default() -> Self {
        Self {
            deck: None,
            tags: Vec::new(),
            limit: 50,
            offset: 0,
        }
    }
}

impl FlashcardFilter {
    pub fn normalized(mut self) -> Result<Self, FlashcardDomainError> {
        self.deck = normalize_optional_text(self.deck.take())?;
        let mut tags = BTreeSet::new();
        for tag in self.tags {
            let tag = normalize_text(&tag).ok_or(FlashcardDomainError::InvalidFilter)?;
            tags.insert(tag);
        }
        self.tags = tags.into_iter().collect();
        self.limit = self.limit.clamp(1, 200);
        Ok(self)
    }

    pub fn matches(&self, card: &FlashcardSummary) -> bool {
        let deck_matches = self.deck.as_deref().is_none_or(|deck| {
            card.deck_path == deck
                || card
                    .deck_path
                    .strip_prefix(deck)
                    .is_some_and(|rest| rest.starts_with('/'))
        });
        deck_matches
            && self
                .tags
                .iter()
                .all(|tag| card.tags.iter().any(|value| value == tag))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewOutcome {
    pub card: FlashcardSummary,
    pub state: ReviewState,
    pub log: ReviewLog,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewTarget {
    pub card: FlashcardSummary,
    pub state: Option<ReviewState>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSummary {
    pub inserted: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub reactivated: u32,
    pub deactivated: u32,
    pub generated_ids: u32,
    pub errors: Vec<SyncError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncError {
    pub source_file: String,
    pub line: Option<u32>,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StudySummary {
    pub active_cards: u32,
    pub new_cards: u32,
    pub due_cards: u32,
    pub deck_count: u32,
    pub tag_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckSummary {
    pub path: String,
    pub card_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagSummary {
    pub tag: String,
    pub card_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceCard {
    pub id: FlashcardId,
    pub source_file: String,
    pub source_order: u32,
    pub deck_path: String,
    pub front_markdown: String,
    pub back_markdown: String,
    pub tags: Vec<String>,
    pub source_hash: String,
    pub generated_id: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceSnapshot {
    pub cards: Vec<SourceCard>,
    pub scanned_markdown_files: BTreeSet<String>,
    pub successful_markdown_files: BTreeSet<String>,
    pub failed_markdown_files: BTreeSet<String>,
    pub errors: Vec<SyncError>,
    pub complete: bool,
}

fn normalize_optional_text(value: Option<String>) -> Result<Option<String>, FlashcardDomainError> {
    value
        .map(|value| normalize_text(&value).ok_or(FlashcardDomainError::InvalidFilter))
        .transpose()
}

fn normalize_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> FlashcardId {
        FlashcardId::new("019a31b7-8c42-7c16-a6d1-0c1f0f34c821").unwrap()
    }

    fn card(deck_path: &str, tags: &[&str]) -> FlashcardSummary {
        FlashcardSummary {
            id: id(),
            source_file: "deck/cards.md".to_owned(),
            source_order: 0,
            deck_path: deck_path.to_owned(),
            front_markdown: "front".to_owned(),
            back_markdown: "back".to_owned(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            active: true,
            due_at_ms: None,
            last_reviewed_at_ms: None,
            state: ReviewStateKind::New,
            review_count: 0,
            lapse_count: 0,
            is_new: true,
        }
    }

    #[test]
    fn filter_uses_deck_subtrees_and_tag_and_semantics() {
        let filter = FlashcardFilter {
            deck: Some("判断推理".to_owned()),
            tags: vec!["错题".to_owned(), "高频".to_owned()],
            limit: 50,
            offset: 0,
        }
        .normalized()
        .unwrap();
        assert!(filter.matches(&card("判断推理/逻辑判断", &["错题", "高频"])));
        assert!(!filter.matches(&card("判断推理/逻辑判断", &["错题"])));
        assert!(!filter.matches(&card("资料分析", &["错题", "高频"])));
    }

    #[test]
    fn card_ids_accept_only_uuidv7_text() {
        assert!(FlashcardId::new("019a31b7-8c42-7c16-a6d1-0c1f0f34c821").is_ok());
        assert!(FlashcardId::new("019a31b7-8c42-6c16-a6d1-0c1f0f34c821").is_err());
        assert!(FlashcardId::new("not-an-id").is_err());
    }
}
