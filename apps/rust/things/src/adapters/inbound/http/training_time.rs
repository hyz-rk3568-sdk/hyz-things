//! Administrator-owned study clock HTTP API. Exam deadlines remain independent.
use super::*;
use crate::application::{
    question_bank::QuestionBankApplication,
    training_time::{
        self as application, StatisticsError, StudyGoals, StudyTimeEvent, TrainingTimePort,
    },
};
use axum::extract::Query;
use axum::Extension;
use chrono::{Duration, Utc};
use std::collections::HashMap;

pub fn with_training_time(
    router: Router,
    application: Option<Arc<dyn TrainingTimePort>>,
) -> Router {
    match application {
        Some(application) => router.layer(Extension(application)),
        None => router,
    }
}

type Service = Option<Extension<Arc<dyn TrainingTimePort>>>;

#[expect(
    clippy::result_large_err,
    reason = "Axum response errors are returned directly"
)]
fn service(application: Service) -> Result<Arc<dyn TrainingTimePort>, Response> {
    application
        .map(|Extension(application)| application)
        .ok_or_else(|| {
            error_json(
                StatusCode::SERVICE_UNAVAILABLE,
                "study_time_unavailable",
                "学习时间数据库不可用",
            )
        })
}
fn failure(message: String) -> Response {
    error_json(
        StatusCode::BAD_REQUEST,
        "study_time_invalid_request",
        &message,
    )
}
pub(super) async fn current(
    State(state): State<AppState>,
    headers: HeaderMap,
    application: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Ok(s) = service(application) else {
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "study_time_unavailable",
            "学习时间数据库不可用",
        );
    };
    match s.current().await {
        Ok(result) => Json(result).into_response(),
        Err(err) => error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "study_time_read_failed",
            &err,
        ),
    }
}
pub(super) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    application: Service,
    body: Result<Json<StudyTimeEvent>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Json(event) = match body {
        Ok(value) => value,
        Err(_) => return failure("无效的学习计时事件".into()),
    };
    let Ok(s) = service(application) else {
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "study_time_unavailable",
            "学习时间数据库不可用",
        );
    };
    match s.event(event).await {
        Ok(result) => Json(result).into_response(),
        Err(err) => failure(err),
    }
}
pub(super) async fn goals(
    State(state): State<AppState>,
    headers: HeaderMap,
    bank: Option<Extension<Arc<QuestionBankApplication>>>,
    application: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Ok(s) = service(application) else {
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "study_time_unavailable",
            "学习时间数据库不可用",
        );
    };
    let Some(Extension(bank)) = bank else {
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_unavailable",
            "题库共享或索引暂不可用",
        );
    };
    let today = Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .date_naive();
    match application::goal_status(s.as_ref(), &bank, &today.to_string()).await {
        Ok(status) => Json(status).into_response(),
        Err(StatisticsError::QuestionBank(_)) => error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_unavailable",
            "题库共享或索引暂不可用",
        ),
        Err(StatisticsError::Time(err)) => error_json(
            StatusCode::INTERNAL_SERVER_ERROR,
            "study_goals_read_failed",
            &err,
        ),
    }
}

pub(super) async fn save_goals(
    State(state): State<AppState>,
    headers: HeaderMap,
    application: Service,
    body: Result<Json<StudyGoals>, JsonRejection>,
) -> Response {
    if let Err(response) = authorize_sensitive_control(&state, &headers).await {
        return response;
    }
    let Json(goals) = match body {
        Ok(value) => value,
        Err(_) => return failure("无效的学习目标".into()),
    };
    let Ok(s) = service(application) else {
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "study_time_unavailable",
            "学习时间数据库不可用",
        );
    };
    match s.save_goals(goals).await {
        Ok(result) => Json(result).into_response(),
        Err(err) => failure(err),
    }
}

pub(super) async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    bank: Option<Extension<Arc<QuestionBankApplication>>>,
    application: Service,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let Ok(s) = service(application) else {
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "study_time_unavailable",
            "学习时间数据库不可用",
        );
    };
    let today = Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .date_naive();
    let default_from = (today - Duration::days(34)).to_string();
    let from = params
        .get("from")
        .map(String::as_str)
        .unwrap_or(&default_from);
    let default_to = today.to_string();
    let to = params.get("to").map(String::as_str).unwrap_or(&default_to);
    let Some(Extension(bank)) = bank else {
        return error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_unavailable",
            "题库共享或索引暂不可用",
        );
    };
    match application::statistics(s.as_ref(), bank.as_ref(), from, to).await {
        Ok(result) => Json(result).into_response(),
        Err(StatisticsError::Time(err)) => failure(err),
        Err(StatisticsError::QuestionBank(_)) => error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            "question_bank_unavailable",
            "题库共享或索引暂不可用",
        ),
    }
}
