//! 20 questions / 20 minutes. Immersive iPad workspace with 60:40 material/question panes.
use super::material_scroll::{scroll_edges, MaterialScrollButton, ScrollEdges};
use super::*;
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use js_sys::Date;
use serde::Serialize;
use wasm_bindgen::JsCast;

#[derive(Properties, PartialEq)]
pub(crate) struct ComprehensiveProps {
    pub csrf: String,
    pub on_immersive_change: Callback<bool>,
    pub on_exit: Callback<()>,
}

const COMPREHENSIVE_JOB_KEY: &str = "hyz_comprehensive_prepare_job";
const COMPREHENSIVE_READY_KEY: &str = "hyz_comprehensive_ready_run";

// One fixed-height workspace serves both exam and answer review. Only the
// material and question panes scroll, never the outer page.
#[derive(Properties, PartialEq)]
struct AnalysisPanesProps {
    material: hyz_things::domain::data_analysis::Material,
    question: Html,
    question_index: usize,
    #[prop_or("题目与选项")]
    right_title: &'static str,
}

// Match the material's `leading-8` (2rem = 32 CSS px) so each click
// advances exactly one rendered text line. Keep controls outside scrollable content.
#[function_component(AnalysisPanes)]
fn analysis_panes(props: &AnalysisPanesProps) -> Html {
    let passage = use_node_ref();
    let question_scroll = use_node_ref();
    let question_edges = use_state(ScrollEdges::default);
    let scratchpad_open = use_context::<bool>().unwrap_or(false);
    let scroll_state = use_state(ScrollEdges::default);
    let refresh_scroll_edges = {
        let passage = passage.clone();
        let scroll_state = scroll_state.clone();
        let question_scroll = question_scroll.clone();
        let question_edges = question_edges.clone();
        Callback::from(move |_: ()| {
            if let Some(element) = passage.cast::<web_sys::HtmlElement>() {
                let next = scroll_edges(&element);
                if *scroll_state != next {
                    scroll_state.set(next);
                }
            }
            if let Some(element) = question_scroll.cast::<web_sys::HtmlElement>() {
                let next = scroll_edges(&element);
                if *question_edges != next {
                    question_edges.set(next);
                }
            }
        })
    };
    let on_scroll = {
        let refresh_scroll_edges = refresh_scroll_edges.clone();
        Callback::from(move |_: web_sys::Event| refresh_scroll_edges.emit(()))
    };
    {
        let refresh_scroll_edges = refresh_scroll_edges.clone();
        use_effect_with(
            (
                scratchpad_open,
                props.material.text.clone(),
                props.question_index,
            ),
            move |_| {
                refresh_scroll_edges.emit(());
                || ()
            },
        );
    }
    {
        let refresh_scroll_edges = refresh_scroll_edges.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window();
            let handler =
                wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                    refresh_scroll_edges.emit(())
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
                            "resize",
                            handler.as_ref().unchecked_ref(),
                        );
                    }
                }
            }
        });
    }

    html! {
        <TrainingSplitWorkspace test_id="analysis-shared-workspace"
            left={html! {
                <article class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box bg-base-100" data-testid="analysis-material">
                <div class={TRAINING_PANE_HEADER}>
                    <div class="min-w-0 text-sm font-semibold">{props.material.topic.clone()}</div>
                    if scratchpad_open {
                        <div class="relative z-30 flex shrink-0 gap-1" role="group" aria-label="材料滚动">
                            <MaterialScrollButton passage={passage.clone()} up={true} disabled={!scroll_state.can_scroll_up} question_index={props.question_index} />
                            <MaterialScrollButton passage={passage.clone()} up={false} disabled={!scroll_state.can_scroll_down} question_index={props.question_index} />
                        </div>
                    }
                </div>
                <div ref={passage} onscroll={on_scroll.clone()} class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y scroll-auto" data-testid="analysis-material-scroll">
                    <div class="whitespace-pre-wrap px-4 pb-4 pt-2 text-base leading-8">{props.material.text.clone()}</div>
                </div>
                </article>
            }}
            right={html! {
                <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box bg-base-100" data-testid="analysis-answer-pane">
                    <div class={classes!(TRAINING_PANE_HEADER, "flex-wrap")} data-testid="analysis-answer-header">
                        <span class="text-sm font-semibold">{props.right_title}</span>
                        <div class="ml-auto flex shrink-0 items-center gap-1">
                            if scratchpad_open {
                                <div class="flex gap-1" role="group" aria-label="题目滚动">
                                    <MaterialScrollButton passage={question_scroll.clone()} up={true} disabled={!question_edges.can_scroll_up} question_index={props.question_index} target="题目" />
                                    <MaterialScrollButton passage={question_scroll.clone()} up={false} disabled={!question_edges.can_scroll_down} question_index={props.question_index} target="题目" />
                                </div>
                            }
                            <InkToolbarSlot />
                        </div>
                    </div>
                    <article ref={question_scroll} onscroll={on_scroll} class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y" data-testid="analysis-question">{props.question.clone()}</article>
                </section>
            }}
        />
    }
}

// The grouping and graded navigation state are shared with logic judgment.
fn analysis_navigation(
    current: UseStateHandle<usize>,
    index: usize,
    markers: &[Option<bool>],
    is_review: bool,
) -> Html {
    question_navigation(
        current,
        index,
        markers,
        is_review,
        Some(5),
        "analysis-navigation",
        "analysis-question-tab",
    )
}

#[derive(Serialize)]
struct ComprehensivePrepareRequest {}

fn remember_preparation(id: Option<&str>) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        match id {
            Some(id) => {
                let _ = storage.set_item(COMPREHENSIVE_JOB_KEY, id);
            }
            None => {
                let _ = storage.remove_item(COMPREHENSIVE_JOB_KEY);
            }
        }
    }
}

async fn resume_preparation(
    id: String,
    progress: UseStateHandle<Option<PrepareProgress>>,
    ready_id: UseStateHandle<Option<String>>,
    preparing: UseStateHandle<bool>,
    error: UseStateHandle<Option<String>>,
) {
    preparing.set(true);
    for _ in 0..800 {
        match fetch_json_typed::<PrepareProgress>(
            &format!("{ANALYSIS_ROOT}/prepare/{id}"),
            "恢复综合训练出题进度",
        )
        .await
        {
            Ok(state) => {
                progress.set(Some(state.clone()));
                if state.state == "ready" {
                    if let Some(run_id) = state.run_id {
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item(COMPREHENSIVE_READY_KEY, &run_id);
                        }
                        remember_preparation(None);
                        ready_id.set(Some(run_id));
                        preparing.set(false);
                        return;
                    }
                    remember_preparation(None);
                    error.set(Some("出题完成但没有找到可进入的训练，请重新出题".into()));
                    preparing.set(false);
                    return;
                }
                if state.state == "failed" {
                    remember_preparation(None);
                    error.set(Some(state.message));
                    preparing.set(false);
                    return;
                }
            }
            Err(e) => {
                remember_preparation(None);
                preparing.set(false);
                error.set(Some(format!("恢复出题任务失败：{e}")));
                return;
            }
        }
        TimeoutFuture::new(1500).await;
    }
    preparing.set(false);
    error.set(Some("出题等待超时，可重新打开页面检查任务进度".into()));
}

// RK3568 is authoritative for deadlines: Safari and server clocks can differ.
fn remaining_with_clock_offset(deadline_ms: i64, browser_now_ms: f64, offset_ms: f64) -> u64 {
    ((deadline_ms as f64 - browser_now_ms - offset_ms) / 1000.0)
        .ceil()
        .max(0.0) as u64
}
fn clock_offset_for_server(server_now_ms: Option<i64>) -> f64 {
    server_now_ms
        .map(|server| server as f64 - Date::now())
        .unwrap_or(0.0)
}
#[function_component(ComprehensiveTraining)]
pub(crate) fn comprehensive_training(props: &ComprehensiveProps) -> Html {
    let status = use_state(|| None::<AnalysisStatus>);
    let progress = use_state(|| None::<PrepareProgress>);
    let run = use_state(|| None::<AnalysisRun>);
    let result = use_state(|| None::<AnalysisResult>);
    let current = use_state(|| 0usize);
    let busy = use_state(|| false);
    let preparing = use_state(|| false);
    let ready_id = use_state(|| None::<String>);
    let loading_status = use_state(|| true);
    let error = use_state(|| None::<String>);
    let tick = use_state(|| 0_u64);
    let history = use_state(Vec::<ComprehensiveHistoryEntry>::new);
    let history_open = use_state(|| false);
    let user_prompt = use_state(String::new);
    let prompt_settings = use_state(|| None::<UserPromptSettings>);
    let prompt_busy = use_state(|| false);
    let prompt_error = use_state(|| None::<String>);
    let reviewing_history = use_state(|| false);
    let review_run_id = use_state(String::new);
    let unfinished = use_state(Vec::<UnfinishedRun>::new);
    let controller = use_state(|| None::<String>);
    let clock_offset_ms = use_state(|| 0f64);
    let opened_card = use_state(|| None::<String>);
    {
        let selected_id = (*opened_card).clone();
        use_effect_with(selected_id, move |_| {
            hydrate_math();
            || ()
        });
    }

    {
        let status = status.clone();
        let error = error.clone();
        let run = run.clone();
        let history = history.clone();
        let progress = progress.clone();
        let ready_id = ready_id.clone();
        let preparing = preparing.clone();
        let loading_status = loading_status.clone();
        let user_prompt = user_prompt.clone();
        let prompt_settings = prompt_settings.clone();
        let prompt_error = prompt_error.clone();
        let unfinished = unfinished.clone();
        let controller = controller.clone();
        let current = current.clone();
        let clock_offset_ms = clock_offset_ms.clone();
        let csrf = props.csrf.clone();
        use_effect_with((), move |_| {
            let history = history.clone();
            spawn_local(async move {
                match fetch_json_typed::<UserPromptSettings>(
                    &format!("{ANALYSIS_ROOT}/user-prompt"),
                    "读取出题要求",
                )
                .await
                {
                    Ok(saved) => {
                        user_prompt.set(saved.user_prompt.clone());
                        prompt_settings.set(Some(saved));
                    }
                    Err(e) => prompt_error.set(Some(format!("读取出题要求失败：{e}"))),
                }
                let status_result = fetch_json_typed::<AnalysisStatus>(
                    &format!("{ANALYSIS_ROOT}/status"),
                    "综合训练状态",
                )
                .await;
                let server_job = status_result
                    .as_ref()
                    .ok()
                    .and_then(|s| s.preparing_job_id.clone());
                match status_result {
                    Ok(v) => status.set(Some(v)),
                    Err(e) => error.set(Some(format!("{e}；综合出题需要管理员登录"))),
                }
                let storage = web_sys::window().and_then(|w| w.local_storage().ok().flatten());
                let saved_ready = storage
                    .as_ref()
                    .and_then(|s| s.get_item(COMPREHENSIVE_READY_KEY).ok().flatten());
                let saved_job = storage
                    .as_ref()
                    .and_then(|s| s.get_item(COMPREHENSIVE_JOB_KEY).ok().flatten());
                // A ready run is persisted in SQLite: even a backend restart
                // cannot accidentally start the timer or lose this prepared set.
                if let Some(id) = saved_ready {
                    ready_id.set(Some(id));
                } else if let Some(job) = saved_job.or(server_job) {
                    remember_preparation(Some(&job));
                    spawn_local(resume_preparation(
                        job,
                        progress,
                        ready_id,
                        preparing,
                        error.clone(),
                    ));
                }
                loading_status.set(false);
                if let Ok(items) = fetch_json_typed::<Vec<UnfinishedRun>>(
                    &format!("{ANALYSIS_ROOT}/unfinished"),
                    "查找未完成训练",
                )
                .await
                {
                    unfinished.set(items);
                }
                if let Ok(items) = fetch_json_typed::<Vec<ComprehensiveHistoryEntry>>(
                    &format!("{ANALYSIS_ROOT}/history"),
                    "综合训练历史",
                )
                .await
                {
                    history.set(items);
                }
                // Local Safari reload reclaims the same server record. Other
                // devices discover it via /unfinished and request takeover.
                if let Some(id) = web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|s| s.get_item("hyz_comprehensive_active_run").ok().flatten())
                {
                    if let Ok(claim) = post_json_response_typed::<_, ClaimedRun>(
                        &format!("{ANALYSIS_ROOT}/runs/{id}/claim"),
                        &csrf,
                        &EmptyRequest {},
                        "恢复综合训练",
                    )
                    .await
                    {
                        current.set(claim.run.current_question.min(19));
                        clock_offset_ms.set(clock_offset_for_server(claim.run.server_now_ms));
                        controller.set(Some(claim.control_token));
                        run.set(Some(claim.run));
                    } else if let Some(storage) =
                        web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                    {
                        let _ = storage.remove_item("hyz_comprehensive_active_run");
                    }
                }
            });
            || ()
        });
    }
    {
        let tick = tick.clone();
        use_effect_with((), move |_| {
            let handle = Interval::new(1000, move || tick.set(Date::now() as u64));
            move || drop(handle)
        });
    }
    // Moving between questions resets only the right-hand reading position.
    // The material keeps its position for questions in the same group, then
    // resets when the selected group changes. Never scroll the outer page.
    {
        use_effect_with(*current, move |_| {
            if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                if let Ok(Some(question)) =
                    document.query_selector("[data-testid='analysis-question']")
                {
                    question.set_scroll_top(0);
                }
            }
            || ()
        });
    }
    {
        use_effect_with(*current / 5, move |_| {
            if let Some(document) = web_sys::window().and_then(|window| window.document()) {
                if let Ok(Some(material)) =
                    document.query_selector("[data-testid='analysis-material-scroll']")
                {
                    material.set_scroll_top(0);
                }
            }
            || ()
        });
    }
    // Server records the current question, not a browser-local guess.
    // Only the holder of the most recently issued control token may write it.
    {
        let error = error.clone();
        let csrf = props.csrf.clone();
        let id = run.as_ref().as_ref().map(|r| r.id.clone());
        let token = (*controller).clone();
        use_effect_with((id, token, *current), move |(id, token, position)| {
            if let (Some(id), Some(token)) = (id.clone(), token.clone()) {
                let error = error.clone();
                let csrf = csrf.clone();
                let position = *position;
                spawn_local(async move {
                    if let Err(e) = post_json_typed(
                        &format!("{ANALYSIS_ROOT}/runs/{id}/position"),
                        &csrf,
                        &PositionRequest {
                            control_token: token,
                            question: position,
                        },
                        "保存当前题号",
                    )
                    .await
                    {
                        error.set(Some(format!("进度同步失败：{e}")));
                    }
                });
            }
            || ()
        });
    }
    // Timed papers and answer review hide the setup shell navigation.
    {
        let notify = props.on_immersive_change.clone();
        use_effect_with(
            (run.is_some(), result.is_some()),
            move |(running, reviewing)| {
                let fullscreen = *running || *reviewing;
                notify.emit(fullscreen);
                if fullscreen {
                    if let Some(window) = web_sys::window() {
                        window.scroll_to_with_x_and_y(0.0, 0.0);
                    }
                }
                || ()
            },
        );
    }
    let submit = {
        let controller = controller.clone();
        let run = run.clone();
        let result = result.clone();
        let error = error.clone();
        let csrf = props.csrf.clone();
        let busy = busy.clone();
        let history = history.clone();
        let reviewing_history = reviewing_history.clone();
        let review_run_id = review_run_id.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            let Some(active) = (*run).clone() else { return };
            busy.set(true);
            let result = result.clone();
            let run = run.clone();
            let controller = controller.clone();
            let error = error.clone();
            let busy = busy.clone();
            let csrf = csrf.clone();
            let token = (*controller).clone();
            let history = history.clone();
            let reviewing_history = reviewing_history.clone();
            let review_run_id = review_run_id.clone();
            spawn_local(async move {
                let Some(token) = token else {
                    error.set(Some("无法确认当前设备的答题权限，请重新进入训练".into()));
                    busy.set(false);
                    return;
                };
                let endpoint = format!("{ANALYSIS_ROOT}/runs/{}/submit", active.id);
                match post_json_response_typed::<_, AnalysisResult>(
                    &endpoint,
                    &csrf,
                    &ControlRequest {
                        control_token: token,
                    },
                    "提交综合训练",
                )
                .await
                {
                    Ok(out) => {
                        reviewing_history.set(false);
                        review_run_id.set(active.id.clone());
                        controller.set(None);
                        run.set(None);
                        result.set(Some(out));
                        if let Ok(items) = fetch_json_typed::<Vec<ComprehensiveHistoryEntry>>(
                            &format!("{ANALYSIS_ROOT}/history"),
                            "刷新练习历史",
                        )
                        .await
                        {
                            history.set(items);
                        }
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.remove_item("hyz_comprehensive_active_run");
                        }
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        })
    };
    {
        let submit = submit.clone();
        let deadline = run.as_ref().as_ref().and_then(|r| r.deadline_at_ms);
        let ended = result.is_some();
        use_effect_with((deadline, ended, *tick), move |(deadline, ended, _)| {
            if !ended && deadline.is_some_and(|v| Date::now() >= v as f64) {
                submit.emit(());
            }
            || ()
        });
    }
    let edit_user_prompt = {
        let user_prompt = user_prompt.clone();
        Callback::from(move |event: web_sys::InputEvent| {
            use yew::TargetCast;
            let textarea: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
            user_prompt.set(textarea.value());
        })
    };
    let save_user_prompt = {
        let csrf = props.csrf.clone();
        let user_prompt = user_prompt.clone();
        let prompt_busy = prompt_busy.clone();
        let prompt_settings = prompt_settings.clone();
        let prompt_error = prompt_error.clone();
        Callback::from(move |_| {
            if *prompt_busy {
                return;
            }
            prompt_busy.set(true);
            prompt_error.set(None);
            let csrf = csrf.clone();
            let payload = (*user_prompt).clone();
            let user_prompt = user_prompt.clone();
            let prompt_busy = prompt_busy.clone();
            let prompt_settings = prompt_settings.clone();
            let prompt_error = prompt_error.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, UserPromptSettings>(
                    &format!("{ANALYSIS_ROOT}/user-prompt"),
                    &csrf,
                    &UserPromptUpdate {
                        user_prompt: Some(payload),
                    },
                    "保存出题要求",
                )
                .await
                {
                    Ok(saved) => {
                        user_prompt.set(saved.user_prompt.clone());
                        prompt_settings.set(Some(saved));
                    }
                    Err(e) => prompt_error.set(Some(format!("保存失败：{e}"))),
                }
                prompt_busy.set(false);
            });
        })
    };
    let reset_user_prompt = {
        let csrf = props.csrf.clone();
        let user_prompt = user_prompt.clone();
        let prompt_busy = prompt_busy.clone();
        let prompt_settings = prompt_settings.clone();
        let prompt_error = prompt_error.clone();
        Callback::from(move |_| {
            if *prompt_busy {
                return;
            }
            prompt_busy.set(true);
            prompt_error.set(None);
            let csrf = csrf.clone();
            let user_prompt = user_prompt.clone();
            let prompt_busy = prompt_busy.clone();
            let prompt_settings = prompt_settings.clone();
            let prompt_error = prompt_error.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, UserPromptSettings>(
                    &format!("{ANALYSIS_ROOT}/user-prompt"),
                    &csrf,
                    &UserPromptUpdate { user_prompt: None },
                    "恢复默认出题要求",
                )
                .await
                {
                    Ok(saved) => {
                        user_prompt.set(saved.user_prompt.clone());
                        prompt_settings.set(Some(saved));
                    }
                    Err(e) => prompt_error.set(Some(format!("恢复默认失败：{e}"))),
                }
                prompt_busy.set(false);
            });
        })
    };

    let start = {
        let progress = progress.clone();
        let error = error.clone();
        let busy = busy.clone();
        let preparing = preparing.clone();
        let ready_id = ready_id.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *busy || *preparing || ready_id.is_some() {
                return;
            }
            busy.set(true);
            error.set(None);
            progress.set(None);
            let progress = progress.clone();
            let error = error.clone();
            let busy = busy.clone();
            let preparing = preparing.clone();
            let ready_id = ready_id.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, PrepareTask>(
                    &format!("{ANALYSIS_ROOT}/prepare"),
                    &csrf,
                    &ComprehensivePrepareRequest {},
                    "准备综合训练",
                )
                .await
                {
                    Ok(task) => {
                        remember_preparation(Some(&task.job_id));
                        busy.set(false);
                        resume_preparation(task.job_id, progress, ready_id, preparing, error).await;
                    }
                    Err(e) => {
                        busy.set(false);
                        error.set(Some(e.to_string()));
                    }
                }
            });
        })
    };
    // Opening the prepared paper is the *only* action which starts 20 minutes.
    // A background generation result can remain ready for hours without timing.
    let enter_ready = {
        let clock_offset_ms = clock_offset_ms.clone();
        let controller = controller.clone();
        let run = run.clone();
        let current = current.clone();
        let ready_id = ready_id.clone();
        let clock_offset_ms = clock_offset_ms.clone();
        let busy = busy.clone();
        let error = error.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            let Some(id) = (*ready_id).clone() else {
                return;
            };
            busy.set(true);
            let run = run.clone();
            let controller = controller.clone();
            let current = current.clone();
            let clock_offset_ms = clock_offset_ms.clone();
            let ready_id = ready_id.clone();
            let busy = busy.clone();
            let error = error.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, ClaimedRun>(
                    &format!("{ANALYSIS_ROOT}/runs/{id}/claim"),
                    &csrf,
                    &EmptyRequest {},
                    "进入综合训练",
                )
                .await
                {
                    Ok(claim) => {
                        let active = claim.run;
                        current.set(active.current_question.min(19));
                        clock_offset_ms.set(clock_offset_for_server(active.server_now_ms));
                        controller.set(Some(claim.control_token));
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item("hyz_comprehensive_active_run", &active.id);
                        }
                        remember_preparation(None);
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.remove_item(COMPREHENSIVE_READY_KEY);
                        }
                        ready_id.set(None);
                        run.set(Some(active));
                    }
                    Err(e) => error.set(Some(format!("进入训练失败：{e}"))),
                }
                busy.set(false);
            });
        })
    };
    let toggle_pause = {
        let clock_offset_ms = clock_offset_ms.clone();
        let controller = controller.clone();
        let run = run.clone();
        let busy = busy.clone();
        let error = error.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            let Some(active) = (*run).clone() else { return };
            busy.set(true);
            let run = run.clone();
            let busy = busy.clone();
            let error = error.clone();
            let csrf = csrf.clone();
            let token = (*controller).clone();
            let clock_offset_ms = clock_offset_ms.clone();
            spawn_local(async move {
                let Some(token) = token else {
                    error.set(Some("无法确认答题权限，请重新进入训练".into()));
                    busy.set(false);
                    return;
                };
                let endpoint = format!("{ANALYSIS_ROOT}/runs/{}/pause", active.id);
                match post_json_response_typed::<_, AnalysisRun>(
                    &endpoint,
                    &csrf,
                    &ControlRequest {
                        control_token: token,
                    },
                    "暂停或继续综合训练",
                )
                .await
                {
                    Ok(next) => {
                        clock_offset_ms.set(clock_offset_for_server(next.server_now_ms));
                        run.set(Some(next));
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        })
    };
    // A different browser sees the same server-held unfinished record and
    // explicitly claims its edit lease. A takeover invalidates the old lease.
    let continue_unfinished = {
        let clock_offset_ms = clock_offset_ms.clone();
        let csrf = props.csrf.clone();
        let run = run.clone();
        let controller = controller.clone();
        let current = current.clone();
        let ready_id = ready_id.clone();
        let error = error.clone();
        let busy = busy.clone();
        Callback::from(move |id: String| {
            if *busy {
                return;
            }
            busy.set(true);
            error.set(None);
            let csrf = csrf.clone();
            let run = run.clone();
            let controller = controller.clone();
            let current = current.clone();
            let clock_offset_ms = clock_offset_ms.clone();
            let ready_id = ready_id.clone();
            let error = error.clone();
            let busy = busy.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, ClaimedRun>(
                    &format!("{ANALYSIS_ROOT}/runs/{id}/claim"),
                    &csrf,
                    &EmptyRequest {},
                    "接管未完成训练",
                )
                .await
                {
                    Ok(claim) => {
                        current.set(claim.run.current_question.min(19));
                        clock_offset_ms.set(clock_offset_for_server(claim.run.server_now_ms));
                        controller.set(Some(claim.control_token));
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item("hyz_comprehensive_active_run", &id);
                            let _ = storage.remove_item(COMPREHENSIVE_READY_KEY);
                        }
                        ready_id.set(None);
                        run.set(Some(claim.run));
                    }
                    Err(e) => error.set(Some(format!("恢复训练失败：{e}"))),
                }
                busy.set(false);
            });
        })
    };
    let pause_and_exit = {
        let run = run.clone();
        let controller = controller.clone();
        let busy = busy.clone();
        let error = error.clone();
        let on_exit = props.on_exit.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            let Some(active) = (*run).clone() else {
                on_exit.emit(());
                return;
            };
            // Already paused: simply leave, without accidentally resuming.
            if active.paused_remaining_ms.is_some() {
                on_exit.emit(());
                return;
            }
            let Some(token) = (*controller).clone() else {
                error.set(Some("无法确认控制权，暂停失败；请重新进入训练".into()));
                return;
            };
            busy.set(true);
            let busy = busy.clone();
            let error = error.clone();
            let on_exit = on_exit.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, AnalysisRun>(
                    &format!("{ANALYSIS_ROOT}/runs/{}/pause", active.id),
                    &csrf,
                    &ControlRequest {
                        control_token: token,
                    },
                    "暂停并退出",
                )
                .await
                {
                    Ok(_) => on_exit.emit(()),
                    Err(e) => error.set(Some(format!("暂停失败，答题仍在计时：{e}"))),
                }
                busy.set(false);
            });
        })
    };
    // The same 60:40 question workspace is reused for redo. The server
    // always clones the original groups into a separate ready run.
    let retry_paper = {
        let clock_offset_ms = clock_offset_ms.clone();
        let controller = controller.clone();
        let csrf = props.csrf.clone();
        let busy = busy.clone();
        let error = error.clone();
        let run = run.clone();
        let result = result.clone();
        let history_open = history_open.clone();
        let current = current.clone();
        let ready_id = ready_id.clone();
        let review_run_id = review_run_id.clone();
        let reviewing_history = reviewing_history.clone();
        Callback::from(move |source_id: String| {
            if *busy {
                return;
            }
            busy.set(true);
            error.set(None);
            let csrf = csrf.clone();
            let busy = busy.clone();
            let error = error.clone();
            let run = run.clone();
            let controller = controller.clone();
            let clock_offset_ms = clock_offset_ms.clone();
            let result = result.clone();
            let history_open = history_open.clone();
            let current = current.clone();
            let ready_id = ready_id.clone();
            let review_run_id = review_run_id.clone();
            let reviewing_history = reviewing_history.clone();
            spawn_local(async move {
                let endpoint = format!("{ANALYSIS_ROOT}/runs/{source_id}/retry");
                match post_json_response_typed::<_, RetryRun>(
                    &endpoint,
                    &csrf,
                    &EmptyRequest {},
                    "重做历史试卷",
                )
                .await
                {
                    Ok(created) => {
                        let new_id = created.run_id;
                        // Keep the ready record recoverable if entering it fails.
                        ready_id.set(Some(new_id.clone()));
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item(COMPREHENSIVE_READY_KEY, &new_id);
                        }
                        match post_json_response_typed::<_, ClaimedRun>(
                            &format!("{ANALYSIS_ROOT}/runs/{new_id}/claim"),
                            &csrf,
                            &EmptyRequest {},
                            "开始重做",
                        )
                        .await
                        {
                            Ok(claim) => {
                                let active = claim.run;
                                clock_offset_ms.set(clock_offset_for_server(active.server_now_ms));
                                controller.set(Some(claim.control_token));
                                if let Some(storage) =
                                    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                                {
                                    let _ =
                                        storage.set_item("hyz_comprehensive_active_run", &new_id);
                                    let _ = storage.remove_item(COMPREHENSIVE_READY_KEY);
                                }
                                remember_preparation(None);
                                ready_id.set(None);
                                current.set(0);
                                review_run_id.set(new_id);
                                reviewing_history.set(false);
                                history_open.set(false);
                                result.set(None);
                                run.set(Some(active));
                            }
                            Err(e) => error.set(Some(format!("进入重做训练失败：{e}"))),
                        }
                    }
                    Err(e) => error.set(Some(format!("创建重做训练失败：{e}"))),
                }
                busy.set(false);
            });
        })
    };

    if let Some(score) = (*result).clone() {
        if score.materials.len() != 4 || score.questions.len() != 20 {
            return html! {<p class="p-5 text-error" role="alert">{"历史成绩材料或题目数量不正确，无法展示解析。"}</p>};
        }
        let index = (*current).min(19);
        let group_index = index / 5;
        let local_index = index % 5;
        let q = score
            .questions
            .iter()
            .find(|q| q.number == index + 1)
            .unwrap_or(&score.questions[index]);
        let marks = (1..=20)
            .map(|number| {
                score
                    .questions
                    .iter()
                    .find(|q| q.number == number)
                    .and_then(|q| q.chosen.map(|selected| selected == q.correct_index))
            })
            .collect::<Vec<_>>();
        let grade = match q.chosen {
            Some(chosen) if chosen == q.correct_index => "正确",
            Some(_) => "错误",
            None => "未答",
        };
        let review_options = q.options.iter().enumerate().map(|(i, option)| {
            let grade = if i == q.correct_index {
                "correct"
            } else if q.chosen == Some(i) {
                "wrong"
            } else {
                "neutral"
            };
            let style = match grade {
                "correct" => "flex items-center justify-between gap-3 rounded-box border border-success bg-success/10 p-3 font-medium text-success",
                "wrong" => "flex items-center justify-between gap-3 rounded-box border border-error bg-error/10 p-3 font-medium text-error",
                _ => "flex items-center justify-between gap-3 rounded-box border border-base-content/20 p-3",
            };
            let hint = match grade {
                "correct" => "✓ 正确答案",
                "wrong" => "✕ 你的错选",
                _ => "",
            };
            html! {
                <div class={style} data-testid="analysis-review-choice" data-state={grade}>
                    <span>{format!("{}. {}",["A", "B", "C", "D"][i],option)}</span>
                    <span class="shrink-0 text-xs">{hint}</span>
                </div>
            }
        });
        let review_question = html! {
            <div class="space-y-4 p-4" data-testid="analysis-review-content">
                <div class="flex items-center justify-between gap-2 text-xs text-base-content/60">
                    <span>{format!("本材料第 {}/5 题 · {}",local_index+1,q.skill)}</span>
                    <span data-testid="analysis-review-status">{grade}</span>
                </div>
                <h2 class="text-lg font-semibold leading-relaxed">{q.question.clone()}</h2>
                <div class="grid gap-2">{for review_options}</div>
                <div class="border-t border-base-content/10 pt-4">
                    <p class="text-sm font-semibold">{format!("你的答案：{}",
                        q.chosen.and_then(|i| ["A","B","C","D"].get(i).copied()).unwrap_or("未答"))}</p>
                    <h3 class="mt-3 text-sm font-semibold">{"解题解析"}</h3>
                    <p class="mt-2 whitespace-pre-wrap text-sm leading-7" data-testid="analysis-explanation">{q.explanation.clone()}</p>
                    <RelatedMethodCard key={format!("analysis-{}-{index}", score.id)}
                        card={q.related_card.clone()} source_file={q.card_path.clone().unwrap_or_default()}
                        test_id="analysis-related-card" />
                    <details class="mt-3 text-xs">
                        <summary class="cursor-pointer">{"查看原始数据证据"}</summary>
                        <p class="mt-2 whitespace-pre-wrap leading-6">{q.evidence.clone()}</p>
                        {if let Some(path) = q.card_path.as_ref() {
                            html! {
                                <p class="mt-2 break-all leading-6">
                                    {"Flashcard 来源："}{path.clone()}
                                    {q.card_id.as_ref().map(|id| format!("（{id}）")).unwrap_or_default()}
                                </p>
                            }
                        } else { html! {} }}
                    </details>
                </div>
            </div>
        };
        let review_id = (*review_run_id).clone();
        let show_history = {
            let result = result.clone();
            let run = run.clone();
            let history_open = history_open.clone();
            Callback::from(move |_| {
                result.set(None);
                run.set(None);
                history_open.set(true);
            })
        };
        let back_from_review = {
            let result = result.clone();
            let run = run.clone();
            let history_open = history_open.clone();
            let on_exit = props.on_exit.clone();
            let reviewing_history = reviewing_history.clone();
            Callback::from(move |_| {
                if *reviewing_history {
                    result.set(None);
                    run.set(None);
                    history_open.set(true);
                } else {
                    on_exit.emit(());
                }
            })
        };
        let redo_from_result = {
            let retry_paper = retry_paper.clone();
            let source_id = score.id.clone();
            Callback::from(move |_| retry_paper.emit(source_id.clone()))
        };
        let again = {
            let result = result.clone();
            let run = run.clone();
            let progress = progress.clone();
            Callback::from(move |_| {
                result.set(None);
                run.set(None);
                progress.set(None);
            })
        };
        return html! {
            <section class="analysis-fullscreen fixed inset-0 z-[70] flex h-dvh w-screen min-w-0 flex-col overflow-hidden bg-base-200"
                data-testid="analysis-result" data-view="review">
                <header class="analysis-header flex shrink-0 items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                    <div class="flex min-w-0 items-center gap-2">
                        <button type="button" class="btn btn-ghost btn-sm" data-testid="analysis-exit"
                            onclick={back_from_review}>{if *reviewing_history {"← 返回记录"} else {"← 返回专项"}}</button>
                        <strong class="truncate text-sm">{"综合训练 · 答案解析"}</strong>
                        <span class="hidden text-xs text-base-content/60 sm:inline">{format!("第 {}/20 题 · 材料 {}/4",index+1,group_index+1)}</span>
                    </div>
                    <div class="flex shrink-0 items-center gap-2">
                        <span class="text-sm font-bold">{format!("{} / {} 正确",score.correct,score.total)}</span>
                        <button type="button" class="btn btn-outline btn-sm" onclick={show_history}>{"查看练习历史"}</button>
                        <button type="button" class="btn btn-outline btn-sm" data-testid="analysis-retry-result" disabled={*busy} onclick={redo_from_result}>{"重做原题"}</button>
                        <button type="button" class="btn btn-primary btn-sm" onclick={again}>{"再出一套"}</button>
                    </div>
                </header>
                <div class="min-h-0 flex-1">
                    <InkOverlay question_index={index} persist_key={format!("{}-workspace-{}",review_id,index)} responsive={true} scroll_linked={true}>
                        <AnalysisPanes material={score.materials[group_index].clone()} question={review_question} question_index={index} right_title="答案与解析" />
                    </InkOverlay>
                </div>
                if let Some(msg)=(*error).clone() {
                    <p class="shrink-0 text-error text-xs" role="alert">{msg}</p>
                }
                {analysis_navigation(current.clone(),index,&marks,true)}
            </section>
        };
    }
    if *history_open {
        let back = {
            let history_open = history_open.clone();
            Callback::from(move |_| history_open.set(false))
        };
        let open_result = {
            let result = result.clone();
            let error = error.clone();
            let history_open = history_open.clone();
            let run = run.clone();
            let current = current.clone();
            let reviewing_history = reviewing_history.clone();
            let review_run_id = review_run_id.clone();
            Callback::from(move |id: String| {
                let result = result.clone();
                let error = error.clone();
                let history_open = history_open.clone();
                let run = run.clone();
                let current = current.clone();
                let reviewing_history = reviewing_history.clone();
                let review_run_id = review_run_id.clone();
                spawn_local(async move {
                    match fetch_json_typed::<AnalysisResult>(
                        &format!("{ANALYSIS_ROOT}/runs/{id}/result"),
                        "历史成绩详情",
                    )
                    .await
                    {
                        Ok(value) => {
                            run.set(None);
                            history_open.set(false);
                            current.set(0);
                            reviewing_history.set(true);
                            review_run_id.set(id.clone());
                            result.set(Some(value));
                        }
                        Err(e) => error.set(Some(e.to_string())),
                    }
                });
            })
        };
        return html! {
            <TrainingSetupLayout test_id="analysis-history" heading={Some(html!{                <div class="flex items-center justify-between">
                    <h2 class="text-xl font-bold">{"综合训练历史"}</h2>
                    <button class="btn btn-outline btn-sm" onclick={back}>{"返回训练"}</button>
                </div>})}>
                <div class="space-y-4">
                {if history.is_empty(){html!{<p class="text-sm text-base-content/60">{"还没有已交卷的综合训练。"}</p>}}else{Html::default()}}
                {for history.iter().map(|entry|{
                    let id=entry.id.clone();let open_result=open_result.clone();
                    let redo_id=entry.id.clone();let redo=retry_paper.clone();
                    let stamp=Date::new(&JsValue::from_f64(entry.started_at_ms as f64));
                    let date=format!("{}-{:02}-{:02} {:02}:{:02}",
                        stamp.get_full_year(),stamp.get_month()+1,stamp.get_date(),stamp.get_hours(),stamp.get_minutes());
                    html!{
                        <article class="rounded-box bg-base-100 p-4" key={entry.id.clone()}>
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <div><strong>{format!("{} · {}/{}题",date,entry.correct,entry.total)}</strong>
                                if entry.attempt_no > 1 {
                                    <span class="ml-2 badge badge-outline">{format!("第{}次作答",entry.attempt_no)}</span>
                                }
                                <p class="text-xs text-base-content/60">{format!("作答用时 {}分{}秒 · {}",entry.elapsed_ms/60000,(entry.elapsed_ms/1000)%60,entry.topics.join("、"))}</p></div>
                                <div class="flex flex-wrap gap-2">
                                    <button class="btn btn-outline btn-sm" onclick={Callback::from(move |_|open_result.emit(id.clone()))}>{"查看原题与解析"}</button>
                                    <button class="btn btn-primary btn-sm" data-testid="analysis-retry" disabled={*busy} onclick={Callback::from(move |_|redo.emit(redo_id.clone()))}>{"重做原题"}</button>
                                </div>
                            </div>
                        </article>
                    }
                })}
                {if let Some(msg)=(*error).clone(){html!{<p class="text-error text-sm">{msg}</p>}}else{Html::default()}}
                </div>
            </TrainingSetupLayout>
        };
    }
    if let Some(active) = (*run).clone() {
        if active.groups.len() != 4 || active.answers.len() != 20 {
            return html! {<p class="p-5 text-error">{"服务端返回的综合训练题量无效，请重新出题。"}</p>};
        }
        if active.paused_remaining_ms.is_some() {
            return html! {
                <section class="analysis-fullscreen analysis-paused fixed inset-0 z-[70] flex h-dvh w-screen items-center justify-center overflow-hidden bg-base-200" data-testid="analysis-paused">
                    <div class="absolute right-4 top-2"><StudyTimeInline /></div>
                    <div class="rounded-box bg-base-100 p-8 text-center">
                        <h2 class="text-2xl font-bold">{"训练已暂停"}</h2>
                        <p class="mt-3 text-base-content/60">{"已隐藏材料和题目，暂停期间不能答题。刷新页面仍保持暂停。"}</p>
                        <p class="my-5 font-mono text-3xl">{format!("{:02}:{:02}",active.paused_remaining_ms.unwrap_or(0)/60000,(active.paused_remaining_ms.unwrap_or(0)/1000)%60)}</p>
                        <div class="flex justify-center gap-3">
                            <button type="button" class="btn btn-outline" data-testid="analysis-exit" onclick={pause_and_exit.clone()}>{"← 退出训练"}</button>
                            <button class="btn btn-primary" disabled={*busy} onclick={toggle_pause}>{"继续计时"}</button>
                        </div>
                        {if let Some(msg)=(*error).clone(){html!{<p class="mt-4 text-error text-sm">{msg}</p>}}else{Html::default()}}
                    </div>
                </section>
            };
        }
        let index = (*current).min(19);
        let group_index = index / 5;
        let local_index = index % 5;
        let group = &active.groups[group_index];
        let q = &group.questions[local_index];
        let remaining = active
            .deadline_at_ms
            .map(|ms| remaining_with_clock_offset(ms, Date::now(), *clock_offset_ms))
            .unwrap_or(1200);
        let clock = format!("{:02}:{:02}", remaining / 60, remaining % 60);
        let answers=(0..4).map(|i|{
            let id=active.id.clone();let run=run.clone();let csrf=props.csrf.clone();
            let controller=controller.clone();
            let error=error.clone();let selected=active.answers[index]==Some(i);
            html!{
                <button type="button" class={if selected {"btn btn-primary justify-start min-h-14 h-auto text-left"}else{"btn btn-outline justify-start min-h-14 h-auto text-left"}}
                    data-testid="analysis-choice" disabled={remaining==0} onclick={Callback::from(move |_|{
                        let run=run.clone();let csrf=csrf.clone();let id=id.clone();let error=error.clone();
                        let token=(*controller).clone();
                        spawn_local(async move {
                            let Some(token)=token else {
                                error.set(Some("请重新接管训练后再作答".into()));
                                return;
                            };
                            let endpoint=format!("{ANALYSIS_ROOT}/runs/{id}/answers/{}",index+1);
                            match post_json_typed(&endpoint,&csrf,&SaveAnswer{choice:i,control_token:token},"保存答案").await{
                                Ok(_)=>{
                                    if let Some(mut copy)=(*run).clone(){
                                        if copy.id==id {copy.answers[index]=Some(i);run.set(Some(copy));}
                                    }
                                }
                                Err(e)=>error.set(Some(e.to_string())),
                            }
                        });
                    })}>
                    {format!("{}. {}",["A","B","C","D"][i],q.options[i])}
                </button>
            }
        });

        let question_content = html! {
            <div class="space-y-4 p-4">
                <div class="flex items-center justify-between text-xs text-base-content/60">
                    <span>{format!("本材料第 {}/5 题",local_index+1)}</span>
                    <span>{"作答中"}</span>
                </div>
                <h2 class="text-lg font-semibold leading-relaxed">{q.question.clone()}</h2>
                <div class="grid gap-3">{for answers}</div>
            </div>
        };
        let markers = active
            .answers
            .iter()
            .map(|answer| answer.map(|_| true))
            .collect::<Vec<_>>();
        return html! {
            <section class="analysis-fullscreen fixed inset-0 z-[70] flex h-dvh w-screen min-w-0 flex-col overflow-hidden bg-base-200"
                data-testid="analysis-run" data-view="answer">
                <header class="analysis-header flex shrink-0 items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                    <div class="flex min-w-0 items-center gap-2">
                        <button type="button" class="btn btn-ghost btn-sm" data-testid="analysis-exit"
                            onclick={pause_and_exit.clone()}>{"← 暂停并退出"}</button>
                        <strong class="truncate text-sm">{"资料分析综合训练"}</strong>
                        <span class="hidden text-xs text-base-content/60 sm:inline">{format!("第 {}/20 题 · 材料 {}/4",index+1,group_index+1)}</span>
                    </div>
                    <div class="flex shrink-0 items-center gap-2">
                        <StudyTimeInline />
                        <button type="button" class={if remaining<=60 {"btn btn-ghost font-mono text-xl text-error"}else{"btn btn-ghost font-mono text-xl"}}
                            data-testid="analysis-clock" title="点击暂停计时" disabled={*busy || remaining==0}
                            onclick={toggle_pause}>{clock}</button>
                        <button type="button" class="btn btn-sm btn-error btn-outline" disabled={*busy}
                            onclick={{let submit=submit.clone();Callback::from(move |_|submit.emit(()))}}>{"交卷"}</button>
                    </div>
                </header>
                <div class="min-h-0 flex-1">
                    <InkOverlay question_index={index} persist_key={format!("{}-workspace-{}",active.id,index)} responsive={true} scroll_linked={true}>
                        <AnalysisPanes material={group.material.clone()} question={question_content} question_index={index} />
                    </InkOverlay>
                </div>
                if let Some(msg)=(*error).clone(){<div class="shrink-0 text-error text-xs" role="alert">{msg}</div>}
                {analysis_navigation(current.clone(),index,&markers,false)}
            </section>
        };
    }
    html! {
        <TrainingSetupLayout test_id="analysis-setup" summary="20题 · 20分钟" detail={if ready_id.is_some() {"试卷已准备好，倒计时尚未开始"} else {"4份材料，每份5题"}}
            action={Some(if ready_id.is_some() {html!{<button type="button" class={TRAINING_PRIMARY_BUTTON} data-testid="analysis-enter" disabled={*busy} onclick={enter_ready}>{if *busy {"进入中…"} else {"进入训练 →"}}</button>}} else {html!{
                <button type="button" class={TRAINING_PRIMARY_BUTTON} data-testid="analysis-start" disabled={*loading_status || *busy || *preparing || status.as_ref().as_ref().is_none_or(|s|!s.configured) || props.csrf.is_empty()} onclick={start}>
                    {if *busy || *preparing {"正在出题中…"} else {"准备试卷 →"}}</button>}})}>
            <div class="training-config">
                <h2 class="font-bold">{"资料分析综合训练"}</h2>
                <p class="mt-1 text-sm text-base-content/60">{"参考历年行测真题组织模拟统计数据 · 4份材料 × 每份5题 · 20分钟"}</p>
                <div class="mt-5 grid gap-3">
                    if !unfinished.is_empty() {
                        <div class="space-y-3 rounded-box border border-primary/30 bg-primary/5 p-4" data-testid="analysis-unfinished">
                            <h3 class="font-semibold">{"未完成的综合训练 · 多设备共享"}</h3>
                            <p class="text-xs text-base-content/65">{"以下进度保存在 RK3568。手机或电脑点击继续将接管答题权限，原设备不能再提交答案。"}</p>
                            {for unfinished.iter().map(|item| {
                                let id = item.id.clone();
                                let resume = continue_unfinished.clone();
                                let state = if item.status == "ready" { "尚未开始" }
                                    else if item.paused_remaining_ms.is_some() { "已暂停" }
                                    else if item.deadline_at_ms.is_some_and(|deadline| deadline as f64 <= Date::now()) { "时间已到" }
                                    else { "进行中" };
                                html! {
                                    <div class="flex flex-wrap items-center justify-between gap-2 rounded-box bg-base-100 p-3" key={item.id.clone()}>
                                        <div>
                                            <div class="font-semibold text-sm">{format!("{} · 已作答 {}/20 题",state,item.answered)}</div>
                                            <p class="text-xs text-base-content/60">{format!("上次位于第{}题",item.current_question+1)}</p>
                                        </div>
                                        <button type="button" class="btn btn-primary btn-sm" data-testid="analysis-resume"
                                            disabled={*busy}
                                            onclick={Callback::from(move |_|resume.emit(id.clone()))}>
                                            {if item.status == "ready" { "开始训练" } else { "继续 / 接管训练" }}
                                        </button>
                                    </div>
                                }
                            })}
                        </div>
                    }
                    <button class="btn btn-outline w-full" type="button" onclick={{let history_open=history_open.clone();Callback::from(move |_|history_open.set(true))}}>{"查看练习历史"}</button>
                    <div class="rounded-box border border-base-content/15 p-4 space-y-2">
                        <p class="font-semibold">{"智能选卡 · 综合训练"}</p>
                        <p class="text-sm text-base-content/60">{"错题与知识点统一入队；按卡片虚拟次数全局选择20张，同篇5个不同题型。不设置题型保底或配额。"}</p>
                    </div>
                    <details class="rounded-box border border-base-content/15 bg-base-100/50 p-3" data-testid="analysis-user-prompt">
                        <summary class="cursor-pointer select-none font-semibold" data-testid="analysis-user-prompt-toggle">
                            {"出题要求"}
                            <span class="ml-2 text-xs font-normal text-base-content/60">
                                {if prompt_settings.as_ref().as_ref().is_some_and(|v| v.customized) {"自定义版本 · 点击编辑"} else {"默认版本 · 点击编辑"}}
                            </span>
                        </summary>
                        <div class="mt-3 space-y-3">
                            <p class="text-xs text-base-content/65">
                                {"决定材料风格、题目难度和出题限制。全卡池来源及同篇不同题型的结构由系统控制，不受本栏内容影响。"}
                            </p>
                            <textarea class="textarea textarea-bordered w-full min-h-48 font-mono text-base leading-6"
                                data-testid="analysis-user-prompt-editor" maxlength="8000"
                                aria-label="用户出题要求" value={(*user_prompt).clone()}
                                oninput={edit_user_prompt} disabled={prompt_settings.is_none() || *prompt_busy}>
                            </textarea>
                            if let Some(message) = (*prompt_error).clone() {
                                <p role="alert" class="text-error text-sm">{message}</p>
                            }
                            <div class="flex flex-wrap justify-end gap-2">
                                <button type="button" class="btn btn-outline btn-sm" data-testid="analysis-user-prompt-reset"
                                    disabled={*prompt_busy || prompt_settings.is_none() || props.csrf.is_empty()}
                                    onclick={reset_user_prompt}>{"恢复默认"}</button>
                                <button type="button" class="btn btn-primary btn-sm" data-testid="analysis-user-prompt-save"
                                    disabled={*prompt_busy || prompt_settings.is_none() || user_prompt.trim().is_empty() || props.csrf.is_empty()}
                                    onclick={save_user_prompt}>{if *prompt_busy {"保存中…"} else {"保存出题要求"}}</button>
                            </div>
                            <p class="text-xs text-base-content/60">{"保存至 RK3568，可跨设备使用；正在生成的试卷继续采用启动时的版本。"}</p>
                        </div>
                    </details>
                    if let Some(info)=(*status).clone(){
                        <TrainingMetrics answered={info.answer_summary.answered_count} correct={info.answer_summary.correct_count} prefix="analysis" />
                        <details class="min-w-0 max-w-full overflow-hidden rounded-box border border-base-content/15 p-3" data-testid="analysis-all-cards">
                            <summary class="cursor-pointer font-semibold">{format!("全部卡片（{}）",info.card_stats.len())}</summary>
                            <div class="mt-3 max-h-72 w-full min-w-0 max-w-full space-y-2 overflow-x-hidden overflow-y-auto overscroll-contain [contain:paint] [-webkit-overflow-scrolling:touch]" data-testid="analysis-all-card-list">
                                if info.card_stats.is_empty() {
                                    <p class="text-sm text-base-content/60">{"尚无已建立调度记录的卡片"}</p>
                                }
                                {for info.card_stats.iter().enumerate().map(|(index, card)|{
                                    let card_id = card.card_id.clone();
                                    let opened_card = opened_card.clone();
                                    let preview = card.front_markdown.lines().find(|line| !line.trim().is_empty())
                                        .unwrap_or("未命名卡片").trim_start_matches('#').trim().trim_matches('*').trim().to_owned();
                                    let accuracy = if card.answered_count == 0 {
                                        "未作答".to_owned()
                                    } else {
                                        format!("{:.1}%", 100.0 * card.correct_count as f64 / card.answered_count as f64)
                                    };
                                    html!{
                                        <button type="button" key={card.card_id.clone()}
                                            data-testid="analysis-all-card-item"
                                            class="flex w-full min-w-0 max-w-full items-center gap-2 overflow-hidden rounded-box bg-base-200 p-3 text-left hover:bg-base-300 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
                                            onclick={Callback::from(move |_|opened_card.set(Some(card_id.clone())))}>
                                            <span class="w-5 shrink-0 text-xs text-base-content/50">{format!("{}",index+1)}</span>
                                            <span class="min-w-0 flex-1 overflow-hidden">
                                                <span class="block truncate text-sm font-semibold">{preview}</span>
                                                <span class="block truncate text-xs text-base-content/60">
                                                    {format!("{} · 作答 {} 次",card.category,card.answered_count)}
                                                </span>
                                            </span>
                                            <span class="shrink-0 text-sm font-semibold">{accuracy}</span>
                                            <span aria-hidden="true">{"›"}</span>
                                        </button>
                                    }
                                })}
                            </div>
                            <p class="mt-2 text-xs text-base-content/60">{"按卡片虚拟调度次数升序 · 可在列表内滑动 · 历史重做计入作答统计"}</p>
                        </details>
                        <p class="text-sm text-base-content/65">{format!("出题引擎：{} · 每次主动出题均生成新材料和20道新题",
                            info.model.as_deref().unwrap_or("未配置"))}</p>
                    }
                    if let Some(card) = (*status).as_ref().and_then(|info| {
                        (*opened_card).as_deref().and_then(|id| info.card_stats.iter().find(|card| card.card_id==id))
                    }).cloned() {
                        <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60 p-3 sm:p-6"
                            data-testid="analysis-card-detail" role="dialog" aria-modal="true" aria-label="Flashcard 卡片详情">
                            <div class="flex max-h-[90dvh] w-full max-w-2xl flex-col overflow-hidden rounded-box bg-base-100 shadow-2xl">
                                <div class="flex shrink-0 items-center justify-between gap-3 border-b border-base-content/10 p-3">
                                    <p class="font-semibold">{format!("卡片详情 · {}",card.category)}</p>
                                    <button type="button" class="btn btn-outline btn-sm" data-testid="analysis-card-back"
                                        onclick={{let opened_card=opened_card.clone();Callback::from(move |_|opened_card.set(None))}}>
                                        {"← 返回"}
                                    </button>
                                </div>
                                <div class="min-h-0 space-y-4 overflow-y-auto p-4">
                                    <p class="text-sm text-base-content/65">
                                        {if card.answered_count == 0 {
                                            "累计作答 0 次 · 未作答".to_owned()
                                        } else {
                                            format!("累计作答 {} 次 · 正确率 {:.1}%", card.answered_count,
                                                100.0 * card.correct_count as f64 / card.answered_count as f64)
                                        }}
                                    </p>
                                    <section class="rounded-box border border-base-content/15 p-3" data-testid="analysis-card-front">
                                        <p class="mb-2 text-xs font-semibold text-base-content/60">{"Front"}</p>
                                        <div class="study-markdown">{render_markdown(&card.front_markdown,&card.source_file)}</div>
                                    </section>
                                    <section class="rounded-box border border-base-content/15 p-3" data-testid="analysis-card-back-content">
                                        <p class="mb-2 text-xs font-semibold text-base-content/60">{"Back"}</p>
                                        <div class="study-markdown">{render_markdown(&card.back_markdown,&card.source_file)}</div>
                                    </section>
                                </div>
                            </div>
                        </div>
                    }
                    if let Some(p)=(*progress).clone(){
                        <div class="rounded-box bg-base-200 p-3 text-sm" data-testid="analysis-progress">
                            <div class="font-semibold">{format!("{} · 已生成 {}/20",p.state,p.verified)}</div>
                            <p>{p.message}</p>
                        </div>
                    }
                    if let Some(err)=(*error).clone(){<p class="text-error text-sm" role="alert">{err}</p>}
                    if ready_id.is_some() {
                        <div class="rounded-box border border-success/40 bg-success/5 p-4" data-testid="analysis-ready">
                            <p class="font-semibold">{"20题已经准备好"}</p>
                            <p class="mt-1 text-sm text-base-content/70">{"倒计时尚未开始。点击进入训练时才启动20分钟计时。"}</p>
                        </div>
                    }
                    <p class="text-xs text-base-content/60">
                        {"在 RK3568 的 /userdata/hyz-things/.env 配置 Codex WebSocket Token。电脑 192.168.8.202 需要运行已登录的 Codex App Server，未通过校验的题不会进入正式训练。"}
                    </p>
                </div>
            </div>
        </TrainingSetupLayout>
    }
}

#[cfg(test)]
mod clock_skew_tests {
    use super::remaining_with_clock_offset;

    #[test]
    fn resuming_after_pause_does_not_add_minutes_from_device_clock_skew() {
        // Server paused with 2:07 left. iPad runs 4 minutes slow.
        let server_now = 1_000_000_f64;
        let ipad_now = server_now - 240_000.0;
        let server_deadline = server_now as i64 + 127_000;
        let clock_offset = server_now - ipad_now;
        assert_eq!(
            remaining_with_clock_offset(server_deadline, ipad_now, clock_offset),
            127
        );
        // One second of elapsed time must cost one second on this device.
        assert_eq!(
            remaining_with_clock_offset(server_deadline, ipad_now + 1_000.0, clock_offset),
            126
        );
        // Without compensation the same clock would incorrectly add four minutes.
        assert_eq!(
            remaining_with_clock_offset(server_deadline, ipad_now, 0.0),
            367
        );
    }

    #[test]
    fn fast_phone_clock_also_uses_server_remaining_time() {
        let server_now = 10_000_000_f64;
        let phone_now = server_now + 180_000.0;
        let deadline = server_now as i64 + 127_000;
        assert_eq!(
            remaining_with_clock_offset(deadline, phone_now, server_now - phone_now),
            127
        );
    }
}
