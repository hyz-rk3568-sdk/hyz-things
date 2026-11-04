use async_trait::async_trait;

use crate::{application::flashcards::FlashcardError, domain::flashcards::FlashcardId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewFavoriteCard {
    pub question_id: String,
    pub target_deck: String,
    pub front: String,
    pub back: String,
    pub correct_answer: Vec<String>,
    pub assets: Vec<FavoriteCardAsset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteCardAsset {
    /// SHA-256 content digest followed by an allowlisted file extension.
    pub file_name: String,
    pub contents: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteWriteResult {
    pub relative_path: String,
    pub card_id: Option<FlashcardId>,
    pub appended: bool,
}

#[async_trait]
pub trait FavoriteFlashcardWriterPort: Send + Sync {
    async fn existing_user_targets(&self, module: &str) -> Result<Vec<String>, FlashcardError>;

    /// Checks the complete source for question identity before changing any files.
    async fn append(&self, card: NewFavoriteCard) -> Result<FavoriteWriteResult, FlashcardError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteMountIdentity {
    pub mount_id: String,
    pub source: String,
}

pub trait FavoriteMountObservationPort: Send + Sync {
    fn observe(&self, root: &std::path::Path) -> Result<FavoriteMountIdentity, FlashcardError>;
}
