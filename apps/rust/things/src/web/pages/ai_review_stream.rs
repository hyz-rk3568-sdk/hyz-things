use super::*;
use hyz_things::domain::question_bank::AiReviewState;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(super) struct StreamEvent {
    version: u8,
    run_id: String,
    seq: u64,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(super) struct StreamSnapshot {
    version: u8,
    question_id: String,
    pub stem_fingerprint: String,
    pub run_id: Option<String>,
    pub seq: u64,
    pub status: String,
    pub text: String,
    pub progress: Vec<StreamEvent>,
}

#[hook]
pub(super) fn use_ai_review_stream(
    question_id: String,
    endpoint: String,
    history: UseStateHandle<Option<AiReviewState>>,
    revision: Rc<RefCell<u64>>,
) -> (
    UseStateHandle<Option<StreamSnapshot>>,
    UseStateHandle<bool>,
    Callback<()>,
) {
    let snapshot = use_state(|| None::<StreamSnapshot>);
    let disconnected = use_state(|| false);
    let reconnect = use_state(|| 0u64);
    let reconnect_count = use_mut_ref(|| 0u64);
    let reconnect_now = {
        let reconnect = reconnect.clone();
        let reconnect_count = reconnect_count.clone();
        Callback::from(move |()| {
            *reconnect_count.borrow_mut() += 1;
            reconnect.set(*reconnect_count.borrow());
        })
    };
    {
        let snapshot = snapshot.clone();
        let disconnected = disconnected.clone();
        let reconnect_now = reconnect_now.clone();
        use_effect_with(
            (question_id, endpoint, *reconnect),
            move |(question_id, endpoint, _)| {
                let alive = Rc::new(Cell::new(true));
                let cursor = Rc::new(RefCell::new(None::<StreamSnapshot>));
                let source = web_sys::EventSource::new(&format!("{endpoint}/events"));
                let mut handlers: Vec<Closure<dyn FnMut(web_sys::MessageEvent)>> = Vec::new();
                let mut error_handler: Option<Closure<dyn FnMut(Event)>> = None;
                if let Ok(source) = &source {
                    let refresh: Rc<dyn Fn()> = {
                        let history = history.clone();
                        let endpoint = endpoint.clone();
                        let revision = revision.clone();
                        let alive = alive.clone();
                        Rc::new(move || {
                            let history = history.clone();
                            let endpoint = endpoint.clone();
                            let revision = revision.clone();
                            let alive = alive.clone();
                            *revision.borrow_mut() += 1;
                            let request_revision = *revision.borrow();
                            spawn_local(async move {
                                if let Ok(value) =
                                    fetch_json::<AiReviewState>(&endpoint, "AI 复盘记录").await
                                {
                                    if alive.get() && *revision.borrow() == request_revision {
                                        history.set(Some(value));
                                    }
                                }
                            });
                        })
                    };
                    let on_snapshot = {
                        let cursor = cursor.clone();
                        let snapshot = snapshot.clone();
                        let disconnected = disconnected.clone();
                        let question_id = question_id.clone();
                        let refresh = refresh.clone();
                        Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                            let Some(data) = event.data().as_string() else {
                                return;
                            };
                            let Ok(value) = serde_json::from_str::<StreamSnapshot>(&data) else {
                                return;
                            };
                            if value.version != 1 || value.question_id != question_id {
                                return;
                            }
                            *cursor.borrow_mut() = Some(value.clone());
                            snapshot.set(Some(value));
                            disconnected.set(false);
                            refresh();
                        }) as Box<dyn FnMut(_)>)
                    };
                    let _ = source.add_event_listener_with_callback(
                        "snapshot",
                        on_snapshot.as_ref().unchecked_ref(),
                    );
                    handlers.push(on_snapshot);
                    let on_message = {
                        let cursor = cursor.clone();
                        let snapshot = snapshot.clone();
                        let reconnect_now = reconnect_now.clone();
                        let source = source.clone();
                        let refresh = refresh.clone();
                        Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                            let Some(data) = event.data().as_string() else {
                                return;
                            };
                            let Ok(event) = serde_json::from_str::<StreamEvent>(&data) else {
                                return;
                            };
                            if event.version != 1 {
                                return;
                            }
                            let mut current = cursor.borrow_mut();
                            let Some(value) = current.as_mut() else {
                                source.close();
                                reconnect_now.emit(());
                                return;
                            };
                            if value.run_id.as_deref() != Some(event.run_id.as_str())
                                || event.seq > value.seq + 1
                            {
                                source.close();
                                reconnect_now.emit(());
                                return;
                            }
                            if event.seq <= value.seq {
                                return;
                            }
                            value.seq = event.seq;
                            match event.kind.as_str() {
                                "text_delta" => {
                                    if let Some(text) =
                                        event.payload.get("text").and_then(|v| v.as_str())
                                    {
                                        value.text.push_str(text);
                                    }
                                }
                                "answer_final" => {
                                    if let Some(text) =
                                        event.payload.get("text").and_then(|v| v.as_str())
                                    {
                                        value.text = text.to_owned();
                                    }
                                }
                                "tool_started"
                                | "tool_finished"
                                | "reasoning_summary_delta"
                                | "phase_changed" => value.progress.push(event.clone()),
                                "run_finished" => value.status = "complete".into(),
                                "run_failed" => value.status = "failed".into(),
                                "run_stopped" => value.status = "stopped".into(),
                                _ => (),
                            }
                            let terminal = matches!(
                                event.kind.as_str(),
                                "run_finished" | "run_failed" | "run_stopped"
                            );
                            snapshot.set(Some(value.clone()));
                            if terminal {
                                refresh();
                            }
                        }) as Box<dyn FnMut(_)>)
                    };
                    source.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
                    handlers.push(on_message);
                    let on_error = {
                        let disconnected = disconnected.clone();
                        Closure::wrap(
                            Box::new(move |_: Event| disconnected.set(true)) as Box<dyn FnMut(_)>
                        )
                    };
                    source.set_onerror(Some(on_error.as_ref().unchecked_ref()));
                    error_handler = Some(on_error);
                } else {
                    disconnected.set(true);
                }
                move || {
                    alive.set(false);
                    if let Ok(source) = source {
                        source.close();
                        if let Some(on_snapshot) = handlers.first() {
                            let _ = source.remove_event_listener_with_callback(
                                "snapshot",
                                on_snapshot.as_ref().unchecked_ref(),
                            );
                        }
                        source.set_onmessage(None);
                        source.set_onerror(None);
                    }
                    drop(handlers);
                    drop(error_handler);
                }
            },
        );
    }
    (snapshot, disconnected, reconnect_now)
}
