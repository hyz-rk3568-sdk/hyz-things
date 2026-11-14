use super::ai_review_stream::{StreamEvent, StreamSnapshot};
use super::*;
use hyz_things::domain::question_bank::AiReviewMessage;

/// Shared command/tool rendering for saved AI analysis and ephemeral question
/// generation. Keep typography, indentation and output layout identical.
pub(super) fn tool_execution_item(
    label: &str,
    command: &str,
    output: &str,
    test_prefix: &str,
) -> Html {
    html! {
        <section data-testid={format!("{test_prefix}-tool")} class="min-w-0 max-w-full">
            <p>{label.to_owned()}</p>
            if !command.is_empty() {
                <pre class="mt-1 min-w-0 max-w-full whitespace-pre-wrap break-all" data-testid={format!("{test_prefix}-command")}>{command.to_owned()}</pre>
            }
            if !output.is_empty() {
                <pre class="mt-1 min-w-0 max-w-full whitespace-pre-wrap break-all" data-testid={format!("{test_prefix}-output")}>{output.to_owned()}</pre>
            }
        </section>
    }
}

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
    scope: &'static str,
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
    let in_flight = use_mut_ref(|| false);
    let load = {
        let endpoint = props.endpoint.clone();
        let scope = props.scope;
        let run_id = props.run.run_id.clone();
        let events = events.clone();
        let after = after.clone();
        let has_more = has_more.clone();
        let loading = loading.clone();
        let error = error.clone();
        let in_flight = in_flight.clone();
        Callback::from(move |_| {
            if *in_flight.borrow() || !*has_more {
                return;
            }
            *in_flight.borrow_mut() = true;
            loading.set(true);
            error.set(None);
            let mut cursor = *after;
            let mut all = (*events).clone();
            let endpoint = endpoint.clone();
            let run_id = run_id.clone();
            let events = events.clone();
            let after = after.clone();
            let has_more = has_more.clone();
            let loading = loading.clone();
            let error = error.clone();
            let in_flight = in_flight.clone();
            spawn_local(async move {
                loop {
                    let url = super::question_bank_practice::ai_scope_url(
                        &endpoint,
                        &format!("/runs/{run_id}/events"),
                        scope,
                        Some(&format!("after={cursor}&limit=50")),
                    );
                    match fetch_json::<HistoryPage<StreamEvent>>(&url, "AI 执行历史").await {
                        Ok(page) => {
                            if page.has_more && page.next_after.is_none_or(|next| next <= cursor) {
                                error.set(Some("AI 执行历史分页游标无效".into()));
                                break;
                            }
                            all.extend(page.items);
                            events.set(all.clone());
                            if let Some(next) = page.next_after {
                                cursor = next;
                                after.set(next);
                            }
                            has_more.set(page.has_more);
                            if !page.has_more {
                                break;
                            }
                        }
                        Err(message) => {
                            error.set(Some(message));
                            break;
                        }
                    }
                }
                loading.set(false);
                *in_flight.borrow_mut() = false;
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
        <div data-testid="question-bank-ai-progress" class="min-w-0 max-w-full py-1">
            <button type="button" class="flex w-full min-w-0 items-start gap-2 text-left text-xs text-base-content/60 hover:text-base-content" aria-expanded={opened.to_string()} onclick={toggle}>
                <span class="shrink-0 whitespace-nowrap">{if *opened {"⌄ "} else {"› "}}{format!("第 {} 轮 · {status}", props.index + 1)}</span>
                <span data-testid="question-bank-ai-progress-summary" class="question-bank-ai-progress-summary flex-1">
                    {tools.last().map(|tool| if tool.2.is_empty() { tool.1.clone() } else { tool.2.clone() }).unwrap_or_default()}
                </span>
            </button>
            if *opened {
                <div class="mt-2 min-w-0 max-w-full space-y-3 border-l border-base-content/15 pl-3">
                    {for tools.iter().map(|(_, label, command, output)| {
                        tool_execution_item(label, command, output, "question-bank-ai")
                    })}
                    if *loading { <p role="status">{"正在加载工具输出…"}</p> }
                    if let Some(message) = error.as_ref() {
                        <p role="alert" class="text-error">{message.clone()}</p>
                        <button type="button" class="btn btn-ghost btn-xs" onclick={Callback::from({let load=load.clone(); move |_| load.emit(())})}>{"重试加载输出"}</button>
                    }
                </div>
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct AiReviewActivityProps {
    pub endpoint: String,
    pub scope: &'static str,
    pub refresh_key: String,
    pub stream: Option<StreamSnapshot>,
    pub messages: Vec<AiReviewMessage>,
}

#[function_component(AiReviewActivity)]
pub(super) fn ai_review_activity(props: &AiReviewActivityProps) -> Html {
    let runs = use_state(Vec::<HistoryRun>::new);
    let loading = use_state(|| false);
    let error = use_state(|| None::<String>);
    let retry = use_state(|| 0u64);
    {
        let runs = runs.clone();
        let loading = loading.clone();
        let error = error.clone();
        use_effect_with(
            (
                props.endpoint.clone(),
                props.scope,
                props.refresh_key.clone(),
                *retry,
            ),
            move |(endpoint, scope, _, _)| {
                let alive = Rc::new(Cell::new(true));
                let request_alive = alive.clone();
                let endpoint = endpoint.clone();
                let scope = *scope;
                loading.set(true);
                error.set(None);
                spawn_local(async move {
                    let mut cursor = 0i64;
                    let mut all = Vec::new();
                    loop {
                        let url = super::question_bank_practice::ai_scope_url(
                            &endpoint,
                            "/runs",
                            scope,
                            Some(&format!("after={cursor}&limit=50")),
                        );
                        let response =
                            fetch_json::<HistoryPage<HistoryRun>>(&url, "AI 执行历史").await;
                        if !request_alive.get() {
                            return;
                        }
                        match response {
                            Ok(page) => {
                                if page.has_more
                                    && page.next_after.is_none_or(|next| next <= cursor)
                                {
                                    error.set(Some("AI 执行历史分页游标无效".into()));
                                    break;
                                }
                                all.extend(page.items);
                                if let Some(next) = page.next_after {
                                    cursor = next;
                                }
                                if !page.has_more {
                                    runs.set(all);
                                    break;
                                }
                            }
                            Err(message) => {
                                error.set(Some(message));
                                break;
                            }
                        }
                    }
                    loading.set(false);
                });
                move || alive.set(false)
            },
        );
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
    let mut rows = Vec::new();
    for (message_index, message) in props.messages.iter().enumerate() {
        for (index, run) in all
            .iter()
            .enumerate()
            .filter(|(_, run)| run.assistant_message_id == Some(message.id) && message.id != 0)
        {
            rows.push(html! { <RunActivity key={format!("run-{}", run.run_id)} endpoint={props.endpoint.clone()} scope={props.scope} run={run.clone()} {index} stream={props.stream.clone()}/> });
        }
        rows.push(html! {
            <div key={format!("message-{}-{message_index}", message.id)} class={if message.role == "user" {"rounded-xl bg-primary/10 p-3"} else {"py-2"}} data-testid="question-bank-ai-message">
                <div class="mb-2 text-xs font-semibold text-base-content/60">{if message.role == "user" {"你的追问"} else {"AI 解析"}}</div>
                if message.role == "assistant" { <AiReply content={message.content.clone()}/> }
                else { <div class="study-markdown text-base leading-7">{super::study_markdown::render_question_bank_user_markdown(&message.content)}</div> }
            </div>
        });
    }
    for (index, run) in all.iter().enumerate().filter(|(_, run)| {
        run.assistant_message_id.is_none()
            || !props
                .messages
                .iter()
                .any(|message| message.id != 0 && Some(message.id) == run.assistant_message_id)
    }) {
        rows.push(html! { <RunActivity key={format!("run-{}", run.run_id)} endpoint={props.endpoint.clone()} scope={props.scope} run={run.clone()} {index} stream={props.stream.clone()}/> });
    }
    html! {
        <div data-testid="question-bank-ai-activity" aria-label="执行活动" class="space-y-2 text-xs text-base-content/60">
            <div class="space-y-3">{for rows}</div>
            if *loading { <p role="status">{"正在加载执行记录…"}</p> }
            if let Some(message) = error.as_ref() {
                <p role="alert" class="text-error">{message.clone()}</p>
                <button type="button" class="btn btn-ghost btn-xs" onclick={Callback::from({let retry=retry.clone(); move |_| retry.set(retry.wrapping_add(1))})}>{"重试加载执行记录"}</button>
            }
        </div>
    }
}
