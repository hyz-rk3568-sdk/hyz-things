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
    #[serde(default)]
    message: String,
    #[serde(default)]
    attachments: Vec<String>,
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
    match service
        .send(&request.id, &request.message, &request.attachments)
        .await
    {
        Ok(()) => Json(serde_json::json!({"accepted":true})).into_response(),
        Err(error) => failure(error),
    }
}

pub(super) async fn agent_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Agent,
    query: Result<
        axum::extract::Query<AiPracticeQuestionId>,
        axum::extract::rejection::QueryRejection,
    >,
    payload: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_image_admin(&state, &headers).await {
        return response;
    }
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();
    if !matches!(media_type, "image/png" | "image/jpeg") {
        return error_json(
            StatusCode::BAD_REQUEST,
            "ai_practice_attachment_type",
            "仅支持 JPG 或 PNG 图片",
        );
    }
    let bytes = match payload {
        Ok(bytes) => bytes,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return payload_too_large_json();
        }
        Err(_) => return invalid_request_json(),
    };
    let Ok(axum::extract::Query(id)) = query else {
        return failure(AiPracticeError::Invalid);
    };
    let Some(Extension(service)) = service else {
        return failure(AiPracticeError::Unavailable);
    };
    match service.attach(&id, media_type, &bytes).await {
        Ok(id) => Json(serde_json::json!({"id": id})).into_response(),
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

pub(super) async fn agent_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Agent,
    query: Result<
        axum::extract::Query<AiPracticeQuestionId>,
        axum::extract::rejection::QueryRejection,
    >,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Ok(axum::extract::Query(id)) = query else {
        return failure(AiPracticeError::Invalid);
    };
    let Some(Extension(service)) = service else {
        return failure(AiPracticeError::Unavailable);
    };
    match service.delete(&id).await {
        Ok(()) => Json(serde_json::json!({"deleted":true})).into_response(),
        Err(error) => failure(error),
    }
}

struct AgentEventConnection {
    state: AppState,
    headers: HeaderMap,
    service: Arc<crate::application::ai_practice::AiPracticeAgent>,
    id: AiPracticeQuestionId,
    receiver: tokio::sync::broadcast::Receiver<AiPracticeQuestionId>,
    snapshot: Option<crate::domain::ai_practice::AiPracticeAgentState>,
    last_emit: std::time::Instant,
    next_authorization: tokio::time::Instant,
}

pub(super) async fn agent_events(
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
    let snapshot = match service.state(&id).await {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    let receiver = service.subscribe();
    let connection = AgentEventConnection {
        state,
        headers,
        service,
        receiver,
        id,
        snapshot: Some(snapshot),
        last_emit: std::time::Instant::now() - std::time::Duration::from_millis(200),
        next_authorization: tokio::time::Instant::now() + std::time::Duration::from_secs(2),
    };
    let stream = futures_util::stream::unfold(connection, |mut connection| async move {
        loop {
            if let Some(snapshot) = connection.snapshot.take() {
                let data = serde_json::to_string(&snapshot).ok()?;
                return Some((
                    Ok::<_, std::convert::Infallible>(
                        axum::response::sse::Event::default().data(data),
                    ),
                    connection,
                ));
            }
            let received = tokio::select! {
                value = connection.receiver.recv() => Some(value),
                _ = tokio::time::sleep_until(connection.next_authorization) => {
                    if authorize_sensitive_read(&connection.state, &connection.headers)
                        .await
                        .is_err()
                    {
                        return None;
                    }
                    connection.next_authorization =
                        tokio::time::Instant::now() + std::time::Duration::from_secs(2);
                    continue;
                }
            };
            match received {
                Some(Ok(changed)) if changed != connection.id => continue,
                Some(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                Some(Ok(_)) | Some(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                    let mut snapshot = connection.service.state(&connection.id).await.ok()?;
                    if snapshot.status == "running" {
                        let window = std::time::Duration::from_millis(100);
                        let elapsed = connection.last_emit.elapsed();
                        if elapsed < window {
                            // Coalesce rapid deltas, then re-read so the emitted
                            // snapshot includes every delta produced meanwhile.
                            tokio::time::sleep(window - elapsed).await;
                            snapshot = connection.service.state(&connection.id).await.ok()?;
                        }
                    }
                    connection.last_emit = std::time::Instant::now();
                    let data = serde_json::to_string(&snapshot).ok()?;
                    return Some((
                        Ok(axum::response::sse::Event::default().data(data)),
                        connection,
                    ));
                }
                None => return None,
            }
        }
    });
    let mut response = axum::response::sse::Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        "x-accel-buffering",
        axum::http::HeaderValue::from_static("no"),
    );
    response
}
