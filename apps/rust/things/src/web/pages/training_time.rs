//! One shared effective-study clock, independent of paper countdowns.
use super::*;
use gloo_timers::callback::Interval;
use js_sys::{Date, Math};
use std::collections::VecDeque;

#[derive(Clone, PartialEq)]
pub(super) struct StudyClockValue {
    snapshot: TrainingTimeSnapshot,
    received_ms: f64,
}
#[derive(Clone, PartialEq)]
pub(super) struct StudyClockContext {
    value: UseStateHandle<Option<StudyClockValue>>,
    pub(super) event: Callback<(String, Option<String>, Option<String>)>,
    tick: u64,
    csrf: String,
    pub(super) goals_revision: u64,
    pub(super) refresh_goals: Callback<()>,
}
#[derive(Properties, PartialEq)]
pub(super) struct StudyClockProviderProps {
    pub csrf: String,
    pub children: Children,
}

#[function_component(StudyClockProvider)]
pub(super) fn study_clock_provider(props: &StudyClockProviderProps) -> Html {
    let value = use_state(|| None::<StudyClockValue>);
    let tick = use_state(|| 0_u64);
    let warning = use_state(|| None::<String>);
    let goals_revision = use_state(|| 0_u64);
    let sequence = use_mut_ref(|| 0_u64);
    let queue = use_mut_ref(VecDeque::<TrainingTimeEventRequest>::new);
    let processing = use_mut_ref(|| false);
    let serial = use_mut_ref(|| 0_u64);
    // The panel bootstrap supplies the CSRF after the first render; keep the
    // latest token in a stable cell so delayed callbacks and heartbeats never
    // send a stale or empty value.
    let csrf = use_mut_ref(|| props.csrf.clone());
    if *csrf.borrow() != props.csrf {
        *csrf.borrow_mut() = props.csrf.clone();
    }
    let event = {
        let value = value.clone();
        let warning = warning.clone();
        let sequence = sequence.clone();
        let queue = queue.clone();
        let processing = processing.clone();
        let serial = serial.clone();
        let csrf = csrf.clone();
        Callback::from(
            move |(action, module, phase): (String, Option<String>, Option<String>)| {
                if csrf.borrow().is_empty() {
                    return;
                }
                let id = {
                    let mut sequence = sequence.borrow_mut();
                    *sequence += 1;
                    format!(
                        "{}-{}-{}",
                        Date::now() as i64,
                        (Math::random() * 1_000_000_000.0) as u64,
                        *sequence
                    )
                };
                let request = TrainingTimeEventRequest {
                    event_id: id,
                    action,
                    module,
                    phase,
                };
                *serial.borrow_mut() += 1;
                queue.borrow_mut().push_back(request);
                if *processing.borrow() {
                    return;
                }
                *processing.borrow_mut() = true;
                let queue = queue.clone();
                let processing = processing.clone();
                let value = value.clone();
                let warning = warning.clone();
                let csrf = csrf.clone();
                spawn_local(async move {
                    loop {
                        let next = queue.borrow_mut().pop_front();
                        let Some(request) = next else { break };
                        let token = csrf.borrow().clone();
                        match post_json_response_typed::<_, TrainingTimeSnapshot>(
                            &format!("{TRAINING_TIME_ROOT}/events"),
                            &token,
                            &request,
                            "学习计时",
                        )
                        .await
                        {
                            Ok(snapshot) => {
                                value.set(Some(StudyClockValue {
                                    snapshot,
                                    received_ms: Date::now(),
                                }));
                                warning.set(None);
                            }
                            Err(err) => warning.set(Some(format!("学习时间同步失败：{err}"))),
                        }
                    }
                    *processing.borrow_mut() = false;
                });
            },
        )
    };
    {
        let value = value.clone();
        let warning = warning.clone();
        let serial = serial.clone();
        use_effect_with((), move |_| {
            let seen = *serial.borrow();
            spawn_local(async move {
                match fetch_json_typed::<TrainingTimeSnapshot>(
                    &format!("{TRAINING_TIME_ROOT}/current"),
                    "读取学习时间",
                )
                .await
                {
                    Ok(snapshot) => {
                        if *serial.borrow() == seen {
                            value.set(Some(StudyClockValue {
                                snapshot,
                                received_ms: Date::now(),
                            }));
                        }
                    }
                    Err(err) => warning.set(Some(format!("学习计时暂不可用：{err}"))),
                }
            });
            || ()
        });
    }
    {
        let tick = tick.clone();
        use_effect_with((), move |_| {
            let timer = Interval::new(1000, move || tick.set(Date::now() as u64));
            move || drop(timer)
        });
    }
    // Heartbeats only while this page is visible and actively counting.
    // A server-side grace window excludes an abandoned browser tab.
    {
        let event = event.clone();
        let running = (*value).as_ref().is_some_and(|v| v.snapshot.running);
        use_effect_with(running, move |running| {
            let timer = if *running {
                Some(Interval::new(15_000, move || {
                    if web_sys::window()
                        .and_then(|w| w.document())
                        .is_some_and(|d| !d.hidden())
                    {
                        event.emit(("heartbeat".into(), None, None));
                    }
                }))
            } else {
                None
            };
            move || drop(timer)
        });
    }
    let refresh_goals = {
        let goals_revision = goals_revision.clone();
        Callback::from(move |_| goals_revision.set(*goals_revision + 1))
    };
    let tick_value = *tick;
    html! {
        <ContextProvider<StudyClockContext> context={StudyClockContext{value,event,tick:tick_value,csrf:props.csrf.clone(),goals_revision:*goals_revision,refresh_goals}}>
            {for props.children.iter()}
            if let Some(message) = (*warning).clone() {
                <div class="pointer-events-none fixed bottom-2 right-2 z-[99] max-w-xs rounded bg-base-100 p-2 text-xs text-error shadow" role="status" data-testid="study-time-error">{message}</div>
            }
        </ContextProvider<StudyClockContext>>
    }
}

fn paid_today_ms(ctx: &StudyClockContext) -> Option<i64> {
    (*ctx.value).as_ref().map(|value| {
        let delta = if value.snapshot.running {
            ((Date::now() - value.received_ms).max(0.0) as i64).min(45_000)
        } else {
            0
        };
        value.snapshot.today_ms.saturating_add(delta)
    })
}

/// Two compact bars shared by every study/review page header: today's answered
/// questions and today's effective study time against the administrator goal.
#[derive(Properties, PartialEq)]
pub(super) struct StudyGoalBarsProps {
    /// Full-width rows for practice headers that give the bars their own line.
    #[prop_or(false)]
    pub wide: bool,
}

#[function_component(StudyGoalBars)]
pub(super) fn study_goal_bars(props: &StudyGoalBarsProps) -> Html {
    let status = use_state(|| None::<StudyGoalStatus>);
    let clock = use_context::<StudyClockContext>();
    let goals_revision = clock
        .as_ref()
        .map(|context| context.goals_revision)
        .unwrap_or(0);
    {
        let status = status.clone();
        use_effect_with(goals_revision, move |_| {
            let refresh: std::rc::Rc<dyn Fn()> = std::rc::Rc::new({
                let status = status.clone();
                move || {
                    let status = status.clone();
                    spawn_local(async move {
                        if let Ok(value) = fetch_json_typed::<StudyGoalStatus>(
                            &format!("{TRAINING_TIME_ROOT}/goals"),
                            "学习目标",
                        )
                        .await
                        {
                            status.set(Some(value));
                        }
                    });
                }
            });
            refresh();
            let timer_refresh = refresh.clone();
            let timer = Interval::new(60_000, move || timer_refresh());
            move || drop(timer)
        });
    }
    let goals = status.as_ref().map(|s| s.goals.clone()).unwrap_or_default();
    let today_ms = clock
        .as_ref()
        .and_then(|ctx| {
            let _ = ctx.tick;
            paid_today_ms(ctx)
        })
        .or_else(|| status.as_ref().map(|s| s.today_ms))
        .unwrap_or(0);
    let today_questions = status.as_ref().map(|s| s.today_questions).unwrap_or(0);
    let hours = |ms: i64| format!("{:.1}", ms as f64 / 3_600_000.0);
    let open = use_state(|| false);
    let csrf = clock
        .as_ref()
        .map(|ctx| ctx.csrf.clone())
        .unwrap_or_default();
    html! {
        <div class={classes!("grid", "gap-1", if props.wide {"w-full"} else {"w-32 shrink-0 sm:w-44"})}>
            <button type="button" class="grid gap-1 rounded-lg text-left" data-testid="study-goal-bars"
                data-today-ms={today_ms.to_string()} aria-label="查看学习统计"
                onclick={Callback::from({let open=open.clone();move |_|open.set(true)})}>
            <div data-testid="study-goal-questions">
                <div class="flex items-baseline justify-between gap-2 text-[10px] leading-none text-base-content/60">
                    <span class="sm:hidden">{"刷题"}</span><span class="hidden sm:inline">{"今日刷题"}</span>
                    <span class="tabular-nums" data-testid="study-goal-questions-value">{format!("{today_questions}/{}", goals.daily_questions)}</span>
                </div>
                <progress class="progress progress-primary study-goal-bar mt-0.5 block w-full" max={goals.daily_questions.max(1).to_string()}
                    value={today_questions.max(0).to_string()} aria-label="今日刷题进度"/>
            </div>
            <div data-testid="study-goal-time">
                <div class="flex items-baseline justify-between gap-2 text-[10px] leading-none text-base-content/60">
                    <span class="sm:hidden">{"时长"}</span><span class="hidden sm:inline">{"今日时长"}</span>
                    <span class="tabular-nums" data-testid="study-goal-time-value">{format!("{}/{}h", hours(today_ms), hours(goals.daily_study_ms))}</span>
                </div>
                <progress class="progress progress-primary study-goal-bar mt-0.5 block w-full" max={goals.daily_study_ms.max(1).to_string()}
                    value={today_ms.max(0).to_string()} aria-label="今日学习时长进度"/>
            </div>
            </button>
            <div class="sr-only" aria-hidden="true"><StudyTimeInline /></div>
            if *open {
                <div class="fixed inset-0 z-[120] overflow-y-auto bg-base-200/95 p-3 sm:p-6" role="dialog" aria-modal="true" aria-label="学习统计" data-testid="study-goal-sheet">
                    <div class="mx-auto w-full max-w-3xl space-y-3">
                        <div class="flex items-center justify-between gap-3">
                            <h2 class="text-lg font-semibold">{"学习统计"}</h2>
                            <button type="button" class="btn btn-ghost btn-sm" data-testid="study-goal-sheet-close"
                                onclick={Callback::from({let open=open.clone();move |_|open.set(false)})}>{"关闭"}</button>
                        </div>
                        <StudyStatisticsBody csrf={csrf.clone()} embedded={true}/>
                    </div>
                </div>
            }
        </div>
    }
}

#[function_component(StudyTimeInline)]
pub(super) fn study_time_inline() -> Html {
    let ctx = use_context::<StudyClockContext>();
    let Some(ctx) = ctx else {
        return Html::default();
    };
    let _ = ctx.tick;
    let ms = match (*ctx.value).as_ref() {
        Some(v) => {
            let delta = if v.snapshot.running {
                ((Date::now() - v.received_ms).max(0.0) as i64).min(45_000)
            } else {
                0
            };
            v.snapshot.today_ms.saturating_add(delta)
        }
        None => 0,
    };
    let seconds = ms.max(0) / 1_000;
    let clock = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    );
    html! {
        <span class="inline-flex min-h-10 h-10 shrink-0 items-center px-2 font-mono text-base font-semibold tabular-nums whitespace-nowrap text-primary"
            data-testid="study-time-inline" title="今日有效学习时长" aria-label="今日有效学习时长" role="timer">{clock}</span>
    }
}
