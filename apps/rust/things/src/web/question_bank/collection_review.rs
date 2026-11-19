//! Review of a saved wrong or favorite question. The timed training
//! sessions stay separate; this view only reads one question at a time and
//! lets the reader swipe through the filtered collection.
use super::*;
use hyz_things::domain::question_bank::{
    FavoriteState, PracticeBrowseItem, PracticeMode, PracticeQuestionDetail, SetFavorite,
};

const ROOT: &str = "/api/v1/question-bank";

#[derive(Properties, PartialEq)]
pub(crate) struct CollectionQuestionReviewProps {
    pub items: Vec<PracticeBrowseItem>,
    pub initial_index: usize,
    pub scope: PracticeMode,
    pub csrf: String,
    /// Card collection entries open the same review with the editor showing
    /// the selected card.
    #[prop_or_default]
    pub initial_card_id: Option<String>,
    #[prop_or_default]
    pub card_ids: Vec<String>,
    /// Collection name shown in the review header; defaults to the scope.
    #[prop_or_default]
    pub scope_label: Option<String>,
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
    let (ai_tab, ai_tab_chosen, initial_ai_review) = use_question_ai_tab(question_id.clone());
    let ai_delete_signal = use_state(|| 0u32);
    let ai_can_delete = use_state(|| false);
    let favorite = use_practice_favorite(question_id.clone());
    let favorite_busy = use_state(|| false);
    let favorite_error = use_state(|| None::<String>);
    let preview = use_state(|| None::<FavoritePreview>);
    let preview_error = use_state(|| None::<String>);
    let preview_retry = use_state(|| 0u32);
    let creating = use_state(|| props.initial_card_id.is_some());
    let navigation_blocked = use_state(|| false);
    let index_effect_ready = use_mut_ref(|| false);
    let flashcard_changed = Callback::from({
        let preview_retry = preview_retry.clone();
        move |_| preview_retry.set((*preview_retry).wrapping_add(1))
    });
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
                        "题目详情",
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
        use_effect_with((question_id.clone(), *preview_retry), move |(id, _)| {
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
        });
    }
    {
        let material_scroll = material_scroll.clone();
        let question_scroll = question_scroll.clone();
        let creating = creating.clone();
        let index_effect_ready = index_effect_ready.clone();
        let card_collection = !props.card_ids.is_empty();
        use_effect_with(*index, move |_| {
            // The first effect run only mounts the view; a card entry keeps
            // its editor open until the reader switches question.
            if *index_effect_ready.borrow() {
                creating.set(card_collection);
            } else {
                *index_effect_ready.borrow_mut() = true;
            }
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
    let scope_label = props.scope_label.clone().unwrap_or_else(|| {
        if props.scope == PracticeMode::Favorite {
            "收藏".into()
        } else {
            "错题".into()
        }
    });
    let back_label = if props.scope_label.is_some() {
        "返回卡片".to_string()
    } else {
        format!("返回{scope_label}")
    };
    let favorite_state = (*favorite.value).as_ref();
    let favorite_active = favorite_state.is_some_and(|state| state.favorite);
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
    let back = {
        let on_back = props.on_back.clone();
        Callback::from(move |_| on_back.emit(()))
    };
    let navigation_disabled = *creating && (props.card_ids.is_empty() || *navigation_blocked);
    let navigation_allowed = {
        let creating = creating.clone();
        let blocked = navigation_blocked.clone();
        let cards = !props.card_ids.is_empty();
        Callback::from(move |()| !*creating || (cards && !*blocked))
    };
    let main = if let Some(current) = &*detail {
        let shared = !current.materials.is_empty();
        let options = answer_options(&current.question.options_markdown);
        let material_pane = html! {
            <section class="flex min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-material-pane">
                <div class={classes!(QUESTION_BANK_PANE_HEADER,"question-bank-pane-header")}>
                    <strong>{"原卷材料"}</strong>
                </div>
                <div ref={material_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                    data-testid="collection-review-material-scroll" data-ink-pane="material">
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
                <div class={classes!(QUESTION_BANK_PANE_HEADER,"question-bank-pane-header")}>
                    <strong>{"题干"}</strong>
                </div>
                <div ref={material_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                    data-testid="collection-review-material-scroll" data-ink-pane="material">
                    <div class="study-markdown question-bank-passage" data-testid="collection-review-stem">
                        {render_question_bank_markdown(&current.question.stem)}
                    </div>
                </div>
            </section>
        };
        let left = if shared { material_pane } else { stem_pane };
        let question_content = html! { <div class="space-y-4">
                    if shared {
                        <div class="study-markdown question-bank-passage" data-testid="collection-review-stem">
                            {render_question_bank_markdown(&current.question.stem)}
                        </div>
                    }
                    <QuestionBankChoices options={options.clone()} selected={Vec::<String>::new()}
                        correct={current.correct_answer.clone()} grade={None::<String>} reviewed={true}
                        disabled={true} pending={false} test_id="collection-review-choice" />
                    <ReviewAnswerStats correct_answer={current.correct_answer.clone()} answer={None::<Vec<String>>}
                        correct_count={current.correct_count} wrong_count={current.wrong_count}
                        answer_test_id="collection-review-answer" counts_test_id="collection-review-attempt-counts"/>
                    <QuestionBankExplanation explanation={current.explanation.clone()} topics={current.question.topics.clone()}
                        visible={!*ai_tab} test_id={Some(AttrValue::from("collection-review-explanation"))} />
                    if let Some(value) = &*preview {
                            if value.existing.is_empty() && !favorite_can_create(value) {
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
        </div> };
        let right = html! {
            <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-question-pane">
                <QuestionBankQuestionHeader
                    title_test_id="collection-review-heading"
                    title={format!("第 {} 题", current.item.number)}
                    subtitle={Some([current.item.module.as_deref(),current.item.category.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · "))}
                    tabs={html! {<QuestionAiHeaderActions active={*ai_tab} can_delete={*ai_can_delete}
                        reset_key={current.item.question_id.clone()}
                        original={Callback::from({let ai_tab=ai_tab.clone();let chosen=ai_tab_chosen.clone();move |_|{*chosen.borrow_mut()=true;ai_tab.set(false);}})}
                        ai={Callback::from({let ai_tab=ai_tab.clone();let chosen=ai_tab_chosen.clone();move |_|{*chosen.borrow_mut()=true;ai_tab.set(true);}})}
                        delete={Callback::from({let signal=ai_delete_signal.clone();move |_|signal.set(signal.wrapping_add(1))})}/>}}
                    leading={html!{
                        <>
                        if let Some((_, message)) = (*favorite.failure).as_ref() {
                            <button type="button" class="btn btn-ghost btn-sm shrink-0 text-error" data-testid="collection-review-favorite-retry"
                                title={message.clone()} onclick={Callback::from({
                                    let retry=favorite.retry.clone(); move |_|retry.emit(())
                                })}>
                                {"重试收藏"}
                            </button>
                        }
                            <QuestionHeaderActions
                                favorite={favorite_state.map(|value| value.favorite)} favorite_busy={*favorite_busy}
                                favorite_label={if *favorite_busy {"正在更新收藏"} else {favorite_label}}
                                favorite_test_id="collection-review-favorite" on_favorite={toggle_favorite}
                                flashcard_count={preview.as_ref().as_ref().map(|p| p.existing.len())}
                                flashcard_can_create={preview.as_ref().as_ref().is_some_and(|value| favorite_can_create(value))}
                                flashcard_create_test_id="collection-review-create-flashcard" flashcard_manage_test_id="collection-review-manage-flashcard"
                                on_flashcard={Callback::from({let creating=creating.clone(); move |_| creating.set(true)})}/>
                        </>
                    }}
                    passage={Some(question_scroll.clone())}
                    can_scroll_up={question_edges.can_scroll_up}
                    can_scroll_down={question_edges.can_scroll_down}
                    question_index={*index} target="题目" label="题目与解析滚动" />
                if *creating {
                    if let Some(value) = &*preview {
                        <div class="min-h-0 flex-1 overflow-y-auto p-3" data-testid="collection-review-flashcard-pane">
                            <FavoriteFlashcardWorkspace key={format!("{}-{}", item.question_id, *index)} question_id={item.question_id.clone()}
                                preview={value.clone()} csrf={props.csrf.clone()}
                                initial_card_id={props.card_ids.get(*index).cloned().or_else(|| props.initial_card_id.clone())}
                                on_navigation_blocked={Callback::from({ let navigation_blocked=navigation_blocked.clone(); move |blocked| navigation_blocked.set(blocked) })}
                                on_changed={flashcard_changed.clone()}
                                on_close={Callback::from({
                                    let creating=creating.clone(); move |_|creating.set(false)
                                })}/>
                        </div>
                    } else if let Some(message) = &*preview_error {
                        <div class="min-h-0 flex-1 overflow-y-auto p-4" data-testid="collection-review-flashcard-error">
                            <p role="alert" class="text-sm text-error">{message.clone()}</p>
                            <button type="button" class="btn btn-outline btn-sm mt-2"
                                data-testid="collection-review-flashcard-retry"
                                onclick={Callback::from({let preview_retry=preview_retry.clone();move |_|preview_retry.set(*preview_retry+1)})}>
                                {"重试加载"}
                            </button>
                        </div>
                    } else {
                        <p class="min-h-0 flex-1 p-4 text-sm opacity-60" role="status" data-testid="collection-review-flashcard-loading">
                            {"正在准备闪卡…"}
                        </p>
                    }
                } else if *ai_tab {
                    <QuestionAiReview initial_review={Some(initial_ai_review.clone())} delete_signal={*ai_delete_signal} on_delete_availability={Callback::from({let value=ai_can_delete.clone();move |available|value.set(available)})} key={item.question_id.clone()} question_id={item.question_id.clone()} csrf={props.csrf.clone()} question_content={question_content.clone()} question_scroll={question_scroll.clone()} on_question_scroll={Callback::from({let refresh=refresh_question_edges.clone();move |_|refresh.emit(())})}/>
                } else {
                    <div ref={question_scroll.clone()}
                        onscroll={Callback::from({let refresh=refresh_question_edges.clone();move |_|refresh.emit(())})}
                        class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                        data-testid="collection-review-question-scroll" data-ink-pane="answer">{question_content.clone()}</div>
                }
            </section>
        };
        html! {
            <TrainingSplitWorkspace test_id="collection-review-split" split_at_md={true} resizable={true}
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
                <p class="opacity-60" role="status">{"正在读取题目…"}</p>
            </div>
        }
    };
    html! {
        <TrainingDetailLayout root_class={classes!(TRAINING_WORKSPACE_ROOT, "z-[80]", "question-bank-workspace")}
            test_id="collection-review" label={Some(AttrValue::from(format!("{scope_label}题目")))}
            leading={html! {<>
                <button class="btn btn-ghost btn-sm" data-testid="collection-review-back" onclick={back}>
                    {format!("← {back_label}")}
                </button>
                <div class="min-w-0">
                    <strong class="block truncate text-sm">{item.paper_title.clone()}</strong>
                    <span class="block truncate text-[11px] text-base-content/50" data-testid="collection-review-scope">
                        {format!("{scope_label} · 第 {}/{} 题",*index+1,props.items.len())}
                    </span>
                </div>
            </>}}
            actions={Html::default()}
            notices={html! {if let Some(message) = (*favorite_error).as_ref() {
                <p class="shrink-0 pb-2 text-sm text-error" role="alert" data-testid="collection-review-favorite-error">{message.clone()}</p>
            }}}
            content={html! {
                <InkOverlay question_index={*index} responsive={true} scroll_linked={true}
                    persist_key={format!("collection-review-{}-{}", scope_label, item.question_id)}>
                    <TrainingSwipeSurface current={index.clone()} total={props.items.len()}
                        navigation_allowed={Some(navigation_allowed.clone())}>
                        {main}
                    </TrainingSwipeSurface>
                </InkOverlay>
            }}
            navigation={html! {
                <CompactQuestionNavigation current={index.clone()}
                    navigation_allowed={Some(navigation_allowed)} disabled={navigation_disabled}
                    state_key={format!("collection-{}", scope_label)}
                    numbers={props.items.iter().enumerate().map(|(i, item)| if props.card_ids.is_empty() {item.number} else {(i+1) as u32}).collect::<Vec<_>>()}
                    answered={vec![false; props.items.len()]} grades={None::<Vec<Option<bool>>>}
                    groups={vec![NavigationGroup {label: scope_label.clone(), subtitle: None, start: 0, end: props.items.len()}]} />
            }} />
    }
}
