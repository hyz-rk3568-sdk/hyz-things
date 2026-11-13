//! Read-only, authenticated SSE for the current study preparation job.
//! No history endpoint or SQLite event persistence.
use super::*;
use crate::adapters::outbound::generation_events::{self, GenerationPush, GenerationSnapshot};
use axum::response::sse::{Event, Sse};
use std::convert::Infallible;

struct Connection {
    state: AppState,
    headers: HeaderMap,
    module: &'static str,
    job_id: String,
    receiver: tokio::sync::broadcast::Receiver<GenerationPush>,
    snapshot: Option<GenerationSnapshot>,
    seq: u64,
    next_authorization: tokio::time::Instant,
}

pub(super) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((module, id)): Path<(String, String)>,
) -> Response {
    if let Err(response) = authorize_sensitive_read(&state, &headers).await {
        return response;
    }
    let module = match module.as_str() {
        "data-analysis" => "data-analysis",
        "logic-judgment" => "logic-judgment",
        "quantitative-relations" => "quantitative-relations",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    if uuid::Uuid::parse_str(&id).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some((snapshot, receiver)) = generation_events::subscribe(module, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let connection = Connection {
        state,
        headers,
        module,
        job_id: id,
        receiver,
        snapshot: Some(snapshot),
        seq: 0,
        next_authorization: tokio::time::Instant::now() + Duration::from_secs(30),
    };
    let stream = futures_util::stream::unfold(connection, |mut c| async move {
        loop {
            if tokio::time::Instant::now() >= c.next_authorization {
                if authorize_sensitive_read(&c.state, &c.headers).await.is_err() {
                    return None;
                }
                c.next_authorization = tokio::time::Instant::now() + Duration::from_secs(30);
            }
            if let Some(snapshot) = c.snapshot.take() {
                c.seq = snapshot.seq;
                let event = Event::default().event("snapshot")
                    .data(serde_json::to_string(&snapshot).ok()?);
                return Some((Ok::<_, Infallible>(event), c));
            }
            let received = tokio::select! {
                event = c.receiver.recv() => event,
                _ = tokio::time::sleep_until(c.next_authorization) => continue,
            };
            match received {
                Ok(push) if push.module == c.module && push.job_id == c.job_id => {
                    if push.event.seq <= c.seq { continue; }
                    if push.event.seq != c.seq + 1 {
                        let (snapshot, receiver) = generation_events::subscribe(c.module, &c.job_id)?;
                        c.snapshot = Some(snapshot);
                        c.receiver = receiver;
                        continue;
                    }
                    c.seq = push.event.seq;
                    let event = Event::default().event("activity")
                        .data(serde_json::to_string(&push.event).ok()?);
                    return Some((Ok(event), c));
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let (snapshot, receiver) = generation_events::subscribe(c.module, &c.job_id)?;
                    c.snapshot = Some(snapshot);
                    c.receiver = receiver;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    let mut response = Sse::new(stream).into_response();
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}
