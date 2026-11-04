use crate::{
    application::{
        favorite_flashcard_ports::FavoriteFlashcardWriterPort,
        flashcards::{FlashcardApplication, FlashcardError},
        question_bank::BankError,
    },
    domain::favorite_flashcard::{FavoriteEligibility, FlashcardCreationMode},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct FavoriteQuestion {
    pub question_id: String,
    pub module: Option<String>,
    pub category: Option<String>,
    pub front: String,
    pub correct_answer: Vec<String>,
    pub source_revision: String,
    pub gradable: bool,
}

#[async_trait]
pub trait FavoriteQuestionPort: Send + Sync {
    async fn load(&self, question_id: &str) -> Result<FavoriteQuestion, BankError>;
    async fn asset(&self, name: String) -> Result<Vec<u8>, BankError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FavoriteFlashcardPreview {
    #[serde(flatten)]
    pub eligibility: FavoriteEligibility,
    pub front: String,
    pub correct_answer: Vec<String>,
    pub source_revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFavoriteFlashcard {
    pub target_deck: String,
    /// The source root must match this mode. Old payloads can only be retried with valid new paths.
    #[serde(default)]
    pub creation_mode: Option<FlashcardCreationMode>,
    pub back: String,
    pub source_revision: String,
    #[serde(default)]
    pub type_confirmed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FavoriteFlashcardResult {
    pub status: String,
    pub target_deck: String,
    pub source_file: String,
    pub card_id: Option<String>,
    pub retryable: bool,
}

#[derive(Debug)]
pub enum FavoriteFlashcardError {
    Bank(BankError),
    Card(FlashcardError),
}
impl From<BankError> for FavoriteFlashcardError {
    fn from(error: BankError) -> Self {
        Self::Bank(error)
    }
}
impl From<FlashcardError> for FavoriteFlashcardError {
    fn from(error: FlashcardError) -> Self {
        Self::Card(error)
    }
}

#[derive(Clone)]
pub struct FavoriteFlashcardApplication {
    source: Arc<dyn FavoriteQuestionPort>,
    flashcards: Arc<FlashcardApplication>,
    writer: Arc<dyn FavoriteFlashcardWriterPort>,
}

impl FavoriteFlashcardApplication {
    pub fn new(
        source: Arc<dyn FavoriteQuestionPort>,
        flashcards: Arc<FlashcardApplication>,
        writer: Arc<dyn FavoriteFlashcardWriterPort>,
    ) -> Self {
        Self {
            source,
            flashcards,
            writer,
        }
    }

    pub async fn preview(
        &self,
        question_id: &str,
    ) -> Result<FavoriteFlashcardPreview, FavoriteFlashcardError> {
        self.available_preview(self.source.load(question_id).await?)
            .await
    }

    async fn available_preview(
        &self,
        question: FavoriteQuestion,
    ) -> Result<FavoriteFlashcardPreview, FavoriteFlashcardError> {
        let mut preview = make_preview(question);
        if let Some(practice) = preview.eligibility.recommended_target.clone() {
            let module = crate::domain::favorite_flashcard::resolve_web_target(&practice)
                .ok_or(BankError::Invalid)?
                .module;
            let mut existing = self.writer.existing_user_targets(module).await?;
            existing.retain(|path| {
                crate::domain::favorite_flashcard::resolve_web_target(path).is_some_and(|target| {
                    target.mode == FlashcardCreationMode::UserOrganized && target.module == module
                })
            });
            existing.sort();
            existing.dedup();
            preview.eligibility.allowed_targets =
                std::iter::once(practice).chain(existing).collect();
        }
        Ok(preview)
    }

    pub async fn create(
        &self,
        question_id: &str,
        request: CreateFavoriteFlashcard,
    ) -> Result<FavoriteFlashcardResult, FavoriteFlashcardError> {
        let permit = self.flashcards.exclusive_write()?;
        let application = self.clone();
        let question_id = question_id.to_owned();
        // Filesystem workers can outlive a disconnected HTTP request.
        tokio::spawn(async move {
            let _permit = permit;
            application.create_locked(&question_id, request).await
        })
        .await
        .map_err(|_| FlashcardError::Storage("favorite creation worker failed".into()))?
    }

    async fn create_locked(
        &self,
        question_id: &str,
        request: CreateFavoriteFlashcard,
    ) -> Result<FavoriteFlashcardResult, FavoriteFlashcardError> {
        let question = self.source.load(question_id).await?;
        let preview = self.available_preview(question.clone()).await?;
        validate_create_request(&preview, &request)?;
        let target = crate::domain::favorite_flashcard::resolve_web_target(&request.target_deck)
            .ok_or(BankError::Invalid)?;
        let (front, assets) = self
            .prepare_assets(&question.front, &target.target_deck)
            .await?;
        let written = self
            .writer
            .append(
                crate::application::favorite_flashcard_ports::NewFavoriteCard {
                    question_id: question.question_id,
                    target_deck: request.target_deck,
                    front,
                    back: if request.back.trim().is_empty() {
                        "-".into()
                    } else {
                        request.back.trim().into()
                    },
                    correct_answer: question.correct_answer,
                    assets,
                },
            )
            .await?;
        let mut result = FavoriteFlashcardResult {
            status: "created_sync_pending".into(),
            target_deck: written
                .relative_path
                .strip_suffix("/cards.md")
                .unwrap_or(&written.relative_path)
                .into(),
            source_file: written.relative_path.clone(),
            card_id: written.card_id.map(|id| id.to_string()),
            retryable: true,
        };
        if let Ok((_, snapshot)) = self.flashcards.sync_locked().await {
            let cards = snapshot
                .cards
                .iter()
                .filter(|card| {
                    card.source_question_id.as_deref() == Some(question_id)
                        && card.source_file == written.relative_path
                })
                .collect::<Vec<_>>();
            if cards.len() == 1 {
                let card = cards[0];
                if snapshot
                    .successful_markdown_files
                    .contains(&card.source_file)
                    && self
                        .flashcards
                        .confirms_synced_card(card)
                        .await
                        .unwrap_or(false)
                {
                    result.status = if written.appended {
                        "created_synced"
                    } else {
                        "already_exists"
                    }
                    .into();
                    result.card_id = Some(card.id.to_string());
                    result.source_file = card.source_file.clone();
                    result.target_deck = card.deck_path.clone();
                    result.retryable = false;
                }
            }
        }
        Ok(result)
    }

    async fn prepare_assets(
        &self,
        front: &str,
        target: &str,
    ) -> Result<
        (
            String,
            Vec<crate::application::favorite_flashcard_ports::FavoriteCardAsset>,
        ),
        FavoriteFlashcardError,
    > {
        let mut assets = std::collections::BTreeMap::new();
        let mut remaining = front;
        while let Some((_, tail)) = remaining.split_once("hyz-bank-asset:") {
            let end = tail.find(')').ok_or(BankError::Invalid)?;
            let name = &tail[..end];
            crate::application::question_bank::images::asset_type(name)?;
            if !assets.contains_key(name) {
                if assets.len() >= 64 {
                    return Err(BankError::Invalid.into());
                }
                let bytes = self.source.asset(name.into()).await?;
                if bytes.len() > 16 * 1024 * 1024
                    || crate::application::question_bank::source_hash(&bytes)
                        != name.split('.').next().unwrap_or("")
                {
                    return Err(BankError::Invalid.into());
                }
                assets.insert(name.to_string(), bytes);
            }
            remaining = &tail[end + 1..];
        }
        let mut rewritten = front.to_string();
        let prefix = "../".repeat(target.split('/').count());
        let assets = assets
            .into_iter()
            .map(|(file_name, contents)| {
                rewritten = rewritten.replace(
                    &format!("hyz-bank-asset:{file_name}"),
                    &format!("{prefix}images/{file_name}"),
                );
                crate::application::favorite_flashcard_ports::FavoriteCardAsset {
                    file_name,
                    contents,
                }
            })
            .collect();
        Ok((rewritten, assets))
    }
}

fn validate_create_request(
    preview: &FavoriteFlashcardPreview,
    request: &CreateFavoriteFlashcard,
) -> Result<(), FavoriteFlashcardError> {
    if request.source_revision != preview.source_revision {
        return Err(BankError::Stale.into());
    }
    let target = crate::domain::favorite_flashcard::resolve_web_target(&request.target_deck)
        .ok_or(BankError::Invalid)?;
    let selected = request.creation_mode.unwrap_or(target.mode);
    let allowed = preview
        .eligibility
        .allowed_targets
        .contains(&request.target_deck)
        && selected == target.mode
        && request.type_confirmed;
    if !preview.eligibility.eligible || !allowed || request.back.len() > 32 * 1024 {
        return Err(BankError::Invalid.into());
    }
    Ok(())
}

fn make_preview(question: FavoriteQuestion) -> FavoriteFlashcardPreview {
    let mut eligibility = crate::domain::favorite_flashcard::eligibility(
        question.module.as_deref(),
        question.category.as_deref(),
    );
    if !question.gradable {
        eligibility.eligible = false;
        eligibility.reason = Some("原题缺少完整题干、选项或有效答案".into());
        eligibility.allowed_targets.clear();
        eligibility.recommended_target = None;
    }
    FavoriteFlashcardPreview {
        eligibility,
        front: question.front,
        correct_answer: question.correct_answer,
        source_revision: question.source_revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_keeps_shared_material_and_answer_outside_front() {
        let question = FavoriteQuestion {
            question_id: "019a3d8e-1b32-7a60-9c83-f34217a6e210".into(),
            module: Some("数量关系".into()),
            category: Some("工程效率".into()),
            front: "共享材料\n\n题干\n\n- A. 甲\n- B. 乙".into(),
            correct_answer: vec!["B".into()],
            source_revision: "revision".into(),
            gradable: true,
        };
        let preview = make_preview(question);
        assert!(preview.eligibility.eligible);
        assert!(preview.front.starts_with("共享材料"));
        assert_eq!(preview.correct_answer, vec!["B"]);
        assert!(!preview.front.contains("correct_answer"));
    }
}
