//! 申论训练：复用 Question Bank 真题复盘工作区，而非建立第二套考试 Session。
use super::favorite_flashcard::{favorite_can_create, FavoriteFlashcardWorkspace, FavoritePreview};
use super::question_bank_practice::{use_practice_favorite, QuestionAiReview, QuestionBankQuestionHeader};
use super::material_scroll::{use_training_scroll_pair, MaterialScrollButton};
use super::*;
use hyz_things::domain::question_bank::{
    CreateShenlunAttempt, FavoriteState, PaperSummary, PaperView, QuestionView, SetFavorite, ShenlunAttempt,
};

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunTrainingProps {
    pub(super) on_immersive_change: Callback<bool>,
    pub(super) csrf: String,
}

fn topic_label(question: &QuestionView) -> &str {
    match question.question_type.as_deref() {
        Some("summary" | "generalization" | "归纳概括") => "归纳概括",
        Some("analysis" | "综合分析") => "综合分析",
        Some("measures" | "countermeasure" | "提出对策") => "提出对策",
        Some("document_writing" | "document" | "official_document" | "公文写作") => "公文写作",
        Some("essay" | "composition" | "大作文") => "大作文",
        _ => "其他",
    }
}

pub(super) fn new_shenlun_attempt_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    web_sys::window()
        .ok_or("浏览器不可用")?
        .crypto()
        .map_err(|_| "无法获取安全随机数")?
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(|_| "无法获取安全随机数")?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}-{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15],
    ))
}

#[function_component(ShenlunTraining)]
pub(super) fn shenlun_training(props: &ShenlunTrainingProps) -> Html {
    let papers = use_state(|| None::<Vec<PaperSummary>>);
    let paper = use_state(|| None::<PaperView>);
    let selected = use_state(|| 0_usize);
    let busy = use_state(|| false);
    let loading = use_state(|| true);
    let error = use_state(|| None::<String>);
    let refresh = use_state(|| 0_u32);
    let attempt = use_state(|| None::<ShenlunAttempt>);
    let attempts = use_state(Vec::<ShenlunAttempt>::new);
    let attempt_loading = use_state(|| false);
    let mode = use_state(|| "ai".to_owned());
    let menu_open = use_state(|| false);
    let ai_delete_signal = use_state(|| 0_u32);
    let scroll = use_training_scroll_pair();
    let question_key = (*paper).as_ref().and_then(|p|
        p.questions.get(*selected).map(|question| (p.id.clone(), question.id.clone())));
    let active_question = use_mut_ref(|| None::<(String, String)>);
    *active_question.borrow_mut() = question_key.clone();
    let favorite=use_practice_favorite(question_key.as_ref().map(|(_,id)|id.clone()));
    let favorite_busy=use_state(||false);
    let flashcard_preview=use_state(||None::<(String,FavoritePreview)>);
    let flashcard_error=use_state(||None::<String>);
    let flashcard_open=use_state(||false);
    let flashcard_revision=use_state(||0_u32);
    {
        let flashcard_preview=flashcard_preview.clone();
        let flashcard_error=flashcard_error.clone();
        let flashcard_open=flashcard_open.clone();
        use_effect_with((question_key.clone(),*flashcard_revision),move |(key,_)|{
            let alive=Rc::new(Cell::new(true));
            let guard=alive.clone();
            flashcard_preview.set(None);
            flashcard_error.set(None);
            flashcard_open.set(false);
            if let Some((_,qid))=key.clone(){
                let flashcard_preview=flashcard_preview.clone();
                let flashcard_error=flashcard_error.clone();
                spawn_local(async move {
                    let result=fetch_json::<FavoritePreview>(
                        &format!("/api/v1/question-bank/practice/questions/{qid}/flashcard-preview"),
                        "申论 Flashcard 预览"
                    ).await;
                    if guard.get(){
                        match result {
                            Ok(preview)=>flashcard_preview.set(Some((qid,preview))),
                            Err(message)=>flashcard_error.set(Some(message)),
                        }
                    }
                });
            }
            move ||alive.set(false)
        });
    }

    {
        let papers = papers.clone();
        let loading = loading.clone();
        let error = error.clone();
        use_effect_with(*refresh, move |_| {
            let alive = Rc::new(Cell::new(true));
            let guard = alive.clone();
            loading.set(true);
            error.set(None);
            spawn_local(async move {
                let result = fetch_json::<Vec<PaperSummary>>(
                    "/api/v1/question-bank/shenlun/papers", "申论原卷"
                ).await;
                if !guard.get() { return; }
                match result {
                    Ok(value) => papers.set(Some(value)),
                    Err(message) => error.set(Some(message)),
                }
                loading.set(false);
            });
            move || alive.set(false)
        });
    }
    // An attempt is only the native grade job's immutable input container.
    // Nothing here creates an objective practice session, countdown, or exam submission.
    let needs_attempt = *mode == "ocr" || *mode == "source";
    {
        let attempt = attempt.clone();
        let attempts = attempts.clone();
        let attempt_loading = attempt_loading.clone();
        let menu_open = menu_open.clone();
        let error = error.clone();
        let csrf = props.csrf.clone();
        use_effect_with((question_key.clone(), needs_attempt), move |(key, needed)| {
            let alive = Rc::new(Cell::new(true));
            if *needed {
            attempt.set(None);
            attempts.set(Vec::new());
            attempt_loading.set(key.is_some());
            menu_open.set(false);
            if let Some((paper_id, question_id)) = key.clone() {
                let guard = alive.clone();
                let attempt = attempt.clone();
                let attempts = attempts.clone();
                let attempt_loading = attempt_loading.clone();
                let error = error.clone();
                let csrf = csrf.clone();
                spawn_local(async move {
                    let url = format!("/api/v1/question-bank/shenlun/attempts?paper_id={paper_id}&question_id={question_id}");
                    let records = fetch_json::<Vec<ShenlunAttempt>>(&url, "申论批改记录").await;
                    if !guard.get() { return; }
                    let result = match records {
                        Ok(items) if !items.is_empty() => Ok(items),
                        Ok(_) => match new_shenlun_attempt_id() {
                            Ok(request_id) => {
                                let request = CreateShenlunAttempt { request_id, paper_id, question_id };
                                let created: Result<ShenlunAttempt, String> = post_json_response(
                                    "/api/v1/question-bank/shenlun/attempts",
                                    &csrf, &request, "准备申论 AI 批改"
                                ).await;
                                created.map(|record| vec![record])
                            }
                            Err(message) => Err(message),
                        },
                        Err(message) => Err(message),
                    };
                    if !guard.get() { return; }
                    match result {
                        Ok(items) => {
                            let active = items.iter().find(|record| record.status == "draft")
                                .or_else(|| items.first()).cloned();
                            attempt.set(active);
                            attempts.set(items);
                        }
                        Err(message) => error.set(Some(message)),
                    }
                    attempt_loading.set(false);
                });
            }
            }
            move || alive.set(false)
        });
    }

    let open = {
        let busy = busy.clone();
        let paper = paper.clone();
        let selected = selected.clone();
        let mode = mode.clone();
        let menu_open = menu_open.clone();
        let error = error.clone();
        let immersive = props.on_immersive_change.clone();
        Callback::from(move |id: String| {
            if *busy { return; }
            busy.set(true);
            error.set(None);
            let busy = busy.clone();
            let paper = paper.clone();
            let selected = selected.clone();
            let mode = mode.clone();
            let menu_open = menu_open.clone();
            let error = error.clone();
            let immersive = immersive.clone();
            spawn_local(async move {
                match fetch_json::<PaperView>(&format!("/api/v1/question-bank/shenlun/papers/{id}"), "申论原卷").await {
                    Ok(value) => {
                        selected.set(0);
                        mode.set("ai".into());
                        menu_open.set(false);
                        paper.set(Some(value));
                        immersive.emit(true);
                    }
                    Err(message) => error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let close = {
        let paper = paper.clone();
        let mode = mode.clone();
        let immersive = props.on_immersive_change.clone();
        Callback::from(move |_| {
            immersive.emit(false);
            mode.set("ai".into());
            paper.set(None);
        })
    };
    let on_confirm = {
        let attempt = attempt.clone();
        let attempts = attempts.clone();
        let active_question = active_question.clone();
        Callback::from(move |next: ShenlunAttempt| {
            if active_question.borrow().as_ref() != Some(&(next.paper_id.clone(), next.question_id.clone())) {
                return;
            }
            let mut history = (*attempts).clone();
            if let Some(row) = history.iter_mut().find(|row| row.id == next.id) {
                *row = next.clone();
            } else {
                history.insert(0, next.clone());
            }
            attempts.set(history);
            attempt.set(Some(next));
        })
    };

    if let Some(current) = (*paper).as_ref() {
        let index = (*selected).min(current.questions.len().saturating_sub(1));
        let question = current.questions.get(index);
        let question_body = question.map(|item| html! {
            <div class="space-y-2">
                <p class="text-xs text-base-content/55">
                    {topic_label(item)}{" · "}
                    {item.max_score.map(|score| format!("{score} 分")).unwrap_or_else(|| "分值待确认".into())}
                </p>
                <div class="study-markdown">{render_markdown(&item.stem, &current.id)}</div>
            </div>
        }).unwrap_or_default();
        let paper_id = current.id.clone();
        let question_id = question.map(|item| item.id.clone()).unwrap_or_default();
        let material_scroll = scroll.first.clone();
        let material_edges = *scroll.first_edges;
        let mode_value = (*mode).clone();
        let icon_button = super::question_header_actions::QUESTION_HEADER_ICON_BUTTON;

        let left = html! {
            <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="shenlun-materials">
                <div class={classes!(TRAINING_PANE_HEADER, "question-bank-pane-header")}>
                    <strong class="text-sm">{"原卷材料"}</strong>
                    <div class="ml-auto flex shrink-0 gap-1" role="group" aria-label="材料滚动">
                        <MaterialScrollButton passage={material_scroll.clone()} up={true}
                            disabled={!material_edges.can_scroll_up} question_index={index}/>
                        <MaterialScrollButton passage={material_scroll.clone()} up={false}
                            disabled={!material_edges.can_scroll_down} question_index={index}/>
                    </div>
                </div>
                <div ref={material_scroll} onscroll={scroll.on_scroll.clone()}
                    class="min-h-0 flex-1 space-y-5 overflow-y-auto overscroll-contain px-4 pb-5 pt-3"
                    data-swipe-ignore="true">
                    {for current.materials.iter().map(|material| html! {
                        <article key={material.id.clone()} class="space-y-2">
                            <h3 class="font-semibold">{material.name.clone()}</h3>
                            <div class="study-markdown">{render_markdown(&material.markdown, &paper_id)}</div>
                        </article>
                    })}
                </div>
            </section>
        };
        let favorite_value=(*favorite.value).as_ref()
            .filter(|state|state.question_id==question_id);
        let card_preview=(*flashcard_preview).as_ref().and_then(|(id,preview)|
            (id==&question_id).then_some(preview));
        let starred=favorite_value.is_some_and(|state|state.favorite);
        let favorite_ready=favorite_value.is_some();
        let on_favorite={
            let csrf=props.csrf.clone();
            let qid=question_id.clone();
            let favorite_value=favorite.value.clone();
            let favorite_source=favorite.source.clone();
            let favorite_generation=favorite.generation.clone();
            let generation=*favorite.generation.borrow();
            let favorite_busy=favorite_busy.clone();
            let error=error.clone();
            Callback::from(move |_|{
                if !favorite_ready || *favorite_busy {return;}
                favorite_busy.set(true);
                let csrf=csrf.clone();
                let qid=qid.clone();
                let favorite_value=favorite_value.clone();
                let favorite_source=favorite_source.clone();
                let favorite_generation=favorite_generation.clone();
                let favorite_busy=favorite_busy.clone();
                let error=error.clone();
                spawn_local(async move{
                    let result:Result<FavoriteState,String>=post_json_response(
                        &format!("/api/v1/question-bank/practice/favorites/{qid}"),
                        &csrf,&SetFavorite{favorite:!starred},"收藏申论原题"
                    ).await;
                    match result{
                        Ok(state) if state.question_id==qid=>{
                            if favorite_source.borrow().as_ref()==Some(&qid)
                                && *favorite_generation.borrow()==generation {
                                favorite_value.set(Some(state));
                            }
                        },
                        Ok(_)=>error.set(Some("收藏响应与当前题目不一致".into())),
                        Err(message)=>error.set(Some(message)),
                    }
                    favorite_busy.set(false);
                });
            })
        };
        let leading=html!{
            <super::question_header_actions::QuestionHeaderActions
                favorite={favorite_value.map(|state|state.favorite)}
                favorite_busy={*favorite_busy || !favorite_ready}
                favorite_label={if !favorite_ready {"正在加载收藏状态"} else if starred {"取消收藏"} else {"收藏题目"}}
                favorite_test_id="shenlun-favorite"
                on_favorite={on_favorite}
                flashcard_count={card_preview.map(|preview|preview.existing.len())}
                flashcard_can_create={card_preview.is_some_and(favorite_can_create)}
                flashcard_create_test_id="shenlun-create-flashcard"
                flashcard_manage_test_id="shenlun-manage-flashcard"
                on_flashcard={Callback::from({
                    let flashcard_open=flashcard_open.clone();
                    move |_|flashcard_open.set(true)
                })}/>
        };
        let menu = {
            let menu_open = menu_open.clone();
            let ai_delete_signal = ai_delete_signal.clone();
            html! {
                <div class="relative">
                    <button type="button" class={icon_button} aria-label="更多 AI 操作"
                        title="更多 AI 操作" data-testid="shenlun-more"
                        onclick={Callback::from({let menu_open=menu_open.clone();move |_|menu_open.set(!*menu_open)})}>
                        {"⋮"}
                    </button>
                    if *menu_open {
                        <div class="absolute right-0 top-full z-50 min-w-40 rounded-box border border-base-content/10 bg-base-100 p-1 shadow-lg">
                            if matches!(mode_value.as_str(), "grade"|"ai") {
                                <button type="button" class="btn btn-ghost btn-sm w-full justify-start text-error"
                                    data-testid="shenlun-delete-current"
                                    onclick={Callback::from({
                                        let menu_open=menu_open.clone();
                                        let signal=ai_delete_signal.clone();
                                        move |_| {
                                            menu_open.set(false);
                                            signal.set(signal.wrapping_add(1));
                                        }
                                    })}>{"删除 AI 对话"}</button>
                            }
                        </div>
                    }
                </div>
            }
        };
        let right = html! {
            <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="shenlun-question">
                <QuestionBankQuestionHeader
                    title_test_id="shenlun-question-heading"
                    title={format!("原卷第 {} 题", question.map(|q|q.number).unwrap_or(0))}
                    subtitle={Some(question.map(topic_label).unwrap_or("申论").to_string())}
                    question_index={index}
                    target="题目"
                    label="申论题目与解析滚动"
                    passage={Some(scroll.second.clone())}
                    can_scroll_up={scroll.second_edges.can_scroll_up}
                    can_scroll_down={scroll.second_edges.can_scroll_down}
                    leading={leading}
                    tabs={html! {
                        <>
                            <button type="button" class={classes!(icon_button,if mode_value=="grade" {"text-primary"} else {""})}
                                aria-label="AI 批改" title="AI 批改" data-testid="shenlun-mode-grade"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set("grade".into())})}>{"批"}</button>
                            <button type="button" class={classes!(icon_button,if mode_value=="ocr" {"text-primary"} else {""})}
                                aria-label="OCR 转写" title="OCR 转写" data-testid="shenlun-mode-ocr"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set("ocr".into())})}>{"识"}</button>
                            <button type="button" class={classes!(icon_button,if mode_value=="ai" {"text-primary"} else {""})}
                                aria-label="AI 解析与追问" title="AI 解析与追问" data-testid="shenlun-mode-ai"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set("ai".into())})}>{"✦"}</button>
                            <button type="button" class={classes!(icon_button,if mode_value=="source" {"text-primary"} else {""})}
                                aria-label="来源参考答案" title="来源参考答案" data-testid="shenlun-mode-source"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set("source".into())})}>{"答"}</button>
                            {menu}
                        </>
                    }}/>
                if *flashcard_open {
                    if let Some(preview)=card_preview {
                        <div class="min-h-0 flex-1 overflow-y-auto p-3" data-testid="shenlun-flashcard-workspace">
                            <FavoriteFlashcardWorkspace key={question_id.clone()}
                                question_id={question_id.clone()} preview={preview.clone()}
                                csrf={props.csrf.clone()}
                                on_close={Callback::from({
                                    let flashcard_open=flashcard_open.clone();
                                    move |_|flashcard_open.set(false)
                                })}
                                on_changed={Callback::from({
                                    let flashcard_revision=flashcard_revision.clone();
                                    move |_|flashcard_revision.set(flashcard_revision.wrapping_add(1))
                                })}/>
                        </div>
                    }
                } else if matches!(mode_value.as_str(), "grade"|"ai") {
                    <QuestionAiReview key={format!("{}:{}",question_id,mode_value)}
                        mode={if mode_value=="grade" {"grade"} else {"analysis"}}
                        csrf={props.csrf.clone()} question_id={question_id.clone()}
                        question_content={question_body.clone()}
                        question_scroll={scroll.second.clone()} on_question_scroll={scroll.on_scroll.clone()}
                        delete_signal={*ai_delete_signal}/>
                } else {
                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4" ref={scroll.second.clone()}
                        onscroll={scroll.on_scroll.clone()} data-swipe-ignore="true">
                        if mode_value!="history" { {question_body.clone()} }
                        if *attempt_loading {
                            <p class="mt-3 text-sm text-base-content/60">{"正在加载本题 AI 批改…"}</p>
                        } else if let Some(record)=(*attempt).as_ref() {
                            <ShenlunAiPanel key={record.id.clone()} attempt={record.clone()}
                                csrf={props.csrf.clone()} attempts={(*attempts).clone()}
                                mode={mode_value.clone()}
                                regrade_signal={0} delete_signal={0}
                                on_confirm={on_confirm.clone()}/>
                        }
                        if let Some(message)=(*error).as_ref() {
                            <p role="alert" class="mt-2 text-sm text-error">{message.clone()}</p>
                        }
                    </div>
                }
            </section>
        };
        let groups = vec![NavigationGroup {
            label: "全部题目".into(), subtitle: None, start: 0, end: current.questions.len()
        }];
        // No formal paper session: do not show a misleading paper-wide "answered" tally.
        // This shared answer sheet is the sole question switcher in review mode.
        let answered = vec![false; current.questions.len()];
        html! {
            <div class="flex h-full min-h-0 flex-col gap-2" data-testid="shenlun-workspace">
                <div class="flex shrink-0 items-center gap-3">
                    <button type="button" class="btn btn-ghost btn-sm" onclick={close}>{"← 返回选题"}</button>
                    <h2 class="min-w-0 truncate text-sm font-semibold">{current.title.clone()}</h2>
                </div>
                <div class="min-h-0 flex-1">
                    <TrainingSplitWorkspace left={left} right={right}
                        test_id="shenlun-split" resizable={true} split_at_md={true}/>
                </div>
                <CompactQuestionNavigation current={selected.clone()}
                    navigation_allowed={Some(Callback::from({
                        let mode=mode.clone();
                        let menu_open=menu_open.clone();
                        move |_:()| {
                            mode.set("ai".into());
                            menu_open.set(false);
                            true
                        }
                    }))}
                    numbers={current.questions.iter().map(|q|q.number).collect::<Vec<_>>()}
                    answered={answered} grades={None::<Vec<Option<bool>>>} groups={groups}/>
            </div>
        }
    } else {
        let mut grouped = std::collections::BTreeMap::<String,std::collections::BTreeMap<i32,Vec<&PaperSummary>>>::new();
        if let Some(all)=(*papers).as_ref() {
            for item in all.iter().filter(|item|item.role=="primary") {
                grouped.entry(item.region.clone()).or_default().entry(item.year).or_default().push(item);
            }
        }
        html! {
            <section class="flex h-full min-h-0 flex-col gap-3 overflow-hidden" data-testid="shenlun-catalog">
                <div class="flex items-center justify-between gap-2">
                    <div>
                        <h2 class="font-semibold">{"申论训练 · 选择原卷"}</h2>
                        <p class="text-xs text-base-content/55">{"选择原卷后直接进入真题 AI 批改与复盘。"}</p>
                    </div>
                    <button type="button" class="btn btn-ghost btn-sm"
                        onclick={Callback::from({let refresh=refresh.clone();move |_|refresh.set(refresh.wrapping_add(1))})}>{"刷新"}</button>
                </div>
                if let Some(message)=(*error).as_ref() {
                    <p role="alert" class="text-sm text-error">{message.clone()}</p>
                }
                <div class="min-h-0 flex-1 overflow-y-auto" data-testid="shenlun-paper-list">
                    if *loading {
                        <p class="py-4 text-sm">{"正在加载申论原卷…"}</p>
                    } else {
                        <div class="question-bank-paper-grid">
                            {for grouped.into_iter().enumerate().map(|(group_index,(region,years))| {
                                let total=years.values().map(Vec::len).sum::<usize>();
                                html!{
                                    <details class="rounded-box border border-base-content/10 bg-base-100"
                                        open={group_index<2} data-testid="shenlun-region-group">
                                        <summary class="flex cursor-pointer justify-between px-4 py-3">
                                            <strong>{region}</strong><span class="text-xs opacity-60">{format!("{total}份")}</span>
                                        </summary>
                                        <div class="space-y-2 border-t border-base-content/10 p-3">
                                            {for years.into_iter().rev().enumerate().map(|(year_index,(year,items))|html!{
                                                <details class="question-bank-year-group rounded-box bg-base-200/50" open={year_index==0}>
                                                    <summary class="flex cursor-pointer justify-between px-3 py-2 text-sm">
                                                        <span>{format!("{year} 年")}</span><span class="text-xs opacity-60">{format!("{} 份",items.len())}</span>
                                                    </summary>
                                                    <div class="grid gap-2 px-2 pb-2">
                                                        {for items.iter().map(|item|{
                                                            let id=item.id.clone();
                                                            let open=open.clone();
                                                            html! {
                                                                <button type="button" class="question-bank-paper-option"
                                                                    data-testid="shenlun-open-paper" disabled={*busy}
                                                                    onclick={Callback::from(move |_|open.emit(id.clone()))}>
                                                                    <span class="min-w-0">
                                                                        <strong class="block text-sm">{item.title.clone()}</strong>
                                                                        <span class="block text-xs opacity-55">{item.variant.clone()}</span>
                                                                    </span>
                                                                    <span class="shrink-0 text-xs opacity-60">{format!("{}题",item.questions)}</span>
                                                                </button>
                                                            }
                                                        })}
                                                    </div>
                                                </details>
                                            })}
                                        </div>
                                    </details>
                                }
                            })}
                        </div>
                    }
                </div>
            </section>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_label_does_not_guess_from_question_number() {
        let item = QuestionView {
            id: "test".into(), number: 5, stem: "请概括".into(),
            options_markdown: String::new(), materials: vec![], module: None,
            category: None, topics: vec![], answer_type: "subjective".into(),
            question_type: Some("other".into()), grading_mode: "ai".into(), max_score: None,
        };
        assert_eq!(topic_label(&item),"其他");
    }
}
