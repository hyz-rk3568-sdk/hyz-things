//! Original-paper Question Bank practice. All four sources share one workspace.
use super::*;
use gloo_timers::callback::Interval;
use hyz_things::domain::question_bank::{
    CreatePracticeRequest, FavoriteState, PaperModules, PaperSummary, PracticeMode,
    PracticeReview, PracticeSession, PracticeWorkspace, SavePracticeAnswer, SetFavorite,
    SetPracticePaused,
};

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionBankTrainingProps {
    pub csrf: String,
    pub on_immersive_change: Callback<bool>,
    pub on_paper_pause_change: Callback<bool>,
}

fn available_choices(markdown: &str) -> Vec<(String, String)> {
    markdown.lines().filter_map(|line| {
        let line = line.trim_start().strip_prefix("- ")?;
        let (label, _) = line.split_once(['.', '．', '、', ' '])?;
        if label.len() == 1 && label.bytes().all(|c| c.is_ascii_uppercase()) {
            Some((label.into(), line.into()))
        } else {
            None
        }
    }).collect()
}

#[function_component(QuestionBankTraining)]
pub(crate) fn question_bank_training(props: &QuestionBankTrainingProps) -> Html {
    let papers = use_state(Vec::<PaperSummary>::new);
    let selected = use_state(|| None::<PaperSummary>);
    let modules = use_state(|| None::<PaperModules>);
    let module = use_state(|| None::<String>);
    let mode = use_state(|| PracticeMode::Paper);
    let workspace = use_state(|| None::<PracticeWorkspace>);
    let review = use_state(|| None::<PracticeReview>);
    let history = use_state(Vec::<PracticeSession>::new);
    let history_reload = use_state(|| 0_u32);
    let index = use_state(|| 0_usize);
    let elapsed = use_state(|| 0_u64);
    let favorite = use_state(|| false);
    let multi_select = use_state(|| false);
    let notice = use_state(|| None::<String>);
    let busy = use_state(|| false);

    {
        let papers = papers.clone();
        let history = history.clone();
        let notice = notice.clone();
        use_effect_with(*history_reload, move |_| {
            spawn_local(async move {
                match fetch_json::<Vec<PaperSummary>>("/api/v1/question-bank/papers", "真题试卷").await {
                    Ok(values) => papers.set(values),
                    Err(error) => notice.set(Some(error)),
                }
                if let Ok(values) = fetch_json::<Vec<PracticeSession>>(
                    "/api/v1/question-bank/practice/history", "练习历史",
                ).await {
                    history.set(values);
                }
            });
            || ()
        });
    }
    let current = (*workspace).as_ref().and_then(|work| work.questions.get(*index)).cloned();
    let review_current = (*review).as_ref().and_then(|work| work.questions.get(*index)).cloned();
    let question_id = current.as_ref().map(|item| item.item.question_id.clone()).unwrap_or_default();

    {
        let favorite = favorite.clone();
        use_effect_with(question_id.clone(), move |id| {
            if !id.is_empty() {
                let id = id.clone();
                spawn_local(async move {
                    if let Ok(state) = fetch_json::<FavoriteState>(
                        &format!("/api/v1/question-bank/practice/favorites/{id}"), "收藏状态",
                    ).await {
                        favorite.set(state.favorite);
                    }
                });
            } else { favorite.set(false); }
            || ()
        });
    }
    {
        let elapsed = elapsed.clone();
        let status = (*workspace).as_ref().map(|work| (
            work.session.id.clone(), work.session.status.clone(),
            work.session.paused, work.session.elapsed_seconds,
        ));
        use_effect_with(status, move |status| {
            let mut timer = None;
            if let Some((_, state, paused, seconds)) = status {
                elapsed.set(*seconds);
                if state == "active" && !paused {
                    let elapsed = elapsed.clone();
                    timer = Some(Interval::new(1000, move || {
                        elapsed.set((*elapsed).saturating_add(1));
                    }));
                }
            }
            move || drop(timer)
        });
    }
    {
        let has_review = review.is_some();
        use_effect_with((question_id.clone(), has_review), move |_| {
            hydrate_math();
            || ()
        });
    }

    let choose_paper = {
        let selected = selected.clone();
        let modules = modules.clone();
        let module = module.clone();
        let notice = notice.clone();
        Callback::from(move |paper: PaperSummary| {
            selected.set(Some(paper.clone()));
            modules.set(None);
            module.set(None);
            let modules = modules.clone();
            let notice = notice.clone();
            spawn_local(async move {
                match fetch_json::<PaperModules>(
                    &format!("/api/v1/question-bank/papers/{}/modules", paper.id), "原卷模块",
                ).await {
                    Ok(list) => modules.set(Some(list)),
                    Err(error) => notice.set(Some(error)),
                }
            });
        })
    };

    let open_session = {
        let workspace = workspace.clone();
        let review = review.clone();
        let index = index.clone();
        let busy = busy.clone();
        let notice = notice.clone();
        let on_immersive = props.on_immersive_change.clone();
        let on_pause = props.on_paper_pause_change.clone();
        Callback::from(move |session: PracticeSession| {
            busy.set(true);
            let workspace = workspace.clone();
            let review = review.clone();
            let index = index.clone();
            let busy = busy.clone();
            let notice = notice.clone();
            let on_immersive = on_immersive.clone();
            let on_pause = on_pause.clone();
            spawn_local(async move {
                let address = format!("/api/v1/question-bank/practice/sessions/{}", session.id);
                match fetch_json::<PracticeWorkspace>(&address, "真题练习").await {
                    Ok(value) => {
                        on_immersive.emit(value.session.status == "active");
                        on_pause.emit(value.session.paused);
                        index.set(0);
                        workspace.set(Some(value));
                        if session.status == "completed" {
                            match fetch_json::<PracticeReview>(
                                &format!("{address}/review"), "交卷复盘",
                            ).await {
                                Ok(value) => review.set(Some(value)),
                                Err(error) => notice.set(Some(error)),
                            }
                        } else {
                            review.set(None);
                        }
                    }
                    Err(error) => notice.set(Some(error)),
                }
                busy.set(false);
            });
        })
    };

    let start = {
        let csrf = props.csrf.clone();
        let selected = selected.clone();
        let module = module.clone();
        let mode = mode.clone();
        let notice = notice.clone();
        let busy = busy.clone();
        let open = open_session.clone();
        Callback::from(move |_| {
            if *busy || csrf.is_empty() { return; }
            let kind = (*mode).clone();
            let paper_id = match kind {
                PracticeMode::Paper | PracticeMode::PaperModule => selected.as_ref().map(|paper| paper.id.clone()),
                _ => None,
            };
            let requested_module = if kind == PracticeMode::PaperModule { (*module).clone() } else { None };
            if matches!(kind, PracticeMode::Paper | PracticeMode::PaperModule) && paper_id.is_none()
                || kind == PracticeMode::PaperModule && requested_module.is_none()
            {
                notice.set(Some("请先选择原卷和模块".into()));
                return;
            }
            let request = CreatePracticeRequest {
                mode: kind, paper_id, module: requested_module,
            };
            busy.set(true);
            notice.set(None);
            let csrf = csrf.clone();
            let busy = busy.clone();
            let notice = notice.clone();
            let open = open.clone();
            spawn_local(async move {
                match post_json_response::<_, PracticeSession>(
                    "/api/v1/question-bank/practice/sessions", &csrf, &request, "创建练习",
                ).await {
                    Ok(session) => open.emit(session),
                    Err(error) => notice.set(Some(error)),
                }
                busy.set(false);
            });
        })
    };

    let save_answer = {
        let index = index.clone();
        let csrf = props.csrf.clone();
        let workspace = workspace.clone();
        let multi_select = multi_select.clone();
        let notice = notice.clone();
        Callback::from(move |selected: String| {
            let Some(mut active) = (*workspace).clone() else { return };
            let Some(item) = active.questions.get(*index) else { return };
            if active.session.paused || active.session.status != "active" { return; }
            let mut answer = if *multi_select { item.item.answer.clone() } else { Vec::new() };
            if answer.contains(&selected) {
                answer.retain(|v| v != &selected);
            } else {
                answer.push(selected);
            }
            answer.sort();
            let session_id = active.session.id.clone();
            let question_id = item.item.question_id.clone();
            let csrf = csrf.clone();
            let workspace = workspace.clone();
            let notice = notice.clone();
            spawn_local(async move {
                let path = format!(
                    "/api/v1/question-bank/practice/sessions/{session_id}/answers/{question_id}"
                );
                match post_json_response::<_, PracticeSession>(
                    &path, &csrf, &SavePracticeAnswer { answer: answer.clone() }, "保存答案",
                ).await {
                    Ok(saved) => {
                        active.session = saved;
                        if let Some(item) = active.questions.iter_mut().find(|q| q.item.question_id == question_id) {
                            item.item.answer = answer;
                        }
                        workspace.set(Some(active));
                    }
                    Err(error) => notice.set(Some(error)),
                }
            });
        })
    };

    let pause = {
        let csrf = props.csrf.clone();
        let workspace = workspace.clone();
        let elapsed = elapsed.clone();
        let notice = notice.clone();
        let on_pause = props.on_paper_pause_change.clone();
        Callback::from(move |_| {
            let Some(mut active) = (*workspace).clone() else { return };
            let next = !active.session.paused;
            let id = active.session.id.clone();
            let csrf = csrf.clone();
            let workspace = workspace.clone();
            let notice = notice.clone();
            let on_pause = on_pause.clone();
            let seconds = *elapsed;
            spawn_local(async move {
                match post_json_response::<_, PracticeSession>(
                    &format!("/api/v1/question-bank/practice/sessions/{id}/pause"),
                    &csrf, &SetPracticePaused { paused: next, elapsed_seconds: seconds }, "暂停恢复",
                ).await {
                    Ok(saved) => {
                        active.session = saved;
                        on_pause.emit(next);
                        workspace.set(Some(active));
                    }
                    Err(error) => notice.set(Some(error)),
                }
            });
        })
    };

    let submit = {
        let csrf = props.csrf.clone();
        let workspace = workspace.clone();
        let review = review.clone();
        let busy = busy.clone();
        let notice = notice.clone();
        let history_reload = history_reload.clone();
        let elapsed = elapsed.clone();
        let on_pause = props.on_paper_pause_change.clone();
        let on_immersive = props.on_immersive_change.clone();
        Callback::from(move |_| {
            let Some(mut active) = (*workspace).clone() else { return };
            if *busy { return; }
            let remaining = active.session.items.iter().filter(|item| item.answer.is_empty()).count();
            if remaining != 0 && !web_sys::window()
                .and_then(|window| window.confirm_with_message(
                    &format!("还有 {remaining} 题未作答，确认交卷？")
                ).ok()).unwrap_or(false) {
                return;
            }
            busy.set(true);
            let id = active.session.id.clone();
            let csrf = csrf.clone();
            let workspace = workspace.clone();
            let review = review.clone();
            let busy = busy.clone();
            let notice = notice.clone();
            let history_reload = history_reload.clone();
            let seconds = *elapsed;
            let on_pause = on_pause.clone();
            let on_immersive = on_immersive.clone();
            spawn_local(async move {
                let path = format!("/api/v1/question-bank/practice/sessions/{id}");
                // Persist time before the idempotent submit so the score includes
                // uninterrupted sessions as well as previously paused ones.
                let paused: Result<PracticeSession, String> = post_json_response(
                    &format!("{path}/pause"), &csrf,
                    &SetPracticePaused { paused: true, elapsed_seconds: seconds }, "记录用时",
                ).await;
                if let Err(error) = paused {
                    notice.set(Some(error));
                    busy.set(false);
                    return;
                }
                match post_json_response::<_, PracticeSession>(
                    &format!("{path}/submit"), &csrf, &EmptyRequest {}, "交卷",
                ).await {
                    Ok(saved) => {
                        active.session = saved;
                        workspace.set(Some(active));
                        on_pause.emit(true);
                        on_immersive.emit(false);
                        history_reload.set(*history_reload + 1);
                        match fetch_json::<PracticeReview>(&format!("{path}/review"), "答案解析").await {
                            Ok(value) => review.set(Some(value)),
                            Err(error) => notice.set(Some(error)),
                        }
                    }
                    Err(error) => notice.set(Some(error)),
                }
                busy.set(false);
            });
        })
    };
    let toggle_favorite = {
        let csrf = props.csrf.clone();
        let favorite = favorite.clone();
        let notice = notice.clone();
        let id = question_id.clone();
        Callback::from(move |_| {
            if id.is_empty() { return; }
            let next = !*favorite;
            let csrf = csrf.clone();
            let favorite = favorite.clone();
            let notice = notice.clone();
            let id = id.clone();
            spawn_local(async move {
                match post_json_response::<_, FavoriteState>(
                    &format!("/api/v1/question-bank/practice/favorites/{id}"),
                    &csrf, &SetFavorite { favorite: next }, "收藏真题",
                ).await {
                    Ok(value) => favorite.set(value.favorite),
                    Err(error) => notice.set(Some(error)),
                }
            });
        })
    };
    let exit = {
        let workspace = workspace.clone();
        let review = review.clone();
        let history_reload = history_reload.clone();
        let on_immersive = props.on_immersive_change.clone();
        let on_pause = props.on_paper_pause_change.clone();
        Callback::from(move |_| {
            workspace.set(None);
            review.set(None);
            history_reload.set(*history_reload + 1);
            on_immersive.emit(false);
            on_pause.emit(false);
        })
    };

    if let Some(active) = (*workspace).as_ref() {
        let total = active.questions.len();
        let selected_question = active.questions.get(*index);
        let detail = review_current.as_ref();
        let is_review = review.is_some();
        let content = selected_question.and_then(|entry| entry.question.as_ref());
        let material_ids = content.map(|q| q.materials.clone()).unwrap_or_default();
        let materials = selected_question.map(|entry| entry.materials.clone()).unwrap_or_default();
        let content_key = material_ids.join(",");
        let question_panel = html! {
            <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-box bg-base-100">
                <div class={TRAINING_PANE_HEADER}>
                    <strong>{format!("原卷第 {} 题 · {}/{}", selected_question.map(|q| q.item.number).unwrap_or_default(), *index+1, total)}</strong>
                    <InkToolbarSlot />
                </div>
                <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4" data-testid="question-bank-question-pane">
                    if let Some(question) = content {
                        if !question.issues.is_empty() {
                            <p class="text-warning text-sm">{format!("待核：{}", question.issues.join("、"))}</p>
                        }
                        <div class="study-markdown leading-8">{render_question_bank_markdown(&question.stem)}</div>
                        <div class="mt-5 space-y-2" data-testid="question-bank-answer-choices">
                            {for available_choices(&question.options_markdown).into_iter().map(|(label, raw)| {
                                let chosen = selected_question.is_some_and(|q| q.item.answer.contains(&label));
                                let send = save_answer.clone();
                                let value = label.clone();
                                html! {
                                    <button type="button" class={classes!("btn","btn-outline","h-auto","min-h-12","w-full","justify-start","whitespace-normal","text-left",chosen.then_some("btn-primary"))}
                                        data-testid={format!("question-bank-option-{label}")} aria-pressed={chosen.to_string()}
                                        disabled={*busy || active.session.paused || is_review}
                                        onclick={Callback::from(move |_| send.emit(value.clone()))}>
                                        <span class="study-markdown">{render_question_bank_markdown(&format!("- {raw}"))}</span>
                                    </button>
                                }
                            })}
                        </div>
                        if is_review {
                            if let Some(item) = detail {
                                <div class="mt-6 border-t border-base-content/20 pt-4 space-y-2" data-testid="question-bank-explanation">
                                    <p class="font-bold">{format!("判定：{}", item.question.item.grade.clone().unwrap_or_default())}</p>
                                    <p>{format!("你的答案：{}", if item.question.item.answer.is_empty() {"未答".into()} else {item.question.item.answer.join("、")})}</p>
                                    <p>{format!("正确答案：{}", if item.correct_answer.is_empty() {"不可判分或旧版本不可用".into()} else {item.correct_answer.join("、")})}</p>
                                    <div class="study-markdown">{render_question_bank_markdown(
                                        if item.explanation.is_empty() {"暂无解析"} else {&item.explanation},
                                    )}</div>
                                    if !question.topics.is_empty() { <p class="text-sm">{format!("考点：{}",question.topics.join("、"))}</p> }
                                    <p class="text-sm">{format!("模块：{} · 题型：{}", question.module.as_deref().unwrap_or("未归类"), question.category.as_deref().unwrap_or("未标注"))}</p>
                                </div>
                            }
                        } else if active.session.paused {
                            <p class="mt-4 text-warning">{"已暂停。恢复后继续答题。"}</p>
                        }
                    } else {
                        <p class="text-warning" data-testid="question-bank-stale">{"原题内容已更新或暂不可用，无法用新版题目冒充原练习版本。"}</p>
                    }
                </div>
            </div>
        };
        let workspace_body = if materials.is_empty() {
            html! {<div class="h-full min-h-0" data-testid="question-bank-single-pane">{question_panel}</div>}
        } else {
            html! {
                <TrainingSplitWorkspace test_id="question-bank-split-workspace" split_at_md={true}
                    left={html! {
                        <article key={content_key} class="flex min-h-0 flex-col overflow-hidden rounded-box bg-base-100" data-testid="question-bank-material-pane">
                            <div class={TRAINING_PANE_HEADER}><strong>{"共用材料"}</strong></div>
                            <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4" data-testid="question-bank-material-scroll">
                                {for materials.iter().map(|material| html! {
                                    <section class="mb-6">
                                        <h4 class="font-semibold">{material.name.clone()}</h4>
                                        <div class="study-markdown leading-8">{render_question_bank_markdown(&material.markdown)}</div>
                                    </section>
                                })}
                            </div>
                        </article>
                    }}
                    right={question_panel} />
            }
        };
        let select_previous = {
            let index = index.clone();
            Callback::from(move |_| index.set(index.saturating_sub(1)))
        };
        let select_next = {
            let index = index.clone();
            Callback::from(move |_| index.set((*index + 1).min(total.saturating_sub(1))))
        };
        return html! {
            <div class={FIXED_TRAINING_LAYOUT} data-testid="question-bank-workspace">
                <div class="flex shrink-0 flex-wrap items-center gap-2 pb-2">
                    <button type="button" class="btn btn-sm" onclick={exit}>{"返回选卷"}</button>
                    <strong class="min-w-0 truncate">{"真题训练"}</strong>
                    <span class="text-sm">{format!("{:02}:{:02}", *elapsed / 60, *elapsed % 60)}</span>
                    <div class="ml-auto flex gap-1">
                        <button type="button" class="btn btn-sm" onclick={toggle_favorite}>
                            {if *favorite {"★ 已收藏"} else {"☆ 收藏"}}
                        </button>
                        if !is_review {
                            <button type="button" class="btn btn-sm" onclick={pause}>
                                {if active.session.paused {"恢复"} else {"暂停"}}
                            </button>
                            <button type="button" class="btn btn-primary btn-sm" onclick={submit} disabled={*busy}>{"交卷"}</button>
                        }
                    </div>
                </div>
                {if let Some(message) = &*notice { html!{<p role="status" class="text-warning text-sm">{message.clone()}</p>} } else { Html::default() }}
                if is_review {
                    if let Some(score)=&active.session.score {
                        <div class="flex gap-3 text-sm pb-2" data-testid="question-bank-score">
                            <span>{format!("正确 {}",score.correct)}</span>
                            <span>{format!("错误 {}",score.wrong)}</span>
                            <span>{format!("未答 {}",score.unanswered)}</span>
                            <span>{format!("不可判分 {}",score.ungradable)}</span>
                        </div>
                    }
                }
                <InkOverlay question_index={*index} persist_key={format!("question-bank-{}-{}",active.session.id,question_id)}
                    responsive={true} scroll_linked={true}>
                    {workspace_body}
                </InkOverlay>
                <div class="flex shrink-0 items-center justify-between gap-2 pt-2">
                    <button type="button" class="btn btn-sm" disabled={*index==0} onclick={select_previous}>{"上一题"}</button>
                    <span>{format!("{} / {}",*index+1,total)}</span>
                    if !is_review {
                        <label class="flex items-center gap-1 text-sm">
                            <input type="checkbox" checked={*multi_select} onchange={Callback::from({
                                let multi_select=multi_select.clone();
                                move |event: Event| {
                                    let input = event.target_unchecked_into::<web_sys::HtmlInputElement>();
                                    multi_select.set(input.checked());
                                }
                            })} />{"多选"}
                        </label>
                    }
                    <button type="button" class="btn btn-sm" disabled={*index+1>=total} onclick={select_next}>{"下一题"}</button>
                </div>
                <div class="flex max-h-12 shrink-0 gap-1 overflow-x-auto pb-1" data-testid="question-bank-navigation">
                    {for (0..total).map(|i| {
                        let index=index.clone();
                        let answered=active.session.items.get(i).is_some_and(|q| !q.answer.is_empty());
                        html! {
                            <button type="button" class={classes!("btn","btn-xs",(*index==i).then_some("btn-primary"),answered.then_some("btn-outline"))}
                                onclick={Callback::from(move |_|index.set(i))}>{format!("{}",active.session.items[i].number)}</button>
                        }
                    })}
                </div>
            </div>
        };
    }

    html! {
        <TrainingSetupLayout test_id="question-bank-setup" heading={Some(html!{
            <h2 class="text-xl font-bold">{"真题训练"}</h2>
        })} summary={AttrValue::from("选择原卷或复练题单")}
        detail={AttrValue::from("完整原卷顺序；不跨卷随机抽题")}
        action={Some(html!{<button type="button" class={TRAINING_PRIMARY_BUTTON}
            disabled={*busy || props.csrf.is_empty()} onclick={start}>{"开始练习"}</button>})}>
            {if let Some(message) = &*notice {
                html!{<p role="status" class="text-warning text-sm">{message.clone()}</p>}
            } else { Html::default() }}
            <div class="flex flex-wrap gap-2 mb-4" data-testid="question-bank-practice-modes">
                {for [
                    (PracticeMode::Paper,"整卷训练"),
                    (PracticeMode::PaperModule,"原卷模块"),
                    (PracticeMode::Wrong,"错题复练"),
                    (PracticeMode::Favorite,"收藏训练"),
                ].into_iter().map(|(value,label)|{
                    let mode=mode.clone();
                    let is_selected=*mode==value;
                    html!{<button type="button" class={classes!("btn","btn-sm",is_selected.then_some("btn-primary"))}
                        aria-pressed={is_selected.to_string()} onclick={Callback::from(move |_|mode.set(value.clone()))}>{label}</button>}
                })}
            </div>
            if matches!(*mode, PracticeMode::Paper | PracticeMode::PaperModule) {
                <div class="space-y-2" data-testid="question-bank-select-paper">
                    {for papers.iter().map(|paper| {
                        let select=choose_paper.clone();
                        let value=paper.clone();
                        let chosen=selected.as_ref().is_some_and(|p| p.id==paper.id);
                        html! {
                            <button type="button" class={classes!("btn","btn-outline","h-auto","min-h-12","w-full","justify-between","text-left",chosen.then_some("btn-primary"))}
                                data-testid="question-bank-select-paper-item" onclick={Callback::from(move |_|select.emit(value.clone()))}>
                                <span>{paper.title.clone()}</span>
                                <span class="text-xs">{format!("{} · {} · {}题",paper.year,paper.region,paper.questions)}</span>
                            </button>
                        }
                    })}
                </div>
                if *mode == PracticeMode::PaperModule {
                    if let Some(list) = &*modules {
                        if list.unclassified > 0 {
                            <p class="text-warning text-sm">{format!("本卷有 {} 题模块未知，未静默归入其他模块",list.unclassified)}</p>
                        }
                        <div class="flex flex-wrap gap-2 mt-4" data-testid="question-bank-module-list">
                            {for list.modules.iter().map(|entry| {
                                let value=entry.name.clone();
                                let label=format!("{} · {}题",entry.name,entry.count);
                                let chosen=module.as_ref()==Some(&value);
                                let module=module.clone();
                                html! {
                                    <button type="button" class={classes!("btn","btn-sm",chosen.then_some("btn-primary"))}
                                        aria-pressed={chosen.to_string()} onclick={Callback::from(move |_|module.set(Some(value.clone())))}>{label}</button>
                                }
                            })}
                        </div>
                    }
                }
            } else {
                <p class="text-sm opacity-70">{"从当前待复练错题或收藏真题中按来源原卷、原题号排序，不随机组卷。"}</p>
            }
            <div class="border-t border-base-content/20 pt-4 mt-6" data-testid="question-bank-history">
                <h3 class="font-semibold mb-2">{"练习历史"}</h3>
                {for history.iter().map(|item| {
                    let open = open_session.clone();
                    let value = item.clone();
                    html!{
                        <button type="button" class="btn btn-outline w-full h-auto justify-between my-1 p-3 text-left"
                            onclick={Callback::from(move |_|open.emit(value.clone()))}>
                            <span>{format!("{} · {}题 · {}",item.module.clone().unwrap_or_else(||"整卷/复练".into()), item.items.len(), if item.status=="completed"{"已交卷"}else{"继续"})}</span>
                            <span class="text-xs">{item.created_at.to_string()}</span>
                        </button>
                    }
                })}
            </div>
        </TrainingSetupLayout>
    }
}
