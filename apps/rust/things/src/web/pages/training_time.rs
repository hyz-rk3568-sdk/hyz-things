//! One shared effective-study clock, independent of paper countdowns.
use super::*;
use gloo_timers::callback::Interval;
use js_sys::{Date, Math};
use std::rc::Rc;
use std::cell::Cell;

#[derive(Clone, PartialEq)]
pub(super) struct StudyClockValue {
    snapshot: TrainingTimeSnapshot,
    received_ms: f64,
}
#[derive(Clone, PartialEq)]
pub(super) struct StudyClockContext {
    value: UseStateHandle<Option<StudyClockValue>>,
    event: Callback<(String, Option<String>, Option<String>)>,
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
    let event = {
        let value = value.clone();
        let warning = warning.clone();
        let sequence = sequence.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |(action,module,phase):(String,Option<String>,Option<String>)| {
            let id = {
                let mut sequence = sequence.borrow_mut();
                *sequence += 1;
                format!("{}-{}-{}", Date::now() as i64, (Math::random()*1_000_000_000.0) as u64, *sequence)
            };
            let request = TrainingTimeEventRequest{event_id:id,action,module,phase};
            let value=value.clone();
            let warning=warning.clone();
            let csrf=csrf.clone();
            spawn_local(async move {
                match post_json_response_typed::<_,TrainingTimeSnapshot>(
                    &format!("{TRAINING_TIME_ROOT}/events"),&csrf,&request,"学习计时"
                ).await {
                    Ok(snapshot) => {
                        value.set(Some(StudyClockValue{snapshot,received_ms:Date::now()}));
                        warning.set(None);
                    },
                    Err(err)=>warning.set(Some(format!("学习时间同步失败：{err}"))),
                }
            });
        })
    };
    {
        let value=value.clone();
        let warning=warning.clone();
        use_effect_with((),move |_| {
            spawn_local(async move {
                match fetch_json_typed::<TrainingTimeSnapshot>(
                    &format!("{TRAINING_TIME_ROOT}/current"),"读取学习时间"
                ).await {
                    Ok(snapshot)=>value.set(Some(StudyClockValue{snapshot,received_ms:Date::now()})),
                    Err(err)=>warning.set(Some(format!("学习计时暂不可用：{err}"))),
                }
            });
            || ()
        });
    }
    {
        let tick=tick.clone();
        use_effect_with((),move |_| {
            let timer=Interval::new(1000,move || tick.set(*tick+1));
            move || drop(timer)
        });
    }
    // Heartbeats only while this page is visible and actively counting.
    // A server-side grace window excludes an abandoned browser tab.
    {
        let event=event.clone();
        let running=value.as_ref().as_ref().is_some_and(|v|v.snapshot.running);
        use_effect_with(running,move |running| {
            let timer=if *running {
                Some(Interval::new(15_000, move || {
                    if web_sys::window().and_then(|w|w.document()).is_some_and(|d|!d.hidden()) {
                        event.emit(("heartbeat".into(),None,None));
                    }
                }))
            } else {None};
            move || drop(timer)
        });
    }
    let _ = *tick;
    html! {
        <ContextProvider<StudyClockContext> context={StudyClockContext{value,event}}>
            {for props.children.iter()}
            if let Some(message) = (*warning).clone() {
                <div class="fixed bottom-2 right-2 z-[99] max-w-xs rounded bg-base-100 p-2 text-xs text-error shadow" role="status" data-testid="study-time-error">{message}</div>
            }
        </ContextProvider<StudyClockContext>>
    }
}

#[function_component(StudyTimeInline)]
pub(super) fn study_time_inline() -> Html {
    let ctx=use_context::<StudyClockContext>();
    let Some(ctx)=ctx else {return Html::default();};
    let (ms,paused)=match ctx.value.as_ref() {
        Some(v)=> {
            let delta=if v.snapshot.running {
                ((Date::now()-v.received_ms).max(0.0) as i64).min(45_000)
            } else {0};
            (v.snapshot.today_ms.saturating_add(delta),v.snapshot.paused)
        },
        None=>(0,false),
    };
    let seconds=ms.max(0)/1_000;
    let clock=format!("{:02}:{:02}:{:02}",seconds/3600,(seconds/60)%60,seconds%60);
    let onclick={let event=ctx.event.clone();Callback::from(move |_| event.emit(("toggle".into(),None,None)))};
    html! {
        <button type="button" class="btn btn-ghost min-h-10 h-10 shrink-0 px-2 font-mono text-base font-semibold tabular-nums whitespace-nowrap text-primary"
            data-testid="study-time-inline" title="点击独立暂停或继续学习正计时" aria-label={if paused {"学习计时已暂停，点击继续"}else{"今日有效学习时长，点击暂停"}}
            onclick={onclick}>{clock}</button>
    }
}
