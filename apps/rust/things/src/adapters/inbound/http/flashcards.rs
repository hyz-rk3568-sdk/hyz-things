use super::*;
use crate::{
    adapters::outbound::flashcard_source::FlashcardAssetError,
    application::flashcards::FlashcardError,
    domain::flashcards::{FlashcardFilter, FlashcardId, FlashcardSummary, ReviewRating},
};
use axum::extract::RawQuery;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FlashcardQuery {
    deck: Option<String>,
    #[serde(default)]
    tag: Vec<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

impl FlashcardQuery {
    fn parse(raw_query: Option<&str>) -> Result<Self, ()> {
        let mut query = Self {
            deck: None,
            tag: Vec::new(),
            limit: None,
            offset: None,
        };
        for (key, value) in url::form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
            match key.as_ref() {
                "deck" => {
                    if query.deck.is_some() {
                        return Err(());
                    }
                    query.deck = Some(value.into_owned());
                }
                "tag" => query.tag.push(value.into_owned()),
                "limit" => {
                    if query.limit.is_some() {
                        return Err(());
                    }
                    query.limit = Some(value.parse().map_err(|_| ())?);
                }
                "offset" => {
                    if query.offset.is_some() {
                        return Err(());
                    }
                    query.offset = Some(value.parse().map_err(|_| ())?);
                }
                _ => return Err(()),
            }
        }
        Ok(query)
    }
}

fn parse_filter(raw_query: Option<&str>) -> Result<FlashcardFilter, ()> {
    FlashcardQuery::parse(raw_query)
        .and_then(|query| FlashcardFilter::try_from(query).map_err(|_| ()))
}

impl TryFrom<FlashcardQuery> for FlashcardFilter {
    type Error = FlashcardError;

    fn try_from(query: FlashcardQuery) -> Result<Self, Self::Error> {
        FlashcardFilter {
            deck: query.deck,
            tags: query.tag,
            limit: query.limit.unwrap_or(50),
            offset: query.offset.unwrap_or(0),
        }
        .normalized()
        .map_err(|error| FlashcardError::InvalidRequest(error.to_string()))
    }
}

#[derive(Serialize)]
struct CardsResponse {
    cards: Vec<FlashcardSummary>,
}

#[derive(Serialize)]
struct DecksResponse {
    decks: Vec<crate::domain::flashcards::DeckSummary>,
}

#[derive(Serialize)]
struct TagsResponse {
    tags: Vec<crate::domain::flashcards::TagSummary>,
}

#[derive(Serialize)]
struct ReviewQueueResponse {
    cards: Vec<FlashcardSummary>,
}

#[derive(Serialize)]
struct SummaryResponse {
    summary: crate::domain::flashcards::StudySummary,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewRequest {
    card_id: String,
    rating: ReviewRating,
}

#[derive(Serialize)]
struct ReviewResponse {
    review: crate::domain::flashcards::ReviewOutcome,
}

#[derive(Serialize)]
struct SyncResponse {
    summary: crate::domain::flashcards::SyncSummary,
}

pub(super) async fn cards(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let filter = match parse_filter(raw_query.as_deref()) {
        Ok(filter) => filter,
        Err(_) => return invalid_request_json(),
    };
    let Some(application) = state.flashcards.clone() else {
        return study_unavailable_json();
    };
    match application.cards(filter).await {
        Ok(cards) => Json(CardsResponse { cards }).into_response(),
        Err(error) => flashcard_error_json(error),
    }
}

pub(super) async fn decks(State(state): State<AppState>) -> Response {
    let Some(application) = state.flashcards.clone() else {
        return study_unavailable_json();
    };
    match application.decks().await {
        Ok(decks) => Json(DecksResponse { decks }).into_response(),
        Err(error) => flashcard_error_json(error),
    }
}

pub(super) async fn tags(State(state): State<AppState>) -> Response {
    let Some(application) = state.flashcards.clone() else {
        return study_unavailable_json();
    };
    match application.tags().await {
        Ok(tags) => Json(TagsResponse { tags }).into_response(),
        Err(error) => flashcard_error_json(error),
    }
}

pub(super) async fn review_queue(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let filter = match parse_filter(raw_query.as_deref()) {
        Ok(filter) => filter,
        Err(_) => return invalid_request_json(),
    };
    let Some(application) = state.flashcards.clone() else {
        return study_unavailable_json();
    };
    match application.review_queue(filter).await {
        Ok(cards) => Json(ReviewQueueResponse { cards }).into_response(),
        Err(error) => flashcard_error_json(error),
    }
}

pub(super) async fn summary(State(state): State<AppState>) -> Response {
    let Some(application) = state.flashcards.clone() else {
        return study_unavailable_json();
    };
    match application.summary().await {
        Ok(summary) => Json(SummaryResponse { summary }).into_response(),
        Err(error) => flashcard_error_json(error),
    }
}

pub(super) async fn review(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ReviewRequest>, JsonRejection>,
) -> Response {
    if !authorize_same_origin_csrf(&state, &headers) {
        return forbidden_json();
    }
    let request = match decode_control_json(payload) {
        Ok(request) => request,
        Err(error) => return error.response(),
    };
    let card_id = match FlashcardId::new(request.card_id) {
        Ok(card_id) => card_id,
        Err(_) => return invalid_request_json(),
    };
    let Some(application) = state.flashcards.clone() else {
        return study_unavailable_json();
    };
    match application.review(card_id, request.rating).await {
        Ok(review) => Json(ReviewResponse { review }).into_response(),
        Err(error) => flashcard_error_json(error),
    }
}

pub(super) async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    if let Err(error) = decode_control_json(payload) {
        return error.response();
    }
    let Some(application) = state.flashcards.clone() else {
        return study_unavailable_json();
    };
    match application.sync().await {
        Ok(summary) => Json(SyncResponse { summary }).into_response(),
        Err(error) => flashcard_error_json(error),
    }
}

pub(super) async fn asset(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let Some(source) = state.flashcard_source.clone() else {
        return study_unavailable_json();
    };
    match source.read_asset(&path) {
        Ok(asset) => {
            let mut response = Response::new(Body::from(asset.contents));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(asset.content_type),
            );
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=300"),
            );
            response
        }
        Err(error) => asset_error_json(error),
    }
}

fn study_unavailable_json() -> Response {
    error_json(
        StatusCode::SERVICE_UNAVAILABLE,
        "study_unavailable",
        "Study cards are temporarily unavailable",
    )
}

fn asset_error_json(error: FlashcardAssetError) -> Response {
    match error {
        FlashcardAssetError::InvalidPath
        | FlashcardAssetError::UnsupportedFormat
        | FlashcardAssetError::OutsideRoot => error_json(
            StatusCode::BAD_REQUEST,
            "study_asset_invalid_path",
            "Study asset path is invalid",
        ),
        FlashcardAssetError::NotFound => not_found_json(),
        FlashcardAssetError::TooLarge => error_json(
            StatusCode::PAYLOAD_TOO_LARGE,
            "study_asset_too_large",
            "Study asset is too large",
        ),
        FlashcardAssetError::Unavailable => error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "study_asset_unavailable",
            "Study asset is temporarily unavailable",
        ),
    }
}

fn flashcard_error_json(error: FlashcardError) -> Response {
    let status = match error {
        FlashcardError::Busy => StatusCode::CONFLICT,
        FlashcardError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        FlashcardError::NotFound => StatusCode::NOT_FOUND,
        FlashcardError::SourceUnavailable(_) | FlashcardError::StorageUnavailable(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        FlashcardError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_json(status, error.code(), "Study request failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parser_accepts_repeated_tags_and_typed_pagination() {
        let query = FlashcardQuery::parse(Some(
            "deck=%E5%88%A4%E6%96%AD%E6%8E%A8%E7%90%86&tag=%E9%94%99%E9%A2%98&tag=%E9%AB%98%E9%A2%91&limit=200&offset=4",
        ))
        .unwrap();
        assert_eq!(query.deck.as_deref(), Some("判断推理"));
        assert_eq!(query.tag, vec!["错题", "高频"]);
        assert_eq!(query.limit, Some(200));
        assert_eq!(query.offset, Some(4));
    }

    #[test]
    fn query_requires_all_tags_and_clamps_page_size() {
        let filter = FlashcardFilter::try_from(FlashcardQuery {
            deck: Some("判断推理".to_owned()),
            tag: vec!["错题".to_owned(), "高频".to_owned(), "错题".to_owned()],
            limit: Some(999),
            offset: Some(4),
        })
        .unwrap();
        assert_eq!(filter.tags, vec!["错题", "高频"]);
        assert_eq!(filter.limit, 200);
        assert_eq!(filter.offset, 4);
    }
}
