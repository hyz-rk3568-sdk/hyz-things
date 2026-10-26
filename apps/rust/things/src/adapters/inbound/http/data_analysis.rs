//! Admin-only API for AI-assisted comprehensive data-analysis training.
use super::*;
use crate::adapters::outbound::data_analysis::{AnswerSummary, CardPerformance, DataAnalysisService};

fn unavailable() -> Response {
    error_json(
        StatusCode::SERVICE_UNAVAILABLE,
        "study_data_analysis_unavailable",
        "综合训练存储未就绪",
    )
}
fn bad_request(msg: &str) -> Response {
    error_json(
        StatusCode::BAD_REQUEST,
        "study_data_analysis_invalid_request",
        msg,
    )
}
fn error(msg: &str) -> Response {
    // Codex tokens, prompts and raw responses must never be reflected.
    // Only the application-generated, bounded Chinese error category is returned.
    error_json(
        StatusCode::SERVICE_UNAVAILABLE,
        "study_data_analysis_failed",
        msg,
    )
}
fn get(state: &AppState) -> Option<Arc<DataAnalysisService>> {
    state.data_analysis.clone()
}
#[derive(Serialize)]
struct StatusBody {
    configured: bool,
    model: Option<String>,
    cached_sources: i64,
    verified_groups: i64,
    preparing_job_id: Option<String>,
    answer_summary: AnswerSummary,
    card_stats: Vec<CardPerformance>,
}
pub(super) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    let (cached_sources, verified_groups) = match service.counts().await {
        Ok(counts) => counts,
        Err(_) => return unavailable(),
    };
    let (answer_summary, card_stats) = match service.card_performance().await {
        Ok(stats) => stats,
        Err(_) => return unavailable(),
    };
    Json(StatusBody {
        configured: service.configured_model().is_some(),
        model: service.configured_model(),
        cached_sources,
        verified_groups,
        preparing_job_id: service.preparing_job().await,
        answer_summary,
        card_stats,
    })
    .into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UserPromptUpdate {
    // null means restore the built-in default; no system instructions are editable.
    user_prompt: Option<String>,
}

pub(super) async fn user_prompt(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.user_prompt_settings().await {
        Ok(settings) => Json(settings).into_response(),
        Err(_) => error("无法读取出题要求"),
    }
}
pub(super) async fn update_user_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<UserPromptUpdate>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.save_user_prompt(body.user_prompt.as_deref()).await {
        Ok(settings) => Json(settings).into_response(),
        Err(e) if e.contains("8000") || e.contains("不能为空") => bad_request(&e),
        Err(_) => error("无法保存出题要求"),
    }
}

#[derive(Serialize)]
struct PrepareBody {
    job_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ComprehensivePrepareRequest {}
pub(super) async fn prepare(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(_body): Json<ComprehensivePrepareRequest>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.prepare().await {
        Ok(job_id) => Json(PrepareBody { job_id }).into_response(),
        Err(reason) if reason.contains("正在执行") => error_json(
            StatusCode::CONFLICT,
            "study_prepare_busy",
            "已有出题任务正在执行，请等待当前任务完成",
        ),
        Err(reason) if reason.contains("考点") => bad_request(&reason),
        Err(_) => error("Codex 配置不可用，请检查 Codex 连接配置"),
    }
}
pub(super) async fn progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.job(&id).await {
        Some(result) => Json(result).into_response(),
        None => error_json(
            StatusCode::NOT_FOUND,
            "study_prepare_not_found",
            "出题任务不存在",
        ),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ControlBody {
    control_token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PositionBody {
    control_token: String,
    question: usize,
}
pub(super) async fn unfinished(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.unfinished().await {
        Ok(items) => Json(items).into_response(),
        Err(_) => error("无法读取未完成训练"),
    }
}
pub(super) async fn claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.claim(&id).await {
        Ok(claimed) => Json(claimed).into_response(),
        Err(_) => bad_request("训练已交卷或不存在，无法接管"),
    }
}
pub(super) async fn position(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<PositionBody>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service
        .save_position(&id, body.question, &body.control_token)
        .await
    {
        Ok(()) => Json(serde_json::json!({"saved":true})).into_response(),
        Err(_) => error_json(
            StatusCode::CONFLICT,
            "study_control_changed",
            "其他设备已接管本次训练，无法保存题号",
        ),
    }
}
pub(super) async fn run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.run(&id).await {
        Ok(run) => Json(run).into_response(),
        Err(_) => bad_request("训练不存在或无法读取"),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AnswerBody {
    choice: usize,
    control_token: String,
}
pub(super) async fn answer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, number)): Path<(String, usize)>,
    Json(body): Json<AnswerBody>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service
        .answer_controlled(&id, number, body.choice, Some(&body.control_token))
        .await
    {
        Ok(()) => Json(serde_json::json!({"saved":true})).into_response(),
        Err(_) => bad_request("题号或答题状态无效，可能已到20分钟截止时间"),
    }
}
pub(super) async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ControlBody>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service
        .submit_controlled(&id, Some(&body.control_token))
        .await
    {
        Ok(result) => Json(result).into_response(),
        Err(_) => bad_request("无法交卷或查询训练结果"),
    }
}

#[derive(Serialize)]
struct RetryBody {
    run_id: String,
}
pub(super) async fn retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.retry(&id).await {
        Ok(run_id) => Json(RetryBody { run_id }).into_response(),
        Err(_) => bad_request("只能重做已交卷且题目完整的历史试卷"),
    }
}
pub(super) async fn comprehensive_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.history().await {
        Ok(history) => Json(history).into_response(),
        Err(_) => error("综合训练历史读取失败"),
    }
}

pub(super) async fn comprehensive_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service.result(&id).await {
        Ok(result) => Json(result).into_response(),
        Err(_) => bad_request("训练尚未交卷或记录不存在"),
    }
}

pub(super) async fn pause_toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ControlBody>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Some(service) = get(&state) else {
        return unavailable();
    };
    match service
        .pause_toggle_controlled(&id, Some(&body.control_token))
        .await
    {
        Ok(run) => Json(run).into_response(),
        Err(_) => bad_request("暂停/继续失败：训练已截止或状态变化"),
    }
}
