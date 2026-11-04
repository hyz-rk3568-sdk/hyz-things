use super::*;

#[derive(Clone, PartialEq, serde::Deserialize)]
pub(crate) struct FavoritePreview {
    pub(crate) eligible: bool,
    pub(crate) reason: Option<String>,
    pub(crate) front: String,
    pub(crate) correct_answer: Vec<String>,
    pub(crate) recommended_target: Option<String>,
    pub(crate) allowed_targets: Vec<String>,
    #[serde(default)]
    pub(crate) ai_targets: Vec<String>,
    pub(crate) requires_type_confirmation: bool,
    pub(crate) source_revision: String,
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

fn favorite_endpoint(id: &str, action: &str) -> String {
    format!("/api/v1/question-bank/practice/questions/{id}/{action}")
}

fn favorite_content(value: &FavoritePreview) -> Html {
    html! {
        <>
            <div class="study-markdown question-bank-passage">{render_question_bank_markdown(&value.front)}</div>
            <section class="rounded-box border border-base-content/15 p-4" aria-label="正确答案">
                <h3 class="font-semibold">{"正确答案"}</h3>
                <p data-testid="favorite-question-answer">{value.correct_answer.join("、")}</p>
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
    // Never auto-select a destination: every new card needs a deliberate choice.
    let target = use_state(String::new);
    let ai_mode = use_state(|| false);
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
        let ai_mode = ai_mode.clone();
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
                ((if *ai_mode {
                    props.preview.ai_targets.contains(&*target)
                } else {
                    props.preview.recommended_target.as_ref() == Some(&*target)
                }) && *confirmed)
                    .then(|| CreateFavoriteCard {
                        creation_mode: if *ai_mode { "ai_generation" } else { "default" },
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
    let valid_target = if *ai_mode {
        props.preview.ai_targets.contains(&*target)
    } else {
        props.preview.recommended_target.as_ref() == Some(&*target)
    };
    let can_submit = !*busy
        && !props.csrf.is_empty()
        && (*locked || (valid_target && *confirmed));
    let close = Callback::from({
        let on_close = props.on_close.clone();
        move |_| on_close.emit(())
    });
    html! {
        <div class="flex h-full w-full justify-center overflow-y-auto rounded-box bg-base-100 p-4 sm:p-6"
            data-testid="favorite-flashcard-panel">
        <section class="my-auto w-full max-w-xl space-y-5" role="region" aria-label="创建 Flashcard" tabindex="-1" ref={dialog}
            data-testid="favorite-flashcard-card"
            onkeydown={Callback::from({let on_close=props.on_close.clone();move |event:KeyboardEvent| {
                if event.key()=="Escape" {event.stop_propagation();on_close.emit(());}
            }})}>
            <header class="flex items-start justify-between gap-3">
                <div>
                    <h2 class="text-xl font-bold">{"创建 Flashcard"}</h2>
                    <p class="mt-1 text-sm opacity-60">{"默认复盘和 AI 出题是两条独立的保存路径。先选择用途，再确认完整文件目录。背面可留空。"}</p>
                </div>
                <button type="button" class="btn btn-ghost btn-sm" onclick={close.clone()}>{"返回复盘"}</button>
            </header>
            <div class="space-y-5 break-words">
                <section aria-label="正面（只读）">{favorite_content(&props.preview)}</section>
                <div class="space-y-2">
                    <span class="font-semibold">{"创建用途"}</span>
                    <div class="join grid grid-cols-2" role="group" aria-label="创建用途" data-testid="favorite-flashcard-mode">
                        <button type="button" class={classes!("join-item","btn",if !*ai_mode {"btn-primary"} else {"btn-outline"})}
                            aria-pressed={(!*ai_mode).to_string()} disabled={*locked}
                            data-testid="favorite-flashcard-default-mode"
                            onclick={Callback::from({let ai_mode=ai_mode.clone();let target=target.clone();let confirmed=confirmed.clone();move |_| {
                                ai_mode.set(false);target.set(String::new());confirmed.set(false);
                            }})}>{"默认复盘"}</button>
                        <button type="button" class={classes!("join-item","btn",if *ai_mode {"btn-primary"} else {"btn-outline"})}
                            aria-pressed={(*ai_mode).to_string()} disabled={*locked || props.preview.ai_targets.is_empty()}
                            data-testid="favorite-flashcard-ai-mode"
                            onclick={Callback::from({let ai_mode=ai_mode.clone();let target=target.clone();let confirmed=confirmed.clone();move |_| {
                                ai_mode.set(true);target.set(String::new());confirmed.set(false);
                            }})}>{"AI 出题"}</button>
                    </div>
                    <p class="text-xs text-base-content/60">
                        {if *ai_mode {
                            "AI 出题路径：只提供当前命题器支持的题型目录，与默认复盘互不替代。"
                        } else {
                            "默认路径：一级模块完整名称 / 原题二级 category / 错题 / cards.md，适用于全部真题模块。"
                        }}
                    </p>
                    if props.preview.ai_targets.is_empty() {
                        <p class="text-xs text-base-content/50" data-testid="favorite-flashcard-ai-unavailable">
                            {"这道题目前没有可用的 AI 出题目录，仍可保存到默认路径。"}
                        </p>
                    }
                </div>
                <label class="block space-y-2">
                    <span class="font-semibold">{"目标牌组"}</span>
                    <select class="select select-bordered w-full" value={(*target).clone()} disabled={*locked}
                        onchange={Callback::from({let target=target.clone();move |event:Event| {
                            target.set(event.target_unchecked_into::<HtmlSelectElement>().value());
                        }})}>
                        <option value="" disabled={true}>{"请选择写入目录"}</option>
                        {if *ai_mode {
                            html! {for props.preview.ai_targets.iter().map(|value|html! {
                                <option value={value.clone()} selected={*target==*value}>{format!("flashcards/{value}/cards.md")}</option>
                            })}
                        } else {
                            html! {for props.preview.recommended_target.iter().map(|value|html! {
                                <option value={value.clone()} selected={*target==*value}>{format!("flashcards/{value}/cards.md")}</option>
                            })}
                        }}
                    </select>
                </label>
                if !target.is_empty() {
                    <p class="break-all text-xs text-base-content/70" data-testid="favorite-flashcard-path">
                        {format!("将追加或创建：flashcards/{}/cards.md",*target)}
                    </p>
                }
                <label class="block space-y-2">
                    <span class="font-semibold">{"背面"}</span>
                    <textarea class="textarea textarea-bordered min-h-40 w-full" value={(*back).clone()} disabled={*locked}
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
                <div class="flex flex-wrap gap-2">
                    if result.as_ref().as_ref().is_none_or(|value|value.retryable) {
                        <button type="button" class="btn btn-primary" disabled={!can_submit} onclick={submit}>
                            {if *busy {"正在保存…"} else if result.is_some() {"重试同步"} else {"创建并同步"}}
                        </button>
                    }
                    <button type="button" class="btn btn-ghost" onclick={close.clone()}>{"返回复盘"}</button>
                </div>
            </div>
        </section>
        </div>
    }
}
