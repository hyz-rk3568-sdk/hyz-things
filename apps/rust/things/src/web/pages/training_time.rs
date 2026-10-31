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
    let tick_value = *tick;
    html! {
        <ContextProvider<StudyClockContext> context={StudyClockContext{value,event,tick:tick_value}}>
            {for props.children.iter()}
            if let Some(message) = (*warning).clone() {
                <div class="fixed bottom-2 right-2 z-[99] max-w-xs rounded bg-base-100 p-2 text-xs text-error shadow" role="status" data-testid="study-time-error">{message}</div>
            }
        </ContextProvider<StudyClockContext>>
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
