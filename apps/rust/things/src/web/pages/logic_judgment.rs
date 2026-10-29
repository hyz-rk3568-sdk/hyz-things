//! Independent logic drills use server-owned answers and deadlines.
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
    {
        let clock = clock.clone();
        use_effect_with((), move |_| {
            let timer = Interval::new(250, move || clock.set(Date::now()));
            move || drop(timer)
        });
    }
    {
        let immersive = props.on_immersive_change.clone();
        use_effect_with((run.is_some(), result.is_some()), move |(active, reviewing)| {
            immersive.emit(*active || *reviewing);
            move || immersive.emit(false)
        });
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
                                    ).await?;
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
                                &format!("{LOGIC_ROOT}/user-prompt"), &csrf,
                                &UserPromptUpdate { user_prompt: None }, "恢复默认出题要求",
                            ).await?;
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
    let remaining = run.as_ref().map(|active| {
        active.paused_remaining_ms
            .map(|ms| (ms.max(0) as u64).div_ceil(1000))
            .or_else(|| active.deadline_at_ms.map(|deadline|
                ((deadline as f64 - (*clock + *offset)).max(0.0) / 1000.0).ceil() as u64
            )).unwrap_or(0)
    }).unwrap_or(0);
    {
        let action = action.clone();
        let expiry_attempted_run = expiry_attempted_run.clone();
        use_effect_with(
            (run.as_ref().map(|r| r.id.clone()), remaining == 0 && run.as_ref().is_some_and(|r| r.paused_remaining_ms.is_none()), *busy),
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
        let previous = {
            let current = current.clone();
            Callback::from(move |_| current.set(index.saturating_sub(1)))
        };
        let next = {
            let current = current.clone();
            Callback::from(move |_| current.set((index + 1).min(9)))
        };
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
                    <InkOverlay question_index={index} responsive={true} persist_key={format!("logic-{}-{index}",active.id)}>
                        <article class="h-full min-w-0 overflow-y-auto overscroll-contain rounded-box bg-base-100 p-4 pb-12 pt-16 sm:p-6 sm:pb-12 sm:pt-16">
                            <div class="mb-3 flex justify-between text-xs text-base-content/60"><span>{format!("第 {}/10 题",index+1)}</span><span>{"作答中"}</span></div>
                            <h2 class="whitespace-pre-wrap break-words text-lg font-semibold leading-8" data-testid="logic-question">{question.question.clone()}</h2>
                            <div class="mt-5 grid gap-3">{for question.options.iter().enumerate().map(|(choice, text)| {
                                let selected = active.answers[index] == Some(choice);
                                html! {
                                <button class={classes!("btn", "h-auto", "min-h-14", "justify-start", "whitespace-normal", "py-3", "text-left", if selected { "btn-primary" } else { "btn-outline" })} data-testid="logic-choice" aria-pressed={selected.to_string()} disabled={*busy || remaining == 0} onclick={invoke(LogicAction::Answer(index, choice))}>{format!("{} · {}", (b'A' + choice as u8) as char, text)}</button>
                            }})}</div>
                        </article>
                    </InkOverlay>
                </div>
                <nav class="flex shrink-0 items-center justify-between gap-2 py-2" aria-label="题号导航" data-testid="logic-navigation">
                    <button class="btn btn-outline btn-sm" disabled={index == 0} onclick={previous}>{"上一题"}</button>
                    <div class="flex min-w-0 flex-1 items-center justify-center gap-1 overflow-x-auto">
                        {for (0..10).map(|i| { let current = current.clone(); html! {
                            <button class={if i==index {"btn btn-primary btn-xs shrink-0"} else if active.answers[i].is_some() {"btn btn-primary btn-outline btn-xs shrink-0"} else {"btn btn-ghost btn-xs shrink-0"}}
                                aria-label={format!("第{}题{}",i+1,if active.answers[i].is_some(){" 已答"}else{" 未答"})} aria-current={(i == index).to_string()}
                                onclick={Callback::from(move |_|current.set(i))}>{i+1}</button>
                        } })}
                    </div>
                    <button class="btn btn-primary btn-sm" disabled={index == 9} onclick={next}>{"下一题"}</button>
                </nav>
            </section>
        };
    }
    if let Some(score) = result.as_ref() {
        let index = (*current).min(score.questions.len().saturating_sub(1));
        let q = &score.questions[index];
        let grade = if q.chosen == Some(q.correct_index) { "正确" }
            else if q.chosen.is_some() { "错误" } else { "未答" };
        let previous = { let current=current.clone(); Callback::from(move |_|current.set(index.saturating_sub(1))) };
        let next = { let current=current.clone(); Callback::from(move |_|current.set((index+1).min(9))) };
        return html! {
            <section class="analysis-fullscreen fixed inset-0 z-[70] flex h-dvh w-screen min-w-0 flex-col overflow-hidden bg-base-200" data-testid="logic-result">
                <header class="analysis-header flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                    <button class="btn btn-ghost btn-sm" disabled={*busy} onclick={invoke(LogicAction::Back)}>{"← 返回训练"}</button>
                    <strong class="text-sm">{format!("答卷结果 · {} / {}",score.correct,score.total)}</strong>
                    <button class="btn btn-primary btn-sm" data-testid="logic-retry" disabled={*busy} onclick={invoke(LogicAction::Retry(score.id.clone()))}>{"重做原题"}</button>
                </header>
                {alert}
                <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain rounded-box bg-base-100 p-4 sm:p-6" data-testid="logic-review-content">
                    <div class="flex justify-between text-xs text-base-content/60">
                        <span>{format!("第 {}/10 题",index+1)}</span>
                        <span>{grade}</span>
                    </div>
                    <h2 class="my-4 whitespace-pre-wrap break-words text-lg font-semibold leading-8">{q.question.clone()}</h2>
                    <div class="grid gap-2">{for q.options.iter().enumerate().map(|(i,option)| {
                        let style = if i==q.correct_index {"flex items-center justify-between rounded-box border border-success bg-success/10 p-3 text-success"}
                            else if q.chosen==Some(i) {"flex items-center justify-between rounded-box border border-error bg-error/10 p-3 text-error"}
                            else {"flex items-center justify-between rounded-box border border-base-content/20 p-3"};
                        let hint = if i==q.correct_index {"✓ 正确答案"} else if q.chosen==Some(i) {"✕ 你的错选"} else {""};
                        html! {<div class={style} data-testid="logic-review-choice">
                            <span>{format!("{}. {}",(b'A'+i as u8) as char,option)}</span><span class="shrink-0 text-xs">{hint}</span>
                        </div>}
                    })}</div>
                    <div class="mt-5 border-t border-base-content/10 pt-4">
                        <p class="text-sm font-semibold">{format!("你的答案：{}",q.chosen.map(|i|((b'A'+i as u8) as char).to_string()).unwrap_or("未答".into()))}</p>
                        <h3 class="mt-3 font-semibold">{"解题解析"}</h3>
                        <p class="mt-2 whitespace-pre-wrap text-sm leading-7">{q.explanation.clone()}</p>
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
                <nav class="flex shrink-0 items-center justify-between gap-2 py-2" aria-label="题号导航">
                    <button class="btn btn-outline btn-sm" disabled={index==0} onclick={previous}>{"上一题"}</button>
                    <div class="flex min-w-0 flex-1 items-center justify-center gap-1 overflow-x-auto">
                        {for score.questions.iter().enumerate().map(|(i,q)| {
                            let current=current.clone();
                            let style=if i==index {"btn btn-primary btn-xs shrink-0"} else if q.chosen.is_none() {"btn btn-ghost btn-xs shrink-0"} else if q.chosen==Some(q.correct_index) {"btn btn-success btn-outline btn-xs shrink-0"} else {"btn btn-error btn-outline btn-xs shrink-0"};
                            html!{<button class={style} onclick={Callback::from(move |_|current.set(i))}>{i+1}</button>}
                        })}
                    </div>
                    <button class="btn btn-primary btn-sm" disabled={index==9} onclick={next}>{"下一题"}</button>
                </nav>
            </section>
        };
    }
    html! {
        <section class="mx-auto max-w-3xl space-y-4 p-4" data-testid="logic-setup">
            <h2 class="text-xl font-bold">{"逻辑判断 · 10题 / 10分钟"}</h2>
            <p>{"形式推理 + 分析推理 · VCNT 自动选卡，无固定题型配额。刷新或切换页面后可恢复；离开不暂停倒计时。"}</p>
            {alert}
            if let Some(state) = status.as_ref() {
                <p>{format!("有效卡片池：{} 张",state.eligible_cards)}</p>
                if !state.configured { <p role="status">{"请先配置 Codex 出题服务。"}</p> }
                if state.eligible_cards < 10 { <p role="status">{"至少需要10张有效且不同的已同步卡片，不能重复凑题。"}</p> }
                <button class="btn btn-primary" data-testid="logic-prepare" disabled={*busy || job.is_some() || !state.configured || state.eligible_cards < 10} onclick={invoke(LogicAction::Prepare)}>{"生成新卷"}</button>
            }
            if job.is_some() { <p role="status" data-testid="logic-progress">{if progress.is_empty(){"正在恢复出题任务…".to_owned()}else{(*progress).clone()}}</p> }
            <div class="space-y-2"><h3>{"未完成试卷"}</h3>{for unfinished.iter().map(|r| html! { <button class="btn btn-outline" data-testid="logic-resume" disabled={*busy} onclick={invoke(LogicAction::Open(r.id.clone()))}>{format!("继续试卷 {} · 已答 {}/10",r.id,r.answers.iter().flatten().count())}</button> })}</div>
            <details><summary>{"出题用户提示"}</summary>
                <label for="logic-prompt">{"补充偏好（保留题型范围与 Codex 自检要求）"}</label>
                <textarea id="logic-prompt" class="textarea w-full" maxlength="8000" value={(*prompt).clone()} oninput={{ let prompt=prompt.clone(); Callback::from(move |e: InputEvent| { let target: web_sys::HtmlTextAreaElement=e.target_unchecked_into(); prompt.set(target.value()); }) }} />
                <button class="btn btn-sm" disabled={*busy} onclick={invoke(LogicAction::Prompt)}>{"保存用户提示"}</button>
            </details>
            <details data-testid="logic-history"><summary>{format!("历史记录 · {} 卷",history.len())}</summary>
                <p>{"重做使用原题，不增加 VCNT；不会重新生成题目。"}</p>
                {for history.iter().map(|h| html! { <div class="flex flex-wrap gap-2 py-2"><span>{format!("{} · {} / {} · 已答 {} · {}",h.id,h.correct,h.total,h.answered,Date::new(&JsValue::from_f64(h.ended_at_ms as f64)).to_locale_string("zh-CN", &JsValue::UNDEFINED))}</span><button class="btn btn-xs" disabled={*busy} onclick={invoke(LogicAction::Result(h.id.clone()))}>{"查看结果"}</button><button class="btn btn-xs" disabled={*busy} onclick={invoke(LogicAction::Retry(h.id.clone()))}>{"重做原卷"}</button></div> })}
            </details>
            <details data-testid="logic-statistics"><summary>{"卡片作答统计"}</summary><p>{"仅统计已交卷且实际作答的题目；资料分析和普通闪卡统计独立。"}</p>{for statistics.iter().map(|s| html! { <p>{format!("{} · 已答 {} · 正确 {}",s.card_id,s.answered_count,s.correct_count)}</p> })}</details>
        </section>
    }
}
