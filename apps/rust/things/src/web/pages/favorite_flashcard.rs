use super::*;

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
    preview.eligible
        && preview
            .allowed_targets
            .iter()
            .any(|target| target_mode(target).is_some_and(|mode| !mode_occupied(preview, mode)))
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

#[derive(Clone, Properties, PartialEq)]
pub(crate) struct FavoriteFlashcardPanelProps {
    pub(crate) question_id: String,
    pub(crate) preview: FavoritePreview,
    pub(crate) csrf: String,
    pub(crate) on_close: Callback<()>,
}

#[function_component(FavoriteFlashcardPanel)]
pub(crate) fn favorite_flashcard_panel(props: &FavoriteFlashcardPanelProps) -> Html {
    let back = use_state(String::new);
    let default_target = favorite_default_target(&props.preview);
    let target = use_state({
        let default_target = default_target.clone();
        move || default_target
    });
    let user_mode = use_state({
        let default_target = default_target.clone();
        move || default_target.starts_with("用户整理/")
    });
    let confirmed = use_state(|| false);
    let busy = use_state(|| false);
    let locked = use_state(|| false);
    let error = use_state(|| None::<String>);
    let result = use_state(|| None::<FavoriteCardResult>);
    let frozen = use_mut_ref(|| None::<CreateFavoriteCard>);
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
    let submit = {
        let back = back.clone();
        let target = target.clone();
        let user_mode = user_mode.clone();
        let confirmed = confirmed.clone();
        let busy = busy.clone();
        let locked = locked.clone();
        let error = error.clone();
        let result = result.clone();
        let frozen = frozen.clone();
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
            let result = result.clone();
            let in_flight = in_flight.clone();
            let mounted = mounted.clone();
            let csrf = props.csrf.clone();
            let endpoint = favorite_endpoint(&props.question_id, "flashcard");
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
                    Ok(value) => result.set(Some(value)),
                    Err(message) => error.set(Some(message)),
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
    let can_submit = !*busy
        && !props.csrf.is_empty()
        && (*locked
            || (valid_target
                && *confirmed
                && (!props.preview.correct_answer.is_empty() || !back.trim().is_empty())));
    let close = Callback::from({
        let on_close = props.on_close.clone();
        move |_| on_close.emit(())
    });
    html! {
        <div class="h-full w-full overflow-y-auto bg-base-100"
            data-testid="favorite-flashcard-panel">
        <section class="mx-auto flex min-h-full w-full max-w-3xl flex-col px-4 pb-5 sm:px-6" role="region" aria-label="创建 Flashcard" tabindex="-1" ref={dialog}
            data-testid="favorite-flashcard-card"
            onkeydown={Callback::from({let on_close=props.on_close.clone();move |event:KeyboardEvent| {
                if event.key()=="Escape" {event.stop_propagation();on_close.emit(());}
            }})}>
            <header class="flex shrink-0 items-center gap-3 border-b border-base-content/10 py-3">
                <button type="button" class="btn btn-ghost btn-sm -ml-2" aria-label="返回复盘" onclick={close.clone()}>
                    <span aria-hidden="true">{"←"}</span>{"返回复盘"}
                </button>
                <div class="min-w-0">
                    <h2 class="truncate text-base font-semibold">{"创建 Flashcard"}</h2>
                    <p class="text-xs text-base-content/55">{if props.preview.correct_answer.is_empty() {"原题暂无标准答案，请在背面填写答案或解题思路"} else {"题目与正确答案将保存到正面"}}</p>
                </div>
            </header>
            <div class="mx-auto my-5 w-full max-w-xl space-y-5 break-words rounded-2xl border border-base-content/15 bg-base-100 p-4 transition focus-within:border-primary/50 focus-within:ring-2 focus-within:ring-primary/15 sm:p-5"
                data-testid="favorite-flashcard-form">
                <details class="group rounded-xl bg-base-200/60 px-4 py-3">
                    <summary class="cursor-pointer text-sm font-medium">{"查看卡片正面与答案"}</summary>
                    <section class="mt-3" aria-label="正面（只读）">{favorite_content(&props.preview)}</section>
                </details>
                <div class="space-y-2">
                    <span class="font-semibold">{"存放位置"}</span>
                    <div class="grid grid-cols-2 gap-1 rounded-xl bg-base-200/70 p-1" role="group" aria-label="存放位置" data-testid="favorite-flashcard-mode">
                        <button type="button" class={classes!("rounded-lg", "px-3", "py-2", "text-sm", "font-medium", if !*user_mode {"bg-base-100 shadow-sm"} else {"text-base-content/60"})}
                            aria-pressed={(!*user_mode).to_string()} disabled={*locked || practice_occupied}
                            data-testid="favorite-flashcard-practice-mode"
                            onclick={Callback::from({let user_mode=user_mode.clone();let target=target.clone();let confirmed=confirmed.clone();let practice=props.preview.allowed_targets.iter().find(|value| target_mode(value)==Some(FavoriteCardMode::Practice) && !mode_occupied(&props.preview, FavoriteCardMode::Practice)).cloned();move |_| {
                                if *user_mode {user_mode.set(false);target.set(practice.clone().unwrap_or_default());confirmed.set(false);}
                            }})}>{"真题练习（默认）"}</button>
                        <button type="button" class={classes!("rounded-lg", "px-3", "py-2", "text-sm", "font-medium", if *user_mode {"bg-base-100 shadow-sm"} else {"text-base-content/60"})}
                            aria-pressed={(*user_mode).to_string()} disabled={*locked || user_occupied}
                            data-testid="favorite-flashcard-user-mode"
                            onclick={Callback::from({let user_mode=user_mode.clone();let target=target.clone();let confirmed=confirmed.clone();move |_| {
                                if !*user_mode {user_mode.set(true);target.set(String::new());confirmed.set(false);}
                            }})}>{"用户整理"}</button>
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
                <label class="block space-y-2">
                    <span class="font-semibold">{"背面"}<span class="ml-2 text-xs font-normal text-base-content/50">{if props.preview.correct_answer.is_empty() {"必填"} else {"可选"}}</span></span>
                    <textarea class="block min-h-28 w-full resize-none border-0 bg-transparent px-3 py-2 text-sm leading-6 focus:outline-none focus:ring-0" placeholder="写下解题思路或复习提示" value={(*back).clone()} disabled={*locked}
                        oninput={Callback::from({let back=back.clone();move |event:InputEvent| {
                            back.set(event.target_unchecked_into::<web_sys::HtmlTextAreaElement>().value());
                        }})}/>
                </label>
                <label class="flex items-center gap-2">
                    <input type="checkbox" class="checkbox checkbox-sm" checked={*confirmed} disabled={*locked}
                        onchange={Callback::from({let confirmed=confirmed.clone();move |event:Event| {
                            confirmed.set(event.target_unchecked_into::<HtmlInputElement>().checked());
                        }})}/>
                    <span>{"确认写入所选目录"}</span>
                </label>
                if let Some(message) = &*error {
                    <p role="alert">{message}{"。重试会使用同一份请求，不会重复创建。"}</p>
                }
                if let Some(value) = &*result {
                    <div role="status" class="space-y-2">
                        <p>{match value.status.as_str() {
                            "created_synced" => "已创建并同步",
                            "created_sync_pending" => "已写入源文件，待同步；请重试同步，不要重复创建。",
                            "already_exists" => "闪卡已存在",
                            _ => "保存结果待确认",
                        }}</p>
                        <p class="text-sm">{format!("{} · {}",value.target_deck,value.source_file)}</p>
                        if let Some(id) = &value.card_id {<p class="text-sm">{format!("卡片 ID：{id}")}</p>}
                    </div>
                }
                <div class="flex justify-end">
                    if result.as_ref().as_ref().is_none_or(|value|value.retryable) {
                        <button type="button" class="btn btn-primary w-full sm:w-auto" disabled={!can_submit} onclick={submit}>
                            {if *busy {"正在保存…"} else if result.is_some() {"重试同步"} else {"创建并同步"}}
                        </button>
                    }
                </div>
            </div>
        </section>
        </div>
    }
}

#[derive(Clone, Properties, PartialEq)]
pub(crate) struct FavoriteExistingCardsProps {
    pub(crate) cards: Vec<FavoriteExistingCard>,
    pub(crate) on_edit: Callback<FavoriteExistingCard>,
}

#[function_component(FavoriteExistingCards)]
pub(crate) fn favorite_existing_cards(props: &FavoriteExistingCardsProps) -> Html {
    html! {
        <section class="space-y-2 rounded-box border border-success/30 bg-success/5 p-3"
            data-testid="favorite-existing-cards" aria-label="已创建 Flashcard">
            <h3 class="font-semibold text-success">{"已创建 Flashcard"}</h3>
            {for props.cards.iter().map(|card| {
                let card_value = card.clone();
                let on_edit = props.on_edit.clone();
                html! {
                    <div class="flex flex-wrap items-center justify-between gap-2 rounded-lg bg-base-100/70 px-3 py-2"
                        data-testid="favorite-existing-card">
                        <div class="min-w-0">
                            <p class="truncate text-sm font-medium" data-testid="favorite-existing-card-deck">{card.deck_path.clone()}</p>
                            <p class="break-all text-xs text-base-content/55" data-testid="favorite-existing-card-path">{card.source_file.clone()}</p>
                        </div>
                        <button type="button" class="btn btn-outline btn-sm shrink-0"
                            data-testid="favorite-existing-card-edit"
                            onclick={Callback::from(move |_| on_edit.emit(card_value.clone()))}>
                            {"修改或删除"}
                        </button>
                    </div>
                }
            })}
        </section>
    }
}

#[derive(Clone, Properties, PartialEq)]
pub(crate) struct FavoriteFlashcardEditorProps {
    pub(crate) question_id: String,
    pub(crate) preview: FavoritePreview,
    pub(crate) existing: FavoriteExistingCard,
    pub(crate) csrf: String,
    pub(crate) on_close: Callback<()>,
    pub(crate) on_changed: Callback<()>,
}

#[function_component(FavoriteFlashcardEditor)]
pub(crate) fn favorite_flashcard_editor(props: &FavoriteFlashcardEditorProps) -> Html {
    let back = use_state({
        let back = props.existing.back.clone();
        move || back
    });
    let busy = use_state(|| false);
    let locked = use_state(|| false);
    let error = use_state(|| None::<String>);
    let result = use_state(|| None::<FavoriteMutationResult>);
    let confirming_delete = use_state(|| false);
    let delete_mode = use_state(|| false);
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
    let close = {
        let on_close = props.on_close.clone();
        Callback::from(move |_| on_close.emit(()))
    };
    let update = {
        let back = back.clone();
        let busy = busy.clone();
        let locked = locked.clone();
        let error = error.clone();
        let result = result.clone();
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
            let result = result.clone();
            let in_flight = in_flight.clone();
            let mounted = mounted.clone();
            let on_changed = on_changed.clone();
            let on_close = on_close.clone();
            let csrf = props.csrf.clone();
            let endpoint = favorite_card_endpoint(&props.question_id, &props.existing.card_id);
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
                        result.set(Some(value));
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
        let busy = busy.clone();
        let locked = locked.clone();
        let error = error.clone();
        let result = result.clone();
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
            locked.set(true);
            busy.set(true);
            error.set(None);
            let busy = busy.clone();
            let locked = locked.clone();
            let error = error.clone();
            let result = result.clone();
            let in_flight = in_flight.clone();
            let mounted = mounted.clone();
            let on_changed = on_changed.clone();
            let on_close = on_close.clone();
            let csrf = props.csrf.clone();
            let endpoint = favorite_card_endpoint(&props.question_id, &props.existing.card_id);
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
                        result.set(Some(value));
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
    let retry_label = result
        .as_ref()
        .as_ref()
        .filter(|value| value.retryable)
        .map(|value| value.status.as_str());
    html! {
        <div class="h-full w-full overflow-y-auto bg-base-100" data-testid="favorite-flashcard-editor">
        <section class="mx-auto flex min-h-full w-full max-w-3xl flex-col px-4 pb-5 sm:px-6" role="region" aria-label="修改 Flashcard" tabindex="-1" ref={dialog}
            onkeydown={Callback::from({let on_close=props.on_close.clone();move |event:KeyboardEvent| {
                if event.key()=="Escape" {event.stop_propagation();on_close.emit(());}
            }})}>
            <header class="flex shrink-0 items-center gap-3 border-b border-base-content/10 py-3">
                <button type="button" class="btn btn-ghost btn-sm -ml-2" aria-label="返回复盘" onclick={close.clone()}>
                    <span aria-hidden="true">{"←"}</span>{"返回复盘"}
                </button>
                <div class="min-w-0">
                    <h2 class="truncate text-base font-semibold">{"修改 Flashcard"}</h2>
                    <p class="break-all text-xs text-base-content/55">{format!("{} · {}", props.existing.deck_path, props.existing.source_file)}</p>
                </div>
            </header>
            <div class="mx-auto my-5 w-full max-w-xl space-y-5 break-words rounded-2xl border border-base-content/15 bg-base-100 p-4 sm:p-5" data-testid="favorite-flashcard-editor-form">
                <details class="group rounded-xl bg-base-200/60 px-4 py-3">
                    <summary class="cursor-pointer text-sm font-medium">{"查看卡片正面与答案"}</summary>
                    <section class="mt-3" aria-label="正面（只读）">
                        <div class="study-markdown question-bank-passage">{render_question_bank_markdown(&props.existing.front)}</div>
                    </section>
                </details>
                <label class="block space-y-2">
                    <span class="font-semibold">{"背面"}</span>
                    <textarea class="block min-h-28 w-full resize-none border-0 bg-transparent px-3 py-2 text-sm leading-6 focus:outline-none focus:ring-0" placeholder="写下解题思路或复习提示" value={(*back).clone()} disabled={*locked}
                        data-testid="favorite-flashcard-editor-back"
                        oninput={Callback::from({let back=back.clone();move |event:InputEvent| {
                            back.set(event.target_unchecked_into::<web_sys::HtmlTextAreaElement>().value());
                        }})}/>
                </label>
                if let Some(message) = &*error {
                    <p role="alert" class="text-sm text-error">{message}</p>
                }
                if let Some(value) = &*result {
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
                    if *confirming_delete {
                        <span class="self-center text-sm text-error">{"确认删除这张闪卡？"}</span>
                        <button type="button" class="btn btn-error btn-sm" data-testid="favorite-flashcard-confirm-delete"
                            disabled={*busy} onclick={delete.clone()}>{if *busy {"正在删除…"} else {"确认删除并同步"}}</button>
                        <button type="button" class="btn btn-ghost btn-sm" data-testid="favorite-flashcard-cancel-delete"
                            disabled={*busy} onclick={Callback::from({let confirming_delete=confirming_delete.clone();move |_|confirming_delete.set(false)})}>{"取消"}</button>
                    } else {
                        <button type="button" class="btn btn-outline btn-error btn-sm" data-testid="favorite-flashcard-delete"
                            disabled={*busy || *locked} onclick={Callback::from({let confirming_delete=confirming_delete.clone();move |_|confirming_delete.set(true)})}>{"删除 Flashcard"}</button>
                    }
                    <button type="button" class="btn btn-primary w-full sm:w-auto" data-testid="favorite-flashcard-save"
                        disabled={*busy || (*locked && retry_label.is_none())}
                        onclick={if *delete_mode {delete.clone()} else {update.clone()}}>
                        {if *busy {"正在保存…"} else if retry_label.is_some() {"重试同步"} else {"保存并同步"}}
                    </button>
                </div>
            </div>
        </section>
        </div>
    }
}
