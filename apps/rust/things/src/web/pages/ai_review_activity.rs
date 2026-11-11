use super::ai_review_stream::{StreamEvent, StreamSnapshot};
use super::*;
use hyz_things::domain::question_bank::AiReviewMessage;

#[derive(Clone, PartialEq, serde::Deserialize)]
struct HistoryRun {
    run_id: String,
    status: String,
    #[serde(default)]
    assistant_message_id: Option<i64>,
}

#[derive(serde::Deserialize)]
struct HistoryPage<T> {
    items: Vec<T>,
    next_after: Option<i64>,
    has_more: bool,
}

#[derive(Properties, PartialEq)]
struct RunActivityProps {
    endpoint: String,
    run: HistoryRun,
    index: usize,
    stream: Option<StreamSnapshot>,
}

#[function_component(RunActivity)]
fn run_activity(props: &RunActivityProps) -> Html {
    let opened = use_state(|| false);
    let events = use_state(Vec::<StreamEvent>::new);
    let after = use_state(|| 0i64);
    let has_more = use_state(|| true);
    let loading = use_state(|| false);
    let error = use_state(|| None::<String>);
    let load = {
        let endpoint = props.endpoint.clone();
        let run_id = props.run.run_id.clone();
        let events = events.clone();
        let after = after.clone();
        let has_more = has_more.clone();
        let loading = loading.clone();
        let error = error.clone();
        Callback::from(move |_| {
            if *loading || !*has_more {
                return;
            }
            loading.set(true);
            let url = format!("{endpoint}/runs/{run_id}/events?after={}&limit=50", *after);
            let events = events.clone();
            let after = after.clone();
            let has_more = has_more.clone();
            let loading = loading.clone();
            let error = error.clone();
            spawn_local(async move {
                match fetch_json::<HistoryPage<StreamEvent>>(&url, "AI 执行历史").await {
                    Ok(page) => {
                        if page.has_more && page.next_after.is_none_or(|next| next <= *after) {
                            error.set(Some("AI 执行历史分页游标无效".into()));
                        } else {
                            let mut all = (*events).clone();
                            all.extend(page.items);
                            events.set(all);
                            if let Some(next) = page.next_after {
                                after.set(next);
                            }
                            has_more.set(page.has_more);
                            error.set(None);
                        }
                    }
                    Err(message) => error.set(Some(message)),
                }
                loading.set(false);
            });
        })
    };
    let toggle = {
        let opened = opened.clone();
        let load = load.clone();
        Callback::from(move |_| {
            if !*opened {
                load.emit(());
            }
            opened.set(!*opened);
        })
    };
    let mut merged = (*events).clone();
    if let Some(stream) = props
        .stream
        .as_ref()
        .filter(|s| s.run_id.as_deref() == Some(&props.run.run_id))
    {
        for event in &stream.progress {
            if !merged.iter().any(|old| old.seq == event.seq) {
                merged.push(event.clone());
            }
        }
    }
    merged.sort_by_key(|event| event.seq);
    let mut tools: Vec<(String, String, String, String)> = Vec::new();
    for event in &merged {
        let id = event
            .payload
            .get("item_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        match event.kind.as_str() {
            "tool_started" => tools.push((
                id,
                event
                    .payload
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .unwrap_or("工具")
                    .to_owned(),
                event
                    .payload
                    .pointer("/details/command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned(),
                String::new(),
            )),
            "command_output_delta" | "tool_output_delta" => {
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.0 == id) {
                    tool.3.push_str(
                        event
                            .payload
                            .get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                    );
                }
            }
            "tool_progress" => {
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.0 == id) {
                    tool.3.push_str(
                        event
                            .payload
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                    );
                    tool.3.push('\n');
                }
            }
            "tool_finished" => {
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.0 == id) {
                    tool.1 = format!(
                        "{} · {}",
                        tool.1,
                        event
                            .payload
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("完成")
                    );
                }
            }
            _ => (),
        }
    }
    let status = match props.run.status.as_str() {
        "running" => "正在执行",
        "complete" => "已完成",
        "failed" => "失败",
        "stopped" => "已停止",
        _ => "执行记录",
    };
    html! {
        <div data-testid="question-bank-ai-progress" class="py-1">
            <button type="button" class="text-left text-xs text-base-content/60 hover:text-base-content" aria-expanded={opened.to_string()} onclick={toggle}>
                {if *opened {"⌄ "} else {"› "}}{format!("第 {} 轮 · {status}", props.index + 1)}
                {tools.last().map(|tool| format!(" · {}", if tool.2.is_empty() { &tool.1 } else { &tool.2 })).unwrap_or_default()}
            </button>
            if *opened {
                <div class="mt-2 space-y-3 border-l border-base-content/15 pl-3">
                    {for tools.iter().map(|(_, label, command, output)| html! {
                        <section data-testid="question-bank-ai-tool">
                            <p>{label.clone()}</p>
                            if !command.is_empty() { <pre class="mt-1 whitespace-pre-wrap break-all" data-testid="question-bank-ai-command">{command.clone()}</pre> }
                            if !output.is_empty() { <pre class="mt-1 whitespace-pre-wrap break-all" data-testid="question-bank-ai-output">{output.clone()}</pre> }
                        </section>
                    })}
                    if let Some(message) = error.as_ref() { <p role="alert" class="text-error">{message.clone()}</p> }
                    if *has_more {
                        <button type="button" class="btn btn-ghost btn-xs" disabled={*loading} onclick={Callback::from({let load=load.clone(); move |_| load.emit(())})}>{if *loading {"加载中…"} else if error.is_some() {"重试"} else {"加载更多输出"}}</button>
                    }
                </div>
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct AiReviewActivityProps {
    pub endpoint: String,
    pub stream: Option<StreamSnapshot>,
    pub messages: Vec<AiReviewMessage>,
}

#[function_component(AiReviewActivity)]
pub(super) fn ai_review_activity(props: &AiReviewActivityProps) -> Html {
    let runs = use_state(Vec::<HistoryRun>::new);
    let after = use_state(|| 0i64);
    let has_more = use_state(|| true);
    let loading = use_state(|| false);
    let error = use_state(|| None::<String>);
    let load = {
        let endpoint = props.endpoint.clone();
        let runs = runs.clone();
        let after = after.clone();
        let has_more = has_more.clone();
        let loading = loading.clone();
        let error = error.clone();
        Callback::from(move |_| {
            if *loading || !*has_more {
                return;
            }
            loading.set(true);
            let url = format!("{endpoint}/runs?after={}&limit=50", *after);
            let runs = runs.clone();
            let after = after.clone();
            let has_more = has_more.clone();
            let loading = loading.clone();
            let error = error.clone();
            spawn_local(async move {
                match fetch_json::<HistoryPage<HistoryRun>>(&url, "AI 执行历史").await {
                    Ok(page) => {
                        if page.has_more && page.next_after.is_none_or(|next| next <= *after) {
                            error.set(Some("AI 执行历史分页游标无效".into()));
                        } else {
                            let mut all = (*runs).clone();
                            all.extend(page.items);
                            runs.set(all);
                            if let Some(next) = page.next_after {
                                after.set(next);
                            }
                            has_more.set(page.has_more);
                            error.set(None);
                        }
                    }
                    Err(message) => error.set(Some(message)),
                }
                loading.set(false);
            });
        })
    };
    {
        let load = load.clone();
        use_effect_with(props.endpoint.clone(), move |_| {
            load.emit(());
            || ()
        });
    }
    let mut all = (*runs).clone();
    if let Some(stream) = &props.stream {
        if let Some(id) = &stream.run_id {
            if let Some(run) = all.iter_mut().find(|run| &run.run_id == id) {
                run.status = stream.status.clone();
            } else {
                all.push(HistoryRun {
                    run_id: id.clone(),
                    status: stream.status.clone(),
                    assistant_message_id: None,
                });
            }
        }
    }
    html! {
        <div data-testid="question-bank-ai-activity" aria-label="执行活动" class="space-y-2 text-xs text-base-content/60">
            <div class="space-y-3">
                {for props.messages.iter().enumerate().map(|(message_index, message)| html! {
                    <div key={format!("message-{message_index}")}>
                        {for all.iter().enumerate().filter(|(_, run)| run.assistant_message_id == Some(message.id) && message.id != 0).map(|(index, run)| html! {
                            <RunActivity key={run.run_id.clone()} endpoint={props.endpoint.clone()} run={run.clone()} {index} stream={props.stream.clone()}/>
                        })}
                        <div class={if message.role == "user" {"rounded-xl bg-primary/10 p-3"} else {"py-2"}} data-testid="question-bank-ai-message">
                            <div class="mb-2 text-xs font-semibold text-base-content/60">{if message.role == "user" {"你的追问"} else {"AI 解析"}}</div>
                            if message.role == "assistant" { <AiReply content={message.content.clone()}/> }
                            else { <div class="study-markdown text-base leading-7">{super::study_markdown::render_question_bank_user_markdown(&message.content)}</div> }
                        </div>
                    </div>
                })}
                {for all.iter().enumerate().filter(|(_, run)| run.assistant_message_id.is_none() || !props.messages.iter().any(|message| message.id != 0 && Some(message.id) == run.assistant_message_id)).map(|(index, run)| html! {
                    <RunActivity key={run.run_id.clone()} endpoint={props.endpoint.clone()} run={run.clone()} {index} stream={props.stream.clone()}/>
                })}
            </div>
            if let Some(message) = error.as_ref() { <p role="alert" class="text-error">{message.clone()}</p> }
            if *has_more && !*loading { <button type="button" class="btn btn-ghost btn-xs" onclick={Callback::from({let load=load.clone(); move |_| load.emit(())})}>{if error.is_some() {"重试"} else {"加载更多执行记录"}}</button> }
        </div>
    }
}
