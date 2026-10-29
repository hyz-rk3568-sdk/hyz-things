//! Administrator-only logic judgment HTTP adapter.
use super::*;

pub(super) const MAX_USER_PROMPT_BODY_BYTES: usize = 16 * 1024;
const MAX_USER_PROMPT_BYTES: usize = 8000;
use crate::application::logic_judgment::{LogicJudgmentError, LogicJudgmentOperation as Operation};

async fn invoke_authorized(state: &AppState, operation: Operation) -> Response {
    let id = match &operation {
        Operation::Job(id)
        | Operation::Run(id)
        | Operation::Start(id)
        | Operation::Pause(id)
        | Operation::Answer { id, .. }
        | Operation::Submit(id)
        | Operation::Result(id)
        | Operation::Retry(id) => Some(id),
        _ => None,
    };
    if id.is_some_and(|id| uuid::Uuid::parse_str(id).is_err()) {
        return failure(LogicJudgmentError::InvalidRequest);
    }
    let Some(service) = &state.logic_judgment else {
        return failure(LogicJudgmentError::Unavailable);
    };
    match service.execute(operation).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => failure(error),
    }
}

fn typed_body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, StatusCode> {
    body.map(|Json(body)| body).map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        }
    })
}

fn body_error(status: StatusCode) -> Response {
    if status == StatusCode::PAYLOAD_TOO_LARGE {
        error_json(
            status,
            "study_logic_judgment_body_too_large",
            "训练请求过大",
        )
    } else {
        failure(LogicJudgmentError::InvalidRequest)
    }
}

fn failure(error: LogicJudgmentError) -> Response {
    let (status, code, message) = match error {
        LogicJudgmentError::InvalidRequest => (
            StatusCode::BAD_REQUEST,
            "study_logic_judgment_invalid_request",
            "训练请求或状态无效",
        ),
        LogicJudgmentError::NotFound => (
            StatusCode::NOT_FOUND,
            "study_logic_judgment_not_found",
            "训练记录不存在",
        ),
        LogicJudgmentError::Busy => (
            StatusCode::CONFLICT,
            "study_logic_judgment_busy",
            "已有出题任务正在执行",
        ),
        LogicJudgmentError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "study_logic_judgment_unavailable",
            "逻辑判断训练暂不可用",
        ),
    };
    error_json(status, code, message)
}
pub(super) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::Status).await
}
pub(super) async fn unfinished(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::Unfinished).await
}
pub(super) async fn history(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::History).await
}
pub(super) async fn card_performance(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::CardPerformance).await
}
pub(super) async fn user_prompt(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::UserPrompt).await
}
pub(super) async fn progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::Job(id)).await
}
pub(super) async fn run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::Run(id)).await
}
pub(super) async fn result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    invoke_authorized(&state, Operation::Result(id)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyBody {}
pub(super) async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<EmptyBody>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    if let Err(status) = typed_body(body) {
        return body_error(status);
    }
    invoke_authorized(&state, Operation::Start(id)).await
}
pub(super) async fn pause(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<EmptyBody>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers) {
        return response;
    }
    if let Err(status) = typed_body(body) {
        return body_error(status);
    }
    invoke_authorized(&state, Operation::Pause(id)).await
}

pub(super) async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<EmptyBody>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    if let Err(status) = typed_body(body) {
        return body_error(status);
    }
    invoke_authorized(&state, Operation::Submit(id)).await
}
pub(super) async fn retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<EmptyBody>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    if let Err(status) = typed_body(body) {
        return body_error(status);
    }
    invoke_authorized(&state, Operation::Retry(id)).await
}

pub(super) async fn prepare(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<EmptyBody>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    if let Err(status) = typed_body(body) {
        return body_error(status);
    }
    invoke_authorized(&state, Operation::Prepare).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AnswerBody {
    question: usize,
    choice: usize,
}
pub(super) async fn answer(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<AnswerBody>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let body = match typed_body(body) {
        Ok(body) => body,
        Err(status) => return body_error(status),
    };
    if body.question >= 10 || body.choice >= 4 {
        return failure(LogicJudgmentError::InvalidRequest);
    }
    invoke_authorized(
        &state,
        Operation::Answer {
            id,
            question: body.question,
            choice: body.choice,
        },
    )
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UserPromptBody {
    user_prompt: Option<String>,
}
pub(super) async fn save_user_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<UserPromptBody>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let body = match typed_body(body) {
        Ok(body) => body,
        Err(status) => return body_error(status),
    };
    if body
        .user_prompt
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > MAX_USER_PROMPT_BYTES)
    {
        return failure(LogicJudgmentError::InvalidRequest);
    }
    invoke_authorized(&state, Operation::SaveUserPrompt(body.user_prompt)).await
}
