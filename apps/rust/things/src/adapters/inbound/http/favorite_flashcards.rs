use super::*;
use crate::application::favorite_flashcards::{
    CreateFavoriteFlashcard, FavoriteFlashcardApplication, FavoriteFlashcardError,
};
use axum::Extension;

type Service = Option<Extension<Arc<FavoriteFlashcardApplication>>>;

pub fn with_favorite_flashcards(
    router: Router,
    application: Option<Arc<FavoriteFlashcardApplication>>,
) -> Router {
    match application {
        Some(application) => router.layer(Extension(application)),
        None => router,
    }
}

fn service(service: Service) -> Result<Arc<FavoriteFlashcardApplication>, Response> {
    service.map(|Extension(app)| app).ok_or_else(|| {
        error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "favorite_flashcard_unavailable",
            "收藏卡片服务暂不可用",
        )
    })
}

fn failure(error: FavoriteFlashcardError) -> Response {
    match error {
        FavoriteFlashcardError::Bank(error) => super::question_bank::bank_error(error),
        FavoriteFlashcardError::Card(error) => super::flashcards::flashcard_error_json(error),
    }
}

pub(super) async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    application: Service,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match service(application) {
        Ok(app) => app,
        Err(response) => return response,
    };
    match app.preview(&id).await {
        Ok(preview) => Json(preview).into_response(),
        Err(error) => failure(error),
    }
}

pub(super) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    application: Service,
    Path(id): Path<String>,
    payload: Result<Json<CreateFavoriteFlashcard>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(request) => request,
        Err(error) => return error.response(),
    };
    let app = match service(application) {
        Ok(app) => app,
        Err(response) => return response,
    };
    match app.create(&id, request).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(error),
    }
}
