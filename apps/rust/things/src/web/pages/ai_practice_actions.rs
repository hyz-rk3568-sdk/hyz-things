use super::*;
use gloo_timers::callback::Interval;
use hyz_things::domain::ai_practice::{
    AiPracticeAgentState, AiPracticeMessage, AiPracticeModule, AiPracticeQuestionId,
};
use std::{cell::Cell, rc::Rc};

const FAVORITES: &str = "/api/v1/study/ai-practice/favorites";
const AGENT: &str = "/api/v1/study/ai-practice/agent";

fn module_key(module: AiPracticeModule) -> &'static str {
    match module {
        AiPracticeModule::DataAnalysis => "data_analysis",
        AiPracticeModule::LogicJudgment => "logic_judgment",
        AiPracticeModule::QuantitativeRelations => "quantitative_relations",
    }
}

/// One state instance drives the favorite marker and the AI 解析与追问 tab in
/// both pane headers, so the two copies can never disagree.
#[derive(Clone, PartialEq)]
pub(crate) struct AiPracticeReview {
    pub favorite: Option<bool>,
    pub favorite_busy: bool,
    pub active: bool,
    pub status: String,
    pub messages: Vec<AiPracticeMessage>,
    pub partial: String,
    pub favorite_error: Option<String>,
    pub agent_error: Option<String>,
    pub busy: bool,
    pub draft: String,
    pub csrf: String,
    pub attachments_endpoint: String,
    pub delete_endpoint: String,
    pub can_delete: bool,
    pub attachments: UseStateHandle<Vec<PendingImage>>,
    pub uploading: UseStateHandle<bool>,
    pub on_favorite: Callback<MouseEvent>,
    pub on_ai: Callback<MouseEvent>,
    pub on_original: Callback<MouseEvent>,
    pub on_input: Callback<InputEvent>,
    pub on_send: Callback<MouseEvent>,
    pub on_stop: Callback<MouseEvent>,
    pub on_keydown: Callback<KeyboardEvent>,
    pub on_deleted: Callback<()>,
}

#[hook]
pub(crate) fn use_ai_practice_review(
    id: Option<AiPracticeQuestionId>,
    csrf: String,
) -> AiPracticeReview {
    let favorite = use_state(|| None::<bool>);
    let favorite_busy = use_state(|| false);
    let favorite_error = use_state(|| None::<String>);
    let active = use_state(|| false);
    let agent = use_state(|| None::<AiPracticeAgentState>);
    let agent_error = use_state(|| None::<String>);
    let agent_busy = use_state(|| false);
    let draft = use_state(String::new);
    let attachments = use_state(Vec::<PendingImage>::new);
    let uploading = use_state(|| false);
    let revision = use_state(|| 0_u64);
    let stream_connected = use_state(|| false);
    {
        let favorite = favorite.clone();
        let favorite_error = favorite_error.clone();
        use_effect_with(id.clone(), move |id| {
            favorite.set(None);
            favorite_error.set(None);
            let alive = Rc::new(Cell::new(true));
            if let Some(id) = id.clone() {
                let pending = alive.clone();
                spawn_local(async move {
                    match fetch_json::<Vec<AiPracticeQuestionId>>(FAVORITES, "练习收藏").await {
                        Ok(items) if pending.get() => favorite.set(Some(items.contains(&id))),
                        // A background read failure leaves the state unknown; headers stay compact.
                        _ => {}
                    }
                });
            }
            move || alive.set(false)
        });
    }
    {
        let agent = agent.clone();
        let agent_error = agent_error.clone();
        let draft = draft.clone();
        let attachments = attachments.clone();
        let uploading = uploading.clone();
        use_effect_with(id.clone(), move |_| {
            agent.set(None);
            agent_error.set(None);
            draft.set(String::new());
            attachments.set(Vec::new());
            uploading.set(false);
            || ()
        });
    }
    let endpoint = id.as_ref().map(|id| {
        format!(
            "{AGENT}?module={}&session_id={}&question={}",
            module_key(id.module),
            id.session_id,
            id.question
        )
    });
    let attachments_endpoint = id.as_ref().map(|id| {
        format!(
            "{AGENT}/attachments?module={}&session_id={}&question={}",
            module_key(id.module),
            id.session_id,
            id.question
        )
    });
    let events_endpoint = id.as_ref().map(|id| {
        format!(
            "{AGENT}/events?module={}&session_id={}&question={}",
            module_key(id.module),
            id.session_id,
            id.question
        )
    });
    {
        let agent = agent.clone();
        let agent_error = agent_error.clone();
        let connected = stream_connected.clone();
        use_effect_with(
            (id.clone(), events_endpoint.clone(), *active),
            move |(_, endpoint, active)| {
                let alive = Rc::new(Cell::new(true));
                connected.set(false);
                let source = if *active {
                    endpoint.as_ref().and_then(|endpoint| {
                        web_sys::EventSource::new(endpoint).ok().map(|stream| {
                            let onmessage = {
                                let agent = agent.clone();
                                let agent_error = agent_error.clone();
                                let connected = connected.clone();
                                let alive = alive.clone();
                                Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                                    let Some(data) = event.data().as_string() else {
                                        return;
                                    };
                                    let Ok(value) =
                                        serde_json::from_str::<AiPracticeAgentState>(&data)
                                    else {
                                        return;
                                    };
                                    if !alive.get() {
                                        return;
                                    }
                                    connected.set(true);
                                    agent_error.set(None);
                                    agent.set(Some(value));
                                })
                                    as Box<dyn FnMut(_)>)
                            };
                            stream.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
                            let onerror = {
                                let connected = connected.clone();
                                Closure::wrap(Box::new(move |_: Event| connected.set(false))
                                    as Box<dyn FnMut(_)>)
                            };
                            stream.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                            (stream, onmessage, onerror)
                        })
                    })
                } else {
                    None
                };
                move || {
                    alive.set(false);
                    if let Some((stream, _, _)) = source {
                        stream.close();
                    }
                }
            },
        );
    }
    let polling = !*stream_connected
        && agent
            .as_ref()
            .as_ref()
            .is_none_or(|state| state.status == "running");
    {
        let agent = agent.clone();
        let agent_error = agent_error.clone();
        let revision = revision.clone();
        use_effect_with(
            (id.clone(), *revision, endpoint.clone(), *active, polling),
            move |(_, _, endpoint, active, polling)| {
                let alive = Rc::new(Cell::new(true));
                let timer = if *active && endpoint.is_some() {
                    if let Some(endpoint) = endpoint.clone() {
                        let alive_request = alive.clone();
                        let agent = agent.clone();
                        let agent_error = agent_error.clone();
                        spawn_local(async move {
                            match fetch_json::<AiPracticeAgentState>(&endpoint, "AI 解析").await {
                                Ok(value) if alive_request.get() => agent.set(Some(value)),
                                Err(message) if alive_request.get() => {
                                    agent_error.set(Some(message))
                                }
                                _ => {}
                            }
                        });
                    }
                    (*polling).then(|| {
                        Interval::new(1000, move || revision.set(revision.wrapping_add(1)))
                    })
                } else {
                    None
                };
                move || {
                    alive.set(false);
                    drop(timer);
                }
            },
        );
    }
    let toggle_favorite = {
        let id = id.clone();
        let csrf = csrf.clone();
        let favorite = favorite.clone();
        let busy = favorite_busy.clone();
        let favorite_error = favorite_error.clone();
        Callback::from(move |_| {
            let (Some(id), Some(current)) = (id.clone(), *favorite) else {
                return;
            };
            if *busy {
                return;
            }
            busy.set(true);
            favorite_error.set(None);
            let favorite = favorite.clone();
            let busy = busy.clone();
            let favorite_error = favorite_error.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let body = serde_json::json!({"id":id,"favorite":!current});
                match post_json(FAVORITES, &csrf, &body, "练习收藏").await {
                    Ok(_) => favorite.set(Some(!current)),
                    Err(message) => favorite_error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let show_ai = {
        let active = active.clone();
        let revision = revision.clone();
        let available = id.is_some();
        Callback::from(move |_| {
            if !available || *active {
                return;
            }
            active.set(true);
            revision.set(revision.wrapping_add(1));
        })
    };
    let show_original = {
        let active = active.clone();
        Callback::from(move |_| active.set(false))
    };
    let send = {
        let id = id.clone();
        let csrf = csrf.clone();
        let busy = agent_busy.clone();
        let agent_error = agent_error.clone();
        let revision = revision.clone();
        let draft = draft.clone();
        let attachments = attachments.clone();
        Callback::from(move |_: ()| {
            let Some(id) = id.clone() else {
                return;
            };
            if *busy {
                return;
            }
            let images = attachments
                .iter()
                .map(|image| image.id.clone())
                .collect::<Vec<_>>();
            let message = {
                let text = draft.trim().to_owned();
                if text.is_empty() {
                    if images.is_empty() {
                        "请解答本题".to_owned()
                    } else {
                        "请解析图片并说明思路".to_owned()
                    }
                } else {
                    text
                }
            };
            busy.set(true);
            agent_error.set(None);
            let busy = busy.clone();
            let agent_error = agent_error.clone();
            let revision = revision.clone();
            let draft = draft.clone();
            let attachments = attachments.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                match post_json(
                    &format!("{AGENT}/messages"),
                    &csrf,
                    &serde_json::json!({"id":id,"message":message,"attachments":images}),
                    "AI 解析",
                )
                .await
                {
                    Ok(_) => {
                        draft.set(String::new());
                        attachments.set(Vec::new());
                        revision.set(revision.wrapping_add(1));
                    }
                    Err(message) => agent_error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let on_send = {
        let send = send.clone();
        Callback::from(move |_: MouseEvent| send.emit(()))
    };
    let on_stop = {
        let id = id.clone();
        let csrf = csrf.clone();
        let busy = agent_busy.clone();
        let agent_error = agent_error.clone();
        let revision = revision.clone();
        Callback::from(move |_: MouseEvent| {
            let Some(id) = id.clone() else {
                return;
            };
            if *busy {
                return;
            }
            busy.set(true);
            let busy = busy.clone();
            let agent_error = agent_error.clone();
            let revision = revision.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                if let Err(message) = post_json(
                    &format!("{AGENT}/stop"),
                    &csrf,
                    &serde_json::json!({"id":id}),
                    "停止生成",
                )
                .await
                {
                    agent_error.set(Some(message));
                }
                revision.set(revision.wrapping_add(1));
                busy.set(false);
            });
        })
    };
    let on_input = {
        let draft = draft.clone();
        Callback::from(move |event: InputEvent| {
            let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
            draft.set(input.value());
        })
    };
    let on_keydown = {
        let send = send.clone();
        let busy = agent_busy.clone();
        let draft = draft.clone();
        let running = agent
            .as_ref()
            .as_ref()
            .is_some_and(|state| state.status == "running");
        Callback::from(move |event: KeyboardEvent| {
            if event.key() != "Enter" || event.shift_key() {
                return;
            }
            event.prevent_default();
            if *busy || running || draft.trim().is_empty() {
                return;
            }
            send.emit(());
        })
    };
    let on_deleted = {
        let agent = agent.clone();
        let agent_error = agent_error.clone();
        let draft = draft.clone();
        let attachments = attachments.clone();
        let revision = revision.clone();
        Callback::from(move |_| {
            agent.set(None);
            agent_error.set(None);
            draft.set(String::new());
            attachments.set(Vec::new());
            revision.set(revision.wrapping_add(1));
        })
    };
    let state = (*agent).as_ref();
    let status = state.map(|state| state.status.clone()).unwrap_or_else(|| {
        if *active {
            "loading".to_owned()
        } else {
            "idle".to_owned()
        }
    });
    let messages = state
        .map(|state| state.messages.clone())
        .unwrap_or_default();
    let can_delete = !messages.is_empty() && status != "running";
    AiPracticeReview {
        favorite: *favorite,
        favorite_busy: *favorite_busy,
        active: *active,
        status,
        messages,
        partial: state.map(|state| state.partial.clone()).unwrap_or_default(),
        favorite_error: (*favorite_error).clone(),
        agent_error: (*agent_error)
            .clone()
            .or_else(|| state.and_then(|state| state.error.clone())),
        busy: *agent_busy,
        draft: (*draft).clone(),
        csrf,
        attachments_endpoint: attachments_endpoint.unwrap_or_default(),
        delete_endpoint: endpoint.clone().unwrap_or_default(),
        can_delete,
        attachments,
        uploading,
        on_favorite: toggle_favorite,
        on_ai: show_ai,
        on_original: show_original,
        on_input,
        on_send,
        on_stop,
        on_keydown,
        on_deleted,
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeHeaderControlsProps {
    pub review: AiPracticeReview,
}

#[function_component(AiPracticeHeaderControls)]
pub(crate) fn ai_practice_header_controls(props: &AiPracticeHeaderControlsProps) -> Html {
    let review = &props.review;
    let favorite_label = if review.favorite == Some(true) {
        "取消收藏"
    } else {
        "收藏题目"
    };
    let delete_open = use_state(|| false);
    let deleting = use_state(|| false);
    let delete_error = use_state(|| None::<String>);
    let request_delete = {
        let delete_open = delete_open.clone();
        let delete_error = delete_error.clone();
        Callback::from(move |_| {
            delete_error.set(None);
            delete_open.set(true);
        })
    };
    let cancel_delete = {
        let delete_open = delete_open.clone();
        let delete_error = delete_error.clone();
        Callback::from(move |_| {
            delete_open.set(false);
            delete_error.set(None);
        })
    };
    let confirm_delete = {
        let review = props.review.clone();
        let delete_open = delete_open.clone();
        let deleting = deleting.clone();
        let delete_error = delete_error.clone();
        Callback::from(move |_| {
            if *deleting {
                return;
            }
            let endpoint = review.delete_endpoint.clone();
            let csrf = review.csrf.clone();
            let on_deleted = review.on_deleted.clone();
            let delete_open = delete_open.clone();
            let deleting = deleting.clone();
            let delete_error = delete_error.clone();
            deleting.set(true);
            delete_error.set(None);
            spawn_local(async move {
                match delete_json_response::<serde_json::Value>(&endpoint, &csrf, "AI 会话").await
                {
                    Ok(_) => {
                        delete_open.set(false);
                        on_deleted.emit(());
                    }
                    Err(message) => delete_error.set(Some(message)),
                }
                deleting.set(false);
            });
        })
    };
    html! {
        <>
            <div class="flex shrink-0 items-center gap-0" role="group" aria-label="AI 解析与收藏">
                <QuestionHeaderActions favorite={review.favorite} favorite_busy={review.favorite_busy}
                    favorite_label={favorite_label} favorite_test_id="ai-practice-favorite"
                    on_favorite={review.on_favorite.clone()}
                    flashcard_count={None::<usize>} flashcard_can_create={false}
                    flashcard_create_test_id="ai-practice-flashcard-create" flashcard_manage_test_id="ai-practice-flashcard-manage"
                    on_flashcard={Callback::noop()}/>
                <QuestionAiModeTabs active={review.active}
                    original_test_id="ai-practice-answer-tab" ai_test_id="ai-practice-agent-tab"
                    original_label="答案与解析" ai_label="AI 解析与追问"
                    original={review.on_original.clone()} ai={review.on_ai.clone()}/>
                <QuestionAiMoreMenu can_delete={review.can_delete} delete={request_delete}
                    reset_key={review.status.clone()}
                    button_test_id="ai-practice-agent-more" delete_test_id="ai-practice-agent-delete"
                    button_label="更多 AI 操作" delete_label="删除 AI 会话"/>
                if let Some(message) = review.favorite_error.as_ref() {
                    <span class="text-error" role="alert" title={message.clone()} aria-label={message.clone()}>{"⚠"}</span>
                }
            </div>
            if *delete_open {
                <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 p-4"
                    role="dialog" aria-modal="true" aria-label="确认删除 AI 会话"
                    data-testid="ai-practice-agent-delete-dialog">
                    <div class="w-full max-w-sm space-y-4 rounded-box bg-base-100 p-5 shadow-xl">
                        <p>{"确定删除这道题的 AI 会话和全部追问吗？"}</p>
                        if let Some(error) = delete_error.as_ref() {
                            <p class="text-sm text-error" role="alert" data-testid="ai-practice-agent-delete-error">{error.clone()}</p>
                        }
                        <div class="flex justify-end gap-2">
                            <button type="button" class="btn btn-ghost btn-sm" data-testid="ai-practice-agent-delete-cancel"
                                disabled={*deleting} onclick={cancel_delete}>{"取消"}</button>
                            <button type="button" class="btn btn-error btn-sm" data-testid="ai-practice-agent-delete-confirm"
                                disabled={*deleting} onclick={confirm_delete}>{"确认删除"}</button>
                        </div>
                    </div>
                </div>
            }
        </>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeAgentPageProps {
    pub review: AiPracticeReview,
}

#[function_component(AiPracticeAgentPage)]
pub(crate) fn ai_practice_agent_page(props: &AiPracticeAgentPageProps) -> Html {
    let review = &props.review;
    let running = review.status == "running";
    let has_messages = !review.messages.is_empty();
    let status = match review.status.as_str() {
        "running" => "正在生成",
        "complete" => "已完成",
        "stopped" => "已停止",
        "failed" => "生成失败",
        "loading" => "正在加载",
        _ => "未生成",
    };
    let scroller = use_node_ref();
    {
        let scroller = scroller.clone();
        use_effect_with(
            (
                review.messages.len(),
                review.partial.len(),
                review.status.clone(),
            ),
            move |_| {
                if let Some(element) = scroller.cast::<web_sys::HtmlElement>() {
                    element.set_scroll_top(element.scroll_height());
                }
                || ()
            },
        );
    }
    let can_send = !review.busy
        && (!has_messages || !review.draft.trim().is_empty() || !review.attachments.is_empty());
    let placeholder = if has_messages {
        "输入消息…"
    } else {
        "输入解析要求（可选）…"
    };
    let composer = html! {
        <AiComposerArea test_id="ai-practice-agent-composer-area">
            {ai_composer(AiComposer {
                id: "ai-practice-agent-input", input_ref: NodeRef::default(),
                compose_testid: "ai-practice-agent-compose",
                input_testid: "ai-practice-agent-input-field",
                button_testid: if running {"ai-practice-agent-stop"} else {"ai-practice-agent-send"},
                value: review.draft.as_str(), max_length: 8000, read_only: false,
                placeholder,
                button_label: if running {"停止生成"} else if has_messages {"发送追问"} else {"开始 AI 解析"},
                is_stop: running,
                disabled: !running && !can_send,
                endpoint: review.attachments_endpoint.clone(), csrf: review.csrf.clone(),
                attachments: review.attachments.clone(), uploading: review.uploading.clone(),
                picker_disabled: review.busy || running, picker_testid: "ai-practice-agent-images",
                oninput: review.on_input.clone(), onkeydown: review.on_keydown.clone(),
                onclick: if running {review.on_stop.clone()} else {review.on_send.clone()},
            })}
        </AiComposerArea>
    };
    html! {
        <AiReviewPanel section_test_id="ai-practice-agent-page" messages_test_id="ai-practice-agent-messages"
            status_test_id="ai-practice-agent-status"
            messages_ref={scroller}
            title="AI 解析与追问" status={status} composer={composer}>
            if let Some(message) = review.agent_error.as_ref() {
                <p class="text-sm text-error" role="alert">{message.clone()}</p>
            }
            if !has_messages && !running && review.agent_error.is_none() {
                <p class="text-sm text-base-content/60">{"AI 根据原题、相关给定材料和你的要求进行解析，来源参考答案仅作参考。"}</p>
            }
            {for review.messages.iter().map(|message| {
                if let Some(tool) = message.tool.as_ref() {
                    html! {
                        <div class="flex min-w-0 items-center gap-2 rounded-lg bg-base-content/5 px-2 py-1 text-xs text-base-content/60"
                            data-testid="ai-practice-agent-tool">
                            <span class="shrink-0 font-semibold text-base-content/70">{format!("工具 · {}", tool.name)}</span>
                            if !tool.detail.is_empty() {
                                <code class="min-w-0 truncate font-mono text-[11px]">{tool.detail.clone()}</code>
                            }
                            <span class="ml-auto shrink-0">{tool_status_label(&tool.status)}</span>
                        </div>
                    }
                } else {
                    html! {
                        <div class={if message.role == "user" {"rounded-xl bg-primary/10 p-3"} else {"py-2"}}>
                            <div class="mb-2 text-xs font-semibold text-base-content/60">{if message.role == "user" {"你的追问"} else {"AI 解析"}}</div>
                            if message.role == "assistant" {
                                <AiReply content={message.content.clone()}/>
                            } else {
                                <div class="study-markdown">{render_question_bank_markdown(&message.content)}</div>
                            }
                        </div>
                    }
                }
            })}
            if !review.partial.is_empty() {
                <div class="py-2" data-testid="ai-practice-agent-stream">
                    <div class="mb-2 text-xs font-semibold text-base-content/60">{"AI 正在回复"}</div>
                    <StreamingMarkdown markdown={review.partial.clone()} complete={streaming_markdown_complete(&review.partial)}/>
                </div>
            }
        </AiReviewPanel>
    }
}

#[derive(Clone, PartialEq)]
struct AiFavoriteRow {
    id: AiPracticeQuestionId,
    title: String,
    group: String,
    category: String,
}

async fn load_favorite_ids(module: AiPracticeModule) -> Result<Vec<AiPracticeQuestionId>, String> {
    let items = fetch_json::<Vec<AiPracticeQuestionId>>(FAVORITES, "练习收藏").await?;
    Ok(items
        .into_iter()
        .filter(|item| item.module == module)
        .collect())
}

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeFavoritesButtonProps {
    pub module: AiPracticeModule,
    pub open: bool,
    pub onclick: Callback<MouseEvent>,
}

#[function_component(AiPracticeFavoritesButton)]
pub(crate) fn ai_practice_favorites_button(props: &AiPracticeFavoritesButtonProps) -> Html {
    let count = use_state(|| None::<usize>);
    {
        let count = count.clone();
        let module = props.module;
        use_effect_with(module, move |_| {
            let alive = Rc::new(Cell::new(true));
            let pending = alive.clone();
            spawn_local(async move {
                if let Ok(items) = load_favorite_ids(module).await {
                    if pending.get() {
                        count.set(Some(items.len()));
                    }
                }
            });
            move || alive.set(false)
        });
    }
    html! {
        <button type="button"
            class={classes!("btn","btn-sm",if props.open {"btn-primary"} else {"btn-outline"})}
            data-testid="ai-practice-favorites" aria-pressed={props.open.to_string()}
            onclick={props.onclick.clone()}>
            {"我的收藏"}
            if let Some(count) = *count {
                <span class={classes!("question-bank-count","font-bold",
                    if props.open {"bg-white text-primary"} else {"bg-primary/10 text-primary"})}
                    data-testid="ai-practice-favorites-count">{count}</span>
            }
        </button>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeFavoritesPageProps {
    pub module: AiPracticeModule,
    pub subject: AttrValue,
    pub on_open: Callback<AiPracticeQuestionId>,
    pub on_back: Callback<MouseEvent>,
}

#[function_component(AiPracticeFavoritesPage)]
pub(crate) fn ai_practice_favorites_page(props: &AiPracticeFavoritesPageProps) -> Html {
    let entries = use_state(|| None::<Vec<AiFavoriteRow>>);
    let selected_group = use_state(String::new);
    let selected_category = use_state(String::new);
    let error = use_state(|| None::<String>);
    {
        let entries = entries.clone();
        let error = error.clone();
        let module = props.module;
        use_effect_with(module, move |_| {
            let alive = Rc::new(Cell::new(true));
            let pending = alive.clone();
            spawn_local(async move {
                let items = match load_favorite_ids(module).await {
                    Ok(items) => items,
                    Err(message) => {
                        if pending.get() {
                            error.set(Some(message));
                        }
                        return;
                    }
                };
                let root = match module {
                    AiPracticeModule::DataAnalysis => "/api/v1/study/data-analysis",
                    AiPracticeModule::LogicJudgment => "/api/v1/study/logic-judgment",
                    AiPracticeModule::QuantitativeRelations => {
                        "/api/v1/study/quantitative-relations"
                    }
                };
                let mut sessions =
                    std::collections::BTreeMap::<String, Option<serde_json::Value>>::new();
                for item in &items {
                    if !sessions.contains_key(&item.session_id) {
                        let result = fetch_json::<serde_json::Value>(
                            &format!("{root}/runs/{}/result", item.session_id),
                            "练习详情",
                        )
                        .await
                        .ok();
                        sessions.insert(item.session_id.clone(), result);
                    }
                }
                let items = items
                    .into_iter()
                    .map(|item| {
                        let title = sessions
                            .get(&item.session_id)
                            .and_then(Option::as_ref)
                            .and_then(|run| run.get("questions"))
                            .and_then(|questions| questions.get(item.question))
                            .and_then(|question| question.get("question"))
                            .and_then(|text| text.as_str())
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                format!("第 {} 题（练习详情暂不可用）", item.question + 1)
                            });
                        let path = sessions
                            .get(&item.session_id)
                            .and_then(Option::as_ref)
                            .and_then(|run| run.get("questions"))
                            .and_then(|questions| questions.get(item.question))
                            .and_then(|question| question.get("card_path"))
                            .and_then(|path| path.as_str());
                        let (group, category) =
                            hyz_things::domain::ai_practice::favorite_card_category(path);
                        AiFavoriteRow {
                            id: item,
                            title,
                            group,
                            category,
                        }
                    })
                    .collect();
                if pending.get() {
                    entries.set(Some(items));
                }
            });
            move || alive.set(false)
        });
    }
    let mut groups =
        std::collections::BTreeMap::<String, std::collections::BTreeMap<String, usize>>::new();
    if let Some(items) = entries.as_ref() {
        for item in items {
            *groups
                .entry(item.group.clone())
                .or_default()
                .entry(item.category.clone())
                .or_default() += 1;
        }
    }
    let filtered = entries
        .as_ref()
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    (selected_group.is_empty() || *selected_group == item.group)
                        && (selected_category.is_empty() || *selected_category == item.category)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let browser_groups = groups
        .iter()
        .map(|(group, categories)| CollectionBrowserGroup {
            name: group.clone(),
            summary: format!("{} 题", categories.values().sum::<usize>()),
            value: categories.values().sum(),
            max: entries.as_ref().map_or(0, Vec::len),
            categories: categories
                .iter()
                .map(|(name, count)| (name.clone(), *count))
                .collect(),
        })
        .collect::<Vec<_>>();
    let rows = filtered
        .iter()
        .map(|item| CollectionBrowserRow {
            title: item.title.clone(),
            subtitle: format!(
                "{} / {} · 第 {} 题",
                item.group,
                item.category,
                item.id.question + 1
            ),
            badge: "AI".to_owned(),
            disabled: false,
        })
        .collect::<Vec<_>>();
    let result_count = format!("{} 题", rows.len());
    let on_open = {
        let ids = filtered
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let on_open = props.on_open.clone();
        Callback::from(move |index: usize| {
            if let Some(id) = ids.get(index) {
                on_open.emit(id.clone());
            }
        })
    };
    let on_select_all = Callback::from({
        let selected_group = selected_group.clone();
        let selected_category = selected_category.clone();
        move |_| {
            selected_group.set(String::new());
            selected_category.set(String::new());
        }
    });
    let on_select_group = Callback::from({
        let selected_group = selected_group.clone();
        let selected_category = selected_category.clone();
        move |group: String| {
            selected_group.set(group);
            selected_category.set(String::new());
        }
    });
    let on_select_category = Callback::from({
        let selected_group = selected_group.clone();
        let selected_category = selected_category.clone();
        move |(group, category): (String, String)| {
            selected_group.set(group);
            selected_category.set(category);
        }
    });
    let body = if let Some(message) = error.as_ref() {
        html! { <p class="text-sm text-error" role="alert">{message.clone()}</p> }
    } else if let Some(items) = entries.as_ref() {
        if items.is_empty() {
            html! { <p class="text-sm text-base-content/60">{"暂无收藏题目"}</p> }
        } else {
            html! {
                <QuestionCollectionBrowser groups={browser_groups} rows={rows}
                    selected_group={(*selected_group).clone()} selected_category={(*selected_category).clone()}
                    groups_label="按卡组查看" all_label={format!("全部题目 {}", items.len())}
                    all_selected={selected_group.is_empty()}
                    result_label="筛选结果" result_count={result_count}
                    empty_label="这个分类还没有题目。"
                    group_test_id="ai-practice-favorite-group" category_test_id="ai-practice-favorite-category"
                    item_test_id="ai-practice-favorite-item"
                    on_select_all={on_select_all} on_select_group={on_select_group}
                    on_select_category={on_select_category} on_open={on_open}/>
            }
        }
    } else {
        html! { <p class="text-sm text-base-content/60">{"正在加载收藏…"}</p> }
    };
    let count_label = match (error.as_ref(), entries.as_ref()) {
        (Some(_), _) => "加载失败".to_owned(),
        (None, Some(items)) => format!("共 {} 题", items.len()),
        (None, None) => "正在加载…".to_owned(),
    };
    html! {
        <TrainingSetupLayout test_id="ai-practice-favorites-page" wide={true}
            heading={Some(html! {
                <div class="flex items-center justify-between gap-3">
                    <div class="min-w-0">
                        <h2 class="text-xl font-bold">{"我的收藏"}</h2>
                        <p class="text-xs text-base-content/60">{props.subject.clone()}{" · "}{count_label}</p>
                    </div>
                    <button type="button" class="btn btn-outline btn-sm" data-testid="ai-practice-favorites-back"
                        onclick={props.on_back.clone()}>{"← 返回练习历史"}</button>
                </div>
            })}>
            <div class="space-y-3" data-testid="ai-practice-favorites-page-content">{body}</div>
        </TrainingSetupLayout>
    }
}

fn tool_status_label(status: &str) -> &'static str {
    match status {
        "running" => "调用中",
        "complete" | "completed" => "完成",
        "failed" => "失败",
        "stopped" => "已停止",
        _ => "已调用",
    }
}
