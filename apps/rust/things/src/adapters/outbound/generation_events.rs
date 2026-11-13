//! Ephemeral Codex execution activity for the three study generators.
//! Events live only while a prepared paper has not been opened. Never persist
//! prompts, tool output, or agent responses to SQLite.
use crate::application::question_bank::practice::{AiReviewEventSink, AiReviewProgress};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
pub struct GenerationEvent {
    pub seq: u64,
    pub kind: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct GenerationSnapshot {
    pub job_id: String,
    pub status: String,
    pub run_id: Option<String>,
    pub seq: u64,
    pub events: Vec<GenerationEvent>,
}

#[derive(Clone)]
pub struct GenerationPush {
    pub module: &'static str,
    pub job_id: String,
    pub event: GenerationEvent,
}

struct Hub {
    current: Mutex<HashMap<&'static str, GenerationSnapshot>>,
    sender: broadcast::Sender<GenerationPush>,
}

fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(|| Hub {
        current: Mutex::new(HashMap::new()),
        sender: broadcast::channel(256).0,
    })
}

pub fn start(module: &'static str, job_id: &str) {
    let mut current = hub().current.lock().unwrap_or_else(|e| e.into_inner());
    current.insert(
        module,
        GenerationSnapshot {
            job_id: job_id.to_owned(),
            status: "running".into(),
            run_id: None,
            seq: 0,
            events: Vec::new(),
        },
    );
}

fn emit(module: &'static str, job_id: &str, kind: &str, payload: Value) {
    let mut current = hub().current.lock().unwrap_or_else(|e| e.into_inner());
    let Some(snapshot) = current.get_mut(module).filter(|r| r.job_id == job_id) else {
        return;
    };
    snapshot.seq += 1;
    let event = GenerationEvent {
        seq: snapshot.seq,
        kind: kind.to_owned(),
        payload,
    };
    snapshot.events.push(event.clone());
    let _ = hub().sender.send(GenerationPush {
        module,
        job_id: job_id.to_owned(),
        event,
    });
}

/// Maps only real Codex tool activity. Do not stream agentMessage JSON:
/// it contains unseen questions, answer keys and explanations.
pub fn sink(module: &'static str, job_id: String, label: String) -> AiReviewEventSink {
    std::sync::Arc::new(move |progress| {
        let job_id = job_id.clone();
        let label = label.clone();
        Box::pin(async move {
            let tagged = |mut details: Value| {
                if let Some(object) = details.as_object_mut() {
                    object.insert("source".into(), Value::String(label.clone()));
                }
                details
            };
            let mapped = match progress {
                AiReviewProgress::ToolStarted {
                    item_id,
                    tool,
                    details,
                } => Some((
                    "tool_started",
                    json!({"item_id":format!("{label}:{item_id}"),"tool":tool,"details":tagged(details)}),
                )),
                AiReviewProgress::ToolFinished {
                    item_id,
                    tool,
                    status,
                    details,
                } => Some((
                    "tool_finished",
                    json!({"item_id":format!("{label}:{item_id}"),"tool":tool,"status":status,"details":tagged(details)}),
                )),
                AiReviewProgress::CommandOutputDelta { item_id, text } => Some((
                    "command_output_delta",
                    json!({"item_id":format!("{label}:{item_id}"),"text":text}),
                )),
                AiReviewProgress::ToolOutputDelta {
                    item_id,
                    tool,
                    text,
                } => Some((
                    "tool_output_delta",
                    json!({"item_id":format!("{label}:{item_id}"),"tool":tool,"text":text}),
                )),
                AiReviewProgress::ToolProgress {
                    item_id,
                    tool,
                    message,
                } => Some((
                    "tool_progress",
                    json!({"item_id":format!("{label}:{item_id}"),"tool":tool,"message":message}),
                )),
                // Reasoning and final JSON are not an execution log and may reveal answers.
                _ => None,
            };
            if let Some((kind, payload)) = mapped {
                emit(module, &job_id, kind, payload);
            }
            Ok(())
        })
    })
}

pub fn phase(module: &'static str, job_id: &str, message: &str) {
    emit(module, job_id, "phase_changed", json!({"message":message}));
}

pub fn finish(module: &'static str, job_id: &str, run_id: Option<&str>, success: bool) {
    let mut current = hub().current.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(snapshot) = current.get_mut(module).filter(|r| r.job_id == job_id) {
        snapshot.status = if success { "ready" } else { "failed" }.into();
        snapshot.run_id = run_id.map(str::to_owned);
        snapshot.seq += 1;
        let event = GenerationEvent {
            seq: snapshot.seq,
            kind: if success {
                "run_finished"
            } else {
                "run_failed"
            }
            .into(),
            payload: json!({}),
        };
        snapshot.events.push(event.clone());
        let _ = hub().sender.send(GenerationPush {
            module,
            job_id: job_id.to_owned(),
            event,
        });
    }
}

/// Clear only the matching paper. Opening another already-prepared paper
/// must not remove the currently running job's activity.
pub fn clear_run(module: &'static str, run_id: &str) {
    let mut current = hub().current.lock().unwrap_or_else(|e| e.into_inner());
    if current.get(module).and_then(|s| s.run_id.as_deref()) == Some(run_id) {
        current.remove(module);
    }
}

pub fn subscribe(
    module: &'static str,
    job_id: &str,
) -> Option<(GenerationSnapshot, broadcast::Receiver<GenerationPush>)> {
    let current = hub().current.lock().unwrap_or_else(|e| e.into_inner());
    let receiver = hub().sender.subscribe();
    current
        .get(module)
        .filter(|r| r.job_id == job_id)
        .cloned()
        .map(|snapshot| (snapshot, receiver))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ephemeral_job_is_isolated_and_dropped_on_open() {
        let module = "test-generation-isolation";
        let first = uuid::Uuid::now_v7().to_string();
        let second = uuid::Uuid::now_v7().to_string();
        start(module, &first);
        phase(module, &first, "正在选卡");
        start(module, &second);
        phase(module, &first, "旧任务迟到事件");
        assert!(subscribe(module, &first).is_none());
        let (snapshot, _) = subscribe(module, &second).unwrap();
        assert!(snapshot.events.is_empty());
        let sink = sink(module, second.clone(), "第1篇".into());
        sink(AiReviewProgress::CommandOutputDelta {
            item_id: "x".into(),
            text: "abc".into(),
        })
        .await
        .unwrap();
        let (snapshot, _) = subscribe(module, &second).unwrap();
        assert_eq!(snapshot.events.len(), 1);
        assert_eq!(snapshot.events[0].payload["text"], "abc");
        finish(module, &second, Some("paper-b"), true);
        clear_run(module, "paper-a");
        assert!(subscribe(module, &second).is_some());
        clear_run(module, "paper-b");
        assert!(subscribe(module, &second).is_none());
    }

    #[tokio::test]
    async fn final_answer_is_not_exposed_and_parallel_tools_are_distinct() {
        let module = "test-generation-privacy";
        let id = uuid::Uuid::now_v7().to_string();
        start(module, &id);
        let first = sink(module, id.clone(), "材料1".into());
        let second = sink(module, id.clone(), "材料2".into());
        first(AiReviewProgress::TextDelta {
            item_id: "out".into(),
            text: "correct_index: 0".into(),
        })
        .await
        .unwrap();
        first(AiReviewProgress::ToolStarted {
            item_id: "1".into(),
            tool: "commandExecution".into(),
            details: json!({"command":"pwd"}),
        })
        .await
        .unwrap();
        second(AiReviewProgress::ToolStarted {
            item_id: "1".into(),
            tool: "commandExecution".into(),
            details: json!({"command":"ls"}),
        })
        .await
        .unwrap();
        let (snapshot, _) = subscribe(module, &id).unwrap();
        assert_eq!(snapshot.events.len(), 2);
        assert_ne!(
            snapshot.events[0].payload["item_id"],
            snapshot.events[1].payload["item_id"]
        );
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("correct_index"));
    }
}
