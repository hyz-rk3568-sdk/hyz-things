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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateFavoriteCard {
    pub question_id: String,
    pub card_id: FlashcardId,
    pub source_file: String,
    pub back: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleteFavoriteCard {
    pub question_id: String,
    pub card_id: FlashcardId,
    pub source_file: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteMutationResult {
    pub relative_path: String,
    pub card_id: FlashcardId,
    pub changed: bool,
}

#[async_trait]
pub trait FavoriteFlashcardWriterPort: Send + Sync {
    async fn existing_user_targets(&self, module: &str) -> Result<Vec<String>, FlashcardError>;

    /// Checks the complete source for question identity before changing any files.
    async fn append(&self, card: NewFavoriteCard) -> Result<FavoriteWriteResult, FlashcardError>;

    /// Replaces the Back section of one exact card block. Repeating the same
    /// back is a no-op so a retry after a failed sync cannot duplicate a card.
    async fn update_back(
        &self,
        card: UpdateFavoriteCard,
    ) -> Result<FavoriteMutationResult, FlashcardError>;

    /// Removes one exact card block. A missing block is treated as already
    /// deleted so the follow-up sync can still make progress.
    async fn delete(
        &self,
        card: DeleteFavoriteCard,
    ) -> Result<FavoriteMutationResult, FlashcardError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteMountIdentity {
    pub mount_id: String,
    pub source: String,
}

pub trait FavoriteMountObservationPort: Send + Sync {
    fn observe(&self, root: &std::path::Path) -> Result<FavoriteMountIdentity, FlashcardError>;
}
