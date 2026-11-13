use super::*;
use crate::adapters::outbound::question_bank_ai::{
    AiRetryCommand, AiReviewError, AiReviewScope, QuestionBankAiService,
};
use crate::application::question_bank::{BankError, QuestionBankApplication};
use crate::domain::question_bank::{
    CreatePracticeRequest, ModulePracticeFilters, PracticeMode, SavePracticeAnswer, SetFavorite,
    SetPracticePaused, StudyPlan,
};
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
pub fn with_question_bank_ai(router: Router, ai: Option<Arc<QuestionBankAiService>>) -> Router {
    match ai {
        Some(ai) => router.layer(Extension(ai)),
        None => router,
    }
}
type AiService = Option<Extension<Arc<QuestionBankAiService>>>;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AiStartRequest {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    instruction: Option<String>,
    #[serde(default)]
    attachments: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AiFollowUpRequest {
    message: String,
    #[serde(default)]
    attachments: Vec<String>,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AiDeleteRequest {
    #[serde(default)]
    confirm_uncertain: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AiRetryRequest {
    request_id: String,
    #[serde(default)]
    confirm_uncertain: bool,
    #[serde(default)]
    confirm_new_thread: bool,
    #[serde(default)]
    replacement_attachments: Option<Vec<String>>,
}

#[derive(serde::Serialize)]
struct AiAttachmentResponse {
    id: String,
}

#[expect(
    clippy::result_large_err,
    reason = "Axum response errors are returned directly"
)]
fn ai_service(service: AiService) -> Result<Arc<QuestionBankAiService>, Response> {
    service.map(|Extension(service)| service).ok_or_else(|| {
        error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_ai_unavailable",
            "AI 解析服务未配置",
        )
    })
}
fn ai_error(error: AiReviewError) -> Response {
    match error {
        AiReviewError::Bank(error) => bank_error(error),
        AiReviewError::Unavailable => error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_ai_unavailable",
            "电脑 Codex App Server 未配置或不可用",
        ),
        AiReviewError::Invalid => error_json(
            StatusCode::BAD_REQUEST,
            "question_bank_ai_invalid",
            "AI 解析请求无效",
        ),
        AiReviewError::Busy => error_json(
            StatusCode::CONFLICT,
            "question_bank_ai_busy",
            "当前题目正在生成或无法追问",
        ),
        AiReviewError::RequestConflict => error_json(
            StatusCode::CONFLICT,
            "question_bank_ai_request_conflict",
            "请求 ID 已用于不同内容",
        ),
        AiReviewError::RequestNotFound => error_json(
            StatusCode::NOT_FOUND,
            "question_bank_ai_request_not_found",
            "AI 请求不存在",
        ),
        AiReviewError::AttachmentExpired => error_json(
            StatusCode::BAD_REQUEST,
            "question_bank_ai_attachment_expired",
            "临时图片已失效，请重新上传后确认重试",
        ),
        AiReviewError::ConfirmationRequired => error_json(
            StatusCode::CONFLICT,
            "question_bank_ai_confirmation_required",
            "远端运行状态不确定，请确认后重试",
        ),
        AiReviewError::Storage => error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_ai_storage",
            "AI 解析存储不可用",
        ),
    }
}

pub(super) async fn ai_rate_limits(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service.rate_limits().await {
        Ok(value) => Json(value).into_response(),
        Err(error) => ai_error(error),
    }
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AiScopeQuery {
    #[serde(default)]
    scope: Option<AiReviewScope>,
}

impl AiScopeQuery {
    fn scope(&self) -> AiReviewScope {
        self.scope.unwrap_or_default()
    }
}

pub(super) async fn ai_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(e) => return e,
    };
    match service.state_in_scope(&question_id, query.scope()).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => ai_error(error),
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AiHistoryQuery {
    #[serde(default)]
    after: i64,
    #[serde(default = "history_limit")]
    limit: i64,
    #[serde(default)]
    scope: Option<AiReviewScope>,
}

fn history_limit() -> i64 {
    50
}

pub(super) async fn ai_history_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiHistoryQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service
        .history_runs_in_scope(
            &question_id,
            query.scope.unwrap_or_default(),
            query.after,
            query.limit,
        )
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(error) => ai_error(error),
    }
}

pub(super) async fn ai_history_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path((question_id, run_id)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<AiHistoryQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service
        .history_events_in_scope(
            &question_id,
            query.scope.unwrap_or_default(),
            &run_id,
            query.after,
            query.limit,
        )
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(error) => ai_error(error),
    }
}

struct AiEventConnection {
    state: AppState,
    headers: HeaderMap,
    service: Arc<QuestionBankAiService>,
    scope: AiReviewScope,
    identity: (String, String),
    receiver: tokio::sync::broadcast::Receiver<
        crate::adapters::outbound::question_bank_ai::StreamMessage,
    >,
    snapshot: Option<crate::adapters::outbound::question_bank_ai::AiReviewStreamSnapshot>,
    run_id: Option<String>,
    seq: u64,
    next_authorization: tokio::time::Instant,
    next_heartbeat: tokio::time::Instant,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl AiEventConnection {
    async fn reauthorize_if_due(&mut self) -> bool {
        if tokio::time::Instant::now() < self.next_authorization {
            return true;
        }
        if authorize_sensitive_read(&self.state, &self.headers)
            .await
            .is_err()
            || self
                .service
                .stream_identity_in_scope(&self.identity.0, self.scope)
                .await
                .ok()
                .as_ref()
                != Some(&self.identity)
        {
            return false;
        }
        self.next_authorization = tokio::time::Instant::now() + Duration::from_secs(2);
        true
    }
}

pub(super) async fn ai_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    let permit = match service.stream_permit() {
        Ok(permit) => permit,
        Err(error) => return ai_error(error),
    };
    let identity = match service
        .stream_identity_in_scope(&question_id, query.scope())
        .await
    {
        Ok(identity) => identity,
        Err(error) => return ai_error(error),
    };
    let review = match service.state_in_scope(&question_id, query.scope()).await {
        Ok(review) => review,
        Err(error) => return ai_error(error),
    };
    let (snapshot, receiver) = service.stream_subscribe(&identity, &review.status);
    let connection = AiEventConnection {
        state,
        headers,
        service,
        scope: query.scope(),
        identity,
        receiver,
        snapshot: Some(snapshot),
        run_id: None,
        seq: 0,
        next_authorization: tokio::time::Instant::now(),
        next_heartbeat: tokio::time::Instant::now() + Duration::from_secs(15),
        _permit: permit,
    };
    let stream = futures_util::stream::unfold(connection, |mut connection| async move {
        loop {
            if !connection.reauthorize_if_due().await {
                return None;
            }
            if let Some(snapshot) = connection.snapshot.take() {
                connection.run_id = snapshot.run_id.clone();
                connection.seq = snapshot.seq;
                let id = snapshot
                    .run_id
                    .as_ref()
                    .map(|id| format!("{id}:{}", snapshot.seq))
                    .unwrap_or_default();
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
                        tokio::time::Instant::now() + Duration::from_secs(15);
                    return Some((
                        Ok(axum::response::sse::Event::default().comment("keepalive")),
                        connection,
                    ));
                }
                Some(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                Some(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                    let review = connection
                        .service
                        .state_in_scope(&connection.identity.0, connection.scope)
                        .await
                        .ok()?;
                    connection.snapshot = Some(
                        connection
                            .service
                            .stream_snapshot(&connection.identity, &review.status),
                    );
                }
                Some(Ok(message)) => {
                    if !QuestionBankAiService::stream_matches(&message, &connection.identity) {
                        continue;
                    }
                    let event = message.event;
                    if connection.run_id.as_deref() == Some(event.run_id.as_str())
                        && event.seq <= connection.seq
                    {
                        continue;
                    }
                    if event.kind == "reset"
                        || connection.run_id.as_deref() != Some(event.run_id.as_str())
                        || event.seq != connection.seq + 1
                    {
                        let review = connection
                            .service
                            .state_in_scope(&connection.identity.0, connection.scope)
                            .await
                            .ok()?;
                        connection.snapshot = Some(
                            connection
                                .service
                                .stream_snapshot(&connection.identity, &review.status),
                        );
                        continue;
                    }
                    connection.seq = event.seq;
                    let id = format!("{}:{}", event.run_id, event.seq);
                    let data = serde_json::to_string(&event).ok()?;
                    return Some((
                        Ok(axum::response::sse::Event::default().id(id).data(data)),
                        connection,
                    ));
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

pub(super) async fn ai_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    bank: Service,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
    payload: Result<Json<AiStartRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    // Preserve the legacy objective-review body limit. Only imported 申论
    // source questions can carry a long handwritten/typed essay.
    let oversized = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > 8 * 1024);
    if oversized {
        let is_shenlun = matches!(&bank, Some(Extension(app)) if
            matches!(app.question_ai_locator(&question_id).await, Ok(locator) if locator.subject == "shenlun")
        );
        if !is_shenlun {
            return ControlJsonError::PayloadTooLarge.response();
        }
    }
    let request = match decode_control_json(payload) {
        Ok(request) => request,
        Err(error) => return error.response(),
    };
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(e) => return e,
    };
    if let Some(request_id) = request.request_id {
        match service
            .start_request_in_scope(
                &question_id,
                query.scope(),
                &request_id,
                request.instruction,
                request.attachments,
            )
            .await
        {
            Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
            Err(error) => ai_error(error),
        }
    } else {
        match service
            .start_in_scope(
                &question_id,
                query.scope(),
                request.instruction,
                request.attachments,
            )
            .await
        {
            Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
            Err(error) => ai_error(error),
        }
    }
}

pub(super) async fn ai_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
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
            "question_bank_ai_attachment_type",
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
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service
        .attach_in_scope(&question_id, query.scope(), media_type, &bytes)
        .await
    {
        Ok(id) => Json(AiAttachmentResponse { id }).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn ai_follow_up(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    bank: Service,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
    payload: Result<Json<AiFollowUpRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let oversized = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > 8 * 1024);
    if oversized {
        // Only 申论 can send essays. A missing/unavailable source index must
        // fail closed and retain the old Question Bank body-size boundary.
        let allowed = matches!(&bank, Some(Extension(app)) if
            matches!(app.question_ai_locator(&question_id).await, Ok(locator) if locator.subject == "shenlun")
        );
        if !allowed {
            return ControlJsonError::PayloadTooLarge.response();
        }
    }
    let payload = match decode_control_json(payload) {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(e) => return e,
    };
    if let Some(request_id) = payload.request_id {
        match service
            .follow_up_request_in_scope(
                &question_id,
                query.scope(),
                &request_id,
                payload.message,
                payload.attachments,
            )
            .await
        {
            Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
            Err(error) => ai_error(error),
        }
    } else {
        match service
            .follow_up_in_scope(
                &question_id,
                query.scope(),
                payload.message,
                payload.attachments,
            )
            .await
        {
            Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
            Err(error) => ai_error(error),
        }
    }
}

pub(super) async fn ai_request_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path((question_id, request_id)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service
        .request_status_in_scope(&question_id, query.scope(), &request_id)
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => ai_error(error),
    }
}

pub(super) async fn ai_retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path((question_id, source_request_id)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
    payload: Result<Json<AiRetryRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let payload = match decode_control_json(payload) {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service
        .retry_request_in_scope(
            &question_id,
            AiRetryCommand {
                scope: query.scope(),
                source_request_id,
                request_id: payload.request_id,
                confirm_uncertain: payload.confirm_uncertain,
                confirm_new_thread: payload.confirm_new_thread,
                replacement_attachments: payload.replacement_attachments,
            },
        )
        .await
    {
        Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn ai_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    if let Err(error) = decode_control_json(payload) {
        return error.response();
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(e) => return e,
    };
    match service.stop_in_scope(&question_id, query.scope()).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => ai_error(error),
    }
}

type Service = Option<Extension<Arc<QuestionBankApplication>>>;
fn available(service: Service) -> Result<Arc<QuestionBankApplication>, BankError> {
    service
        .map(|Extension(app)| app)
        .ok_or(BankError::Unavailable)
}
pub(super) async fn ai_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path(question_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<AiScopeQuery>,
    payload: Result<axum::body::Bytes, axum::extract::rejection::BytesRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let body = match payload {
        Ok(body) => body,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return ControlJsonError::PayloadTooLarge.response();
        }
        Err(_) => return ControlJsonError::InvalidRequest.response(),
    };
    // Deletion never carries essay text: preserve the pre-existing 8 KiB bound.
    if body.len() > 8 * 1024 {
        return ControlJsonError::PayloadTooLarge.response();
    }
    let request = if body.is_empty() {
        AiDeleteRequest::default()
    } else {
        match serde_json::from_slice::<AiDeleteRequest>(&body) {
            Ok(request) => request,
            Err(_) => return ControlJsonError::InvalidRequest.response(),
        }
    };
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(error) => return error,
    };
    match service
        .delete_with_confirmation_in_scope(&question_id, query.scope(), request.confirm_uncertain)
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => ai_error(error),
    }
}

pub(super) fn bank_error(error: BankError) -> Response {
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
pub(super) async fn data_version(
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
    match app.data_version().await {
        Ok(version) => Json(serde_json::json!({"version": version})).into_response(),
        Err(error) => bank_error(error),
    }
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
pub(super) async fn drill_collections(
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
    match app.drill_collections().await {
        Ok(collections) => Json(collections).into_response(),
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
/// The dedicated read-only shenlun endpoints do not expose reference answers.
pub(super) async fn shenlun_papers(
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
    match app.shenlun_papers().await {
        Ok(papers) => Json(papers).into_response(),
        Err(error) => bank_error(error),
    }
}

pub(super) async fn shenlun_paper(
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
    match app.shenlun_paper(&id).await {
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
    match app.start_sync() {
        Ok(status) => (StatusCode::ACCEPTED, Json(status)).into_response(),
        Err(error) => bank_error(error),
    }
}
pub(super) async fn sync_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    match available(service) {
        Ok(app) => Json(app.sync_status()).into_response(),
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

pub(super) async fn modules(
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
        Err(e) => return bank_error(e),
    };
    match app.modules(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    payload: Result<Json<CreatePracticeRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(value) => value,
        Err(e) => return e.response(),
    };
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.create_practice(request).await {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(e) => bank_error(e),
    }
}

#[derive(serde::Deserialize, Default)]
pub(super) struct PracticeCatalogQuery {
    year_min: Option<i32>,
    year_max: Option<i32>,
    regions: Option<String>,
    exclude_regions: Option<bool>,
}

pub(super) async fn practice_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    axum::extract::Query(query): axum::extract::Query<PracticeCatalogQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    let filters = ModulePracticeFilters {
        year_min: query.year_min,
        year_max: query.year_max,
        regions: query
            .regions
            .unwrap_or_default()
            .split(',')
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect(),
        exclude_regions: query.exclude_regions.unwrap_or(false),
        count: None,
    };
    match app.practice_catalog(&filters).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn module_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.module_settings().await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn save_module_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    payload: Result<Json<ModulePracticeFilters>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let payload = match decode_control_json(payload) {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.save_module_settings(payload).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn study_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.study_plan().await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn save_study_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    payload: Result<Json<StudyPlan>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let payload = match decode_control_json(payload) {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.save_study_plan(payload).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn study_plan_forecast(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    axum::extract::Query(query): axum::extract::Query<PracticeCatalogQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    let filters = ModulePracticeFilters {
        year_min: query.year_min,
        year_max: query.year_max,
        regions: query
            .regions
            .unwrap_or_default()
            .split(',')
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        exclude_regions: query.exclude_regions.unwrap_or(false),
        count: None,
    };
    match app.study_plan_forecast(&filters).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

#[derive(serde::Deserialize)]
pub(super) struct PracticeBrowseQuery {
    mode: PracticeMode,
    paper_id: Option<String>,
    module: Option<String>,
    category: Option<String>,
    year_min: Option<i32>,
    year_max: Option<i32>,
    regions: Option<String>,
    exclude_regions: Option<bool>,
}

pub(super) async fn practice_browse(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    axum::extract::Query(query): axum::extract::Query<PracticeBrowseQuery>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    let filters = ModulePracticeFilters {
        year_min: query.year_min,
        year_max: query.year_max,
        regions: query
            .regions
            .unwrap_or_default()
            .split(',')
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        exclude_regions: query.exclude_regions.unwrap_or(false),
        count: None,
    };
    match app
        .practice_browse(
            query.mode,
            query.paper_id.as_deref(),
            query.module.as_deref(),
            query.category.as_deref(),
            &filters,
        )
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(error) => bank_error(error),
    }
}

pub(super) async fn practice_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.practice_history().await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_question_detail(
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
        Err(e) => return bank_error(e),
    };
    match app.practice_question_detail(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_session(
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
        Err(e) => return bank_error(e),
    };
    match app.practice_workspace(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_answer(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path((id, question_id)): Path<(String, String)>,
    payload: Result<Json<SavePracticeAnswer>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(value) => value,
        Err(e) => return e.response(),
    };
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app
        .save_practice_answer(&id, &question_id, request.answer)
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_pause(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<SetPracticePaused>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(value) => value,
        Err(e) => return e.response(),
    };
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app
        .pause_practice(&id, request.paused, request.elapsed_seconds)
        .await
    {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    if let Err(e) = decode_control_json(payload) {
        return e.response();
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.submit_practice(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_repeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<EmptyJsonRequest>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    if let Err(e) = decode_control_json(payload) {
        return e.response();
    }
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.repeat_practice(&id).await {
        Ok(value) => (StatusCode::CREATED, Json(value)).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn practice_review(
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
        Err(e) => return bank_error(e),
    };
    match app.practice_review(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn favorite_get(
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
        Err(e) => return bank_error(e),
    };
    match app.favorite(&id).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}

pub(super) async fn favorite_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: Service,
    Path(id): Path<String>,
    payload: Result<Json<SetFavorite>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control_admin(&state, &headers).await {
        return response;
    }
    let request = match decode_control_json(payload) {
        Ok(value) => value,
        Err(e) => return e.response(),
    };
    let app = match available(service) {
        Ok(app) => app,
        Err(e) => return bank_error(e),
    };
    match app.set_favorite(&id, request.favorite).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => bank_error(e),
    }
}
