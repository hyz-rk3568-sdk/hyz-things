//! Administrator-owned study clock HTTP API. Exam deadlines remain independent.
use super::*;
use crate::adapters::outbound::training_time::{StudyTimeEvent, TrainingTimeService};
use axum::extract::Query;
use chrono::{Duration, Utc};
use std::collections::HashMap;

async fn service(state: &AppState) -> Result<Arc<TrainingTimeService>,Response> {
    state.training_time.clone().ok_or_else(|| error_json(StatusCode::SERVICE_UNAVAILABLE,
        "study_time_unavailable","学习时间数据库不可用"))
}
fn failure(message: String) -> Response {
    error_json(StatusCode::BAD_REQUEST,"study_time_invalid_request",&message)
}
pub(super) async fn current(State(state):State<AppState>,headers:HeaderMap)->Response {
    if let Err(response)=authorize_sensitive_read(&state,&headers).await{return response;}
    let Ok(s)=service(&state).await else{return error_json(StatusCode::SERVICE_UNAVAILABLE,"study_time_unavailable","学习时间数据库不可用");};
    match s.current().await {
        Ok(result)=>Json(result).into_response(),
        Err(err)=>error_json(StatusCode::INTERNAL_SERVER_ERROR,"study_time_read_failed",&err),
    }
}
pub(super) async fn events(State(state):State<AppState>,headers:HeaderMap,body:Result<Json<StudyTimeEvent>,JsonRejection>)->Response {
    if let Err(response)=authorize_sensitive_control(&state,&headers).await{return response;}
    let Json(event)=match body {Ok(value)=>value,Err(_)=>return failure("无效的学习计时事件".into())};
    let Ok(s)=service(&state).await else{return error_json(StatusCode::SERVICE_UNAVAILABLE,"study_time_unavailable","学习时间数据库不可用");};
    match s.event(event).await {
        Ok(result)=>Json(result).into_response(),
        Err(err)=>failure(err),
    }
}
pub(super) async fn stats(State(state):State<AppState>,headers:HeaderMap,Query(params):Query<HashMap<String,String>>)->Response {
    if let Err(response)=authorize_sensitive_read(&state,&headers).await{return response;}
    let Ok(s)=service(&state).await else{return error_json(StatusCode::SERVICE_UNAVAILABLE,"study_time_unavailable","学习时间数据库不可用");};
    let today=Utc::now().with_timezone(&chrono::FixedOffset::east_opt(8*3600).unwrap()).date_naive();
    let default_from=(today-Duration::days(34)).to_string();
    let from=params.get("from").map(String::as_str).unwrap_or(&default_from);
    let default_to=today.to_string();
    let to=params.get("to").map(String::as_str).unwrap_or(&default_to);
    match s.statistics(from,to).await {
        Ok(result)=>Json(result).into_response(),
        Err(err)=>failure(err),
    }
}
