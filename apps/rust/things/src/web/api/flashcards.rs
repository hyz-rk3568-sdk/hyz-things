use hyz_things::domain::flashcards::{
    DeckSummary, FlashcardId, FlashcardSummary, ReviewOutcome, ReviewRating, StudySummary,
    SyncSummary, TagSummary,
};
use serde::{Deserialize, Serialize};

pub(crate) const STUDY_CARDS_ENDPOINT: &str = "/api/v1/study/cards";
pub(crate) const STUDY_DECKS_ENDPOINT: &str = "/api/v1/study/decks";
pub(crate) const STUDY_TAGS_ENDPOINT: &str = "/api/v1/study/tags";
pub(crate) const STUDY_REVIEW_QUEUE_ENDPOINT: &str = "/api/v1/study/review";
pub(crate) const STUDY_SUMMARY_ENDPOINT: &str = "/api/v1/study/summary";
pub(crate) const STUDY_REVIEW_ENDPOINT: &str = "/api/v1/study/review";
pub(crate) const STUDY_SYNC_ENDPOINT: &str = "/api/v1/control/study/sync";

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StudyCardsResponse {
    pub(crate) cards: Vec<FlashcardSummary>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StudyDecksResponse {
    pub(crate) decks: Vec<DeckSummary>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StudyTagsResponse {
    pub(crate) tags: Vec<TagSummary>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StudySummaryResponse {
    pub(crate) summary: StudySummary,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StudyReviewResponse {
    pub(crate) review: ReviewOutcome,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct StudySyncResponse {
    pub(crate) summary: SyncSummary,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StudyReviewRequest {
    pub(crate) card_id: FlashcardId,
    pub(crate) rating: ReviewRating,
}

pub(crate) fn study_query(
    deck: Option<&str>,
    tags: &[String],
    limit: Option<u32>,
    offset: Option<u32>,
) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    if let Some(deck) = deck.filter(|value| !value.is_empty()) {
        query.append_pair("deck", deck);
    }
    for tag in tags {
        if !tag.is_empty() {
            query.append_pair("tag", tag);
        }
    }
    if let Some(limit) = limit {
        query.append_pair("limit", &limit.to_string());
    }
    if let Some(offset) = offset {
        query.append_pair("offset", &offset.to_string());
    }
    let query = query.finish();
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn study_query_encodes_combined_deck_and_tags() {
        let query = study_query(
            Some("判断推理/逻辑判断"),
            &["错题".to_owned(), "高频".to_owned()],
            Some(20),
            Some(4),
        );
        assert_eq!(
            query,
            "?deck=%E5%88%A4%E6%96%AD%E6%8E%A8%E7%90%86%2F%E9%80%BB%E8%BE%91%E5%88%A4%E6%96%AD&tag=%E9%94%99%E9%A2%98&tag=%E9%AB%98%E9%A2%91&limit=20&offset=4"
        );
    }
}
