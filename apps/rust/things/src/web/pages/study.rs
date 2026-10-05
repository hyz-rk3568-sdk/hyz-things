use super::*;
use hyz_things::domain::flashcards::{
    DeckSummary, FlashcardSummary, ReviewRating, StudySummary, TagSummary,
};
use pulldown_cmark::{html::push_html, CowStr, Event as MarkdownEvent, Options, Parser, Tag};

#[derive(Properties, PartialEq)]
struct StudyWorkspaceProps {
    state: UseReducerHandle<AppState>,
    route: PortalRoute,
    route_state: UseStateHandle<PortalRoute>,
    is_admin: bool,
}

#[derive(Properties, PartialEq)]
struct CardsLibraryProps {
    cards: Option<Vec<FlashcardSummary>>,
    decks: Vec<DeckSummary>,
    tags: Vec<TagSummary>,
    error: Option<String>,
    route: PortalRoute,
    route_state: UseStateHandle<PortalRoute>,
}

#[derive(Properties, PartialEq)]
struct StudyReviewProps {
    state: UseReducerHandle<AppState>,
    route: PortalRoute,
    route_state: UseStateHandle<PortalRoute>,
}

pub(crate) fn render_study(
    state: UseReducerHandle<AppState>,
    route: PortalRoute,
    route_state: UseStateHandle<PortalRoute>,
    is_admin: bool,
) -> Html {
    html! {
        <StudyWorkspace state={state} route={route} route_state={route_state} is_admin={is_admin} />
    }
}

pub(crate) fn render_study_review(
    state: UseReducerHandle<AppState>,
    route: PortalRoute,
    route_state: UseStateHandle<PortalRoute>,
) -> Html {
    html! { <StudyReview state={state} route={route} route_state={route_state} /> }
}

#[function_component(StudyWorkspace)]
fn study_workspace(props: &StudyWorkspaceProps) -> Html {
    let summary = use_state(|| None::<StudySummary>);
    let summary_error = use_state(|| None::<String>);
    let cards = use_state(|| None::<Vec<FlashcardSummary>>);
    let decks = use_state(Vec::<DeckSummary>::new);
    let tags = use_state(Vec::<TagSummary>::new);
    let cards_error = use_state(|| None::<String>);
    let sync_busy = use_state(|| false);
    let notice = use_state(|| None::<String>);
    let reload = use_state(|| 0_u64);

    let dependencies = (
        props.route.study_view,
        props.route.study_deck.clone(),
        props.route.study_tags.clone(),
        *reload,
    );
    {
        let summary = summary.clone();
        let summary_error = summary_error.clone();
        let cards = cards.clone();
        let decks = decks.clone();
        let tags = tags.clone();
        let cards_error = cards_error.clone();
        use_effect_with(dependencies, move |dependencies| {
            summary.set(None);
            summary_error.set(None);
            cards_error.set(None);
            if dependencies.0 == StudyView::Cards {
                cards.set(None);
            }
            let query = study_query(
                dependencies.1.as_deref(),
                &dependencies.2,
                Some(200),
                Some(0),
            );
            let cards_view = dependencies.0 == StudyView::Cards;
            spawn_local(async move {
                match fetch_json_typed::<StudySummaryResponse>(STUDY_SUMMARY_ENDPOINT, "学习摘要")
                    .await
                {
                    Ok(response) => summary.set(Some(response.summary)),
                    Err(error) => summary_error.set(Some(error.to_string())),
                }
                if !cards_view {
                    return;
                }
                let cards_result = fetch_json_typed::<StudyCardsResponse>(
                    &format!("{STUDY_CARDS_ENDPOINT}{query}"),
                    "卡片库",
                )
                .await;
                match cards_result {
                    Ok(response) => cards.set(Some(response.cards)),
                    Err(error) => cards_error.set(Some(error.to_string())),
                }
                if let Ok(response) =
                    fetch_json_typed::<StudyDecksResponse>(STUDY_DECKS_ENDPOINT, "Deck 列表").await
                {
                    decks.set(response.decks);
                }
                if let Ok(response) =
                    fetch_json_typed::<StudyTagsResponse>(STUDY_TAGS_ENDPOINT, "Tag 列表").await
                {
                    tags.set(response.tags);
                }
            });
            || ()
        });
    }

    {
        let dependencies = (props.route.study_view, summary.is_some(), cards.is_some());
        use_effect_with(dependencies, move |_| {
            hydrate_math();
            || ()
        });
    }

    let on_sync = {
        let csrf = props
            .state
            .panel
            .as_ref()
            .map(|panel| panel.csrf_token.clone())
            .unwrap_or_default();
        let sync_busy = sync_busy.clone();
        let notice = notice.clone();
        let reload = reload.clone();
        Callback::from(move |_| {
            if *sync_busy {
                return;
            }
            if csrf.is_empty() {
                notice.set(Some("安全令牌尚未准备好，请稍后重试".to_owned()));
                return;
            }
            sync_busy.set(true);
            notice.set(None);
            let sync_busy = sync_busy.clone();
            let notice = notice.clone();
            let reload = reload.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response_typed::<_, StudySyncResponse>(
                    STUDY_SYNC_ENDPOINT,
                    &csrf,
                    &EmptyRequest {},
                    "同步卡片",
                )
                .await;
                sync_busy.set(false);
                match result {
                    Ok(response) => {
                        let summary = response.summary;
                        notice.set(Some(format!(
                            "同步完成：新增 {}，更新 {}，恢复 {}，停用 {}，错误 {}",
                            summary.inserted,
                            summary.updated,
                            summary.reactivated,
                            summary.deactivated,
                            summary.errors.len()
                        )));
                        reload.set((*reload).wrapping_add(1));
                    }
                    Err(error) => notice.set(Some(error.to_string())),
                }
            });
        })
    };

    let open_cards = {
        let route_state = props.route_state.clone();
        Callback::from(move |_| {
            navigate_route(
                &route_state,
                PortalRoute::study(StudyView::Cards, None, Vec::new()),
                false,
            )
        })
    };
    let open_review = {
        let route_state = props.route_state.clone();
        let deck = props.route.study_deck.clone();
        let tags = props.route.study_tags.clone();
        Callback::from(move |_| {
            navigate_route(
                &route_state,
                PortalRoute::study(StudyView::Review, deck.clone(), tags.clone()),
                false,
            )
        })
    };

    match props.route.study_view {
        StudyView::Dashboard => html! {
            <>
                <div class={VIEW_HEADING}>
                    <div>
                        <p class={EYEBROW}>{"STUDY"}</p>
                        <h2 id="study-title" class={SECTION_TITLE}>{"学习"}</h2>
                    </div>
                    <span class={SECTION_META}>{"Markdown 卡片与间隔复习"}</span>
                </div>
                <SectionCard title_id="study-flashcards-title" busy={Some(summary.is_none())}>
                    <div class={SECTION_HEAD}>
                        <div>
                            <h3 id="study-flashcards-title" class={SECTION_TITLE}>{"Flashcards"}</h3>
                            <p class={SECTION_META}>{"内容来自 /mnt/hyz-cards；同步会更新索引与复习队列。"}</p>
                        </div>
                        <div class={BUTTON_ROW}>
                            <button class={BUTTON} type="button" data-testid="study-open-cards" onclick={open_cards.clone()}>{"卡片库"}</button>
                            <button class={BUTTON_PRIMARY} type="button" data-testid="study-sync" onclick={on_sync.clone()} disabled={*sync_busy}>{if *sync_busy { "同步中…" } else { "同步卡片" }}</button>
                        </div>
                    </div>
                    if let Some(notice) = (*notice).clone() {
                        <div class={FEEDBACK} role="status" data-testid="study-notice">{notice}</div>
                    }
                    if let Some(error) = (*summary_error).clone() {
                        <ErrorState message={format!("学习摘要读取失败：{error}")} />
                    } else if let Some(summary) = (*summary).clone() {
                        <div class="grid grid-cols-2 gap-3 sm:grid-cols-4" data-testid="study-summary">
                            <StudyMetric label="今日待复习" value={summary.due_cards.to_string()} />
                            <StudyMetric label="新卡" value={summary.new_cards.to_string()} />
                            <StudyMetric label="活跃卡片" value={summary.active_cards.to_string()} />
                            <StudyMetric label="Deck / Tag" value={format!("{} / {}", summary.deck_count, summary.tag_count)} />
                        </div>
                        <div class={BUTTON_ROW}>
                            <button class={BUTTON_PRIMARY} type="button" data-testid="study-start-review" onclick={open_review} disabled={summary.due_cards == 0}>{"开始复习"}</button>
                            if !props.is_admin {
                                <span class={HELP_TEXT}>{"阅读和复习开放；同步需要管理员会话。"}</span>
                            }
                        </div>
                    }
                </SectionCard>
                <CustomCountdownPanel />
                <ExamCountdownPanel />
            </>
        },
        StudyView::Cards => html! {
            <>
                <div class={VIEW_HEADING}>
                    <div>
                        <p class={EYEBROW}>{"STUDY / CARDS"}</p>
                        <h2 id="study-cards-title" class={SECTION_TITLE}>{"卡片库"}</h2>
                    </div>
                    <button class={BUTTON_GHOST} type="button" onclick={open_review}>{"开始复习"}</button>
                </div>
                <CardsLibrary
                    cards={(*cards).clone()}
                    decks={(*decks).clone()}
                    tags={(*tags).clone()}
                    error={(*cards_error).clone()}
                    route={props.route.clone()}
                    route_state={props.route_state.clone()}
                />
                <CustomCountdownPanel />
                <ExamCountdownPanel />
            </>
        },
        StudyView::Review => Html::default(),
    }
}

#[derive(Properties, PartialEq)]
struct StudyMetricProps {
    label: AttrValue,
    value: AttrValue,
}

#[function_component(StudyMetric)]
fn study_metric(props: &StudyMetricProps) -> Html {
    html! {
        <article class="grid gap-1 rounded-box border border-base-content/10 bg-base-100/60 p-4 shadow-sm">
            <span class="text-xs text-base-content/65">{props.label.clone()}</span>
            <strong class="font-mono text-2xl font-black">{props.value.clone()}</strong>
        </article>
    }
}

#[function_component(CardsLibrary)]
fn cards_library(props: &CardsLibraryProps) -> Html {
    let on_deck = {
        let route_state = props.route_state.clone();
        let tags = props.route.study_tags.clone();
        Callback::from(move |event: web_sys::Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            let deck = (!value.is_empty()).then_some(value);
            navigate_route(
                &route_state,
                PortalRoute::study(StudyView::Cards, deck, tags.clone()),
                false,
            );
        })
    };

    let deck = props.route.study_deck.clone();
    let tag_buttons = props.tags.iter().map(|tag| {
        let selected = props.route.study_tags.iter().any(|value| value == &tag.tag);
        let route_state = props.route_state.clone();
        let current_deck = deck.clone();
        let current_tags = props.route.study_tags.clone();
        let tag_value = tag.tag.clone();
        let onclick = Callback::from(move |_| {
            let mut next_tags = current_tags.clone();
            if let Some(index) = next_tags.iter().position(|value| value == &tag_value) {
                next_tags.remove(index);
            } else {
                next_tags.push(tag_value.clone());
            }
            navigate_route(
                &route_state,
                PortalRoute::study(StudyView::Cards, current_deck.clone(), next_tags),
                false,
            );
        });
        html! {
            <button
                class={classes!("badge", "badge-lg", "cursor-pointer", selected.then_some("badge-primary"))}
                type="button"
                aria-pressed={selected.to_string()}
                onclick={onclick}
            >
                {format!("{} ({})", tag.tag, tag.card_count)}
            </button>
        }
    });

    html! {
        <SectionCard title_id="study-card-library-title">
            <div class={SECTION_HEAD}>
                <div>
                    <h3 id="study-card-library-title" class={SECTION_TITLE}>{"筛选"}</h3>
                    <p class={SECTION_META}>{"Deck 支持目录树匹配；多个 Tag 同时满足。"}</p>
                </div>
                <span class={SECTION_META}>{props.cards.as_ref().map_or("读取中…".to_owned(), |cards| format!("{} 张卡片", cards.len()))}</span>
            </div>
            <div class="grid gap-4 md:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
                <label class={FIELD}>
                    <span class={FIELD_LABEL}>{"Deck"}</span>
                    <select class={SELECT} data-testid="study-deck-filter" onchange={on_deck}>
                        <option value="" selected={props.route.study_deck.is_none()}>{"全部 Deck"}</option>
                        {for props.decks.iter().map(|item| html! {
                            <option value={item.path.clone()} selected={props.route.study_deck.as_deref() == Some(item.path.as_str())}>{format!("{} ({})", item.path, item.card_count)}</option>
                        })}
                    </select>
                </label>
                <div class="grid gap-2">
                    <span class={FIELD_LABEL}>{"Tag"}</span>
                    <div class="flex flex-wrap gap-2" data-testid="study-tag-filter">
                        {for tag_buttons}
                        if props.tags.is_empty() { <span class={HELP_TEXT}>{"暂无 Tag"}</span> }
                    </div>
                </div>
            </div>
            if let Some(error) = &props.error {
                <ErrorState message={format!("卡片库读取失败：{error}")} />
            } else if let Some(cards) = &props.cards {
                if cards.is_empty() {
                    <div class={EMPTY_STATE} data-testid="study-empty-cards"><strong class={EMPTY_TITLE}>{"没有符合条件的卡片"}</strong><p class={EMPTY_COPY}>{"可以调整 Deck 或 Tag 筛选。"}</p></div>
                } else {
                    <div class="grid gap-3" data-testid="study-card-list">
                        {for cards.iter().map(card_library_item)}
                    </div>
                }
            } else {
                <div class={LOADING_GRID}><div class={SKELETON}></div><div class={SKELETON}></div></div>
            }
        </SectionCard>
    }
}

fn card_library_item(card: &FlashcardSummary) -> Html {
    html! {
        <article class="grid min-w-0 gap-3 rounded-box border border-base-content/10 bg-base-100/60 p-4 shadow-sm" data-testid="study-card-item">
            <div class="flex flex-wrap items-center justify-between gap-2 text-xs">
                <span class="font-semibold text-primary">{if card.deck_path.is_empty() { "根目录" } else { card.deck_path.as_str() }}</span>
                <span class="font-mono text-base-content/55">{card.source_file.clone()}</span>
            </div>
            <div class="flex flex-wrap gap-1.5">
                {for card.tags.iter().map(|tag| html! { <span class="badge badge-outline text-[0.65rem]">{tag}</span> })}
            </div>
            <div class="grid gap-3 md:grid-cols-2">
                <div class="study-markdown rounded-box border border-base-content/10 bg-base-200/40 p-3" aria-label="卡片正面">
                    <span class="mb-2 block text-[0.65rem] font-bold uppercase tracking-wider text-base-content/55">{"Front"}</span>
                    {render_markdown(&card.front_markdown, &card.source_file)}
                </div>
                <div class="study-markdown rounded-box border border-base-content/10 bg-base-200/40 p-3" aria-label="卡片背面">
                    <span class="mb-2 block text-[0.65rem] font-bold uppercase tracking-wider text-base-content/55">{"Back"}</span>
                    {render_markdown(&card.back_markdown, &card.source_file)}
                </div>
            </div>
        </article>
    }
}

#[function_component(StudyReview)]
fn study_review(props: &StudyReviewProps) -> Html {
    let queue = use_state(|| None::<Vec<FlashcardSummary>>);
    let total_count = use_state(|| 0_usize);
    let loading = use_state(|| true);
    let error = use_state(|| None::<String>);
    let notice = use_state(|| None::<String>);
    let current_index = use_state(|| 0_usize);
    let answer_shown = use_state(|| false);
    let review_busy = use_state(|| false);

    let filter_dependencies = (
        props.route.study_deck.clone(),
        props.route.study_tags.clone(),
    );
    {
        let queue = queue.clone();
        let total_count = total_count.clone();
        let loading = loading.clone();
        let error = error.clone();
        let notice = notice.clone();
        let current_index = current_index.clone();
        let answer_shown = answer_shown.clone();
        use_effect_with(filter_dependencies, move |dependencies| {
            loading.set(true);
            error.set(None);
            notice.set(None);
            current_index.set(0);
            answer_shown.set(false);
            let query = study_query(
                dependencies.0.as_deref(),
                &dependencies.1,
                Some(200),
                Some(0),
            );
            spawn_local(async move {
                match fetch_json_typed::<StudyCardsResponse>(
                    &format!("{STUDY_REVIEW_QUEUE_ENDPOINT}{query}"),
                    "复习队列",
                )
                .await
                {
                    Ok(response) => {
                        total_count.set(response.cards.len());
                        queue.set(Some(response.cards));
                    }
                    Err(fetch_error) => {
                        queue.set(Some(Vec::new()));
                        error.set(Some(fetch_error.to_string()));
                    }
                }
                loading.set(false);
            });
            || ()
        });
    }

    let csrf = props
        .state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let submit_rating = {
        let queue = queue.clone();
        let current_index = current_index.clone();
        let answer_shown = answer_shown.clone();
        let review_busy = review_busy.clone();
        let notice = notice.clone();
        let csrf = csrf.clone();
        Callback::from(move |rating: ReviewRating| {
            if *review_busy || !*answer_shown {
                return;
            }
            let Some(items) = (*queue).as_ref() else {
                return;
            };
            let index = *current_index;
            let Some(card) = items.get(index) else {
                return;
            };
            if csrf.is_empty() {
                notice.set(Some("安全令牌尚未准备好，请稍后重试".to_owned()));
                return;
            }
            review_busy.set(true);
            notice.set(None);
            let card_id = card.id.clone();
            let queue = queue.clone();
            let current_index = current_index.clone();
            let answer_shown = answer_shown.clone();
            let review_busy = review_busy.clone();
            let notice = notice.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response_typed::<_, StudyReviewResponse>(
                    STUDY_REVIEW_ENDPOINT,
                    &csrf,
                    &StudyReviewRequest { card_id, rating },
                    "记录复习",
                )
                .await;
                match result {
                    Ok(_) => {
                        let mut next = (*queue).clone().unwrap_or_default();
                        if index < next.len() {
                            next.remove(index);
                        }
                        queue.set(Some(next.clone()));
                        current_index.set(index.min(next.len().saturating_sub(1)));
                        answer_shown.set(false);
                        notice.set(Some(format!("已记录：{}", rating_label(rating))));
                    }
                    Err(error) => notice.set(Some(error.to_string())),
                }
                review_busy.set(false);
            });
        })
    };

    {
        let answer_shown = answer_shown.clone();
        let submit_rating = submit_rating.clone();
        let review_busy = review_busy.clone();
        use_effect_with(csrf.clone(), move |_| {
            let window = web_sys::window().expect("browser window");
            let listener = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
                if *review_busy {
                    return;
                }
                match event.key().as_str() {
                    " " | "Spacebar" => {
                        event.prevent_default();
                        answer_shown.set(true);
                    }
                    "1" => submit_rating.emit(ReviewRating::Again),
                    "2" => submit_rating.emit(ReviewRating::Hard),
                    "3" => submit_rating.emit(ReviewRating::Good),
                    _ => {}
                }
            });
            let _ = window
                .add_event_listener_with_callback("keydown", listener.as_ref().unchecked_ref());
            move || {
                let _ = window.remove_event_listener_with_callback(
                    "keydown",
                    listener.as_ref().unchecked_ref(),
                );
            }
        });
    }

    let exit = {
        let route_state = props.route_state.clone();
        Callback::from(move |_| {
            navigate_route(
                &route_state,
                PortalRoute::study(StudyView::Dashboard, None, Vec::new()),
                false,
            )
        })
    };
    let current_card = queue
        .as_ref()
        .and_then(|items| items.get(*current_index))
        .cloned();
    let remaining = queue.as_ref().map_or(0, Vec::len);
    {
        let dependencies = (*loading, *answer_shown, remaining);
        use_effect_with(dependencies, move |_| {
            hydrate_math();
            || ()
        });
    }
    let position = if remaining == 0 {
        0
    } else {
        total_count.saturating_sub(remaining).saturating_add(1)
    };
    let scope = props
        .route
        .study_deck
        .clone()
        .unwrap_or_else(|| "全部 Deck".to_owned());
    let scope = if props.route.study_tags.is_empty() {
        scope
    } else {
        format!("{scope} · {}", props.route.study_tags.join(" · "))
    };

    html! {
        <div class="study-review-shell min-h-screen bg-base-300 px-4 py-4 sm:px-8 sm:py-8" data-testid="study-review-layout">
            <header class="mx-auto flex w-full max-w-5xl items-center justify-between gap-4">
                <button class="btn btn-ghost btn-sm" type="button" data-testid="study-review-exit" onclick={exit.clone()}>{"← 退出复习"}</button>
                <div class="text-right">
                    <p class="text-xs font-bold uppercase tracking-[0.2em] text-primary">{"FLASHCARDS"}</p>
                    <p class="text-sm text-base-content/65">{scope}</p>
                </div>
            </header>
            <main class="mx-auto grid w-full max-w-5xl gap-5 py-8">
                <div class="flex items-center justify-between gap-3 text-sm text-base-content/65">
                    <span>{if position == 0 { "复习队列".to_owned() } else { format!("{position} / {}", *total_count) }}</span>
                    <span>{"Space 显示答案 · 1 / 2 / 3 评分"}</span>
                </div>
                if *loading {
                    <div class="skeleton h-96 w-full rounded-box bg-base-200"></div>
                } else if let Some(error) = (*error).clone() {
                    <div class={EMPTY_STATE} role="alert"><strong class={EMPTY_TITLE}>{"复习队列不可用"}</strong><p class={EMPTY_COPY}>{error}</p></div>
                } else if let Some(card) = current_card {
                    <article class="grid min-h-[22rem] gap-6 rounded-box border border-base-content/10 bg-base-100 p-6 shadow-xl sm:p-10" data-testid="study-review-card">
                        <div class="flex flex-wrap items-start justify-between gap-3">
                            <div>
                                <p class="text-xs font-bold uppercase tracking-wider text-primary">{"Front"}</p>
                                <p class="mt-1 text-xs text-base-content/55">{card.deck_path.clone()}</p>
                            </div>
                            <div class="flex flex-wrap gap-1.5">{for card.tags.iter().map(|tag| html! { <span class="badge badge-outline text-[0.65rem]">{tag}</span> })}</div>
                        </div>
                        <div class="study-markdown text-lg leading-relaxed sm:text-xl">{render_markdown(&card.front_markdown, &card.source_file)}</div>
                        if *answer_shown {
                            <div class="border-t border-base-content/10 pt-6" data-testid="study-review-answer">
                                <p class="text-xs font-bold uppercase tracking-wider text-secondary">{"Back"}</p>
                                <div class="study-markdown mt-3 text-base leading-relaxed">{render_markdown(&card.back_markdown, &card.source_file)}</div>
                            </div>
                        } else {
                            <button class={BUTTON_PRIMARY} type="button" data-testid="study-show-answer" onclick={Callback::from({ let answer_shown = answer_shown.clone(); move |_| answer_shown.set(true) })}>{"显示答案"}</button>
                        }
                    </article>
                    <div class="grid grid-cols-3 gap-3 sm:mx-auto sm:w-full sm:max-w-2xl">
                        <button class="btn btn-error min-h-14 text-base" type="button" data-testid="study-rate-again" disabled={!*answer_shown || *review_busy} onclick={Callback::from({ let submit_rating = submit_rating.clone(); move |_| submit_rating.emit(ReviewRating::Again) })}>{"不会"}</button>
                        <button class="btn btn-warning min-h-14 text-base" type="button" data-testid="study-rate-hard" disabled={!*answer_shown || *review_busy} onclick={Callback::from({ let submit_rating = submit_rating.clone(); move |_| submit_rating.emit(ReviewRating::Hard) })}>{"模糊"}</button>
                        <button class="btn btn-success min-h-14 text-base" type="button" data-testid="study-rate-good" disabled={!*answer_shown || *review_busy} onclick={Callback::from({ let submit_rating = submit_rating.clone(); move |_| submit_rating.emit(ReviewRating::Good) })}>{"会了"}</button>
                    </div>
                } else {
                    <div class={EMPTY_STATE} data-testid="study-review-complete"><strong class={EMPTY_TITLE}>{"复习完成"}</strong><p class={EMPTY_COPY}>{"当前筛选范围没有更多待复习卡片。"}</p><button class={BUTTON_PRIMARY} type="button" onclick={exit.clone()}>{"返回学习"}</button></div>
                }
                if let Some(notice) = (*notice).clone() { <div class={FEEDBACK} role="status">{notice}</div> }
            </main>
        </div>
    }
}

fn hydrate_math() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(renderer) = Reflect::get(&window, &JsValue::from_str("hyzRenderMath")) else {
        return;
    };
    let Ok(renderer) = renderer.dyn_into::<Function>() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let _ = renderer.call1(&window, &document.into());
}

fn rating_label(rating: ReviewRating) -> &'static str {
    match rating {
        ReviewRating::Again => "不会",
        ReviewRating::Hard => "模糊",
        ReviewRating::Good => "会了",
    }
}

fn render_markdown(markdown: &str, source_file: &str) -> Html {
    Html::from_html_unchecked(AttrValue::from(markdown_html(markdown, source_file)))
}

fn markdown_html(markdown: &str, source_file: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_MATH);
    let events = Parser::new_ext(markdown, options).map(|event| match event {
        MarkdownEvent::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => MarkdownEvent::Start(Tag::Image {
            link_type,
            dest_url: CowStr::Boxed(
                resolve_asset_url(source_file, dest_url.as_ref())
                    .unwrap_or_default()
                    .into_boxed_str(),
            ),
            title,
            id,
        }),
        MarkdownEvent::Html(raw) | MarkdownEvent::InlineHtml(raw) => MarkdownEvent::Text(raw),
        event => event,
    });
    let mut html = String::new();
    push_html(&mut html, events);
    html
}

fn resolve_asset_url(source_file: &str, destination: &str) -> Option<String> {
    if destination.is_empty()
        || destination.starts_with('/')
        || destination.contains('\\')
        || destination.contains('\0')
        || destination.contains("://")
        || destination.starts_with("data:")
    {
        return None;
    }
    let source_directory = source_file
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let mut segments = source_directory
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    for segment in destination.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            segment => segments.push(segment),
        }
    }
    if segments.is_empty() {
        return None;
    }
    let encoded = segments
        .into_iter()
        .map(|segment| url::form_urlencoded::byte_serialize(segment.as_bytes()).collect::<String>())
        .collect::<Vec<_>>()
        .join("/");
    Some(format!("/api/v1/study/assets/{encoded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_escapes_raw_html_and_resolves_relative_assets() {
        let html = markdown_html(
            "<script>alert(1)</script>\n\n![题目](./images/q001.png)\n\n$$x^2$$",
            "判断推理/cards.md",
        );
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html
            .contains("/api/v1/study/assets/%E5%88%A4%E6%96%AD%E6%8E%A8%E7%90%86/images/q001.png"));
        assert!(html.contains("math-display"));
    }

    #[test]
    fn asset_resolution_rejects_escape_and_external_urls() {
        assert!(resolve_asset_url("deck/cards.md", "../../secret.png").is_none());
        assert!(resolve_asset_url("deck/cards.md", "https://example.com/a.png").is_none());
        assert_eq!(
            resolve_asset_url("deck/cards.md", "./images/a.png").as_deref(),
            Some("/api/v1/study/assets/deck/images/a.png")
        );
    }

    #[test]
    fn ratings_keep_the_three_user_facing_labels() {
        assert_eq!(rating_label(ReviewRating::Again), "不会");
        assert_eq!(rating_label(ReviewRating::Hard), "模糊");
        assert_eq!(rating_label(ReviewRating::Good), "会了");
    }
}
