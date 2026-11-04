//! Review of a saved wrong or favorite question. The timed training
//! sessions stay separate; this view only reads one question at a time and
//! lets the reader swipe through the filtered collection.
use super::favorite_flashcard::{FavoriteFlashcardPanel, FavoritePreview};
use super::material_scroll::{scroll_edges, ScrollEdges};
use super::question_bank_practice::{
    answer_options, use_practice_favorite, QuestionBankQuestionHeader,
};
use super::*;
use hyz_things::domain::question_bank::{
    FavoriteState, PracticeBrowseItem, PracticeMode, PracticeQuestionDetail,
    SetFavorite,
};

const ROOT: &str = "/api/v1/question-bank";

#[derive(Properties, PartialEq)]
pub(crate) struct CollectionQuestionReviewProps {
    pub items: Vec<PracticeBrowseItem>,
    pub initial_index: usize,
    pub scope: PracticeMode,
    pub csrf: String,
    pub on_back: Callback<()>,
    pub on_favorite_change: Callback<()>,
}

#[function_component(CollectionQuestionReview)]
pub(crate) fn collection_question_review(props: &CollectionQuestionReviewProps) -> Html {
    let index = use_state(|| props.initial_index.min(props.items.len().saturating_sub(1)));
    let detail = use_state(|| None::<PracticeQuestionDetail>);
    let error = use_state(|| None::<String>);
    let retry = use_state(|| 0u32);
    let material_scroll = use_node_ref();
    let question_scroll = use_node_ref();
    let question_edges = use_state(ScrollEdges::default);
    let current_item = props.items.get(*index).cloned();
    let question_id = current_item.as_ref().map(|item| item.question_id.clone());
    let favorite = use_practice_favorite(question_id.clone());
    let favorite_busy = use_state(|| false);
    let favorite_error = use_state(|| None::<String>);
    let preview = use_state(|| None::<FavoritePreview>);
    let preview_error = use_state(|| None::<String>);
    let preview_retry = use_state(|| 0u32);
    let creating = use_state(|| false);
    {
        let detail = detail.clone();
        let error = error.clone();
        use_effect_with((question_id.clone(), *retry), move |(id, _)| {
            let active = Rc::new(Cell::new(true));
            let current = active.clone();
            detail.set(None);
            error.set(None);
            if let Some(id) = id.clone() {
                spawn_local(async move {
                    let result = fetch_json::<PracticeQuestionDetail>(
                        &format!("{ROOT}/practice/questions/{id}"),
                        "题目复盘",
                    )
                    .await;
                    if current.get() {
                        match result {
                            Ok(value) => detail.set(Some(value)),
                            Err(message) => error.set(Some(message)),
                        }
                    }
                });
            }
            move || active.set(false)
        });
    }
    {
        let question = detail
            .as_ref()
            .as_ref()
            .map(|detail| detail.question.id.clone());
        use_effect_with(question, |_| {
            hydrate_math();
            || ()
        });
    }
    let refresh_question_edges = {
        let question_scroll = question_scroll.clone();
        let question_edges = question_edges.clone();
        Callback::from(move |_: ()| {
            if let Some(node) = question_scroll.cast::<web_sys::HtmlElement>() {
                let value = scroll_edges(&node);
                if *question_edges != value {
                    question_edges.set(value);
                }
            }
        })
    };
    {
        let refresh_question_edges = refresh_question_edges.clone();
        let question = detail
            .as_ref()
            .as_ref()
            .map(|detail| detail.question.id.clone());
        use_effect_with((*index, question), move |_| {
            refresh_question_edges.emit(());
            || ()
        });
    }
    {
        let preview = preview.clone();
        let preview_error = preview_error.clone();
        use_effect_with(
            (question_id.clone(), *preview_retry),
            move |(id, _)| {
                let active = Rc::new(Cell::new(true));
                let current = active.clone();
                preview.set(None);
                preview_error.set(None);
                if let Some(id) = id.clone() {
                    spawn_local(async move {
                        let result = fetch_json::<FavoritePreview>(
                            &format!("{ROOT}/practice/questions/{id}/flashcard-preview"),
                            "闪卡预览",
                        )
                        .await;
                        if current.get() {
                            match result {
                                Ok(value) => preview.set(Some(value)),
                                Err(message) => preview_error.set(Some(message)),
                            }
                        }
                    });
                }
                move || active.set(false)
            },
        );
    }
    {
        let material_scroll = material_scroll.clone();
        let question_scroll = question_scroll.clone();
        let creating = creating.clone();
        use_effect_with(*index, move |_| {
            creating.set(false);
            for target in [&material_scroll, &question_scroll] {
                if let Some(element) = target.cast::<HtmlElement>() {
                    element.set_scroll_top(0);
                }
            }
            || ()
        });
    }
    let Some(item) = current_item else {
        return Html::default();
    };
    let scope_label = if props.scope == PracticeMode::Favorite {
        "收藏"
    } else {
        "错题"
    };
    let favorite_state = (*favorite.value).as_ref();
    let favorite_active = favorite_state.is_some_and(|state| state.favorite);
    let favorite_symbol = if favorite_state.is_none() {
        "…"
    } else if favorite_active {
        "★"
    } else {
        "☆"
    };
    let favorite_label = if favorite_active {
        "取消收藏"
    } else {
        "收藏题目"
    };
    let toggle_favorite = {
        let favorite_busy = favorite_busy.clone();
        let favorite_error = favorite_error.clone();
        let source = favorite.source.clone();
        let ready = favorite.ready.clone();
        let value = favorite.value.clone();
        let question_id = item.question_id.clone();
        let csrf = props.csrf.clone();
        let on_favorite_change = props.on_favorite_change.clone();
        Callback::from(move |_| {
            if *favorite_busy {
                return;
            }
            let Some(state) = (*value).as_ref() else {
                return;
            };
            let target = !state.favorite;
            favorite_busy.set(true);
            favorite_error.set(None);
            let source = source.clone();
            let ready = ready.clone();
            let value = value.clone();
            let favorite_busy = favorite_busy.clone();
            let favorite_error = favorite_error.clone();
            let on_favorite_change = on_favorite_change.clone();
            let question_id = question_id.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response::<_, FavoriteState>(
                    &format!("{ROOT}/practice/favorites/{question_id}"),
                    &csrf,
                    &SetFavorite { favorite: target },
                    "收藏真题",
                )
                .await;
                favorite_busy.set(false);
                match result {
                    Ok(updated) if updated.question_id == question_id => {
                        if source.borrow().as_ref() == Some(&question_id) {
                            *ready.borrow_mut() = Some(updated.clone());
                            value.set(Some(updated));
                        }
                        on_favorite_change.emit(());
                    }
                    Ok(_) => favorite_error.set(Some("收藏状态响应不匹配，请重试".into())),
                    Err(message) => favorite_error.set(Some(message)),
                }
            });
        })
    };
    let can_create = preview
        .as_ref()
        .is_some_and(|value| value.eligible && !value.allowed_targets.is_empty());
    let back = {
        let on_back = props.on_back.clone();
        Callback::from(move |_| on_back.emit(()))
    };
    let previous = {
        let index = index.clone();
        Callback::from(move |_| index.set(index.saturating_sub(1)))
    };
    let next = {
        let index = index.clone();
        let total = props.items.len();
        Callback::from(move |_| index.set((*index + 1).min(total.saturating_sub(1))))
    };
    let main = if *creating {
        if let Some(value) = &*preview {
            html! {
                <FavoriteFlashcardPanel question_id={item.question_id.clone()} preview={value.clone()}
                    csrf={props.csrf.clone()} on_close={Callback::from({
                        let creating=creating.clone(); move |_|creating.set(false)
                    })}/>
            }
        } else {
            html! {
                <div class="flex h-full items-center justify-center rounded-box bg-base-100" data-testid="collection-review-loading">
                    <p class="opacity-60" role="status">{"正在准备闪卡…"}</p>
                </div>
            }
        }
    } else if let Some(current) = &*detail {
        let shared = !current.materials.is_empty();
        let options = answer_options(&current.question.options_markdown);
        let material_pane = html! {
            <section class="flex min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-material-pane">
                <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                    <strong>{"原卷材料"}</strong>
                    <span class="text-[11px] text-base-content/45">{"复盘"}</span>
                </div>
                <div ref={material_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                    data-testid="collection-review-material-scroll">
                    {for current.materials.iter().map(|material| html!{
                        <section class="mb-5">
                            if current.materials.len()>1 {<h3 class="font-bold">{material.name.clone()}</h3>}
                            <div class="study-markdown question-bank-passage">{render_question_bank_markdown(&material.markdown)}</div>
                        </section>
                    })}
                </div>
            </section>
        };
        let stem_pane = html! {
            <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-stem-pane">
                <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                    <strong>{"题干"}</strong>
                    <span class="text-[11px] text-base-content/45">{"复盘"}</span>
                </div>
                <div ref={material_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                    data-testid="collection-review-material-scroll">
                    <div class="study-markdown question-bank-passage" data-testid="collection-review-stem">
                        {render_question_bank_markdown(&current.question.stem)}
                    </div>
                </div>
            </section>
        };
        let left = if shared { material_pane } else { stem_pane };
        let right = html! {
            <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-question-pane">
                <QuestionBankQuestionHeader
                    title_test_id="collection-review-heading"
                    title={format!("第 {} 题", current.item.number)}
                    subtitle={Some([current.item.module.as_deref(),current.item.category.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · "))}
                    leading={html!{
                        if let Some((_, message)) = (*favorite.failure).as_ref() {
                            <button type="button" class="btn btn-ghost btn-sm shrink-0 text-error" data-testid="collection-review-favorite-retry"
                                title={message.clone()} onclick={Callback::from({
                                    let retry=favorite.retry.clone(); move |_|retry.emit(())
                                })}>
                                {"重试收藏"}
                            </button>
                        } else {
                            <button type="button" class="btn btn-ghost btn-sm shrink-0 text-lg" data-testid="collection-review-favorite"
                                aria-label={if *favorite_busy {"正在更新收藏"} else {favorite_label}}
                                disabled={*favorite_busy || favorite_state.is_none()}
                                onclick={toggle_favorite}>{if *favorite_busy {"…"} else {favorite_symbol}}</button>
                        }
                    }}
                    passage={Some(question_scroll.clone())}
                    can_scroll_up={question_edges.can_scroll_up}
                    can_scroll_down={question_edges.can_scroll_down}
                    question_index={*index} target="题目" label="题目与解析滚动" />
                <div ref={question_scroll.clone()}
                    onscroll={Callback::from({let refresh=refresh_question_edges.clone();move |_|refresh.emit(())})}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4 space-y-4"
                    data-testid="collection-review-question-scroll">
                    if shared {
                        <div class="study-markdown question-bank-passage" data-testid="collection-review-stem">
                            {render_question_bank_markdown(&current.question.stem)}
                        </div>
                    }
                    <div class="space-y-2">
                        {for options.iter().map(|(label,markdown)|{
                            let correct=current.correct_answer.contains(label);
                            html!{<div class={classes!("flex","items-start","gap-2.5","rounded-xl","border","p-3","text-sm","leading-6",if correct {"border-success bg-success/10 text-success"} else {"border-base-content/20"})}
                                data-testid="collection-review-choice">
                                <span class="shrink-0 font-bold">{format!("{label}.")}</span>
                                <span class="study-markdown min-w-0">{render_question_bank_markdown(markdown)}</span>
                            </div>}
                        })}
                    </div>
                    <section class="rounded-box border border-base-content/15 p-3" aria-label="正确答案">
                        <p class="text-xs opacity-60">{"正确答案"}</p>
                        <p class="font-semibold" data-testid="collection-review-answer">
                            {if current.correct_answer.is_empty() {"暂无可靠答案".into()} else {current.correct_answer.join("、")}}
                        </p>
                    </section>
                    if !current.explanation.is_empty() {
                        <section class="border-t border-base-content/10 pt-3">
                            <h3 class="font-bold">{"原始解析"}</h3>
                            <div class="study-markdown mt-2 text-sm leading-7 text-base-content/75" data-testid="collection-review-explanation">
                                {render_question_bank_markdown(&current.explanation)}
                            </div>
                        </section>
                    }
                    if !current.question.topics.is_empty() {
                        <section>
                            <h3 class="font-bold">{"考点"}</h3>
                            <p class="text-sm text-base-content/60">{current.question.topics.join("、")}</p>
                        </section>
                    }
                    if let Some(value) = &*preview {
                            if can_create {
                                <button type="button" class="btn btn-primary" data-testid="collection-review-create-flashcard"
                                    onclick={Callback::from({let creating=creating.clone();move |_|creating.set(true)})}>
                                    {"创建 Flashcard"}
                                </button>
                            } else {
                                <p class="text-xs opacity-60" role="status" data-testid="collection-review-flashcard-unavailable">
                                    {value.reason.as_deref().unwrap_or("此题暂不支持创建闪卡")}
                                </p>
                            }
                        } else if let Some(message) = &*preview_error {
                            <p role="alert" class="text-sm text-error">{message.clone()}</p>
                            <button type="button" class="btn btn-outline btn-sm" data-testid="collection-review-flashcard-retry"
                                onclick={Callback::from({let preview_retry=preview_retry.clone();move |_|preview_retry.set(*preview_retry+1)})}>
                                {"重试加载"}
                            </button>
                        } else {
                            <p class="text-xs opacity-60" role="status">{"正在检查闪卡资格…"}</p>
                        }
                </div>
            </section>
        };
        html! {
            <TrainingSplitWorkspace test_id="collection-review-split" split_at_md={true}
                left={left} right={right}/>
        }
    } else if let Some(message) = &*error {
        html! {
            <div class="flex h-full items-center justify-center rounded-box bg-base-100" data-testid="collection-review-error">
                <div class="space-y-3 text-center">
                    <p role="alert" class="text-error">{message.clone()}</p>
                    <button type="button" class="btn btn-outline btn-sm"
                        onclick={Callback::from({let retry=retry.clone();move |_|retry.set(*retry+1)})}>{"重试加载"}</button>
                </div>
            </div>
        }
    } else {
        html! {
            <div class="flex h-full items-center justify-center rounded-box bg-base-100" data-testid="collection-review-loading">
                <p class="opacity-60" role="status">{"正在读取题目复盘…"}</p>
            </div>
        }
    };
    html! {
        <section class="question-bank-workspace training-workspace fixed inset-0 z-[80] flex h-dvh w-screen flex-col overflow-hidden bg-base-200 px-3 py-2"
            data-testid="collection-review" aria-label={format!("{scope_label}题目复盘")}>
            <header class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                <div class="flex min-w-0 items-center gap-3">
                    <button class="btn btn-ghost btn-sm" data-testid="collection-review-back" onclick={back}>
                        {format!("← 返回{scope_label}")}
                    </button>
                    <div class="min-w-0">
                        <strong class="block truncate text-sm">{item.paper_title.clone()}</strong>
                        <span class="block truncate text-[11px] text-base-content/50" data-testid="collection-review-scope">
                            {format!("{scope_label}复盘 · 第 {}/{} 题",*index+1,props.items.len())}
                        </span>
                    </div>
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    <span class="badge badge-outline">{"复盘"}</span>
                </div>
            </header>
            if let Some(message) = (*favorite_error).as_ref() {
                <p class="shrink-0 pb-2 text-sm text-error" role="alert" data-testid="collection-review-favorite-error">
                    {message.clone()}
                </p>
            }
            <div class="relative min-h-0 flex-1 overflow-hidden">
                if *creating {
                    {main}
                } else {
                    <InkOverlay question_index={*index} responsive={true} scroll_linked={true}
                        persist_key={format!("collection-review-{}-{}", scope_label, item.question_id)}>
                        <TrainingSwipeSurface current={index.clone()} total={props.items.len()}
                            navigation_allowed={Some(Callback::from({
                                let creating=creating.clone(); move |()| !*creating
                            }))}>
                            {main}
                        </TrainingSwipeSurface>
                    </InkOverlay>
                }
            </div>
            <nav class="flex shrink-0 items-center justify-center gap-3 border-t border-base-content/10 pt-2"
                aria-label="复盘题目导航">
                <button type="button" class="btn btn-outline btn-sm" data-testid="collection-review-previous"
                    disabled={*creating || *index==0} onclick={previous}>{"上一题"}</button>
                <span class="min-w-16 text-center text-sm tabular-nums" data-testid="collection-review-position">
                    {format!("{} / {}",*index+1,props.items.len())}
                </span>
                <button type="button" class="btn btn-outline btn-sm" data-testid="collection-review-next"
                    disabled={*creating || *index+1>=props.items.len()} onclick={next}>{"下一题"}</button>
            </nav>
        </section>
    }
}
