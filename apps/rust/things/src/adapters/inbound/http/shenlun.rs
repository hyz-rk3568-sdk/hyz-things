//! Authenticated, transient Markdown OCR and source-reference endpoints.
use super::*;
use crate::adapters::outbound::shenlun_ai::{ShenlunAiError, ShenlunAiService};
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

#[derive(serde::Serialize)]
struct OcrResponse {
    markdown: String,
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
        ShenlunAiError::Remote(message) => (
            StatusCode::BAD_GATEWAY,
            "shenlun_ocr_failed",
            message,
        ),
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
    match service.ocr(&id, &input.attachments).await {
        Ok(markdown) => Json(OcrResponse { markdown }).into_response(),
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
