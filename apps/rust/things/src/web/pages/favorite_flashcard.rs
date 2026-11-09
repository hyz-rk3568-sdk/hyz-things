use super::*;
use hyz_things::domain::question_bank::{FavoriteState, SetFavorite};

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FavoriteCardMode {
    Practice,
    UserOrganized,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
pub(crate) struct FavoriteExistingCard {
    pub(crate) card_id: String,
    pub(crate) deck_path: String,
    pub(crate) source_file: String,
    pub(crate) front: String,
    pub(crate) back: String,
    pub(crate) mode: Option<FavoriteCardMode>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
pub(crate) struct FavoritePreview {
    pub(crate) eligible: bool,
    pub(crate) reason: Option<String>,
    pub(crate) front: String,
    pub(crate) correct_answer: Vec<String>,
    pub(crate) recommended_target: Option<String>,
    pub(crate) allowed_targets: Vec<String>,
    pub(crate) requires_type_confirmation: bool,
    pub(crate) source_revision: String,
    #[serde(default)]
    pub(crate) existing: Vec<FavoriteExistingCard>,
}

#[derive(Clone, serde::Serialize)]
struct CreateFavoriteCard {
    creation_mode: &'static str,
    target_deck: String,
    back: String,
    source_revision: String,
    type_confirmed: bool,
}

#[derive(Clone, serde::Deserialize)]
struct FavoriteCardResult {
    status: String,
    target_deck: String,
    source_file: String,
    card_id: Option<String>,
    retryable: bool,
}

#[derive(Clone, serde::Deserialize)]
struct FavoriteMutationResult {
    status: String,
    source_file: String,
    retryable: bool,
}

#[derive(Clone, serde::Serialize)]
struct UpdateFavoriteCard {
    back: String,
}

fn favorite_endpoint(id: &str, action: &str) -> String {
    format!("/api/v1/question-bank/practice/questions/{id}/{action}")
}

fn favorite_card_endpoint(id: &str, card_id: &str) -> String {
    format!("/api/v1/question-bank/practice/questions/{id}/flashcard/{card_id}")
}

fn target_mode(path: &str) -> Option<FavoriteCardMode> {
    if path.starts_with("真题练习/") {
        Some(FavoriteCardMode::Practice)
    } else if path.starts_with("用户整理/") {
        Some(FavoriteCardMode::UserOrganized)
    } else {
        None
    }
}

fn mode_occupied(preview: &FavoritePreview, mode: FavoriteCardMode) -> bool {
    preview.existing.iter().any(|card| match card.mode {
        Some(existing) => existing == mode,
        None => true,
    })
}

pub(crate) fn favorite_can_create(preview: &FavoritePreview) -> bool {
    preview.eligible && preview.existing.is_empty() && !preview.allowed_targets.is_empty()
}

fn favorite_default_target(preview: &FavoritePreview) -> String {
    preview
        .allowed_targets
        .iter()
        .find(|target| target_mode(target).is_some_and(|mode| !mode_occupied(preview, mode)))
        .cloned()
        .unwrap_or_default()
}

fn favorite_content(value: &FavoritePreview) -> Html {
    html! {
        <>
            <div class="study-markdown question-bank-passage">{render_question_bank_markdown(&value.front)}</div>
            <section class="rounded-box border border-base-content/15 p-4" aria-label="正确答案">
                <h3 class="font-semibold">{"正确答案"}</h3>
                <p data-testid="favorite-question-answer">{if value.correct_answer.is_empty() {"原题暂无标准答案".to_owned()} else {value.correct_answer.join("、")}}</p>
            </section>
        </>
    }
}

#[derive(Clone, PartialEq)]
enum FavoriteCardFormMode {
    Create,
    Edit(FavoriteExistingCard),
}

#[derive(Clone, Properties, PartialEq)]
struct FavoriteCardFormProps {
    question_id: String,
    preview: FavoritePreview,
    csrf: String,
    mode: FavoriteCardFormMode,
    #[prop_or_default]
    existing_cards: Vec<FavoriteExistingCard>,
    #[prop_or_default]
    on_select_card: Callback<String>,
    on_close: Callback<()>,
    on_changed: Callback<()>,
}

#[function_component(FavoriteCardForm)]
fn favorite_card_form(props: &FavoriteCardFormProps) -> Html {
    let existing = match &props.mode {
        FavoriteCardFormMode::Edit(card) => Some(card.clone()),
        FavoriteCardFormMode::Create => None,
    };
    let editing = existing.is_some();
    let default_target = favorite_default_target(&props.preview);
    let back = use_state({
        let initial = existing
            .as_ref()
            .map(|card| card.back.clone())
            .unwrap_or_default();
        move || initial
    });
    let target = use_state({
        let default_target = default_target.clone();
        move || default_target
    });
    let user_mode = use_state({
        let default_target = default_target.clone();
        move || default_target.starts_with("用户整理/")
    });
    let confirmed = use_state(|| false);
    let also_favorite = use_state(|| false);
    let busy = use_state(|| false);
    let locked = use_state(|| false);
    let error = use_state(|| None::<String>);
    let create_result = use_state(|| None::<FavoriteCardResult>);
    let mutation_result = use_state(|| None::<FavoriteMutationResult>);
    let confirming_delete = use_state(|| false);
    let delete_mode = use_state(|| false);
    let frozen_create = use_mut_ref(|| None::<CreateFavoriteCard>);
    let frozen_back = use_mut_ref(|| None::<String>);
    let in_flight = use_mut_ref(|| false);
    let mounted = use_mut_ref(|| true);
    let dialog = use_node_ref();
    {
        let mounted = mounted.clone();
        let dialog = dialog.clone();
        use_effect_with((), move |_| {
            hydrate_math();
            if let Some(element) = dialog.cast::<HtmlElement>() {
                let _ = element.focus();
            }
            move || *mounted.borrow_mut() = false
        });
    }
    {
        let back = (*back).clone();
        use_effect_with(back, move |_| {
            hydrate_math();
            || ()
        });
    }
    let submit_create = {
        let back = back.clone();
        let target = target.clone();
        let user_mode = user_mode.clone();
        let confirmed = confirmed.clone();
        let busy = busy.clone();
        let locked = locked.clone();
        let error = error.clone();
        let create_result = create_result.clone();
        let also_favorite = also_favorite.clone();
        let frozen = frozen_create.clone();
        let in_flight = in_flight.clone();
        let mounted = mounted.clone();
        let props = props.clone();
        Callback::from(move |_: MouseEvent| {
            if *in_flight.borrow() || props.csrf.is_empty() {
                return;
            }
            let request = frozen.borrow().clone().or_else(|| {
                ((if *user_mode {
                    props.preview.allowed_targets.contains(&*target)
                        && target_mode(&target) == Some(FavoriteCardMode::UserOrganized)
                        && !mode_occupied(&props.preview, FavoriteCardMode::UserOrganized)
                } else {
                    props.preview.allowed_targets.contains(&*target)
                        && target_mode(&target) == Some(FavoriteCardMode::Practice)
                        && !mode_occupied(&props.preview, FavoriteCardMode::Practice)
                }) && *confirmed
                    && (!props.preview.correct_answer.is_empty() || !back.trim().is_empty()))
                .then(|| CreateFavoriteCard {
                    creation_mode: if *user_mode {
                        "user_organized"
                    } else {
                        "practice"
                    },
                    target_deck: (*target).clone(),
                    back: (*back).clone(),
                    source_revision: props.preview.source_revision.clone(),
                    type_confirmed: *confirmed,
                })
            });
            let Some(request) = request else { return };
            *in_flight.borrow_mut() = true;
            *frozen.borrow_mut() = Some(request.clone());
            locked.set(true);
            busy.set(true);
            error.set(None);
            let busy = busy.clone();
            let error = error.clone();
            let create_result = create_result.clone();
            let in_flight = in_flight.clone();
            let mounted = mounted.clone();
            let on_changed = props.on_changed.clone();
            let on_close = props.on_close.clone();
            let csrf = props.csrf.clone();
            let endpoint = favorite_endpoint(&props.question_id, "flashcard");
            let question_id = props.question_id.clone();
            let also_favorite = *also_favorite;
            spawn_local(async move {
                let response = post_json_response::<_, FavoriteCardResult>(
                    &endpoint,
                    &csrf,
                    &request,
                    "创建并同步",
                )
                .await;
                if !*mounted.borrow() {
                    return;
                }
                *in_flight.borrow_mut() = false;
                busy.set(false);
                match response {
                    Ok(value) => {
                        let synced = !value.retryable;
                        let mut favorite_ok = true;
                        if also_favorite {
                            let favorite = post_json_response::<_, FavoriteState>(
                                &format!("/api/v1/question-bank/practice/favorites/{question_id}"),
                                &csrf,
                                &SetFavorite { favorite: true },
                                "收藏本题",
                            )
                            .await;
                            if let Err(message) = favorite {
                                favorite_ok = false;
                                error.set(Some(format!(
                                    "闪卡已保存，但收藏本题失败：{message}。可稍后在题目星标处重试。"
                                )));
                            }
                        }
                        create_result.set(Some(value));
                        if synced {
                            on_changed.emit(());
                            if favorite_ok {
                                on_close.emit(());
                            }
                        }
                    }
                    Err(message) => error.set(Some(message)),
                }
            });
        })
    };
    let submit_edit = {
        let existing = existing.clone();
        let back = back.clone();
        let busy = busy.clone();
        let locked = locked.clone();
        let error = error.clone();
        let mutation_result = mutation_result.clone();
        let frozen_back = frozen_back.clone();
        let in_flight = in_flight.clone();
        let mounted = mounted.clone();
        let delete_mode = delete_mode.clone();
        let on_changed = props.on_changed.clone();
        let on_close = props.on_close.clone();
        let props = props.clone();
        Callback::from(move |_: MouseEvent| {
            if *in_flight.borrow() || props.csrf.is_empty() {
                return;
            }
            let request = frozen_back
                .borrow()
                .clone()
                .unwrap_or_else(|| (*back).clone());
            *frozen_back.borrow_mut() = Some(request.clone());
            delete_mode.set(false);
            *in_flight.borrow_mut() = true;
            locked.set(true);
            busy.set(true);
            error.set(None);
            let busy = busy.clone();
            let locked = locked.clone();
            let error = error.clone();
            let mutation_result = mutation_result.clone();
            let in_flight = in_flight.clone();
            let mounted = mounted.clone();
            let on_changed = on_changed.clone();
            let on_close = on_close.clone();
            let csrf = props.csrf.clone();
            let Some(card) = existing.as_ref() else {
                return;
            };
            let endpoint = favorite_card_endpoint(&props.question_id, &card.card_id);
            spawn_local(async move {
                let response = put_json_response::<_, FavoriteMutationResult>(
                    &endpoint,
                    &csrf,
                    &UpdateFavoriteCard { back: request },
                    "保存并同步",
                )
                .await;
                if !*mounted.borrow() {
                    return;
                }
                *in_flight.borrow_mut() = false;
                busy.set(false);
                match response {
                    Ok(value) => {
                        let synced = !value.retryable;
                        mutation_result.set(Some(value));
                        if synced {
                            on_changed.emit(());
                            on_close.emit(());
                        }
                    }
                    Err(message) => {
                        locked.set(false);
                        error.set(Some(message));
                    }
                }
            });
        })
    };
    let delete = {
        let existing = existing.clone();
        let busy = busy.clone();
        let locked = locked.clone();
        let error = error.clone();
        let mutation_result = mutation_result.clone();
        let delete_mode = delete_mode.clone();
        let in_flight = in_flight.clone();
        let mounted = mounted.clone();
        let on_changed = props.on_changed.clone();
        let on_close = props.on_close.clone();
        let props = props.clone();
        Callback::from(move |_: MouseEvent| {
            if *in_flight.borrow() || props.csrf.is_empty() {
                return;
            }
            delete_mode.set(true);
            *in_flight.borrow_mut() = true;
            busy.set(true);
            error.set(None);
            let busy = busy.clone();
            let locked = locked.clone();
            let error = error.clone();
            let mutation_result = mutation_result.clone();
            let in_flight = in_flight.clone();
            let mounted = mounted.clone();
            let on_changed = on_changed.clone();
            let on_close = on_close.clone();
            let csrf = props.csrf.clone();
            let Some(card) = existing.as_ref() else {
                return;
            };
            let endpoint = favorite_card_endpoint(&props.question_id, &card.card_id);
            spawn_local(async move {
                let response =
                    delete_json_response::<FavoriteMutationResult>(&endpoint, &csrf, "删除并同步")
                        .await;
                if !*mounted.borrow() {
                    return;
                }
                *in_flight.borrow_mut() = false;
                busy.set(false);
                match response {
                    Ok(value) => {
                        let synced = !value.retryable;
                        mutation_result.set(Some(value));
                        if synced {
                            on_changed.emit(());
                            on_close.emit(());
                        }
                    }
                    Err(message) => {
                        locked.set(false);
                        error.set(Some(message));
                    }
                }
            });
        })
    };
    let practice_occupied = mode_occupied(&props.preview, FavoriteCardMode::Practice);
    let user_occupied = mode_occupied(&props.preview, FavoriteCardMode::UserOrganized);
    let valid_target = if *user_mode {
        props.preview.allowed_targets.contains(&*target)
            && target_mode(&target) == Some(FavoriteCardMode::UserOrganized)
            && !mode_occupied(&props.preview, FavoriteCardMode::UserOrganized)
    } else {
        props.preview.allowed_targets.contains(&*target)
            && target_mode(&target) == Some(FavoriteCardMode::Practice)
            && !mode_occupied(&props.preview, FavoriteCardMode::Practice)
    };
    let can_create = !*busy
        && !props.csrf.is_empty()
        && (*locked
            || (valid_target
                && *confirmed
                && (!props.preview.correct_answer.is_empty() || !back.trim().is_empty())));
    let retry_label = mutation_result
        .as_ref()
        .as_ref()
        .filter(|value| value.retryable)
        .map(|value| value.status.as_str());
    let close = Callback::from({
        let on_close = props.on_close.clone();
        move |_| on_close.emit(())
    });
    let title = if editing {
        "修改 Flashcard"
    } else {
        "创建 Flashcard"
    };
    let subtitle = existing.as_ref().map_or_else(
        || {
            if props.preview.correct_answer.is_empty() {
                "原题暂无标准答案，请在背面填写答案或解题思路".to_owned()
            } else {
                "题目与正确答案将保存到正面".to_owned()
            }
        },
        |card| format!("{} · {}", card.deck_path, card.source_file),
    );
    let outer_test_id = if editing {
        "favorite-flashcard-editor"
    } else {
        "favorite-flashcard-panel"
    };
    let section_test_id = if editing {
        "favorite-flashcard-editor-card"
    } else {
        "favorite-flashcard-card"
    };
    let form_test_id = if editing {
        "favorite-flashcard-editor-form"
    } else {
        "favorite-flashcard-form"
    };
    let back_test_id = if editing {
        "favorite-flashcard-editor-back"
    } else {
        "favorite-flashcard-back-input"
    };
    let current_card_id = existing
        .as_ref()
        .map(|card| card.card_id.clone())
        .unwrap_or_default();
    html! {
        <div class="h-full w-full overflow-y-auto bg-gradient-to-b from-primary/5 via-base-100 to-base-100" data-testid={outer_test_id}>
        <section class="mx-auto flex min-h-full w-full max-w-3xl flex-col px-4 pb-5 sm:px-6" role="region" aria-label={title} tabindex="-1" ref={dialog}
            data-testid={section_test_id}
            onkeydown={Callback::from({let on_close=props.on_close.clone();move |event:KeyboardEvent| {
                if event.key()=="Escape" {event.stop_propagation();on_close.emit(());}
            }})}>
            <header class="flex shrink-0 flex-wrap items-center gap-3 border-b border-base-content/10 py-3">
                <button type="button" class="btn btn-ghost btn-sm -ml-2" aria-label="返回复盘" onclick={close.clone()}>
                    <span aria-hidden="true">{"←"}</span>{"返回复盘"}
                </button>
                <div class="flex min-w-0 flex-1 items-center gap-3">
                    <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-gradient-to-br from-primary via-primary to-secondary text-lg text-primary-content shadow-sm" aria-hidden="true">
                        {if editing {"✎"} else {"✦"}}
                    </span>
                    <div class="min-w-0">
                        <h2 class="truncate text-base font-semibold">{title}</h2>
                        <p class="break-all text-xs text-base-content/55">{subtitle}</p>
                    </div>
                </div>
                if !editing {
                    <div class="hidden shrink-0 items-center gap-1 md:flex" aria-hidden="true">
                        <span class="badge badge-primary badge-outline badge-sm">{"Markdown"}</span>
                        <span class="badge badge-secondary badge-outline badge-sm">{"LaTeX"}</span>
                        <span class="badge badge-accent badge-outline badge-sm">{"图片"}</span>
                    </div>
                }
                if editing && props.existing_cards.len() > 1 {
                    <label class="flex items-center gap-2 text-xs text-base-content/60">
                        <span class="shrink-0">{"切换卡片"}</span>
                        <select class="select select-bordered select-sm max-w-56" value={current_card_id.clone()}
                            data-testid="favorite-flashcard-card-select"
                            onchange={Callback::from({let on_select=props.on_select_card.clone();move |event:Event| {
                                on_select.emit(event.target_unchecked_into::<HtmlSelectElement>().value());
                            }})}>
                            {for props.existing_cards.iter().map(|card| html! {
                                <option value={card.card_id.clone()} selected={card.card_id == current_card_id}>
                                    {card.deck_path.clone()}
                                </option>
                            })}
                        </select>
                    </label>
                }
            </header>
            <div class="mx-auto my-5 w-full min-w-0 space-y-5">
                <div class="relative min-w-0 space-y-5 overflow-hidden break-words rounded-2xl border border-base-content/15 bg-gradient-to-br from-base-100 via-base-100 to-primary/5 p-4 shadow-sm transition focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/15 sm:p-5" data-testid={form_test_id}>
                    <span aria-hidden="true" class="pointer-events-none absolute inset-x-0 top-0 h-1 bg-gradient-to-r from-primary via-secondary to-accent"></span>
                    if !editing {
                        <div class="space-y-2">
                            <span class="flex items-center gap-2 font-semibold">
                                <span class="flex h-6 w-6 items-center justify-center rounded-full bg-gradient-to-br from-primary to-secondary text-xs font-semibold text-primary-content shadow-sm" aria-hidden="true">{"1"}</span>
                                {"存放位置"}
                            </span>
                            <div class="grid grid-cols-2 gap-1 rounded-xl bg-base-200/70 p-1" role="group" aria-label="存放位置" data-testid="favorite-flashcard-mode">
                                <button type="button" class={classes!("flex", "flex-col", "items-start", "rounded-lg", "px-3", "py-2", "text-left", "text-sm", "font-medium", "transition", if !*user_mode {"bg-gradient-to-br from-primary/20 via-secondary/10 to-accent/10 shadow-sm ring-1 ring-primary/30"} else {"text-base-content/60 hover:bg-base-100/70"})}
                                    aria-pressed={(!*user_mode).to_string()} disabled={*locked || practice_occupied}
                                    data-testid="favorite-flashcard-practice-mode"
                                    onclick={Callback::from({let user_mode=user_mode.clone();let target=target.clone();let confirmed=confirmed.clone();let practice=props.preview.allowed_targets.iter().find(|value| target_mode(value)==Some(FavoriteCardMode::Practice) && !mode_occupied(&props.preview, FavoriteCardMode::Practice)).cloned();move |_| {
                                        if *user_mode {user_mode.set(false);target.set(practice.clone().unwrap_or_default());confirmed.set(false);}
                                    }})}>
                                    <span class="flex items-center gap-1.5"><span aria-hidden="true">{"▤"}</span>{"真题练习"}</span>
                                    <span class="text-[11px] font-normal opacity-70">{"按原题分类"}</span>
                                </button>
                                <button type="button" class={classes!("flex", "flex-col", "items-start", "rounded-lg", "px-3", "py-2", "text-left", "text-sm", "font-medium", "transition", if *user_mode {"bg-gradient-to-br from-secondary/20 via-accent/10 to-primary/10 shadow-sm ring-1 ring-secondary/30"} else {"text-base-content/60 hover:bg-base-100/70"})}
                                    aria-pressed={(*user_mode).to_string()} disabled={*locked || user_occupied}
                                    data-testid="favorite-flashcard-user-mode"
                                    onclick={Callback::from({let user_mode=user_mode.clone();let target=target.clone();let confirmed=confirmed.clone();move |_| {
                                        if !*user_mode {user_mode.set(true);target.set(String::new());confirmed.set(false);}
                                    }})}>
                                    <span class="flex items-center gap-1.5"><span aria-hidden="true">{"▥"}</span>{"用户整理"}</span>
                                    <span class="text-[11px] font-normal opacity-70">{"按自定义目录"}</span>
                                </button>
                            </div>
                            <p class="text-xs text-base-content/60">
                                {if *user_mode {
                                    "选择已有分类目录，卡片会保存到该目录的错题文件夹。"
                                } else {
                                    "按原题的模块和分类保存，可切换到用户整理。"
                                }}
                            </p>
                        </div>
                        <label class="block space-y-2">
                            <span class="font-semibold">{"目标牌组"}</span>
                            <select key={if *user_mode {"user"} else {"practice"}} class="select w-full bg-base-200/60" value={(*target).clone()} disabled={*locked}
                                onchange={Callback::from({let target=target.clone();let confirmed=confirmed.clone();move |event:Event| {
                                    target.set(event.target_unchecked_into::<HtmlSelectElement>().value());
                                    confirmed.set(false);
                                }})}>
                                <option value="" disabled={true} selected={target.is_empty()}>{"请选择写入目录"}</option>
                                {for props.preview.allowed_targets.iter().filter(|value| {
                                    value.starts_with(if *user_mode {"用户整理/"} else {"真题练习/"})
                                        && !mode_occupied(&props.preview, if *user_mode {FavoriteCardMode::UserOrganized} else {FavoriteCardMode::Practice})
                                }).map(|value|html! {
                                    <option value={value.clone()} selected={*target==*value}>{if *user_mode {
                                        value.strip_suffix("/错题").unwrap_or(value).to_owned()
                                    } else { value.clone() }}</option>
                                })}
                            </select>
                        </label>
                        if *user_mode && !props.preview.allowed_targets.iter().any(|value| value.starts_with("用户整理/")) {
                            <p class="text-sm text-base-content/60" data-testid="favorite-flashcard-empty-directories">{"当前模块还没有用户整理目录，请先创建分类目录。"}</p>
                        }
                        if !target.is_empty() {
                            <p class="break-all text-xs text-base-content/70" data-testid="favorite-flashcard-path">
                                {format!("将追加或创建：flashcards/{}/cards.md",*target)}
                            </p>
                        }
                        <div class="flex items-start gap-2 rounded-xl border border-info/25 bg-gradient-to-r from-info/10 via-info/5 to-primary/5 p-3 text-xs text-base-content/70">
                            <span aria-hidden="true" class="text-base text-info">{"ⓘ"}</span>
                            <span>{"背面支持 Markdown、LaTeX 公式和图片；保存后会同步到卡片库。"}</span>
                        </div>
                    } else if let Some(card) = existing.as_ref() {
                        <div class="rounded-xl bg-base-200/60 px-4 py-3 text-xs text-base-content/65" data-testid="favorite-flashcard-path">
                            <p class="break-all">{format!("{} · {}", card.deck_path, card.source_file)}</p>
                            <p class="mt-1">{"存放位置在创建后保持不变。"}</p>
                        </div>
                    }
                    <label class="block space-y-2 rounded-xl border border-secondary/20 bg-gradient-to-br from-base-100 to-secondary/5 p-3">
                        <span class="flex items-center gap-2 font-semibold">
                            <span class="flex h-6 w-6 items-center justify-center rounded-full bg-gradient-to-br from-secondary to-accent text-xs font-semibold text-secondary-content shadow-sm" aria-hidden="true">{"2"}</span>
                            {"背面"}
                            <span class="ml-auto text-xs font-normal text-base-content/50">{if !editing && props.preview.correct_answer.is_empty() {"必填"} else {"可选"}}</span>
                        </span>
                        <textarea class="block min-h-32 w-full resize-y rounded-lg border-0 bg-base-100/80 px-3 py-2 text-sm leading-6 focus:outline-none focus:ring-2 focus:ring-primary/20" placeholder="写下解题思路或复习提示，支持 Markdown、公式和图片" value={(*back).clone()} disabled={*locked}
                            data-testid={back_test_id}
                            oninput={Callback::from({let back=back.clone();move |event:InputEvent| {
                                back.set(event.target_unchecked_into::<web_sys::HtmlTextAreaElement>().value());
                            }})}/>
                    </label>
                    if !editing {
                        <div class="grid gap-2 rounded-xl border border-base-content/10 bg-gradient-to-r from-base-100 to-primary/5 p-3 sm:grid-cols-2">
                            <label class="flex items-center gap-2">
                                <input type="checkbox" class="checkbox checkbox-sm checkbox-primary" checked={*also_favorite} disabled={*locked}
                                    data-testid="favorite-flashcard-also-favorite"
                                    onchange={Callback::from({let also_favorite=also_favorite.clone();move |event:Event| {
                                        also_favorite.set(event.target_unchecked_into::<HtmlInputElement>().checked());
                                    }})}/>
                                <span>{"同时收藏本题"}</span>
                            </label>
                            <label class="flex items-center gap-2">
                                <input type="checkbox" class="checkbox checkbox-sm checkbox-primary" checked={*confirmed} disabled={*locked}
                                    onchange={Callback::from({let confirmed=confirmed.clone();move |event:Event| {
                                        confirmed.set(event.target_unchecked_into::<HtmlInputElement>().checked());
                                    }})}/>
                                <span>{"确认写入所选目录"}</span>
                            </label>
                        </div>
                    }
                    if let Some(message) = &*error {
                        <p role="alert" class="text-sm text-error">{message}{if editing {""} else {"。重试会使用同一份请求，不会重复创建。"}}</p>
                    }
                    if let Some(value) = &*create_result {
                        <div role="status" class="space-y-2 rounded-lg bg-base-200/60 p-3 text-sm">
                            <p>{match value.status.as_str() {
                                "created_synced" => "已创建并同步",
                                "created_sync_pending" => "已写入源文件，待同步；请重试同步，不要重复创建。",
                                "already_exists" => "闪卡已存在",
                                _ => "保存结果待确认",
                            }}</p>
                            <p class="break-all">{format!("{} · {}",value.target_deck,value.source_file)}</p>
                            if let Some(id) = &value.card_id {<p class="break-all">{format!("卡片 ID：{id}")}</p>}
                        </div>
                    }
                    if let Some(value) = &*mutation_result {
                        <div role="status" class="space-y-1 rounded-lg bg-base-200/60 p-3 text-sm">
                            <p>{match value.status.as_str() {
                                "updated_synced" => "已保存并同步",
                                "updated_sync_pending" => "源文件已修改，等待同步；重试只会再次同步。",
                                "deleted_synced" => "已删除并同步",
                                "deleted_sync_pending" => "源文件已删除，等待同步；重试只会再次同步。",
                                _ => "保存结果待确认",
                            }}</p>
                            <p class="break-all text-xs text-base-content/60">{value.source_file.clone()}</p>
                        </div>
                    }
                    <div class="flex flex-wrap justify-end gap-2">
                        if editing {
                            if *confirming_delete {
                                <span class="self-center text-sm text-error">{"确认删除这张闪卡？"}</span>
                                <button type="button" class="btn btn-primary w-full sm:w-40" data-testid="favorite-flashcard-confirm-delete"
                                    disabled={*busy} onclick={delete.clone()}>{if *busy {"正在删除…"} else {"确认删除并同步"}}</button>
                                <button type="button" class="btn btn-ghost w-full sm:w-40" data-testid="favorite-flashcard-cancel-delete"
                                    disabled={*busy} onclick={Callback::from({let confirming_delete=confirming_delete.clone();move |_|confirming_delete.set(false)})}>{"取消"}</button>
                            } else {
                                <button type="button" class="btn btn-primary w-full sm:w-40" data-testid="favorite-flashcard-delete"
                                    disabled={*busy || *locked} onclick={Callback::from({let confirming_delete=confirming_delete.clone();move |_|confirming_delete.set(true)})}>
                                    <span aria-hidden="true">{"🗑"}</span>{"删除 Flashcard"}
                                </button>
                            }
                            <button type="button" class="btn btn-primary w-full sm:w-40" data-testid="favorite-flashcard-save"
                                disabled={*busy || (*locked && retry_label.is_none())}
                                onclick={if *delete_mode {delete.clone()} else {submit_edit.clone()}}>
                                {if *busy {"正在保存…"} else if retry_label.is_some() {"重试同步"} else {"保存并同步"}}
                            </button>
                        } else if create_result.as_ref().as_ref().is_none_or(|value|value.retryable) {
                            <button type="button" class="btn btn-primary w-full sm:w-40" disabled={!can_create} onclick={submit_create}>
                                {if *busy {"正在保存…"} else if create_result.is_some() {"重试同步"} else {"创建并同步"}}
                            </button>
                        }
                    </div>
                </div>
                <details class="group min-w-0 overflow-hidden rounded-2xl border border-base-content/15 bg-gradient-to-r from-base-200/60 via-base-100 to-secondary/10" data-testid="favorite-flashcard-preview"
                    ontoggle={Callback::from(|_: Event| hydrate_math())}>
                    <summary class="flex cursor-pointer list-none items-center justify-between gap-3 px-4 py-3 font-semibold" data-testid="favorite-flashcard-preview-toggle">
                        <span class="flex items-center gap-2">
                            <span class="flex h-6 w-6 items-center justify-center rounded-full bg-gradient-to-br from-accent to-secondary text-xs font-semibold text-accent-content shadow-sm" aria-hidden="true">{"3"}</span>
                            {"预览卡片"}
                        </span>
                        <span class="flex items-center gap-2 text-xs font-normal text-base-content/55">
                            <span>{"点按展开或收起"}</span>
                            <span aria-hidden="true" class="text-sm transition-transform group-open:rotate-180">{"▾"}</span>
                        </span>
                    </summary>
                    <div class="space-y-3 border-t border-base-content/10 p-4">
                        <div class="min-w-0" data-testid="favorite-flashcard-front-preview">{favorite_content(&props.preview)}</div>
                        <section class="min-w-0">
                            <h4 class="mb-1 text-xs font-semibold text-base-content/60">{"背面"}</h4>
                            <div key={(*back).clone()} class="study-markdown min-w-0" data-testid="favorite-flashcard-back-preview">
                                if back.trim().is_empty() {
                                    <p class="text-sm text-base-content/50">{"背面预览"}</p>
                                } else {
                                    {render_question_bank_markdown(&back)}
                                }
                            </div>
                        </section>
                    </div>
                </details>
            </div>
        </section>
        </div>
    }
}

#[derive(Clone, Properties, PartialEq)]
pub(crate) struct FavoriteFlashcardWorkspaceProps {
    pub(crate) question_id: String,
    pub(crate) preview: FavoritePreview,
    pub(crate) csrf: String,
    pub(crate) on_close: Callback<()>,
    pub(crate) on_changed: Callback<()>,
}

#[function_component(FavoriteFlashcardWorkspace)]
pub(crate) fn favorite_flashcard_workspace(props: &FavoriteFlashcardWorkspaceProps) -> Html {
    let selected = use_state(|| {
        props
            .preview
            .existing
            .first()
            .map(|card| card.card_id.clone())
    });
    let current = props
        .preview
        .existing
        .iter()
        .find(|card| Some(card.card_id.as_str()) == (*selected).as_deref())
        .or_else(|| props.preview.existing.first());
    if let Some(existing) = current {
        let on_select_card = Callback::from({
            let selected = selected.clone();
            move |card_id: String| selected.set(Some(card_id))
        });
        html! {
            <FavoriteCardForm key={existing.card_id.clone()}
                question_id={props.question_id.clone()} preview={props.preview.clone()}
                mode={FavoriteCardFormMode::Edit(existing.clone())}
                existing_cards={props.preview.existing.clone()}
                on_select_card={on_select_card}
                csrf={props.csrf.clone()}
                on_changed={props.on_changed.clone()} on_close={props.on_close.clone()}/>
        }
    } else {
        html! {
            <FavoriteCardForm question_id={props.question_id.clone()}
                preview={props.preview.clone()} mode={FavoriteCardFormMode::Create}
                csrf={props.csrf.clone()}
                on_changed={props.on_changed.clone()} on_close={props.on_close.clone()}/>
        }
    }
}
