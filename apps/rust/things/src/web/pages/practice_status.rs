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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TrainingStatusCounts {
    pub answered: usize,
    pub correct: usize,
    pub wrong: usize,
    pub unanswered: usize,
    pub ungradable: usize,
}

impl TrainingStatusCounts {
    pub(crate) fn active(answered: usize, total: usize) -> Self {
        Self {
            answered: answered.min(total),
            unanswered: total.saturating_sub(answered),
            ..Self::default()
        }
    }

    pub(crate) fn review(
        correct: usize,
        wrong: usize,
        unanswered: usize,
        ungradable: usize,
    ) -> Self {
        Self {
            answered: correct.saturating_add(wrong).saturating_add(ungradable),
            correct,
            wrong,
            unanswered,
            ungradable,
        }
    }

    pub(crate) fn from_practice_score(
        score: &hyz_things::domain::question_bank::PracticeScore,
    ) -> Self {
        Self::review(
            score.correct,
            score.wrong,
            score.unanswered,
            score.ungradable,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrainingStatusMode {
    Active,
    Review,
}

#[derive(Properties, PartialEq)]
pub(crate) struct TrainingStatusBarProps {
    pub prefix: AttrValue,
    pub counts: TrainingStatusCounts,
    pub mode: TrainingStatusMode,
    #[prop_or_default]
    pub test_id: Option<AttrValue>,
}

fn status_metric(
    label: &'static str,
    value: usize,
    class: &'static str,
    path: &'static str,
    test_id: String,
) -> Html {
    html! {
        <span class={classes!("training-status-metric", "inline-flex", "flex-col", "items-center", "gap-0", class)}
            data-testid={test_id}
            aria-label={format!("{label} {value}")}
            title={format!("{label} {value}")}>
            <svg aria-hidden="true" class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d={path}/>
            </svg>
            <span class="tabular-nums">{value}</span>
        </span>
    }
}

#[function_component(TrainingStatusBar)]
pub(crate) fn training_status_bar(props: &TrainingStatusBarProps) -> Html {
    let test_id = props
        .test_id
        .clone()
        .unwrap_or_else(|| format!("{}-status-bar", props.prefix).into());
    let prefix = props.prefix.to_string();
    let active = props.mode == TrainingStatusMode::Active;
    html! {
        <div class="training-status-bar flex shrink-0 items-center gap-2 text-sm font-semibold tabular-nums"
            data-testid={test_id}
            data-mode={if active { "active" } else { "review" }}
            aria-label="训练答题状态">
            if active {
                {status_metric("已答", props.counts.answered, "text-primary", "M5 12h14", format!("{prefix}-answered"))}
                {status_metric("未答", props.counts.unanswered, "text-base-content/60", "M12 5v14", format!("{prefix}-unanswered"))}
            } else {
                {status_metric("正确", props.counts.correct, "text-success", "M5 12l4 4L19 6", format!("{prefix}-correct"))}
                {status_metric("错误", props.counts.wrong, "text-error", "M6 6l12 12M18 6L6 18", format!("{prefix}-wrong"))}
                {status_metric("未答", props.counts.unanswered, "text-base-content/60", "M12 5v14", format!("{prefix}-unanswered"))}
                if props.counts.ungradable > 0 {
                    {status_metric("不可判分", props.counts.ungradable, "text-base-content/60", "M5 12h14", format!("{prefix}-ungradable"))}
                }
            }
        </div>
    }
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
                <div class="training-progress-label flex items-baseline justify-between gap-2 text-base-content/60">
                    <span>{label}</span><span class="tabular-nums">{percent.map(|n|format!("{n}%")).unwrap_or_else(||"—".into())}</span>
                </div>
                <div class="training-progress-track mt-0.5 overflow-hidden rounded-full bg-base-content/10">
                    <div class={classes!("h-full", "rounded-full", color)} style={format!("width:{width}%")}></div>
                </div>
            </div>
        }
    };
    let five = limits.as_ref().and_then(|value| value.five_hour.as_ref());
    let week = limits.as_ref().and_then(|value| value.week.as_ref());
    html! {
        <div class="training-progress-bars relative" data-testid="codex-limit-bars">
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
    html! {
        <TrainingStatusBar prefix="question-bank-practice-score"
            test_id="question-bank-practice-score"
            counts={TrainingStatusCounts::from_practice_score(score)}
            mode={TrainingStatusMode::Review}/>
    }
}

#[cfg(test)]
mod tests {
    use super::TrainingStatusCounts;

    #[test]
    fn active_counts_only_expose_answered_and_unanswered_counts() {
        let counts = TrainingStatusCounts::active(2, 3);

        assert_eq!(counts.answered, 2);
        assert_eq!(counts.unanswered, 1);
        assert_eq!(counts.correct, 0);
        assert_eq!(counts.wrong, 0);
    }

    #[test]
    fn review_counts_expose_colored_grade_counts() {
        let counts = TrainingStatusCounts::review(1, 1, 1, 0);

        assert_eq!(counts.answered, 2);
        assert_eq!(counts.unanswered, 1);
        assert_eq!(counts.correct, 1);
        assert_eq!(counts.wrong, 1);
    }
}
