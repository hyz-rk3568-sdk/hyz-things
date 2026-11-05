//! Real stored learning durations, not quiz elapsed-time estimates.
use super::*;
use std::collections::BTreeMap;

fn module_name(id: &str) -> &'static str {
    match id {
        "calculation" => "速算训练",
        "flashcards" => "闪卡回顾",
        "comprehensive" => "资料分析",
        "logic" => "判断推理",
        "quantitative" => "数量关系",
        "question_bank" => "真题训练",
        _ => "其他",
    }
}
fn hours(ms: i64) -> String {
    format!("{:.1} 小时", ms as f64 / 3_600_000.0)
}
fn minutes(ms: i64) -> String {
    format!("{} 分钟", ms / 60_000)
}

#[derive(Properties, PartialEq)]
pub(crate) struct TrainingStatisticsProps {
    pub csrf: String,
}

#[function_component(TrainingStatistics)]
pub(super) fn training_statistics(props: &TrainingStatisticsProps) -> Html {
    html! {
        <TrainingSetupLayout test_id="training-statistics">
            <StudyStatisticsBody csrf={props.csrf.clone()} embedded={false}/>
        </TrainingSetupLayout>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct StudyStatisticsProps {
    pub csrf: String,
    #[prop_or(false)]
    pub embedded: bool,
}

/// Shared statistics content for the 学习统计 tab and the in-page goal sheet.
#[function_component(StudyStatisticsBody)]
pub(crate) fn study_statistics_body(props: &StudyStatisticsProps) -> Html {
    let stats = use_state(|| None::<TrainingTimeStats>);
    let goals = use_state(|| None::<StudyGoals>);
    let error = use_state(|| None::<String>);
    let selected = use_state(|| None::<String>);
    let editing = use_state(|| false);
    let draft_questions = use_state(String::new);
    let draft_hours = use_state(String::new);
    let save_error = use_state(|| None::<String>);
    let saving = use_state(|| false);
    let refresh = use_state(|| 0_u32);
    {
        let stats = stats.clone();
        let goals = goals.clone();
        let error = error.clone();
        use_effect_with(*refresh, move |_| {
            spawn_local(async move {
                match fetch_json_typed::<TrainingTimeStats>(
                    &format!("{TRAINING_TIME_ROOT}/stats"),
                    "学习统计",
                )
                .await
                {
                    Ok(v) => stats.set(Some(v)),
                    Err(e) => error.set(Some(e.to_string())),
                }
                if let Ok(v) = fetch_json_typed::<StudyGoalStatus>(
                    &format!("{TRAINING_TIME_ROOT}/goals"),
                    "学习目标",
                )
                .await
                {
                    goals.set(Some(v.goals));
                }
            });
            || ()
        });
    }
    let Some(data) = stats.as_ref() else {
        return html! {
            if let Some(e)=error.as_ref(){<p role="alert" class="text-error">{e.clone()}</p>}
            else{<p class="opacity-60">{"正在读取真实学习记录…"}</p>}
        };
    };
    let goal = goals.as_ref().cloned().unwrap_or_default();
    let today = data.days.last().map(|d| d.total_ms).unwrap_or_default();
    let week = data
        .days
        .iter()
        .rev()
        .take(7)
        .map(|d| d.total_ms)
        .sum::<i64>();
    let total = data.days.iter().map(|d| d.total_ms).sum::<i64>();
    let today_count = data
        .days
        .last()
        .map(|d| d.answered_count)
        .unwrap_or_default();
    let week_count: i64 = data
        .days
        .iter()
        .rev()
        .take(7)
        .map(|d| d.answered_count)
        .sum();
    let total_count: i64 = data.days.iter().map(|d| d.answered_count).sum();
    let streak = data
        .days
        .iter()
        .rev()
        .skip_while(|d| d.total_ms == 0)
        .take_while(|d| d.total_ms > 0)
        .count();
    let chosen = data
        .days
        .iter()
        .find(|d| Some(&d.date) == selected.as_ref())
        .or_else(|| data.days.last());
    let mut by_module = BTreeMap::<String, i64>::new();
    if let Some(day) = chosen {
        for (k, v) in &day.modules {
            *by_module.entry(k.clone()).or_default() += v;
        }
    }
    let start_edit = {
        let editing = editing.clone();
        let questions = draft_questions.clone();
        let hours = draft_hours.clone();
        let save_error = save_error.clone();
        let goal = goal.clone();
        Callback::from(move |_| {
            questions.set(goal.daily_questions.to_string());
            hours.set(format!("{}", goal.daily_study_ms as f64 / 3_600_000.0));
            save_error.set(None);
            editing.set(true);
        })
    };
    let save = {
        let csrf = props.csrf.clone();
        let editing = editing.clone();
        let saving = saving.clone();
        let save_error = save_error.clone();
        let goals = goals.clone();
        let refresh = refresh.clone();
        let questions = draft_questions.clone();
        let hours = draft_hours.clone();
        Callback::from(move |_| {
            let Ok(questions_value) = questions.trim().parse::<u32>() else {
                save_error.set(Some("请输入 1–2000 之间的题数".into()));
                return;
            };
            let Ok(hours_value) = hours.trim().parse::<f64>() else {
                save_error.set(Some("请输入 0.5–24 之间的小时数".into()));
                return;
            };
            let study_ms = (hours_value * 3_600_000.0).round() as i64;
            if !(1..=2_000).contains(&questions_value)
                || !(1_800_000..=86_400_000).contains(&study_ms)
            {
                save_error.set(Some(
                    "目标超出允许范围：题数 1–2000，时长 0.5–24 小时".into(),
                ));
                return;
            }
            let csrf = csrf.clone();
            let editing = editing.clone();
            let saving = saving.clone();
            let save_error = save_error.clone();
            let goals = goals.clone();
            let refresh = refresh.clone();
            saving.set(true);
            save_error.set(None);
            spawn_local(async move {
                let body = StudyGoals {
                    daily_questions: questions_value,
                    daily_study_ms: study_ms,
                };
                let result = post_json_response_typed::<_, StudyGoals>(
                    &format!("{TRAINING_TIME_ROOT}/goals"),
                    &csrf,
                    &body,
                    "保存学习目标",
                )
                .await;
                saving.set(false);
                match result {
                    Ok(saved) => {
                        goals.set(Some(saved));
                        editing.set(false);
                        refresh.set(*refresh + 1);
                    }
                    Err(error) => save_error.set(Some(error.to_string())),
                }
            });
        })
    };
    let goal_days = [
        ("今日", 1_i64, today_count, today, "study-practice-today"),
        ("最近 7 天", 7, week_count, week, "study-practice-week"),
        ("最近 35 天", 35, total_count, total, "study-practice-total"),
    ];
    html! {
        <div class="space-y-5" data-testid="training-statistics-content">
            if !props.embedded {
                <div><h2 class="text-2xl font-bold">{"学习统计"}</h2><p class="mt-1 text-sm opacity-60">{"按北京时间统计有效学习时间 · 真实数据"}</p></div>
            }
            <section class="space-y-3 rounded-xl bg-base-100 p-4" aria-label="学习目标" data-testid="study-goals">
                <div class="flex items-center justify-between gap-3">
                    <h3 class="font-semibold">{"学习目标"}</h3>
                    <div class="flex items-center gap-2">
                        <span class="text-xs opacity-60">{format!("连续学习 {streak} 天")}</span>
                        <button type="button" class="btn btn-ghost btn-xs" data-testid="study-goals-edit" onclick={start_edit}>{"设置"}</button>
                    </div>
                </div>
                if *editing {
                    <div class="flex flex-wrap items-end gap-2" data-testid="study-goals-form">
                        <label class="grid gap-1 text-xs">{"每日题量"}
                            <input type="number" min="1" max="2000" class="input input-sm w-24" data-testid="study-goal-questions-input"
                                value={(*draft_questions).clone()} disabled={*saving}
                                oninput={Callback::from({let draft=draft_questions.clone();move |e:web_sys::InputEvent|draft.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().value())})}/>
                        </label>
                        <label class="grid gap-1 text-xs">{"每日时长（小时）"}
                            <input type="number" min="0.5" max="24" step="0.5" class="input input-sm w-24" data-testid="study-goal-hours-input"
                                value={(*draft_hours).clone()} disabled={*saving}
                                oninput={Callback::from({let draft=draft_hours.clone();move |e:web_sys::InputEvent|draft.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().value())})}/>
                        </label>
                        <button type="button" class="btn btn-primary btn-sm" data-testid="study-goals-save" disabled={*saving} onclick={save}>
                            {if *saving {"保存中…"} else {"保存"}}
                        </button>
                        <button type="button" class="btn btn-ghost btn-sm" data-testid="study-goals-cancel" disabled={*saving}
                            onclick={Callback::from({let editing=editing.clone();let save_error=save_error.clone();move |_|{editing.set(false);save_error.set(None);}})}>{"取消"}</button>
                    </div>
                    if let Some(message)=save_error.as_ref(){<p role="alert" class="text-xs text-error">{message.clone()}</p>}
                }
                <div class="grid gap-3 sm:grid-cols-3">
                    {for goal_days.into_iter().map(|(label,multiplier,count,ms,count_test)|{
                        let question_target=i64::from(goal.daily_questions)*multiplier;
                        let time_target=goal.daily_study_ms*multiplier;
                        html!{<div class="rounded-lg bg-base-200/60 p-3" data-testid="study-goal-card">
                            <p class="text-xs opacity-60">{label}</p>
                            <div class="mt-2 flex justify-between text-xs"><span>{"刷题"}</span><strong data-testid={count_test}>{format!("{count} 题")}</strong></div>
                            <progress class="progress progress-primary mt-1 block h-2 w-full" max={question_target.max(1).to_string()}
                                value={count.max(0).to_string()} aria-label={format!("{}刷题进度",label)}/>
                            <p class="mt-1 text-[11px] opacity-60">{format!("/ {question_target} 题")}</p>
                            <div class="mt-2 flex justify-between text-xs"><span>{"时长"}</span><span>{format!("{} / {:.1} 小时",hours(ms),time_target as f64/3_600_000.0)}</span></div>
                            <progress class="progress progress-primary mt-1 block h-2 w-full" max={time_target.max(1).to_string()}
                                value={ms.max(0).to_string()} aria-label={format!("{}学习时长进度",label)}/>
                        </div>}
                    })}
                </div>
                <p class="text-xs opacity-60">{"每日目标存于服务端；最近 7 天与最近 35 天目标按 7 倍、35 倍自动计算，某天超额可补足其他天。速算与闪卡回顾只计入学习时长。"}</p>
            </section>
            <div class="rounded-xl bg-base-100 p-4 space-y-3">
                <h3 class="font-semibold">{"每日学习热力图"}</h3>
                <div class="grid grid-cols-7 gap-2" data-testid="training-heatmap">
                    {for ["一","二","三","四","五","六","日"].into_iter().map(|d|html!{<span class="text-center text-xs opacity-60">{d}</span>})}
                    {for data.days.iter().map(|day|{
                        let date=day.date.clone();
                        let active=Some(&date)==selected.as_ref();
                        let goal_ms=goal.daily_study_ms.max(1);
                        let ratio=day.total_ms as f64/goal_ms as f64;
                        let level=if day.total_ms==0 {0}else if ratio<0.25{1}else if ratio<0.5{2}else if ratio<0.75{3}else{4};
                        // Opacity ramps read the same in light and dark themes;
                        // fixed green shades invert on a light background.
                        let color=["bg-base-content/10","bg-primary/20","bg-primary/40","bg-primary/65","bg-primary"][level];
                        let onselect={let selected=selected.clone();let date=date.clone();Callback::from(move |_|selected.set(Some(date.clone())))};
                        html!{<button type="button" class={classes!("aspect-square","min-h-7","rounded-md",color,active.then_some("ring-2 ring-primary"))}
                            data-testid="study-heat-day" aria-label={format!("{}：{} · 已交卷 {} 题",day.date,minutes(day.total_ms),day.answered_count)}
                            title={format!("{} · {} · 已交卷 {} 题",day.date,minutes(day.total_ms),day.answered_count)} onclick={onselect}/>}
                    })}
                </div>
                <p class="text-xs opacity-60">{"颜色越深，表示当天越接近每日时长目标。点击日期查看明细。"}</p>
            </div>
            <section class="rounded-xl bg-base-100 p-4 space-y-3" data-testid="study-practice-detail">
                <h3 class="font-semibold">{format!("{} · 刷题明细",chosen.map(|d|d.date.as_str()).unwrap_or("今日"))}</h3>
                if let Some(day)=chosen.filter(|day|day.answered_count>0) {
                    <p class="text-sm">{format!("已交卷作答 {} 题",day.answered_count)}</p>
                    {for day.answered_modules.iter().map(|(id,count)|html!{<div class="flex justify-between gap-3 text-sm"><span>{module_name(id)}</span><span>{format!("{count} 题")}</span></div>})}
                }else {
                    <p class="text-sm opacity-60">{"当天暂无已交卷的作答记录。"}</p>
                }
            </section>
            <div class="rounded-xl bg-base-100 p-4 space-y-3">
                <h3 class="font-semibold">{format!("{} · 模块耗时",chosen.map(|d|d.date.as_str()).unwrap_or("今日"))}</h3>
                if by_module.is_empty() {
                    <p class="text-sm opacity-60">{"当天暂无有效学习记录，开始训练后自动生成。"}</p>
                }else {
                    {for by_module.iter().map(|(id,ms)|{
                        let max=chosen.map(|d|d.total_ms).unwrap_or(0).max(1);
                        html!{<div class="space-y-1"><div class="flex justify-between text-sm"><span>{module_name(id)}</span><span>{minutes(*ms)}</span></div>
                            <progress class="progress progress-primary block h-2 w-full" max={max.to_string()} value={(*ms).max(0).to_string()} aria-label={format!("{}耗时",module_name(id))}/></div>}
                    })}
                }
            </div>
        </div>
    }
}
