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
    pub on_favorite: Callback<MouseEvent>,
    pub on_ai: Callback<MouseEvent>,
    pub on_original: Callback<MouseEvent>,
    pub on_input: Callback<InputEvent>,
    pub on_send: Callback<MouseEvent>,
    pub on_stop: Callback<MouseEvent>,
    pub on_keydown: Callback<KeyboardEvent>,
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
    let revision = use_state(|| 0_u64);
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
        use_effect_with(id.clone(), move |_| {
            agent.set(None);
            agent_error.set(None);
            draft.set(String::new());
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
    let polling = agent
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
        Callback::from(move |_: ()| {
            let Some(id) = id.clone() else {
                return;
            };
            if *busy {
                return;
            }
            let message = {
                let text = draft.trim().to_owned();
                if text.is_empty() {
                    "请解答本题".to_owned()
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
            let csrf = csrf.clone();
            spawn_local(async move {
                match post_json(
                    &format!("{AGENT}/messages"),
                    &csrf,
                    &serde_json::json!({"id":id,"message":message}),
                    "AI 解析",
                )
                .await
                {
                    Ok(_) => {
                        draft.set(String::new());
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
    let state = (*agent).as_ref();
    let status = state.map(|state| state.status.clone()).unwrap_or_else(|| {
        if *active {
            "loading".to_owned()
        } else {
            "idle".to_owned()
        }
    });
    AiPracticeReview {
        favorite: *favorite,
        favorite_busy: *favorite_busy,
        active: *active,
        status,
        messages: state
            .map(|state| state.messages.clone())
            .unwrap_or_default(),
        partial: state.map(|state| state.partial.clone()).unwrap_or_default(),
        favorite_error: (*favorite_error).clone(),
        agent_error: (*agent_error)
            .clone()
            .or_else(|| state.and_then(|state| state.error.clone())),
        busy: *agent_busy,
        draft: (*draft).clone(),
        on_favorite: toggle_favorite,
        on_ai: show_ai,
        on_original: show_original,
        on_input,
        on_send,
        on_stop,
        on_keydown,
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeHeaderControlsProps {
    pub review: AiPracticeReview,
    #[prop_or("ai-practice-favorite")]
    pub favorite_test_id: &'static str,
    #[prop_or("ai-practice-answer-tab")]
    pub original_test_id: &'static str,
    #[prop_or("ai-practice-agent-tab")]
    pub tab_test_id: &'static str,
}

#[function_component(AiPracticeHeaderControls)]
pub(crate) fn ai_practice_header_controls(props: &AiPracticeHeaderControlsProps) -> Html {
    let review = &props.review;
    let favorite_label = if review.favorite == Some(true) {
        "取消收藏"
    } else {
        "收藏题目"
    };
    html! {
        <div class="flex shrink-0 items-center gap-0" role="group" aria-label="AI 解析与收藏">
            <QuestionHeaderActions favorite={review.favorite} favorite_busy={review.favorite_busy}
                favorite_label={favorite_label} favorite_test_id={props.favorite_test_id}
                on_favorite={review.on_favorite.clone()}
                flashcard_count={None::<usize>} flashcard_can_create={false}
                flashcard_create_test_id="ai-practice-flashcard-create" flashcard_manage_test_id="ai-practice-flashcard-manage"
                on_flashcard={Callback::noop()}/>
            <QuestionAiModeTabs active={review.active}
                original_test_id={props.original_test_id} ai_test_id={props.tab_test_id}
                original_label="答案与解析" ai_label="AI 解析与追问"
                original={review.on_original.clone()} ai={review.on_ai.clone()}/>
            if let Some(message) = review.favorite_error.as_ref() {
                <span class="text-error" role="alert" title={message.clone()} aria-label={message.clone()}>{"⚠"}</span>
            }
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeAgentPageProps {
    pub review: AiPracticeReview,
    pub question: Html,
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
    let can_send = !review.busy && (!has_messages || !review.draft.trim().is_empty());
    let placeholder = if has_messages {
        "输入消息…"
    } else {
        "输入解析要求（可选）…"
    };
    html! {
        <section class="flex min-h-0 min-w-0 flex-1 flex-col" data-testid="ai-practice-agent-page">
            <div ref={scroller} class="min-h-0 flex-1 space-y-3 overflow-y-auto overscroll-contain touch-pan-y p-4"
                role="log" aria-live="polite" aria-label="AI 解析与追问记录" data-testid="ai-practice-agent-messages">
                <div class="space-y-4" data-testid="ai-practice-agent-question">{props.question.clone()}</div>
                <div class="flex items-center justify-between gap-2">
                    <strong>{"AI 解析与追问"}</strong>
                    <span class="text-xs text-base-content/60" data-testid="ai-practice-agent-status">{status}</span>
                </div>
                if let Some(message) = review.agent_error.as_ref() {
                    <p class="text-sm text-error" role="alert">{message.clone()}</p>
                }
                if !has_messages && !running && review.agent_error.is_none() {
                    <p class="text-sm text-base-content/60">{"AI 会结合本题与关联方法卡进行解析；可以先输入你的要求，或直接开始。"}</p>
                }
                {for review.messages.iter().map(|message| html! {
                    <div class={if message.role == "user" {"rounded-xl bg-primary/10 p-3"} else {"py-2"}}>
                        <div class="mb-2 text-xs font-semibold text-base-content/60">{if message.role == "user" {"你的追问"} else {"AI 解析"}}</div>
                        if message.role == "assistant" {
                            <AiReply content={message.content.clone()}/>
                        } else {
                            <div class="study-markdown">{render_question_bank_markdown(&message.content)}</div>
                        }
                    </div>
                })}
                if !review.partial.is_empty() {
                    <div class="py-2" data-testid="ai-practice-agent-stream">
                        <div class="mb-2 text-xs font-semibold text-base-content/60">{"AI 正在回复"}</div>
                        <StreamingMarkdown markdown={review.partial.clone()} complete={streaming_markdown_complete(&review.partial)}/>
                    </div>
                }
            </div>
            <div class="relative shrink-0 bg-base-100 px-2 pb-3 pt-2">
                <label class="sr-only" for="ai-practice-agent-input">{"AI 解析与追问输入"}</label>
                <div class="relative rounded-2xl border border-base-content/15 bg-base-100 transition focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/15"
                    data-testid="ai-practice-agent-compose">
                    <textarea id="ai-practice-agent-input" rows="1"
                        class="block h-6 min-w-16 flex-1 resize-none overflow-y-auto border-0 bg-transparent px-1 py-0 pr-12 text-sm leading-6 focus:outline-none focus:ring-0"
                        data-testid="ai-practice-agent-input-field" maxlength="8000"
                        value={review.draft.clone()} placeholder={placeholder}
                        onkeydown={review.on_keydown.clone()} oninput={review.on_input.clone()}/>
                    <div class="absolute right-2 top-1/2 flex -translate-y-1/2 items-center justify-end">
                        <button type="button" class="btn btn-primary btn-sm btn-circle"
                            aria-label={if running {"停止生成"} else if has_messages {"发送追问"} else {"开始 AI 解析"}}
                            title={if running {"停止生成"} else if has_messages {"发送追问"} else {"开始 AI 解析"}}
                            data-testid={if running {"ai-practice-agent-stop"} else {"ai-practice-agent-send"}}
                            disabled={!running && !can_send}
                            onclick={if running {review.on_stop.clone()} else {review.on_send.clone()}}>
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                                if running { <rect x="7" y="7" width="10" height="10" rx="1" fill="currentColor" stroke="none"/> }
                                else { <path d="M12 19V5M5 12l7-7 7 7"/> }
                            </svg>
                        </button>
                    </div>
                </div>
            </div>
        </section>
    }
}

#[derive(Clone, PartialEq)]
struct AiFavoriteRow {
    id: AiPracticeQuestionId,
    title: String,
    group: String,
    category: String,
}

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeFavoritesPanelProps {
    pub module: AiPracticeModule,
    pub on_open: Callback<AiPracticeQuestionId>,
}

#[function_component(AiPracticeFavoritesPanel)]
pub(crate) fn ai_practice_favorites_panel(props: &AiPracticeFavoritesPanelProps) -> Html {
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
                let items =
                    match fetch_json::<Vec<AiPracticeQuestionId>>(FAVORITES, "练习收藏").await {
                        Ok(items) => items
                            .into_iter()
                            .filter(|item| item.module == module)
                            .collect::<Vec<_>>(),
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
    html! {
        <details class="rounded-box bg-base-100" data-testid="ai-practice-favorites">
            <summary class="flex cursor-pointer items-center gap-2 p-4 font-semibold" data-testid="ai-practice-favorites-summary">
                <span>{"收藏题目"}</span>
                if let Some(items) = entries.as_ref() {
                    <span class="badge badge-ghost badge-sm">{items.len()}</span>
                }
            </summary>
            <div class="space-y-3 px-4 pb-4">{body}</div>
        </details>
    }
}
