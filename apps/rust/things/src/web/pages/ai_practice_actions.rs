use super::*;
use gloo_timers::callback::Interval;
use hyz_things::domain::ai_practice::{
    AiPracticeAgentState, AiPracticeModule, AiPracticeQuestionId,
};
use std::{cell::Cell, rc::Rc};

const FAVORITES: &str = "/api/v1/study/ai-practice/favorites";
const AGENT: &str = "/api/v1/study/ai-practice/agent";

#[derive(Properties, PartialEq)]
pub(crate) struct AiPracticeActionsProps {
    pub id: AiPracticeQuestionId,
    pub csrf: String,
}

#[function_component(AiPracticeActions)]
pub(crate) fn ai_practice_actions(props: &AiPracticeActionsProps) -> Html {
    let favorite = use_state(|| None::<bool>);
    let favorite_busy = use_state(|| false);
    let error = use_state(|| None::<String>);
    let open = use_state(|| false);
    let agent = use_state(|| None::<AiPracticeAgentState>);
    let agent_busy = use_state(|| false);
    let draft = use_state(String::new);
    let revision = use_state(|| 0_u64);
    {
        let id = props.id.clone();
        let favorite = favorite.clone();
        use_effect_with(id.clone(), move |_| {
            let alive = Rc::new(Cell::new(true));
            let pending = alive.clone();
            spawn_local(async move {
                match fetch_json::<Vec<AiPracticeQuestionId>>(FAVORITES, "练习收藏").await {
                    Ok(items) if pending.get() => favorite.set(Some(items.contains(&id))),
                    // A background read failure leaves the favorite state unknown; the header
                    // must not grow an inline error that crowds the training actions.
                    _ => {}
                }
            });
            move || alive.set(false)
        });
    }
    let toggle = {
        let id = props.id.clone();
        let csrf = props.csrf.clone();
        let favorite = favorite.clone();
        let busy = favorite_busy.clone();
        let error = error.clone();
        Callback::from(move |_| {
            let Some(current) = *favorite else {
                return;
            };
            if *busy {
                return;
            }
            busy.set(true);
            error.set(None);
            let (id, csrf, favorite, busy, error) = (
                id.clone(),
                csrf.clone(),
                favorite.clone(),
                busy.clone(),
                error.clone(),
            );
            spawn_local(async move {
                let body = serde_json::json!({"id":id,"favorite":!current});
                match post_json(FAVORITES, &csrf, &body, "练习收藏").await {
                    Ok(_) => favorite.set(Some(!current)),
                    Err(message) => error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let module = match props.id.module {
        AiPracticeModule::DataAnalysis => "data_analysis",
        AiPracticeModule::LogicJudgment => "logic_judgment",
        AiPracticeModule::QuantitativeRelations => "quantitative_relations",
    };
    let endpoint = format!(
        "{AGENT}?module={module}&session_id={}&question={}",
        props.id.session_id, props.id.question
    );
    let polling = agent
        .as_ref()
        .as_ref()
        .is_none_or(|state| state.status == "running");
    {
        let agent = agent.clone();
        let revision = revision.clone();
        let error = error.clone();
        use_effect_with(
            (*open, *revision, endpoint, polling),
            move |(open, _, endpoint, polling)| {
                let alive = Rc::new(Cell::new(true));
                let timer = if *open {
                    let alive_request = alive.clone();
                    let agent = agent.clone();
                    let error = error.clone();
                    let endpoint = endpoint.clone();
                    spawn_local(async move {
                        match fetch_json::<AiPracticeAgentState>(&endpoint, "解题 Agent").await {
                            Ok(value) if alive_request.get() => agent.set(Some(value)),
                            Err(message) if alive_request.get() => error.set(Some(message)),
                            _ => {}
                        }
                    });
                    if *polling {
                        Some(Interval::new(1000, move || {
                            revision.set(revision.wrapping_add(1))
                        }))
                    } else {
                        None
                    }
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
    let send = {
        let id = props.id.clone();
        let csrf = props.csrf.clone();
        let busy = agent_busy.clone();
        let error = error.clone();
        let revision = revision.clone();
        let draft = draft.clone();
        Callback::from(move |message: String| {
            if *busy || message.trim().is_empty() {
                return;
            }
            busy.set(true);
            error.set(None);
            let (id, csrf, busy, error, revision, draft) = (
                id.clone(),
                csrf.clone(),
                busy.clone(),
                error.clone(),
                revision.clone(),
                draft.clone(),
            );
            spawn_local(async move {
                match post_json(
                    &format!("{AGENT}/messages"),
                    &csrf,
                    &serde_json::json!({"id":id,"message":message}),
                    "解题 Agent",
                )
                .await
                {
                    Ok(_) => {
                        draft.set(String::new());
                        revision.set(revision.wrapping_add(1));
                    }
                    Err(message) => error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let stop = {
        let id = props.id.clone();
        let csrf = props.csrf.clone();
        let busy = agent_busy.clone();
        let error = error.clone();
        let revision = revision.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            busy.set(true);
            let (id, csrf, busy, error, revision) = (
                id.clone(),
                csrf.clone(),
                busy.clone(),
                error.clone(),
                revision.clone(),
            );
            spawn_local(async move {
                if let Err(message) = post_json(
                    &format!("{AGENT}/stop"),
                    &csrf,
                    &serde_json::json!({"id":id}),
                    "停止解题",
                )
                .await
                {
                    error.set(Some(message));
                }
                revision.set(revision.wrapping_add(1));
                busy.set(false);
            });
        })
    };
    let running = agent
        .as_ref()
        .as_ref()
        .is_some_and(|state| state.status == "running");
    let has_messages = agent
        .as_ref()
        .as_ref()
        .is_some_and(|state| !state.messages.is_empty());
    let favorite_label = if *favorite == Some(true) {
        "取消收藏"
    } else {
        "收藏题目"
    };
    html! {
        <>
            <QuestionHeaderActions favorite={*favorite} favorite_busy={*favorite_busy} favorite_label={favorite_label}
                favorite_test_id="ai-practice-favorite" on_favorite={toggle}
                flashcard_count={None::<usize>} flashcard_can_create={false}
                flashcard_create_test_id="ai-practice-flashcard-create" flashcard_manage_test_id="ai-practice-flashcard-manage"
                on_flashcard={Callback::noop()}/>
            <button type="button" class="btn btn-ghost btn-sm btn-square" aria-label="解题 Agent" title="解题 Agent"
                onclick={Callback::from({let open=open.clone(); move |_| open.set(true)})}>
                <svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="6" width="16" height="14" rx="3"/><path d="M12 3v3M2 12h2m16 0h2M8 15h8"/><circle cx="9" cy="11" r=".8" fill="currentColor"/><circle cx="15" cy="11" r=".8" fill="currentColor"/></svg>
            </button>
            if let Some(message) = error.as_ref() { <span class="text-error" role="alert" title={message.clone()} aria-label={message.clone()}>{"⚠"}</span> }
            if *open {
                <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 p-3" role="dialog" aria-modal="true" aria-label="解题 Agent" data-testid="ai-practice-agent-dialog">
                    <section class="flex max-h-[90dvh] w-full max-w-2xl flex-col overflow-hidden rounded-box bg-base-100 shadow-xl">
                        <header class="flex items-center justify-between border-b border-base-content/10 p-4"><strong>{"解题 Agent"}</strong><button class="btn btn-ghost btn-sm" aria-label="关闭解题 Agent" onclick={Callback::from({let open=open.clone(); move |_| open.set(false)})}>{"✕"}</button></header>
                        <div class="min-h-40 flex-1 space-y-3 overflow-y-auto p-4" role="log" aria-live="polite">
                            if let Some(message) = error.as_ref() { <p class="text-error" role="alert">{message.clone()}</p> }
                            if let Some(state) = agent.as_ref() {
                                {for state.messages.iter().map(|message| html! {
                                    <div class={if message.role == "user" {"rounded-xl bg-primary/10 p-3"} else {"p-3"}}>
                                        <div class="mb-2 text-xs font-semibold">{if message.role == "user" {"你的问题"} else {"解题 Agent"}}</div>
                                        if message.role == "assistant" { <AiReply content={message.content.clone()}/> }
                                        else { <div class="study-markdown">{render_question_bank_markdown(&message.content)}</div> }
                                    </div>
                                })}
                                if !state.partial.is_empty() { <StreamingMarkdown markdown={state.partial.clone()} complete={false}/> }
                                if let Some(message) = &state.error { <p class="text-error" role="alert">{message.clone()}</p> }
                            } else { <p>{"正在加载对话…"}</p> }
                        </div>
                        <div class="border-t border-base-content/10 p-4">
                            if !has_messages && !running {
                                <button class="btn btn-primary btn-sm" disabled={*agent_busy || agent.is_none()} onclick={Callback::from({let send=send.clone(); move |_| send.emit("请解答本题".into())})}>{"开始解题"}</button>
                            } else {
                                <label class="sr-only" for="ai-practice-agent-input">{"追问解题 Agent"}</label>
                                <textarea id="ai-practice-agent-input" class="textarea textarea-bordered w-full" aria-label="追问解题 Agent" value={(*draft).clone()} maxlength="8000" oninput={Callback::from({let draft=draft.clone(); move |event: InputEvent| { let input: web_sys::HtmlTextAreaElement=event.target_unchecked_into(); draft.set(input.value()); }})}/>
                                if running { <button class="btn btn-outline btn-sm mt-2" disabled={*agent_busy} onclick={stop}>{"停止生成"}</button> }
                                else { <button class="btn btn-primary btn-sm mt-2" disabled={*agent_busy || draft.trim().is_empty()} onclick={Callback::from({let send=send.clone(); let draft=draft.clone(); move |_| send.emit((*draft).clone())})}>{"发送追问"}</button> }
                            }
                        </div>
                    </section>
                </div>
            }
        </>
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
    html! {
        <section class="space-y-3" data-testid="ai-practice-favorites">
            <h3 class="font-semibold">{"收藏题目"}</h3>
            if let Some(message) = error.as_ref() { <p class="text-sm text-error" role="alert">{message.clone()}</p> }
            else if let Some(items) = entries.as_ref() {
                if items.is_empty() { <p class="text-sm text-base-content/60">{"暂无收藏题目"}</p> }
                else {
                    <div class="question-bank-collections-layout">
                        <div class="space-y-3">
                            <div class="flex items-center justify-between gap-2">
                                <h3 class="font-semibold">{"按卡组查看"}</h3>
                                <button type="button" class="btn btn-ghost btn-xs" aria-pressed={selected_group.is_empty().to_string()}
                                    onclick={Callback::from({let selected_group=selected_group.clone(); let selected_category=selected_category.clone(); move |_| { selected_group.set(String::new()); selected_category.set(String::new()); }})}>
                                    {format!("全部题目 {}",items.len())}
                                </button>
                            </div>
                            <div class="question-bank-collection-grid">
                                {for groups.iter().map(|(group, categories)| {
                                    let total: usize = categories.values().sum();
                                    let selected = *selected_group == *group;
                                    let group_for_select = group.clone();
                                    let select_group = Callback::from({let selected_group=selected_group.clone(); let selected_category=selected_category.clone(); move |_| { selected_group.set(group_for_select.clone()); selected_category.set(String::new()); }});
                                    html! {
                                        <QuestionBankModuleCard name={group.clone()} summary={format!("{total} 题")} value={total} max={items.len()} selected={selected} test_id="ai-practice-favorite-group" on_select={select_group.clone()}>
                                            <QuestionBankCategoryButton label="全部" count={total.to_string()} selected={selected && selected_category.is_empty()} test_id="ai-practice-favorite-category" on_select={select_group}/>
                                            {for categories.iter().map(|(category, count)| {
                                                let group=group.clone(); let category=category.clone();
                                                let is_selected=selected && *selected_category == category;
                                                html! {<QuestionBankCategoryButton label={category.clone()} count={count.to_string()} selected={is_selected} test_id="ai-practice-favorite-category"
                                                    on_select={Callback::from({let selected_group=selected_group.clone(); let selected_category=selected_category.clone(); move |_| { selected_group.set(group.clone()); selected_category.set(category.clone()); }})}/>}
                                            })}
                                        </QuestionBankModuleCard>
                                    }
                                })}
                            </div>
                        </div>
                        <div class="space-y-3 min-w-0">
                            <div class="flex min-h-6 items-center justify-between gap-2"><h3 class="font-semibold">{"筛选结果"}</h3><span class="text-xs text-base-content/50">{format!("{} 题",filtered.len())}</span></div>
                            <div class="question-bank-results"><div class="question-bank-results-list">
                                if filtered.is_empty() { <p class="p-4 text-sm text-base-content/60">{"这个分类还没有题目。"}</p> }
                                else { {for filtered.iter().map(|item| { let id=item.id.clone(); let on_open=props.on_open.clone(); html! {
                                    <QuestionCollectionRow title={item.title.clone()} subtitle={format!("{} / {} · 第 {} 题",item.group,item.category,id.question+1)} badge={"AI".to_owned()} test_id="ai-practice-favorite-item"
                                        on_open={Callback::from(move |_| on_open.emit(id.clone()))}/>
                                } })} }
                            </div></div>
                        </div>
                    </div>
                }
            } else { <p class="text-sm text-base-content/60">{"正在加载收藏…"}</p> }
        </section>
    }
}
