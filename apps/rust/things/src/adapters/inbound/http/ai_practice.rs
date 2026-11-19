use super::*;
use crate::{
    application::ai_practice::{AiPracticeError, AiPracticeFavorites},
    domain::ai_practice::AiPracticeQuestionId,
};
use axum::Extension;

pub fn with_ai_practice(router: Router, application: Option<Arc<AiPracticeFavorites>>) -> Router {
    match application {
        Some(app) => router.layer(Extension(app)),
        None => router,
    }
}
type Service = Option<Extension<Arc<AiPracticeFavorites>>>;

fn failure(error: AiPracticeError) -> Response {
    let (status, code, message) = match error {
        AiPracticeError::Invalid => (
            StatusCode::BAD_REQUEST,
            "ai_practice_invalid",
            "练习题标识无效",
        ),
        AiPracticeError::NotFound => (
            StatusCode::NOT_FOUND,
            "ai_practice_not_found",
            "练习题不存在",
        ),
        AiPracticeError::Busy => (
            StatusCode::CONFLICT,
            "ai_practice_busy",
            "该题已有解题任务正在运行",
        ),
        AiPracticeError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "ai_practice_unavailable",
            "练习收藏暂不可用",
        ),
    };
    error_json(status, code, message)
}

pub(super) async fn favorites(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(Extension(service)) = service else {
        return failure(AiPracticeError::Unavailable);
    };
    match service.list().await {
        Ok(items) => Json(items).into_response(),
        Err(error) => failure(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetFavorite {
    id: AiPracticeQuestionId,
    favorite: bool,
}

pub(super) async fn set_favorite(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    payload: Result<Json<SetFavorite>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(request) => request,
        Err(error) => return error.response(),
    };
    let Some(Extension(service)) = service else {
        return failure(AiPracticeError::Unavailable);
    };
    match service.set(&request.id, request.favorite).await {
        Ok(()) => Json(serde_json::json!({"favorite":request.favorite})).into_response(),
        Err(error) => failure(error),
    }
}

pub fn with_ai_practice_agent(
    router: Router,
    application: Option<Arc<crate::application::ai_practice::AiPracticeAgent>>,
) -> Router {
    match application {
        Some(app) => router.layer(Extension(app)),
        None => router,
    }
}
type Agent = Option<Extension<Arc<crate::application::ai_practice::AiPracticeAgent>>>;

pub(super) async fn agent_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Agent,
    query: Result<
        axum::extract::Query<AiPracticeQuestionId>,
        axum::extract::rejection::QueryRejection,
    >,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Ok(axum::extract::Query(id)) = query else {
        return failure(AiPracticeError::Invalid);
    };
    let Some(Extension(service)) = service else {
        return failure(AiPracticeError::Unavailable);
    };
    match service.state(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentMessage {
    id: AiPracticeQuestionId,
    message: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentStop {
    id: AiPracticeQuestionId,
}

pub(super) async fn agent_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Agent,
    payload: Result<Json<AgentMessage>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(request) => request,
        Err(error) => return error.response(),
    };
    let Some(Extension(service)) = service else {
        return failure(AiPracticeError::Unavailable);
    };
    match service.send(&request.id, &request.message).await {
        Ok(()) => Json(serde_json::json!({"accepted":true})).into_response(),
        Err(error) => failure(error),
    }
}

pub(super) async fn agent_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Agent,
    payload: Result<Json<AgentStop>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(request) => request,
        Err(error) => return error.response(),
    };
    let Some(Extension(service)) = service else {
        return failure(AiPracticeError::Unavailable);
    };
    match service.stop(&request.id).await {
        Ok(()) => Json(serde_json::json!({"stopped":true})).into_response(),
        Err(error) => failure(error),
    }
}
