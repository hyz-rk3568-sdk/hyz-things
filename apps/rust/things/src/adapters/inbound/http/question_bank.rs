use super::*;
use crate::application::question_bank::{BankError, QuestionBankApplication};
use axum::Extension;

pub fn with_question_bank(
    router: Router,
    application: Option<Arc<QuestionBankApplication>>,
) -> Router {
    match application {
        Some(app) => router.layer(Extension(app)),
        None => router,
    }
}
type Service = Option<Extension<Arc<QuestionBankApplication>>>;
fn available(service: Service) -> Result<Arc<QuestionBankApplication>, BankError> {
    service
        .map(|Extension(app)| app)
        .ok_or(BankError::Unavailable)
}
fn bank_error(error: BankError) -> Response {
    let (status, code, message) = match error {
        BankError::Invalid => (
            StatusCode::BAD_REQUEST,
            "question_bank_invalid",
            "题库格式或请求不符合规范",
        ),
        BankError::NotFound => (
            StatusCode::NOT_FOUND,
            "question_bank_not_found",
            "试卷不存在",
        ),
        BankError::Stale => (
            StatusCode::CONFLICT,
            "question_bank_stale",
            "题库文件已变化，请重新同步",
        ),
        BankError::Busy => (StatusCode::CONFLICT, "question_bank_busy", "题库正在同步"),
        BankError::Conflict => (
            StatusCode::CONFLICT,
            "question_bank_conflict",
            "题库身份或内容有冲突，请检查后重新同步",
        ),
        BankError::Unavailable | BankError::Storage => (
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_unavailable",
            "题库共享或索引暂不可用",
        ),
    };
    error_json(status, code, message)
}
pub(super) async fn papers(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(error) => return bank_error(error),
    };
    match app.papers().await {
        Ok(papers) => Json(papers).into_response(),
        Err(error) => bank_error(error),
    }
}
pub(super) async fn paper(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(error) => return bank_error(error),
    };
    match app.paper(&id).await {
        Ok(paper) => Json(paper).into_response(),
        Err(error) => bank_error(error),
    }
}
pub(super) async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    if let Err(error) = decode_control_json(payload) {
        return error.response();
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(error) => return bank_error(error),
    };
    match app.sync().await {
        Ok(summary) => Json(summary).into_response(),
        Err(error) => bank_error(error),
    }
}
pub(super) async fn asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(name): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(error) => return bank_error(error),
    };
    match app.asset(name).await {
        Ok(asset) => {
            let mut response = Response::new(Body::from(asset.contents));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(asset.content_type),
            );
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
        Err(error) => bank_error(error),
    }
}
