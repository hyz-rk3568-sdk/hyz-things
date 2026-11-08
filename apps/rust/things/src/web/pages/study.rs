use super::*;
use hyz_things::domain::flashcards::{DeckSummary, FlashcardSummary, StudySummary, TagSummary};

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
struct StudyScopePickerProps {
    decks: Vec<DeckSummary>,
    tags: Vec<TagSummary>,
    route: PortalRoute,
    route_state: UseStateHandle<PortalRoute>,
    view: StudyView,
    test_prefix: String,
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
    let open_calculation = {
        let route_state = props.route_state.clone();
        Callback::from(move |_| {
            navigate_route(
                &route_state,
                PortalRoute::study(StudyView::Training, None, Vec::new()),
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
                        <h2 id="study-title" class={SECTION_TITLE}>{"学习中心"}</h2>
                        <p class="mt-1 text-xs text-base-content/60">{"考试进度 · 卡片复习 · 专项训练"}</p>
                    </div>
                </div>
                <ExamCountdownPanel on_training={open_calculation.clone()}/>
                if props.route.page == AppPage::Study {
                    <QuestionBankPanel state={props.state.clone()} is_admin={props.is_admin}/>
                }
                <div class="mt-5 grid gap-4 lg:grid-cols-2" data-testid="study-quick-start">
                    <SectionCard title_id="study-flashcards-title" busy={Some(summary.is_none())}>
                        <div class={SECTION_HEAD}>
                            <div>
                                <h3 id="study-flashcards-title" class={SECTION_TITLE}>{"Flashcards"}</h3>
                                <p class={SECTION_META}>{"间隔复习 · 数据来自 /mnt/hyz-cards"}</p>
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
                            <div class="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-2 xl:grid-cols-4" data-testid="study-summary">
                                <StudyMetric label="今日待复习" value={summary.due_cards.to_string()} />
                                <StudyMetric label="新卡" value={summary.new_cards.to_string()} />
                                <StudyMetric label="活跃卡片" value={summary.active_cards.to_string()} />
                                <StudyMetric label="Deck / Tag" value={format!("{} / {}", summary.deck_count, summary.tag_count)} />
                            </div>
                            if !props.is_admin {
                                <p class={HELP_TEXT}>{"阅读开放；同步需要管理员权限。复习请前往专项训练。"}</p>
                            }
                        }
                    </SectionCard>
                    <SectionCard title_id="study-calculation-title">
                        <div class={SECTION_HEAD}>
                            <div>
                                <h3 id="study-calculation-title" class={SECTION_TITLE}>{"专项训练"}</h3>
                                <p class={SECTION_META}>{"固定全屏答题 · Apple Pencil 草稿"}</p>
                            </div>
                            <span class="badge badge-outline">{"5种训练模式"}</span>
                        </div>
                        <p class="text-sm leading-7 text-base-content/75">{"速算训练 / 闪卡回顾 / 资料分析 / 判断推理 / 数量关系 · 学习统计"}</p>
                        <p class="text-xs text-base-content/60">{"综合训练支持原题重做、暂停及跨设备续答。"}</p>
                        <div class={BUTTON_ROW}>
                            <button class={BUTTON_PRIMARY} type="button" data-testid="study-open-calculation" onclick={open_calculation}>{"选择训练 →"}</button>
                        </div>
                    </SectionCard>
                </div>

            </>
        },
        StudyView::Cards => html! {
            <>
                <div class={VIEW_HEADING}>
                    <div>
                        <p class={EYEBROW}>{"STUDY / CARDS"}</p>
                        <h2 id="study-cards-title" class={SECTION_TITLE}>{"卡片库"}</h2>
                    </div>
                </div>
                <CardsLibrary
                    cards={(*cards).clone()}
                    decks={(*decks).clone()}
                    tags={(*tags).clone()}
                    error={(*cards_error).clone()}
                    route={props.route.clone()}
                    route_state={props.route_state.clone()}
                />
                <ExamCountdownPanel />
            </>
        },
        StudyView::Training => Html::default(),
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

#[function_component(StudyScopePicker)]
fn study_scope_picker(props: &StudyScopePickerProps) -> Html {
    let tags_open = use_state(|| false);
    let on_deck = {
        let route_state = props.route_state.clone();
        let tags = props.route.study_tags.clone();
        let view = props.view;
        Callback::from(move |event: web_sys::Event| {
            let value = event.target_unchecked_into::<HtmlSelectElement>().value();
            let deck = (!value.is_empty()).then_some(value);
            navigate_route(
                &route_state,
                PortalRoute::study(view, deck, tags.clone()),
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
        let view = props.view;
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
                PortalRoute::study(view, current_deck.clone(), next_tags),
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
    let prefix = props.test_prefix.as_str();
    let tags_expanded = *tags_open;
    let toggle_tags = {
        let tags_open = tags_open.clone();
        Callback::from(move |_| tags_open.set(!*tags_open))
    };
    let tag_summary = match props.route.study_tags.as_slice() {
        [] => "全部 Tag".to_owned(),
        selected if selected.len() <= 2 => selected.join("、"),
        selected => format!("{} 等 {} 个", selected[0], selected.len()),
    };
    let toggle_label = if tags_expanded {
        "收起 Tag 筛选"
    } else {
        "展开 Tag 筛选"
    };
    let tag_panel_id = format!("{prefix}-tag-panel");

    html! {
        <div class="relative z-20 grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]" data-testid={format!("{prefix}-scope")}>
            <label class={FIELD}>
                <span class={FIELD_LABEL}>{"Deck"}</span>
                <select class={SELECT} data-testid={format!("{prefix}-deck-filter")} onchange={on_deck}>
                    <option value="" selected={props.route.study_deck.is_none()}>{"全部 Deck"}</option>
                    {for props.decks.iter().map(|item| html! {
                        <option value={item.path.clone()} selected={props.route.study_deck.as_deref() == Some(item.path.as_str())}>{format!("{} ({})", item.path, item.card_count)}</option>
                    })}
                </select>
            </label>
            <div class="relative min-w-0">
                <div class="flex items-center justify-between gap-2">
                    <div class="min-w-0">
                        <div class="flex min-w-0 items-center gap-2">
                            <span class={FIELD_LABEL}>{"Tag"}</span>
                            <span class="truncate text-xs text-base-content/60" title={tag_summary.clone()}>{tag_summary}</span>
                        </div>
                    </div>
                    <button
                        class="btn btn-ghost btn-sm shrink-0"
                        type="button"
                        data-testid={format!("{prefix}-tag-toggle")}
                        aria-controls={tag_panel_id.clone()}
                        aria-expanded={tags_expanded.to_string()}
                        aria-label={toggle_label}
                        onclick={toggle_tags}
                    >
                        <span aria-hidden="true">{if tags_expanded { "⌃" } else { "⌄" }}</span>
                    </button>
                </div>
                if tags_expanded {
                    <div
                        id={tag_panel_id}
                        class="study-tag-panel"
                        data-testid={format!("{prefix}-tag-panel")}
                        role="region"
                        aria-label="Tag 筛选"
                    >
                        <div class="flex flex-wrap gap-2" data-testid={format!("{prefix}-tag-filter")}>
                            {for tag_buttons}
                            if props.tags.is_empty() {
                                <span class={HELP_TEXT}>{"暂无 Tag"}</span>
                            }
                        </div>
                    </div>
                }
            </div>
        </div>
    }
}

#[function_component(CardsLibrary)]
fn cards_library(props: &CardsLibraryProps) -> Html {
    html! {
        <SectionCard title_id="study-card-library-title">
            <div class={SECTION_HEAD}>
                <div>
                    <h3 id="study-card-library-title" class={SECTION_TITLE}>{"筛选"}</h3>
                    <p class={SECTION_META}>{"Deck 支持目录树匹配；多个 Tag 同时满足。"}</p>
                </div>
                <span class={SECTION_META}>{props.cards.as_ref().map_or("读取中…".to_owned(), |cards| format!("{} 张卡片", cards.len()))}</span>
            </div>
            <StudyScopePicker
                decks={props.decks.clone()}
                tags={props.tags.clone()}
                route={props.route.clone()}
                route_state={props.route_state.clone()}
                view={StudyView::Cards}
                test_prefix={"study".to_owned()}
            />
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
}
