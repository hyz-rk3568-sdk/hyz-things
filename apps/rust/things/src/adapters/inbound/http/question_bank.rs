use super::*;
use crate::adapters::outbound::question_bank_ai::{AiReviewError, QuestionBankAiService};
use crate::application::question_bank::{BankError, QuestionBankApplication};
use crate::domain::question_bank::{
    AiReviewFollowUp, CreatePracticeRequest, ModulePracticeFilters, PracticeMode,
    SavePracticeAnswer, SetFavorite, SetPracticePaused, StudyPlan,
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
        AiReviewError::Storage => error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_ai_storage",
            "AI 解析存储不可用",
        ),
    }
}

pub(super) async fn ai_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path((id, question_id)): Path<(String, String)>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let service = match ai_service(service) {
        Ok(service) => service,
        Err(e) => return e,
    };
    match service.state(&id, &question_id).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn ai_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path((id, question_id)): Path<(String, String)>,
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
    match service.start(&id, &question_id).await {
        Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn ai_follow_up(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path((id, question_id)): Path<(String, String)>,
    payload: Result<Json<AiReviewFollowUp>, JsonRejection>,
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
        Err(e) => return e,
    };
    match service.follow_up(&id, &question_id, payload.message).await {
        Ok(value) => (StatusCode::ACCEPTED, Json(value)).into_response(),
        Err(error) => ai_error(error),
    }
}
pub(super) async fn ai_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    service: AiService,
    Path((id, question_id)): Path<(String, String)>,
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
    match service.stop(&id, &question_id).await {
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
