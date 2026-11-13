//! Dedicated, authenticated 申论 AI task HTTP adapter.
use super::*;
use crate::adapters::outbound::shenlun_ai::{ShenlunAiError, ShenlunAiService};
use crate::domain::question_bank::{ConfirmShenlunTranscription, ShenlunJobRequest};
use axum::Extension;

pub fn with_shenlun_ai(router: Router, ai: Option<Arc<ShenlunAiService>>) -> Router {
    match ai {
        Some(service) => router.layer(Extension(service)),
        None => router,
    }
}
type Service = Option<Extension<Arc<ShenlunAiService>>>;

#[expect(clippy::result_large_err, reason = "Axum responses are returned directly by the handler")]
fn require_service(service: Service) -> Result<Arc<ShenlunAiService>, Response> {
    service.map(|Extension(service)| service).ok_or_else(|| {
        error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "shenlun_ai_unavailable",
            "申论 AI 未配置",
        )
    })
}
fn ai_error(error: ShenlunAiError) -> Response {
    let (status, code, message) = match error {
        ShenlunAiError::Invalid => (
            StatusCode::BAD_REQUEST,
            "shenlun_ai_invalid",
            "申论请求格式或内容无效",
        ),
        ShenlunAiError::NotFound => (
            StatusCode::NOT_FOUND,
            "shenlun_ai_missing",
            "申论记录不存在",
        ),
        ShenlunAiError::Conflict => (
            StatusCode::CONFLICT,
            "shenlun_ai_conflict",
            "申论请求版本或幂等键冲突",
        ),
        ShenlunAiError::Stale => (
            StatusCode::CONFLICT,
            "shenlun_ai_stale",
            "原题来源已更新，请重新确认题目",
        ),
        ShenlunAiError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "shenlun_ai_unavailable",
            "Codex 或上传服务不可用",
        ),
        ShenlunAiError::Storage => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "shenlun_ai_storage",
            "申论任务存储失败",
        ),
        ShenlunAiError::NotReady => (
            StatusCode::CONFLICT,
            "shenlun_ai_not_ready",
            "需要先完成本稿批改，或远端状态尚未确认",
        ),
    };
    error_json(status, code, message)
}

pub(super) async fn jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.history(&id).await {
        Ok(jobs) => Json(jobs).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<ShenlunJobRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(request) => request,
        Err(error) => return error.response(),
    };
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.start(&id, request).await {
        Ok(job) => (StatusCode::ACCEPTED, Json(job)).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.get(&id).await {
        Ok(job) => Json(job).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    if let Err(error) = decode_control_json(payload) {
        return error.response();
    }
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.stop(&id).await {
        Ok(job) => Json(job).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<ConfirmShenlunTranscription>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let input = match decode_control_json(payload) {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.confirm_transcription(&id, input).await {
        Ok(attempt) => Json(attempt).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn reference(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.reference_answer(&id).await {
        Ok(answer) => Json(answer).into_response(),
        Err(error) => ai_error(error),
    }
}

/// One bounded SSE connection; state is held in SQLite, so disconnect/reload
/// can resume from GET. Every event read revalidates the authentication context.
pub(super) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    if let Err(error) = service.get(&id).await {
        return ai_error(error);
    }
    let stream = futures_util::stream::unfold(
        (state, headers, service, id, false, 0_i64),
        |(state, headers, service, id, finished, last)| async move {
            if finished {
                return None;
            }
            if authorize_sensitive_read(&state, &headers).await.is_err() {
                return None;
            }
            let job = service.get(&id).await.ok()?;
            let changed = job.updated_at_ms != last;
            if !changed {
                tokio::time::sleep(Duration::from_secs(2)).await;
                return Some((
                    Ok::<_, std::convert::Infallible>(
                        axum::response::sse::Event::default().comment("keepalive"),
                    ),
                    (state, headers, service, id, false, last),
                ));
            }
            let final_state = job.status != "running";
            let data = serde_json::to_string(&job).ok()?;
            let event = axum::response::sse::Event::default()
                .event("snapshot")
                .id(job.updated_at_ms.to_string())
                .data(data);
            Some((
                Ok::<_, std::convert::Infallible>(event),
                (state, headers, service, id, final_state, job.updated_at_ms),
            ))
        },
    );
    let mut response = axum::response::sse::Sse::new(stream).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}
