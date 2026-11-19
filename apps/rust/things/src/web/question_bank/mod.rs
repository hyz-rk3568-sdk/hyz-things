//! Question Bank front-end feature: 行测 practice and 申论 share one module.
//! 申论 is a subject inside the question bank, not a sibling product module.
use super::*;

mod ai_composer;
mod ai_reply;
mod ai_review;
mod ai_review_api;
mod ai_review_recovery;
mod ai_review_stream;
mod ai_review_timeline;
mod collection_review;
mod manage;
mod ocr_panel;
mod practice;
mod setup;
mod shenlun;
mod source_answer_panel;

pub(crate) use ai_review::*;
pub(crate) use collection_review::*;
pub(crate) use manage::*;
pub(crate) use practice::*;
pub(crate) use shenlun::*;

use ai_composer::*;
pub(crate) use ai_reply::*;
pub(crate) use ai_review_api::streaming_markdown_complete;
use ai_review_api::*;
use ai_review_recovery::*;
use setup::*;

/// Deck scope is optional so callers can page through every synced card.
pub(crate) fn study_cards_url(deck: Option<&str>, offset: u32) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    if let Some(deck) = deck {
        query.append_pair("deck", deck);
    }
    query.append_pair("limit", "200");
    query.append_pair("offset", &offset.to_string());
    format!("{STUDY_CARDS_ENDPOINT}?{}", query.finish())
}

pub(crate) fn flashcard_list_title(front: &str) -> String {
    let line = front
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let mut chars = line.chars();
    let short: String = chars.by_ref().take(64).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}
