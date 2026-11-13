//! Reuse the AI review tool-activity presentation for the current study job.
//! Live-only: no history controls and no generated question/answer JSON.
use super::*;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct GenerationEvent {
    seq: u64,
    kind: String,
    payload: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
struct GenerationSnapshot {
    job_id: String,
    seq: u64,
    status: String,
    events: Vec<GenerationEvent>,
}

#[derive(Properties, PartialEq)]
pub(super) struct GenerationActivityProps {
    pub module: &'static str,
    pub job_id: String,
    pub test_prefix: &'static str,
}

#[derive(Default)]
struct ToolActivity {
    id: String,
    tool: String,
    command: String,
    output: String,
    source: String,
    status: String,
}

#[function_component(GenerationActivity)]
pub(super) fn generation_activity(props: &GenerationActivityProps) -> Html {
    let snapshot = use_state(|| None::<GenerationSnapshot>);
    let expanded = use_state(|| true);
    let disconnected = use_state(|| false);
    {
        let snapshot = snapshot.clone();
        let disconnected = disconnected.clone();
        use_effect_with(
            (props.module, props.job_id.clone()),
            move |(module, job_id)| {
                let current = Rc::new(RefCell::new(None::<GenerationSnapshot>));
                snapshot.set(None);
                disconnected.set(false);
                let source = web_sys::EventSource::new(&format!(
                    "/api/v1/study/{module}/prepare/{job_id}/events"
                ))
                .ok();
                let mut handlers: Vec<Closure<dyn FnMut(web_sys::MessageEvent)>> = Vec::new();
                let mut errors: Vec<Closure<dyn FnMut(web_sys::Event)>> = Vec::new();
                if let Some(source) = &source {
                    let snapshot_current = current.clone();
                    let snapshot_state = snapshot.clone();
                    let snapshot_disconnected = disconnected.clone();
                    let handler = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                        let Some(raw) = event.data().as_string() else {
                            return;
                        };
                        let Ok(value) = serde_json::from_str::<GenerationSnapshot>(&raw) else {
                            return;
                        };
                        *snapshot_current.borrow_mut() = Some(value.clone());
                        snapshot_state.set(Some(value));
                        snapshot_disconnected.set(false);
                    }) as Box<dyn FnMut(_)>);
                    let _ = source.add_event_listener_with_callback(
                        "snapshot",
                        handler.as_ref().unchecked_ref(),
                    );
                    handlers.push(handler);

                    let event_current = current.clone();
                    let event_state = snapshot.clone();
                    let handler = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                        let Some(raw) = event.data().as_string() else {
                            return;
                        };
                        let Ok(value) = serde_json::from_str::<GenerationEvent>(&raw) else {
                            return;
                        };
                        let mut state = event_current.borrow_mut();
                        let Some(data) = state.as_mut() else {
                            return;
                        };
                        if value.seq <= data.seq {
                            return;
                        }
                        // The server sends a complete snapshot after a transport gap.
                        if value.seq != data.seq + 1 {
                            return;
                        }
                        data.seq = value.seq;
                        if value.kind == "run_finished" {
                            data.status = "ready".into();
                        }
                        if value.kind == "run_failed" {
                            data.status = "failed".into();
                        }
                        data.events.push(value);
                        event_state.set(Some(data.clone()));
                    }) as Box<dyn FnMut(_)>);
                    let _ = source.add_event_listener_with_callback(
                        "activity",
                        handler.as_ref().unchecked_ref(),
                    );
                    handlers.push(handler);

                    let error_disconnected = disconnected.clone();
                    let handler = Closure::wrap(Box::new(move |_: web_sys::Event| {
                        error_disconnected.set(true)
                    }) as Box<dyn FnMut(_)>);
                    source.set_onerror(Some(handler.as_ref().unchecked_ref()));
                    errors.push(handler);
                } else {
                    disconnected.set(true);
                }
                move || {
                    if let Some(source) = &source {
                        source.close();
                        source.set_onerror(None);
                    }
                    drop(handlers);
                    drop(errors);
                }
            },
        );
    }
    let mut tools = Vec::<ToolActivity>::new();
    let mut phases = Vec::<String>::new();
    if let Some(snapshot) = snapshot.as_ref() {
        for event in &snapshot.events {
            let item_id = event
                .payload
                .get("item_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match event.kind.as_str() {
                "phase_changed" => {
                    if let Some(text) = event.payload.get("message").and_then(|v| v.as_str()) {
                        phases.push(text.to_owned());
                    }
                }
                "tool_started" => tools.push(ToolActivity {
                    id: item_id.to_owned(),
                    tool: event
                        .payload
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("工具")
                        .into(),
                    command: event
                        .payload
                        .pointer("/details/command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                    source: event
                        .payload
                        .pointer("/details/source")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                    ..Default::default()
                }),
                "command_output_delta" | "tool_output_delta" | "tool_progress" => {
                    if let Some(item) = tools.iter_mut().rev().find(|tool| tool.id == item_id) {
                        let text = event
                            .payload
                            .get(if event.kind == "tool_progress" {
                                "message"
                            } else {
                                "text"
                            })
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        item.output.push_str(text);
                        if event.kind == "tool_progress" {
                            item.output.push('\n');
                        }
                    }
                }
                "tool_finished" => {
                    if let Some(item) = tools.iter_mut().rev().find(|tool| tool.id == item_id) {
                        item.status = event
                            .payload
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("completed")
                            .into();
                    }
                }
                _ => (),
            }
        }
    }
    let state = snapshot
        .as_ref()
        .map(|s| s.status.as_str())
        .unwrap_or("running");
    let description = match state {
        "ready" => "本轮生成完成",
        "failed" => "本轮生成失败",
        _ => "Codex 正在执行",
    };
    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    html! {
        <section class="rounded-box border border-base-content/15 bg-base-100/50 p-3 space-y-2"
            data-testid={format!("{}-activity",props.test_prefix)}>
            <button type="button" class="flex w-full items-center justify-between text-left"
                onclick={toggle} aria-expanded={expanded.to_string()}>
                <span class="font-semibold text-sm">{format!("AI 执行过程 · {description}")}</span>
                <span class="text-xs text-base-content/60">{if *expanded {"收起 ⌃"} else {"展开 ⌄"}}</span>
            </button>
            if *expanded {
                <div class="max-h-80 overflow-y-auto space-y-3 border-l border-base-content/15 pl-3 text-xs text-base-content/70"
                    data-testid={format!("{}-activity-events",props.test_prefix)}>
                    {for phases.iter().map(|phase| html! { <p>{phase.clone()}</p> })}
                    {for tools.iter().map(|tool| html! {
                        <div class="space-y-1" data-testid={format!("{}-activity-tool",props.test_prefix)}>
                            <p class="font-medium">{format!("{} · {}{}",if tool.source.is_empty() {"Codex"} else {&tool.source},tool.tool,if tool.status.is_empty() {""} else {" · 已结束"})}</p>
                            if !tool.command.is_empty() {
                                <pre class="whitespace-pre-wrap break-all rounded bg-base-200 p-2">{tool.command.clone()}</pre>
                            }
                            if !tool.output.is_empty() {
                                <pre class="whitespace-pre-wrap break-all rounded bg-base-200 p-2">{tool.output.clone()}</pre>
                            }
                        </div>
                    })}
                    if snapshot.as_ref().is_none_or(|s| s.events.is_empty()) {
                        <p>{"等待 Codex 工具执行事件…"}</p>
                    }
                    if *disconnected && state == "running" {
                        <p role="status">{"执行流连接中断，正在重新连接…"}</p>
                    }
                </div>
            }
        </section>
    }
}
