use super::*;

fn limit_reset_text(
    window: Option<&hyz_things::domain::question_bank::CodexLimitWindow>,
) -> String {
    let Some(seconds) = window.and_then(|window| window.resets_at) else {
        return "重置时间未知".into();
    };
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(seconds as f64 * 1000.0));
    format!(
        "{}-{:02}-{:02} {:02}:{:02} 重置",
        date.get_full_year(),
        date.get_month() + 1,
        date.get_date(),
        date.get_hours(),
        date.get_minutes()
    )
}

#[function_component(CodexLimitBars)]
pub(super) fn codex_limit_bars() -> Html {
    let limits = use_state(|| None::<hyz_things::domain::question_bank::CodexLimits>);
    let open = use_state(|| false);
    {
        let limits = limits.clone();
        use_effect_with((), move |_| {
            let refresh = std::rc::Rc::new(move || {
                let limits = limits.clone();
                spawn_local(async move {
                    let result =
                        fetch_json_typed::<hyz_things::domain::question_bank::CodexLimits>(
                            "/api/v1/question-bank/ai/rate-limits",
                            "Codex 限额",
                        )
                        .await;
                    limits.set(result.ok());
                });
            });
            refresh();
            let repeat = refresh.clone();
            let timer = gloo_timers::callback::Interval::new(60_000, move || repeat());
            move || drop(timer)
        });
    }
    let bar = |label: &'static str,
               testid: &'static str,
               window: Option<&hyz_things::domain::question_bank::CodexLimitWindow>| {
        let percent = window.map(|item| item.remaining_percent.min(100));
        let width = percent.unwrap_or(0);
        let color = if width <= 20 && percent.is_some() {
            "bg-error"
        } else {
            "bg-primary"
        };
        html! {
            <div data-testid={testid}>
                <div class="flex justify-between gap-2 text-[10px] leading-none text-base-content/60">
                    <span>{label}</span><span class="tabular-nums">{percent.map(|n|format!("{n}%")).unwrap_or_else(||"—".into())}</span>
                </div>
                <div class="mt-1 h-1.5 overflow-hidden rounded-full bg-base-content/10">
                    <div class={classes!("h-full", "rounded-full", color)} style={format!("width:{width}%")}></div>
                </div>
            </div>
        }
    };
    let five = limits.as_ref().and_then(|value| value.five_hour.as_ref());
    let week = limits.as_ref().and_then(|value| value.week.as_ref());
    html! {
        <div class="relative w-28 shrink-0 sm:w-36">
            <button type="button" class="grid w-full gap-1 text-left" aria-label="Codex 限额详情"
                aria-expanded={open.to_string()} onclick={Callback::from({let open=open.clone();move |_|open.set(!*open)})}>
                {bar("5 小时", "codex-limit-five-hour", five)}
                {bar("每周", "codex-limit-week", week)}
            </button>
            if *open {
                <div class="absolute right-0 top-full z-30 mt-1 w-52 rounded-lg bg-base-100 p-3 text-xs shadow-lg ring-1 ring-base-content/10"
                    data-testid="codex-limit-details" role="status">
                    <p>{format!("5 小时：{}", limit_reset_text(five))}</p>
                    <p class="mt-1">{format!("每周：{}", limit_reset_text(week))}</p>
                </div>
            }
        </div>
    }
}

pub(super) fn score_summary(score: &hyz_things::domain::question_bank::PracticeScore) -> Html {
    let count = |label: &str, number: usize, class: &str, path: &str| {
        html! {
            <span class={classes!("inline-flex", "flex-col", "items-center", "gap-0", class.to_owned())}
                aria-label={format!("{label} {number}")} title={format!("{label} {number}")}>
                <svg aria-hidden="true" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d={path.to_owned()}/>
                </svg>
                {number}
            </span>
        }
    };
    html! {
        <span class="flex items-center gap-2 text-sm font-semibold tabular-nums" aria-label="交卷成绩">
            {count("正确", score.correct, "text-success", "M5 12l4 4L19 6")}
            {count("错误", score.wrong, "text-error", "M6 6l12 12M18 6L6 18")}
            {count("不可判分", score.ungradable, "text-base-content/60", "M5 12h14")}

        </span>
    }
}
