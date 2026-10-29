//! Independent logic drills use server-owned answers and deadlines.
use super::material_scroll::{scroll_edges, MaterialScrollButton, ScrollEdges};
use super::*;
use gloo_timers::callback::Interval;

#[derive(Properties, PartialEq)]
pub(crate) struct LogicJudgmentProps {
    pub csrf: String,
    pub on_immersive_change: Callback<bool>,
}

#[derive(Clone)]
enum LogicAction {
    Prepare,
    Open(String),
    Answer(usize, usize),
    Submit,
    Pause,
    PauseAndExit,
    Result(String),
    Retry(String),
    Prompt,
    ResetPrompt,
    Back,
}

// Practice and review share one two-pane workspace: left is the question,
// right is the options (plus explanations in review). Each pane scrolls
// independently while Apple Pencil ink follows its underlying content.
#[derive(Properties, PartialEq)]
struct LogicScrollPaneProps {
    question_index: usize,
    status: &'static str,
    right_title: &'static str,
    question: Html,
    answers: Html,
}

#[function_component(LogicScrollPane)]
fn logic_scroll_pane(props: &LogicScrollPaneProps) -> Html {
    let question_scroll = use_node_ref();
    let answer_scroll = use_node_ref();
    let scratchpad_open = use_context::<bool>().unwrap_or(false);
    let question_edges = use_state(ScrollEdges::default);
    let answer_edges = use_state(ScrollEdges::default);
    let refresh_edges = {
        let question_scroll = question_scroll.clone();
        let answer_scroll = answer_scroll.clone();
        let question_edges = question_edges.clone();
        let answer_edges = answer_edges.clone();
        Callback::from(move |_: ()| {
            if let Some(element) = question_scroll.cast::<web_sys::HtmlElement>() {
                let next = scroll_edges(&element);
                if *question_edges != next {
                    question_edges.set(next);
                }
            }
            if let Some(element) = answer_scroll.cast::<web_sys::HtmlElement>() {
                let next = scroll_edges(&element);
                if *answer_edges != next {
                    answer_edges.set(next);
                }
            }
        })
    };
    let on_scroll = {
        let refresh_edges = refresh_edges.clone();
        Callback::from(move |_: web_sys::Event| refresh_edges.emit(()))
    };
    {
        let question_scroll = question_scroll.clone();
        let answer_scroll = answer_scroll.clone();
        let refresh_edges = refresh_edges.clone();
        use_effect_with(props.question_index, move |_| {
            if let Some(element) = question_scroll.cast::<web_sys::HtmlElement>() {
                element.set_scroll_top(0);
            }
            if let Some(element) = answer_scroll.cast::<web_sys::HtmlElement>() {
                element.set_scroll_top(0);
            }
            refresh_edges.emit(());
            || ()
        });
    }
    {
        let refresh_edges = refresh_edges.clone();
        use_effect_with(scratchpad_open, move |_| {
            refresh_edges.emit(());
            || ()
        });
    }
    {
        let refresh_edges = refresh_edges.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window();
            let handler =
                wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                    refresh_edges.emit(());
                });
            let attached = window.as_ref().is_some_and(|window| {
                window
                    .add_event_listener_with_callback("resize", handler.as_ref().unchecked_ref())
                    .is_ok()
            });
            move || {
                if attached {
                    if let Some(window) = window {
                        let _ = window.remove_event_listener_with_callback(
                            "resize", handler.as_ref().unchecked_ref(),
                        );
                    }
                }
            }
        });
    }

    html! {
        <TrainingSplitWorkspace test_id="logic-scroll-workspace" split_at_md={true}
            left={html! {
                <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box bg-base-100"
                    data-testid="logic-question-pane">
                <div class="relative z-30 flex shrink-0 items-center justify-between gap-2 px-4 pb-2 pt-16 sm:px-6 md:pt-3">
                    <div class="flex min-w-0 items-center gap-3 text-xs text-base-content/60">
                        <span>{format!("第 {}/10 题", props.question_index + 1)}</span>
                        <span>{props.status}</span>
                    </div>
                    if scratchpad_open {
                        <div class="flex shrink-0 gap-1" role="group" aria-label="题目滚动" data-testid="logic-scroll-buttons">
                            <MaterialScrollButton passage={question_scroll.clone()} up={true}
                                disabled={!question_edges.can_scroll_up} question_index={props.question_index} target="题目" />
                            <MaterialScrollButton passage={question_scroll.clone()} up={false}
                                disabled={!question_edges.can_scroll_down} question_index={props.question_index} target="题目" />
                        </div>
                    }
                </div>
                <article ref={question_scroll} onscroll={on_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y px-4 pb-8 sm:px-6"
                    data-testid="logic-question-scroll">
                    {props.question.clone()}
                </article>
                </section>
            }}
            right={html! {
                <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box bg-base-100"
                    data-testid="logic-answer-pane">
                <div class="relative z-30 flex shrink-0 items-center justify-between gap-2 px-4 pb-2 pt-3 sm:px-6 md:pt-16">
                    <span class="text-xs font-semibold text-base-content/60">{props.right_title}</span>
                    if scratchpad_open {
                        <div class="flex shrink-0 gap-1" role="group" aria-label="选项滚动" data-testid="logic-answer-scroll-buttons">
                            <MaterialScrollButton passage={answer_scroll.clone()} up={true}
                                disabled={!answer_edges.can_scroll_up} question_index={props.question_index} target="选项" />
                            <MaterialScrollButton passage={answer_scroll.clone()} up={false}
                                disabled={!answer_edges.can_scroll_down} question_index={props.question_index} target="选项" />
                        </div>
                    }
                </div>
                <article ref={answer_scroll} onscroll={on_scroll}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y px-4 pb-8 sm:px-6"
                    data-testid="logic-answer-scroll">
                    {props.answers.clone()}
                </article>
                </section>
            }}
        />
    }
}

#[function_component(LogicJudgmentTraining)]
pub(crate) fn logic_judgment_training(props: &LogicJudgmentProps) -> Html {
    let status = use_state(|| None::<LogicStatus>);
    let unfinished = use_state(Vec::<LogicRun>::new);
    let history = use_state(Vec::<LogicHistory>::new);
    let statistics = use_state(Vec::<LogicPerformance>::new);
    let history_open = use_state(|| false);
    let opened_card = use_state(|| None::<String>);
    let prompt = use_state(String::new);
    let run = use_state(|| None::<LogicRun>);
    let result = use_state(|| None::<LogicResult>);
    let job = use_state(|| None::<String>);
    let progress = use_state(String::new);
    let error = use_state(|| None::<String>);
    let busy = use_state(|| false);
    let lock = use_mut_ref(|| false);
    let current = use_state(|| 0usize);
    let reload = use_state(|| 0u32);
    let clock = use_state(Date::now);
    let offset = use_state(|| 0f64);
    let expiry_attempted_run = use_mut_ref(|| None::<String>);
    // Match data-analysis: hydrate Markdown math when the visible question,
    // submitted review, or Flashcard detail is mounted/changed.
    {
        let math_view = (
            run.as_ref().map(|r| {
                (
                    r.id.clone(),
                    r.answers.clone(),
                    r.paused_remaining_ms.is_some(),
                )
            }),
            result.as_ref().map(|r| r.id.clone()),
            *current,
            (*opened_card).clone(),
        );
        use_effect_with(math_view, move |_| {
            hydrate_math();
            || ()
        });
    }
    {
        let clock = clock.clone();
        use_effect_with((), move |_| {
            let timer = Interval::new(250, move || clock.set(Date::now()));
            move || drop(timer)
        });
    }
    {
        let immersive = props.on_immersive_change.clone();
        use_effect_with(
            (run.is_some(), result.is_some()),
            move |(active, reviewing)| {
                immersive.emit(*active || *reviewing);
                move || immersive.emit(false)
            },
        );
    }
    {
        let (status, unfinished, history, statistics, prompt, job, error) = (
            status.clone(),
            unfinished.clone(),
            history.clone(),
            statistics.clone(),
            prompt.clone(),
            job.clone(),
            error.clone(),
        );
        use_effect_with(*reload, move |_| {
            let alive = Rc::new(Cell::new(true));
            let check = alive.clone();
            spawn_local(async move {
                let loaded = async {
                    let state: LogicStatus =
                        fetch_json(&format!("{LOGIC_ROOT}/status"), "逻辑判断状态").await?;
                    let pending: Vec<LogicRun> =
                        fetch_json(&format!("{LOGIC_ROOT}/unfinished"), "未完成试卷").await?;
                    let records: Vec<LogicHistory> =
                        fetch_json(&format!("{LOGIC_ROOT}/history"), "历史记录").await?;
                    let settings: UserPromptSettings =
                        fetch_json(&format!("{LOGIC_ROOT}/user-prompt"), "用户提示").await?;
                    Ok::<_, String>((state, pending, records, settings))
                }
                .await;
                if !check.get() {
                    return;
                }
                match loaded {
                    Ok((state, pending, records, settings)) => {
                        job.set(state.preparing_job_id.clone());
                        status.set(Some(state));
                        unfinished.set(pending);
                        history.set(records);
                        prompt.set(settings.user_prompt);
                    }
                    Err(message) => error.set(Some(message)),
                }
                match fetch_json::<Vec<LogicPerformance>>(
                    &format!("{LOGIC_ROOT}/card-performance"),
                    "卡片统计",
                )
                .await
                {
                    Ok(items) if check.get() => statistics.set(items),
                    Err(message) if check.get() => error.set(Some(message)),
                    _ => {}
                }
            });
            move || alive.set(false)
        });
    }
    {
        let (job, progress, error, reload) =
            (job.clone(), progress.clone(), error.clone(), reload.clone());
        use_effect_with((*job).clone(), move |id| {
            let alive = Rc::new(Cell::new(true));
            if let Some(id) = id.clone() {
                let check = alive.clone();
                spawn_local(async move {
                    while check.get() {
                        match fetch_json::<PrepareProgress>(
                            &format!("{LOGIC_ROOT}/prepare/{id}"),
                            "出题进度",
                        )
                        .await
                        {
                            Ok(state) if check.get() => {
                                progress.set(format!(
                                    "{} · 已接收 {}/10",
                                    state.message, state.verified
                                ));
                                if state.state == "ready" {
                                    if state.run_id.is_none() {
                                        error.set(Some("出题完成但缺少试卷标识".into()));
                                    }
                                    job.set(None);
                                    reload.set(*reload + 1);
                                    break;
                                }
                                if matches!(state.state.as_str(), "failed" | "cancelled") {
                                    error.set(Some(state.message));
                                    job.set(None);
                                    break;
                                }
                            }
                            Err(message) if check.get() => {
                                error.set(Some(message));
                            }
                            _ => break,
                        }
                        TimeoutFuture::new(1500).await;
                    }
                });
            }
            move || alive.set(false)
        });
    }
    let action = {
        let (run, result, job, error, busy, lock, current, reload, offset, prompt) = (
            run.clone(),
            result.clone(),
            job.clone(),
            error.clone(),
            busy.clone(),
            lock.clone(),
            current.clone(),
            reload.clone(),
            offset.clone(),
            prompt.clone(),
        );
        let csrf = props.csrf.clone();
        Callback::from(move |command: LogicAction| {
            if *lock.borrow() {
                return;
            }
            *lock.borrow_mut() = true;
            busy.set(true);
            error.set(None);
            let (run, result, job, error, busy, lock, current, reload, offset, prompt, csrf) = (
                run.clone(),
                result.clone(),
                job.clone(),
                error.clone(),
                busy.clone(),
                lock.clone(),
                current.clone(),
                reload.clone(),
                offset.clone(),
                prompt.clone(),
                csrf.clone(),
            );
            spawn_local(async move {
                let outcome = async {
                    match command {
                        LogicAction::Prepare => {
                            let task: PrepareTask = post_json_response(
                                &format!("{LOGIC_ROOT}/prepare"),
                                &csrf,
                                &EmptyRequest {},
                                "出题",
                            )
                            .await?;
                            job.set(Some(task.job_id));
                        }
                        LogicAction::Open(id) => {
                            let active = enter_logic_run(&id, &csrf).await?;
                            if active.status == "submitted" {
                                let score = read_logic_result(&id).await?;
                                result.set(Some(score));
                                run.set(None);
                                reload.set(*reload + 1);
                            } else {
                                offset.set(active.server_now_ms as f64 - Date::now());
                                current.set(0);
                                result.set(None);
                                run.set(Some(active));
                            }
                        }
                        LogicAction::Answer(question, choice) => {
                            if let Some(mut active) = (*run).clone() {
                                post_json(
                                    &format!("{LOGIC_ROOT}/runs/{}/answers", active.id),
                                    &csrf,
                                    &LogicAnswer { question, choice },
                                    "保存答案",
                                )
                                .await?;
                                active.answers[question] = Some(choice);
                                run.set(Some(active));
                            }
                        }
                        LogicAction::Pause | LogicAction::PauseAndExit => {
                            if let Some(active) = run.as_ref() {
                                let exiting = matches!(command, LogicAction::PauseAndExit);
                                if active.paused_remaining_ms.is_none() || !exiting {
                                    let updated: LogicRun = post_json_response(
                                        &format!("{LOGIC_ROOT}/runs/{}/pause", active.id),
                                        &csrf,
                                        &EmptyRequest {},
                                        "暂停或继续训练",
                                    )
                                    .await?;
                                    offset.set(updated.server_now_ms as f64 - Date::now());
                                    run.set(Some(updated));
                                }
                                if exiting {
                                    run.set(None);
                                    reload.set(*reload + 1);
                                }
                            }
                        }
                        LogicAction::Submit => {
                            if let Some(active) = run.as_ref() {
                                let score: LogicResult = post_json_response(
                                    &format!("{LOGIC_ROOT}/runs/{}/submit", active.id),
                                    &csrf,
                                    &EmptyRequest {},
                                    "交卷",
                                )
                                .await?;
                                result.set(Some(score));
                                run.set(None);
                                reload.set(*reload + 1);
                            }
                        }
                        LogicAction::Result(id) => {
                            let score = read_logic_result(&id).await?;
                            result.set(Some(score));
                            run.set(None);
                        }
                        LogicAction::Retry(id) => {
                            let retry: LogicRetry = post_json_response(
                                &format!("{LOGIC_ROOT}/runs/{id}/retry"),
                                &csrf,
                                &EmptyRequest {},
                                "重做原卷",
                            )
                            .await?;
                            let active = enter_logic_run(&retry.run_id, &csrf).await?;
                            offset.set(active.server_now_ms as f64 - Date::now());
                            current.set(0);
                            result.set(None);
                            run.set(Some(active));
                        }
                        LogicAction::ResetPrompt => {
                            let settings: UserPromptSettings = post_json_response(
                                &format!("{LOGIC_ROOT}/user-prompt"),
                                &csrf,
                                &UserPromptUpdate { user_prompt: None },
                                "恢复默认出题要求",
                            )
                            .await?;
                            prompt.set(settings.user_prompt);
                        }
                        LogicAction::Prompt => {
                            let settings: UserPromptSettings = post_json_response(
                                &format!("{LOGIC_ROOT}/user-prompt"),
                                &csrf,
                                &UserPromptUpdate {
                                    user_prompt: Some((*prompt).clone())
                                        .filter(|text| !text.trim().is_empty()),
                                },
                                "保存用户提示",
                            )
                            .await?;
                            prompt.set(settings.user_prompt);
                        }
                        LogicAction::Back => {
                            run.set(None);
                            result.set(None);
                            reload.set(*reload + 1);
                        }
                    }
                    Ok::<_, String>(())
                }
                .await;
                if let Err(message) = outcome {
                    error.set(Some(message));
                }
                *lock.borrow_mut() = false;
                busy.set(false);
            });
        })
    };
    let remaining = run
        .as_ref()
        .map(|active| {
            active
                .paused_remaining_ms
                .map(|ms| (ms.max(0) as u64).div_ceil(1000))
                .or_else(|| {
                    active.deadline_at_ms.map(|deadline| {
                        ((deadline as f64 - (*clock + *offset)).max(0.0) / 1000.0).ceil() as u64
                    })
                })
                .unwrap_or(0)
        })
        .unwrap_or(0);
    {
        let action = action.clone();
        let expiry_attempted_run = expiry_attempted_run.clone();
        use_effect_with(
            (
                run.as_ref().map(|r| r.id.clone()),
                remaining == 0
                    && run
                        .as_ref()
                        .is_some_and(|r| r.paused_remaining_ms.is_none()),
                *busy,
            ),
            move |(id, expired, busy)| {
                if let Some(id) = id {
                    if *expired && !*busy && expiry_attempted_run.borrow().as_ref() != Some(id) {
                        *expiry_attempted_run.borrow_mut() = Some(id.clone());
                        action.emit(LogicAction::Submit);
                    }
                }
                || ()
            },
        );
    }

    let invoke = |command: LogicAction| {
        let action = action.clone();
        Callback::from(move |_| action.emit(command.clone()))
    };
    let alert = html! { if let Some(message) = error.as_ref() { <p class="text-error break-words" role="alert">{message}</p> } };
    if let Some(active) = run.as_ref() {
        if let Some(remaining_ms) = active.paused_remaining_ms {
            return html! {
                <section class="analysis-fullscreen analysis-paused fixed inset-0 z-[70] flex h-dvh w-screen items-center justify-center overflow-hidden bg-base-200 p-4" data-testid="logic-paused">
                    <div class="w-full max-w-md rounded-box bg-base-100 p-6 text-center sm:p-8">
                        <h2 class="text-2xl font-bold">{"训练已暂停"}</h2>
                        <p class="mt-3 text-sm text-base-content/60">{"题目已隐藏，暂停期间不能答题。刷新或退出后剩余时间保持不变。"}</p>
                        <p class="my-6 font-mono text-3xl" role="timer" data-testid="logic-clock">{format!("{:02}:{:02}", remaining_ms.max(0)/60000, (remaining_ms.max(0)/1000)%60)}</p>
                        <div class="flex flex-wrap justify-center gap-3">
                            <button class="btn btn-outline" disabled={*busy} onclick={invoke(LogicAction::PauseAndExit)}>{"← 退出训练"}</button>
                            <button class="btn btn-primary" data-testid="logic-continue" disabled={*busy} onclick={invoke(LogicAction::Pause)}>{"继续计时"}</button>
                        </div>
                        {alert}
                    </div>
                </section>
            };
        }
        let index = (*current).min(active.questions.len().saturating_sub(1));
        let question = &active.questions[index];
        return html! {
            <section class="analysis-fullscreen fixed inset-0 z-[70] flex h-dvh w-screen min-w-0 flex-col overflow-hidden bg-base-200" data-testid="logic-run">
                <header class="analysis-header flex shrink-0 items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                    <div class="flex min-w-0 items-center gap-2">
                        <button class="btn btn-ghost btn-sm" disabled={*busy} onclick={invoke(LogicAction::PauseAndExit)}>{"← 暂停并退出"}</button>
                        <strong class="truncate text-sm">{"逻辑判断综合训练"}</strong>
                        <span class="hidden text-xs text-base-content/60 sm:inline">{format!("第 {}/10 题",index+1)}</span>
                    </div>
                    <div class="flex shrink-0 items-center gap-2">
                        <button type="button" role="timer" class={if remaining<=60 {"btn btn-ghost font-mono text-xl text-error"} else {"btn btn-ghost font-mono text-xl"}} data-testid="logic-clock" title="点击暂停计时" disabled={*busy || remaining==0} onclick={invoke(LogicAction::Pause)}>{format!("{:02}:{:02}",remaining/60,remaining%60)}</button>
                        <button class="btn btn-sm btn-error btn-outline" data-testid="logic-submit" disabled={*busy} onclick={invoke(LogicAction::Submit)}>{"交卷"}</button>
                    </div>
                </header>
                {alert}
                <div class="relative min-h-0 flex-1 overflow-hidden">
                    <InkOverlay question_index={index} responsive={true} scroll_linked={true} persist_key={format!("logic-{}-{index}",active.id)}>
                        <LogicScrollPane question_index={index} status="作答中" right_title="选项"
                            question={html! {
                                <div class="study-markdown whitespace-pre-wrap break-words text-lg font-semibold leading-8" data-testid="logic-question">
                                    {render_markdown(&question.question, "")}
                                </div>
                            }}
                            answers={html! {
                                <div class="grid gap-3">{for question.options.iter().enumerate().map(|(choice, text)| {
                                    let selected = active.answers[index] == Some(choice);
                                    html! {
                                        <button class={classes!("btn", "h-auto", "min-h-14", "justify-start", "whitespace-normal", "py-3", "text-left", if selected { "btn-primary" } else { "btn-outline" })}
                                            data-testid="logic-choice" aria-pressed={selected.to_string()} disabled={*busy || remaining == 0}
                                            onclick={invoke(LogicAction::Answer(index, choice))}>
                                            <span class="mr-2 shrink-0">{format!("{} ·", (b'A' + choice as u8) as char)}</span>
                                            <span class="study-markdown min-w-0">{render_markdown(text, "")}</span>
                                        </button>
                                    }
                                })}</div>
                            }}
                        />
                    </InkOverlay>
                </div>
                {question_navigation(
                    current.clone(),
                    index,
                    &active.answers.iter().map(|answer| answer.is_some().then_some(true)).collect::<Vec<_>>(),
                    false,
                    None,
                    "logic-navigation",
                    "logic-question-tab",
                )}
            </section>
        };
    }
    if let Some(score) = result.as_ref() {
        let index = (*current).min(score.questions.len().saturating_sub(1));
        let q = &score.questions[index];
        let grade = if q.chosen == Some(q.correct_index) {
            "正确"
        } else if q.chosen.is_some() {
            "错误"
        } else {
            "未答"
        };
        return html! {
            <section class="analysis-fullscreen fixed inset-0 z-[70] flex h-dvh w-screen min-w-0 flex-col overflow-hidden bg-base-200" data-testid="logic-result">
                <header class="analysis-header flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                    <button class="btn btn-ghost btn-sm" disabled={*busy} onclick={invoke(LogicAction::Back)}>{"← 返回训练"}</button>
                    <strong class="text-sm">{format!("答卷结果 · {} / {}",score.correct,score.total)}</strong>
                    <button class="btn btn-primary btn-sm" data-testid="logic-retry" disabled={*busy} onclick={invoke(LogicAction::Retry(score.id.clone()))}>{"重做原题"}</button>
                </header>
                {alert}
                <div class="relative min-h-0 flex-1 overflow-hidden">
                    <InkOverlay question_index={index} responsive={true} scroll_linked={true} persist_key={format!("logic-{}-{index}",score.id)}>
                        <LogicScrollPane question_index={index} status={grade} right_title="答案与解析"
                            question={html! {
                                <div class="study-markdown whitespace-pre-wrap break-words text-lg font-semibold leading-8" data-testid="logic-question">
                                    {render_markdown(&q.question, "")}
                                </div>
                            }}
                            answers={html! {
                                <div data-testid="logic-review-content">
                                    <div class="grid gap-2">{for q.options.iter().enumerate().map(|(i,option)| {
                        let style = if i==q.correct_index {"flex items-center justify-between rounded-box border border-success bg-success/10 p-3 text-success"}
                            else if q.chosen==Some(i) {"flex items-center justify-between rounded-box border border-error bg-error/10 p-3 text-error"}
                            else {"flex items-center justify-between rounded-box border border-base-content/20 p-3"};
                        let hint = if i==q.correct_index {"✓ 正确答案"} else if q.chosen==Some(i) {"✕ 你的错选"} else {""};
                        html! {<div class={style} data-testid="logic-review-choice">
                            <span class="flex min-w-0 items-start gap-2"><span class="shrink-0">{format!("{}.",(b'A'+i as u8) as char)}</span><span class="study-markdown min-w-0">{render_markdown(option, "")}</span></span><span class="shrink-0 text-xs">{hint}</span>
                        </div>}
                    })}</div>
                    <div class="mt-5 border-t border-base-content/10 pt-4">
                        <p class="text-sm font-semibold">{format!("你的答案：{}",q.chosen.map(|i|((b'A'+i as u8) as char).to_string()).unwrap_or("未答".into()))}</p>
                        <h3 class="mt-3 font-semibold">{"解题解析"}</h3>
                        <div class="study-markdown mt-2 whitespace-pre-wrap text-sm leading-7">{render_markdown(&q.explanation, "")}</div>
                        <details class="mt-4 rounded-box border border-base-content/15 p-3">
                            <summary class="cursor-pointer font-semibold">{"查看关联方法卡"}</summary>
                            <div class="mt-3 space-y-3">
                                <div class="study-markdown">{render_markdown(&q.front,&q.card_path)}</div>
                                <div class="divider"></div>
                                <div class="study-markdown">{render_markdown(&q.back,&q.card_path)}</div>
                            </div>
                        </details>
                                    </div>
                                </div>
                            }}
                        />
                    </InkOverlay>
                </div>
                {question_navigation(
                    current.clone(),
                    index,
                    &score.questions.iter().map(|q| q.chosen.map(|chosen| chosen == q.correct_index)).collect::<Vec<_>>(),
                    true,
                    None,
                    "logic-navigation",
                    "logic-question-tab",
                )}
            </section>
        };
    }
    if *history_open {
        return html! {
            <section class="mx-auto max-w-4xl space-y-4 p-5" data-testid="logic-history">
                <div class="flex items-center justify-between">
                    <h2 class="text-xl font-bold">{"逻辑判断练习历史"}</h2>
                    <button class="btn btn-outline btn-sm" onclick={{let history_open=history_open.clone(); Callback::from(move |_|history_open.set(false))}}>{"返回训练"}</button>
                </div>
                if history.is_empty() { <p class="text-sm text-base-content/60">{"尚无已交卷的逻辑判断训练。"}</p> }
                {for history.iter().map(|h| {
                    let finished=Date::new(&JsValue::from_f64(h.ended_at_ms as f64)).to_locale_string("zh-CN",&JsValue::UNDEFINED);
                    html! {
                        <article class="rounded-box bg-base-100 p-4" key={h.id.clone()}>
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <div>
                                    <strong>{format!("{} · {}/{}题",finished,h.correct,h.total)}</strong>
                                    <p class="mt-1 text-xs text-base-content/60">{format!("作答 {} 题 · 可查看解析或重做",h.answered)}</p>
                                </div>
                                <div class="flex flex-wrap gap-2">
                                    <button class="btn btn-outline btn-sm" disabled={*busy} onclick={invoke(LogicAction::Result(h.id.clone()))}>{"查看原题与解析"}</button>
                                    <button class="btn btn-primary btn-sm" disabled={*busy} onclick={invoke(LogicAction::Retry(h.id.clone()))}>{"重做原题"}</button>
                                </div>
                            </div>
                        </article>
                    }
                })}
                {alert}
            </section>
        };
    }
    let answered: i64 = statistics.iter().map(|s| s.answered_count as i64).sum();
    let correct: i64 = statistics.iter().map(|s| s.correct_count as i64).sum();
    html! {
        <section class="mx-auto max-w-3xl p-4 sm:p-6" data-testid="logic-setup">
            <div class="rounded-box bg-base-100 p-5 sm:p-7">
                <h2 class="text-xl font-bold">{"逻辑判断综合训练"}</h2>
                <p class="mt-1 text-sm text-base-content/60">{"形式推理 · 每卷10道独立题 · 10分钟 · 支持暂停与恢复"}</p>
                <div class="mt-5 grid gap-3">
                    if !unfinished.is_empty() {
                        <div class="space-y-3 rounded-box border border-primary/30 bg-primary/5 p-4" data-testid="logic-unfinished">
                            <h3 class="font-semibold">{"未完成的逻辑判断训练"}</h3>
                            <p class="text-xs text-base-content/60">{"答案和剩余时间保存在设备，暂停后可以随时回来继续。"}</p>
                            {for unfinished.iter().map(|r| {
                                let name=if r.status=="prepared" {"尚未开始"} else if r.paused_remaining_ms.is_some() {"已暂停"} else {"进行中"};
                                html! {
                                    <div class="flex flex-wrap items-center justify-between gap-3 rounded-box bg-base-100 p-3" key={r.id.clone()}>
                                        <div>
                                            <div class="font-semibold text-sm">{format!("{} · 已作答 {}/10 题",name,r.answers.iter().flatten().count())}</div>
                                            <p class="text-xs text-base-content/60">{if let Some(ms)=r.paused_remaining_ms {format!("剩余 {:02}:{:02}",ms.max(0)/60000,(ms.max(0)/1000)%60)} else {"服务器保存答题进度".into()}}</p>
                                        </div>
                                        <button class="btn btn-primary btn-sm" data-testid="logic-resume" disabled={*busy} onclick={invoke(LogicAction::Open(r.id.clone()))}>{if r.status=="prepared" {"开始训练"} else {"继续训练"}}</button>
                                    </div>
                                }
                            })}
                        </div>
                    }
                    <button class="btn btn-outline w-full" data-testid="logic-history-toggle" onclick={{let history_open=history_open.clone();Callback::from(move |_|history_open.set(true))}}>{"查看练习历史"}</button>
                    <div class="rounded-box border border-base-content/15 p-4 space-y-2">
                        <p class="font-semibold">{"智能选卡 · 逻辑判断"}</p>
                        <p class="text-sm text-base-content/60">{"按小P S1 直言、S2 联言选言假言、S3 组合排列、S4 特殊题型，从方法卡和错题卡中选取10张有效卡；不出一拖五。"}</p>
                        if let Some(state)=status.as_ref() {
                            <p class="text-xs text-base-content/60">{format!("当前有效卡片：{} 张",state.eligible_cards)}</p>
                        }
                    </div>
                    <details class="rounded-box border border-base-content/15 bg-base-100/50 p-3" data-testid="logic-user-prompt">
                        <summary class="cursor-pointer select-none font-semibold">{"出题要求"}<span class="ml-2 text-xs font-normal text-base-content/60">{"点击编辑"}</span></summary>
                        <div class="mt-3 space-y-3">
                            <p class="text-xs text-base-content/60">{"仅调整难度与命题偏好，系统仍固定题型范围及 Codex 自检约束。"}</p>
                            <label for="logic-prompt" class="text-sm">{"补充偏好（保留题型范围与 Codex 自检要求）"}</label>
                            <textarea id="logic-prompt" class="textarea textarea-bordered w-full min-h-48 font-mono text-sm leading-6" maxlength="8000" value={(*prompt).clone()} oninput={{let prompt=prompt.clone(); Callback::from(move |e: InputEvent| {let target:web_sys::HtmlTextAreaElement=e.target_unchecked_into();prompt.set(target.value());})}}/>
                            <div class="flex flex-wrap justify-end gap-2">
                                <button class="btn btn-outline btn-sm" disabled={*busy} onclick={invoke(LogicAction::ResetPrompt)}>{"恢复默认"}</button>
                                <button class="btn btn-primary btn-sm" disabled={*busy} onclick={invoke(LogicAction::Prompt)}>{"保存用户提示"}</button>
                            </div>
                        </div>
                    </details>
                    <TrainingMetrics answered={answered} correct={correct} prefix="logic" />
                    <details class="min-w-0 max-w-full overflow-hidden rounded-box border border-base-content/15 p-3" data-testid="logic-statistics">
                        <summary class="cursor-pointer font-semibold">{format!("全部卡片（{}）",statistics.len())}</summary>
                        <div class="mt-3 max-h-72 w-full min-w-0 space-y-2 overflow-y-auto">
                            {for statistics.iter().enumerate().map(|(i,card)|{
                                let preview=card.front.lines().find(|line|!line.trim().is_empty())
                                    .unwrap_or("未命名卡片").trim_start_matches('#').trim().trim_matches('*').to_owned();
                                let accuracy=if card.answered_count==0 {"未作答".into()} else {format!("{:.1}%",100.0*card.correct_count as f64/card.answered_count as f64)};
                                let opened_card=opened_card.clone();
                                let id=card.card_id.clone();
                                html! {
                                    <button class="flex w-full min-w-0 items-center gap-2 overflow-hidden rounded-box bg-base-200 p-3 text-left hover:bg-base-300" key={card.card_id.clone()} data-testid="logic-card-item"
                                        onclick={Callback::from(move |_|opened_card.set(Some(id.clone())))}>
                                        <span class="w-5 shrink-0 text-xs text-base-content/50">{i+1}</span>
                                        <span class="min-w-0 flex-1 overflow-hidden">
                                            <span class="block truncate text-sm font-semibold">{preview}</span>
                                            <span class="block truncate text-xs text-base-content/60">{format!("{} · 作答 {} 次",card.category,card.answered_count)}</span>
                                        </span>
                                        <span class="shrink-0 text-sm font-semibold">{accuracy}</span>
                                        <span aria-hidden="true">{"›"}</span>
                                    </button>
                                }
                            })}
                        </div>
                        <p class="mt-2 text-xs text-base-content/60">{"按虚拟调度次数排序，点开卡片可查看 Front/Back。"}</p>
                    </details>
                    if let Some(card)=opened_card.as_ref().as_ref().and_then(|id|statistics.iter().find(|c|c.card_id.as_str()==id.as_str())).cloned() {
                        <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 p-3 sm:p-6" data-testid="logic-card-detail" role="dialog" aria-modal="true" aria-label="Flashcard 卡片详情">
                            <div class="flex max-h-[90dvh] w-full max-w-2xl flex-col overflow-hidden rounded-box bg-base-100 shadow-2xl">
                                <div class="flex shrink-0 items-center justify-between border-b border-base-content/10 p-3">
                                    <p class="font-semibold">{format!("卡片详情 · {}",card.category)}</p>
                                    <button class="btn btn-outline btn-sm" onclick={{let opened_card=opened_card.clone();Callback::from(move |_|opened_card.set(None))}}>{"← 返回"}</button>
                                </div>
                                <div class="min-h-0 space-y-4 overflow-y-auto p-4">
                                    <section class="rounded-box border border-base-content/15 p-3"><p class="mb-2 text-xs font-semibold text-base-content/60">{"Front"}</p><div class="study-markdown">{render_markdown(&card.front,&card.card_path)}</div></section>
                                    <section class="rounded-box border border-base-content/15 p-3"><p class="mb-2 text-xs font-semibold text-base-content/60">{"Back"}</p><div class="study-markdown">{render_markdown(&card.back,&card.card_path)}</div></section>
                                </div>
                            </div>
                        </div>
                    }
                    if job.is_some() {
                        <div class="rounded-box bg-base-200 p-3 text-sm" role="status" data-testid="logic-progress">{if progress.is_empty() {"正在恢复出题任务…".to_owned()} else {(*progress).clone()}}</div>
                    }
                    {alert}
                    if let Some(state)=status.as_ref() {
                        if !state.configured {<p class="text-error text-sm" role="status">{"请先配置 Codex 出题服务。"}</p>}
                        if state.eligible_cards<10 {<p class="text-error text-sm" role="status">{"至少需要10张不同的有效卡片。"}</p>}
                        <button class="btn btn-primary w-full" data-testid="logic-prepare" disabled={*busy || job.is_some() || !state.configured || state.eligible_cards<10}
                            onclick={invoke(LogicAction::Prepare)}>{if job.is_some() {"正在出题中…（可以切换页面）"} else {"✨ 智能出题"}}</button>
                    }
                </div>
            </div>
        </section>
    }
}
