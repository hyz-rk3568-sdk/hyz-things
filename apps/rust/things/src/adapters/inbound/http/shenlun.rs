//! Authenticated, transient Markdown OCR and source-reference endpoints.
use super::*;
use crate::adapters::outbound::shenlun_ai::{
    OcrRunPush, OcrRunSnapshot, ShenlunAiError, ShenlunAiService,
};
use axum::Extension;

pub fn with_shenlun_ai(router: Router, ai: Option<Arc<ShenlunAiService>>) -> Router {
    match ai {
        Some(service) => router.layer(Extension(service)),
        None => router,
    }
}
type Service = Option<Extension<Arc<ShenlunAiService>>>;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OcrRequest {
    attachments: Vec<String>,
}

#[expect(
    clippy::result_large_err,
    reason = "Axum responses are returned directly by the handler"
)]
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
    let (status, code, message): (StatusCode, &str, &str) = match &error {
        ShenlunAiError::Invalid => (
            StatusCode::BAD_REQUEST,
            "shenlun_ai_invalid",
            "申论 OCR 图片或参数无效",
        ),
        ShenlunAiError::NotFound => (
            StatusCode::NOT_FOUND,
            "shenlun_ai_missing",
            "未找到这道申论题目",
        ),
        ShenlunAiError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "shenlun_ai_unavailable",
            "申论 Codex 或上传服务不可用",
        ),
        ShenlunAiError::NotReady => (
            StatusCode::CONFLICT,
            "shenlun_ai_not_ready",
            "尚未完成本题批改，或当前处理任务繁忙",
        ),
        ShenlunAiError::Remote(message) => (StatusCode::BAD_GATEWAY, "shenlun_ocr_failed", message),
    };
    error_json(status, code, message)
}

pub(super) async fn ocr(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<OcrRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let input = match decode_control_json(payload) {
        Ok(input) => input,
        Err(error) => return error.response(),
    };
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.start_ocr(&id, &input.attachments).await {
        Ok(receipt) => (StatusCode::ACCEPTED, Json(receipt)).into_response(),
        Err(error) => ai_error(error),
    }
}

struct OcrEventConnection {
    state: AppState,
    headers: HeaderMap,
    service: Arc<ShenlunAiService>,
    question_id: String,
    run_id: String,
    receiver: tokio::sync::broadcast::Receiver<OcrRunPush>,
    snapshot: Option<OcrRunSnapshot>,
    seq: u64,
    next_authorization: tokio::time::Instant,
    next_heartbeat: tokio::time::Instant,
    done: bool,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl OcrEventConnection {
    async fn reauthorize_if_due(&mut self) -> bool {
        if tokio::time::Instant::now() < self.next_authorization {
            return true;
        }
        if authorize_sensitive_read(&self.state, &self.headers)
            .await
            .is_err()
            || self
                .service
                .ocr_run_snapshot(&self.question_id, &self.run_id)
                .is_none()
        {
            return false;
        }
        self.next_authorization = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        true
    }
}

pub(super) async fn ocr_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path((question_id, run_id)): Path<(String, String)>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match require_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    let permit = match service.ocr_stream_permit() {
        Ok(permit) => permit,
        Err(error) => return ai_error(error),
    };
    let Some((snapshot, receiver)) = service.subscribe_ocr_run(&question_id, &run_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let connection = OcrEventConnection {
        state,
        headers,
        service,
        question_id,
        run_id,
        receiver,
        snapshot: Some(snapshot),
        seq: 0,
        next_authorization: tokio::time::Instant::now(),
        next_heartbeat: tokio::time::Instant::now() + std::time::Duration::from_secs(15),
        done: false,
        _permit: permit,
    };
    let stream = futures_util::stream::unfold(connection, |mut connection| async move {
        if connection.done {
            return None;
        }
        loop {
            if !connection.reauthorize_if_due().await {
                return None;
            }
            if let Some(snapshot) = connection.snapshot.take() {
                connection.seq = snapshot.seq;
                connection.done = snapshot.status != "running";
                let id = format!("{}:{}", snapshot.run_id, snapshot.seq);
                let data = serde_json::to_string(&snapshot).ok()?;
                let event = axum::response::sse::Event::default()
                    .event("snapshot")
                    .id(id)
                    .data(data);
                return Some((Ok::<_, std::convert::Infallible>(event), connection));
            }
            let received = tokio::select! {
                value = connection.receiver.recv() => Some(value),
                _ = tokio::time::sleep_until(connection.next_authorization) => continue,
                _ = tokio::time::sleep_until(connection.next_heartbeat) => None,
            };
            if !connection.reauthorize_if_due().await {
                return None;
            }
            match received {
                None => {
                    connection.next_heartbeat =
                        tokio::time::Instant::now() + std::time::Duration::from_secs(15);
                    return Some((
                        Ok(axum::response::sse::Event::default().comment("keepalive")),
                        connection,
                    ));
                }
                Some(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                Some(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                    connection.snapshot = connection
                        .service
                        .ocr_run_snapshot(&connection.question_id, &connection.run_id);
                    connection.snapshot.as_ref()?;
                }
                Some(Ok(push)) => {
                    if push.question_id != connection.question_id
                        || push.run_id != connection.run_id
                        || push.event.seq <= connection.seq
                    {
                        continue;
                    }
                    if push.event.seq != connection.seq + 1 {
                        connection.snapshot = connection
                            .service
                            .ocr_run_snapshot(&connection.question_id, &connection.run_id);
                        connection.snapshot.as_ref()?;
                        continue;
                    }
                    connection.seq = push.event.seq;
                    connection.done =
                        matches!(push.event.kind.as_str(), "run_finished" | "run_failed");
                    let id = format!("{}:{}", push.event.run_id, push.event.seq);
                    let data = serde_json::to_string(&push.event).ok()?;
                    let event = axum::response::sse::Event::default().id(id).data(data);
                    return Some((Ok(event), connection));
                }
            }
        }
    });
    let mut response = axum::response::sse::Sse::new(stream).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
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
