//! Real stored learning durations, not quiz elapsed-time estimates.
use super::question_bank_practice::{module_filter_query, read_module_filters};
use super::*;
use hyz_things::domain::question_bank::{PracticeCatalog, PracticeCategory};
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
    let clock = use_context::<StudyClockContext>();
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
        let clock = clock.clone();
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
            let clock = clock.clone();
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
                        if let Some(context) = clock.clone() {
                            context.refresh_goals.emit(());
                        }
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
            <StudyPlanCard csrf={props.csrf.clone()}/>
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
                    {for day.answered_modules.iter().filter(|(id,_)|id.as_str()!="question_bank").map(|(id,count)|html!{<div class="flex justify-between gap-3 text-sm"><span>{module_name(id)}</span><span>{format!("{count} 题")}</span></div>})}
                    if let Some(total)=day.answered_modules.get("question_bank") {
                        <div class="space-y-2 rounded-lg bg-base-200/50 p-3" data-testid="study-question-bank-breakdown">
                            <div class="flex justify-between gap-3 text-sm font-medium"><span>{"真题训练"}</span><span>{format!("{total} 题")}</span></div>
                            if !day.question_bank.modules.is_empty() {
                                <div class="space-y-1.5" data-testid="study-question-bank-modules">
                                    <p class="text-xs opacity-60">{"模块刷题"}</p>
                                    {for day.question_bank.modules.iter().map(|(module,categories)|{
                                        let count:i64=categories.values().sum();
                                        html!{<div class="space-y-0.5 pl-2">
                                            <div class="flex justify-between gap-3 text-xs"><span class="font-medium">{module}</span><span>{format!("{count} 题")}</span></div>
                                            {for categories.iter().map(|(category,count)|html!{
                                                <div class="flex justify-between gap-3 pl-3 text-[11px] opacity-70">
                                                    <span>{category}</span><span>{format!("{count} 题")}</span>
                                                </div>
                                            })}
                                        </div>}
                                    })}
                                </div>
                            }
                            if !day.question_bank.papers.is_empty() {
                                <div class="space-y-1" data-testid="study-question-bank-papers">
                                    <p class="text-xs opacity-60">{"套卷"}</p>
                                    {for day.question_bank.papers.iter().map(|(paper,count)|html!{
                                        <div class="flex justify-between gap-3 pl-2 text-xs">
                                            <span class="min-w-0 truncate" title={paper.clone()}>{paper}</span><span class="shrink-0">{format!("{count} 题")}</span>
                                        </div>
                                    })}
                                </div>
                            }
                        </div>
                    }
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

fn plan_parent_selected(plan: &StudyPlan, module: &str) -> bool {
    plan.items
        .iter()
        .any(|item| item.module == module && item.category.is_none())
}

fn plan_category_selected(plan: &StudyPlan, module: &str, category: &str) -> bool {
    plan_parent_selected(plan, module)
        || plan
            .items
            .iter()
            .any(|item| item.module == module && item.category.as_deref() == Some(category))
}

fn toggle_plan_parent(plan: &mut StudyPlan, module: &str) {
    let selected = plan_parent_selected(plan, module);
    plan.items.retain(|item| item.module != module);
    if !selected {
        plan.items.push(StudyPlanItem {
            module: module.to_owned(),
            category: None,
        });
    }
}

fn toggle_plan_category(
    plan: &mut StudyPlan,
    module: &str,
    category: &str,
    categories: &[PracticeCategory],
) {
    if plan_parent_selected(plan, module) {
        return;
    }
    let exists = plan
        .items
        .iter()
        .any(|item| item.module == module && item.category.as_deref() == Some(category));
    if exists {
        plan.items
            .retain(|item| !(item.module == module && item.category.as_deref() == Some(category)));
    } else {
        plan.items.push(StudyPlanItem {
            module: module.to_owned(),
            category: Some(category.to_owned()),
        });
    }
    let all_selected = categories.iter().all(|category| {
        plan.items.iter().any(|item| {
            item.module == module && item.category.as_deref() == Some(category.name.as_str())
        })
    });
    if all_selected {
        plan.items.retain(|item| item.module != module);
        plan.items.push(StudyPlanItem {
            module: module.to_owned(),
            category: None,
        });
    }
}

fn plan_item_label(item: &StudyPlanForecastItem) -> String {
    match &item.category {
        Some(category) => format!("{} · {}", item.module, category),
        None => format!("{} · 全部", item.module),
    }
}

fn plan_days_range(forecast: &StudyPlanForecast) -> Option<String> {
    forecast
        .days_7
        .or(forecast.days_yesterday)
        .map(|days| days.to_string())
}

#[derive(Properties, PartialEq)]
pub(super) struct StudyPlanCardProps {
    pub csrf: String,
}

#[function_component(StudyPlanCard)]
pub(super) fn study_plan_card(props: &StudyPlanCardProps) -> Html {
    let data = use_state(|| None::<StudyPlanForecast>);
    let catalog = use_state(|| None::<PracticeCatalog>);
    let error = use_state(|| None::<String>);
    let save_error = use_state(|| None::<String>);
    let refresh = use_state(|| 0_u32);
    let managing = use_state(|| false);
    let saving = use_state(|| false);
    let draft = use_state(StudyPlan::default);
    let filters = use_state(read_module_filters);
    let query = module_filter_query(&filters);
    {
        let data = data.clone();
        let catalog = catalog.clone();
        let error = error.clone();
        let query = query.clone();
        use_effect_with((*refresh, query.clone()), move |_| {
            spawn_local(async move {
                match fetch_json_typed::<StudyPlanForecast>(
                    &format!("{STUDY_PLAN_ROOT}/forecast?{query}"),
                    "备考计划",
                )
                .await
                {
                    Ok(value) => data.set(Some(value)),
                    Err(message) => error.set(Some(message.to_string())),
                }
                if let Ok(value) = fetch_json_typed::<PracticeCatalog>(
                    &format!("/api/v1/question-bank/practice/catalog?{query}"),
                    "题库目录",
                )
                .await
                {
                    catalog.set(Some(value));
                }
            });
            || ()
        });
    }
    let open_manage = {
        let managing = managing.clone();
        let draft = draft.clone();
        let data = data.clone();
        let save_error = save_error.clone();
        Callback::from(move |_| {
            if let Some(value) = data.as_ref() {
                draft.set(StudyPlan {
                    prediction_enabled: value.prediction_enabled,
                    items: value
                        .items
                        .iter()
                        .map(|item| StudyPlanItem {
                            module: item.module.clone(),
                            category: item.category.clone(),
                        })
                        .collect(),
                });
            }
            save_error.set(None);
            managing.set(true);
        })
    };
    let close_manage = {
        let managing = managing.clone();
        let save_error = save_error.clone();
        Callback::from(move |_| {
            save_error.set(None);
            managing.set(false);
        })
    };
    let toggle_prediction = {
        let csrf = props.csrf.clone();
        let data = data.clone();
        let saving = saving.clone();
        let refresh = refresh.clone();
        let error = error.clone();
        Callback::from(move |event: web_sys::Event| {
            if *saving {
                return;
            }
            let Some(current) = data.as_ref().cloned() else {
                return;
            };
            let input: web_sys::HtmlInputElement = event.target_unchecked_into();
            let enabled = input.checked();
            let plan = StudyPlan {
                prediction_enabled: enabled,
                items: current
                    .items
                    .iter()
                    .map(|item| StudyPlanItem {
                        module: item.module.clone(),
                        category: item.category.clone(),
                    })
                    .collect(),
            };
            let mut optimistic = current;
            optimistic.prediction_enabled = enabled;
            data.set(Some(optimistic));
            saving.set(true);
            error.set(None);
            let csrf = csrf.clone();
            let saving = saving.clone();
            let refresh = refresh.clone();
            let error = error.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, StudyPlan>(
                    STUDY_PLAN_ROOT,
                    &csrf,
                    &plan,
                    "保存备考预测",
                )
                .await
                {
                    Ok(_) => refresh.set(*refresh + 1),
                    Err(message) => {
                        error.set(Some(message.to_string()));
                        refresh.set(*refresh + 1);
                    }
                }
                saving.set(false);
            });
        })
    };
    let set_draft_enabled = {
        let draft = draft.clone();
        Callback::from(move |event: web_sys::Event| {
            let input: web_sys::HtmlInputElement = event.target_unchecked_into();
            let mut next = (*draft).clone();
            next.prediction_enabled = input.checked();
            draft.set(next);
        })
    };
    let save_plan = {
        let csrf = props.csrf.clone();
        let draft = draft.clone();
        let saving = saving.clone();
        let refresh = refresh.clone();
        let managing = managing.clone();
        let save_error = save_error.clone();
        Callback::from(move |_| {
            if *saving {
                return;
            }
            let plan = (*draft).clone();
            saving.set(true);
            save_error.set(None);
            let csrf = csrf.clone();
            let saving = saving.clone();
            let refresh = refresh.clone();
            let managing = managing.clone();
            let save_error = save_error.clone();
            spawn_local(async move {
                match post_json_response_typed::<_, StudyPlan>(
                    STUDY_PLAN_ROOT,
                    &csrf,
                    &plan,
                    "保存备考计划",
                )
                .await
                {
                    Ok(_) => {
                        managing.set(false);
                        refresh.set(*refresh + 1);
                    }
                    Err(message) => save_error.set(Some(message.to_string())),
                }
                saving.set(false);
            });
        })
    };
    let forecast = data.as_ref();
    html! {
        <section class="space-y-3 rounded-xl bg-base-100 p-4" aria-label="备考计划" data-testid="study-plan">
            <div class="flex flex-wrap items-center justify-between gap-3">
                <h3 class="font-semibold">{"备考计划"}</h3>
                <div class="flex items-center gap-2">
                    <label class="flex items-center gap-2 text-xs">
                        <span>{"预测"}</span>
                        <input type="checkbox" class="toggle toggle-primary toggle-sm" data-testid="study-plan-toggle"
                            checked={forecast.map(|value| value.prediction_enabled).unwrap_or(false)}
                            disabled={*saving || forecast.is_none()} onchange={toggle_prediction}/>
                    </label>
                    <button type="button" class="btn btn-ghost btn-xs" data-testid="study-plan-manage" onclick={open_manage.clone()}>{"管理"}</button>
                </div>
            </div>
            if let Some(message) = error.as_ref() {
                <p role="alert" class="text-xs text-error">{message.clone()}</p>
            }
            if let Some(value) = forecast {
                if !value.prediction_enabled {
                    <p class="text-sm opacity-60">{"预测已关闭。计划会保留，开启后恢复预计天数和日期。"}</p>
                }
                if value.items.is_empty() {
                    <button type="button" class="btn btn-outline btn-sm" data-testid="study-plan-add" onclick={open_manage}>{"添加计划模块"}</button>
                } else {
                    <div class="space-y-2">
                        {for value.items.iter().map(|item|{
                            let pace = if !value.prediction_enabled {
                                Html::default()
                            } else if item.remaining == 0 {
                                html!{<p class="text-xs text-success">{"计划内已完成"}</p>}
                            } else {
                                let yesterday = if item.answered_yesterday == 0 {
                                    "昨日未刷".to_string()
                                } else if let Some(days) = item.days_yesterday {
                                    format!("昨日 {} 题 · 按昨日速度约 {} 天{}", item.answered_yesterday, days,
                                        item.finish_yesterday.as_ref().map(|date| format!("（预计 {date}）")).unwrap_or_default())
                                } else {
                                    "昨日数据不足".to_string()
                                };
                                let seven = if item.answered_7 == 0 {
                                    "近 7 个完整日无作答".to_string()
                                } else if let Some(days) = item.days_7 {
                                    format!("近 7 个完整日日均 {:.1} 题 · 约 {} 天{}", item.answered_7 as f64 / 7.0, days,
                                        item.finish_7.as_ref().map(|date| format!("（预计 {date}）")).unwrap_or_default())
                                } else {
                                    "近 7 个完整日数据不足".to_string()
                                };
                                html!{<div class="space-y-0.5 text-xs"><p>{yesterday}</p><p>{seven}</p></div>}
                            };
                            html!{<div class="space-y-1.5 rounded-lg bg-base-200/40 p-3" data-testid="study-plan-item"
                                data-module={item.module.clone()} data-category={item.category.clone().unwrap_or_default()}>
                                <div class="flex items-start justify-between gap-3">
                                    <span class="min-w-0 text-sm font-medium">{plan_item_label(item)}</span>
                                    <span class="shrink-0 text-sm tabular-nums">{format!("还剩 {} 题", item.remaining)}</span>
                                </div>
                                <progress class="progress progress-primary block h-2 w-full" max={item.total.max(1).to_string()}
                                    value={item.done.max(0).to_string()} aria-label={format!("{} 完成进度", plan_item_label(item))}/>
                                <div class="flex flex-wrap items-center justify-between gap-2 text-[11px] opacity-60">
                                    <span>{format!("已做 {}/{} 题", item.done, item.total)}</span>
                                    if value.prediction_enabled && item.low_confidence {<span class="badge badge-ghost badge-sm">{"样本不足"}</span>}
                                </div>
                                {pace}
                            </div>}
                        })}
                    </div>
                    <div class="space-y-1 rounded-lg bg-base-200/60 p-3" data-testid="study-plan-summary">
                        if value.total_remaining == 0 {
                            <p class="text-sm font-medium">{"计划内题目已完成"}</p>
                        } else {
                            <p class="text-sm font-medium">{format!("合计：{} 个计划 · 剩 {} 题{}",
                                value.items.len(), value.total_remaining,
                                plan_days_range(value).map(|range| format!(" · 约 {range} 天")).unwrap_or_default())}</p>
                        }
                        <p class="text-[11px] opacity-60">{format!("当前筛选：{}", value.filter_label)}</p>
                    </div>
                }
            } else if error.is_none() {
                <p class="text-sm opacity-60">{"正在读取备考计划…"}</p>
            }
            if *managing {
                <div class="fixed inset-0 z-[130] overflow-y-auto bg-base-200/95 p-3 sm:p-6" role="dialog" aria-modal="true"
                    aria-label="管理备考计划" data-testid="study-plan-sheet">
                    <div class="mx-auto w-full max-w-2xl space-y-4 rounded-2xl bg-base-100 p-4 sm:p-5">
                        <div class="flex items-center justify-between gap-3">
                            <h2 class="text-lg font-semibold">{"管理备考计划"}</h2>
                            <button type="button" class="btn btn-ghost btn-sm" data-testid="study-plan-sheet-close" onclick={close_manage.clone()}>{"关闭"}</button>
                        </div>
                        <label class="flex items-center justify-between gap-3 rounded-xl bg-base-200/50 p-3">
                            <span><strong>{"显示预测"}</strong><span class="block text-xs opacity-60">{"关闭后保留计划，只隐藏预计天数和日期。"}</span></span>
                            <input type="checkbox" class="toggle toggle-primary toggle-sm" data-testid="study-plan-sheet-toggle"
                                checked={draft.prediction_enabled} disabled={*saving} onchange={set_draft_enabled}/>
                        </label>
                        if let Some(catalog) = catalog.as_ref() {
                            <div class="space-y-2">
                                {for catalog.modules.iter().map(|module|{
                                    let module_name = module.name.clone();
                                    let parent_selected = plan_parent_selected(&draft, &module_name);
                                    let on_parent = {
                                        let draft = draft.clone();
                                        let module_name = module_name.clone();
                                        Callback::from(move |_|{
                                            let mut next = (*draft).clone();
                                            toggle_plan_parent(&mut next, &module_name);
                                            draft.set(next);
                                        })
                                    };
                                    html!{<div class="rounded-xl border border-base-content/10 p-3" data-testid="study-plan-module">
                                        <label class="flex items-center justify-between gap-3">
                                            <span class="flex min-w-0 items-center gap-2">
                                                <input type="checkbox" class="checkbox checkbox-primary checkbox-sm"
                                                    checked={parent_selected} disabled={*saving} onchange={on_parent}
                                                    aria-label={format!("{} 全部", module_name)}/>
                                                <strong class="truncate">{&module.name}</strong>
                                            </span>
                                            <span class="shrink-0 text-xs opacity-60">{format!("{} / {} 题", module.done, module.count)}</span>
                                        </label>
                                        if !module.categories.is_empty() {
                                            <div class="mt-2 flex flex-wrap gap-1.5 pl-6">
                                                {for module.categories.iter().map(|category|{
                                                    let category_name = category.name.clone();
                                                    let selected = plan_category_selected(&draft, &module_name, &category_name);
                                                    let locked = parent_selected || *saving;
                                                    let categories = module.categories.clone();
                                                    let on_category = {
                                                        let draft = draft.clone();
                                                        let module_name = module_name.clone();
                                                        let category_name = category_name.clone();
                                                        Callback::from(move |_|{
                                                            let mut next = (*draft).clone();
                                                            toggle_plan_category(&mut next, &module_name, &category_name, &categories);
                                                            draft.set(next);
                                                        })
                                                    };
                                                    html!{<label class={classes!("flex", "items-center", "gap-1.5", "rounded-full", "border", "border-base-content/15", "px-2", "py-1", "text-xs", locked.then_some("opacity-60"))}>
                                                        <input type="checkbox" class="checkbox checkbox-xs checkbox-primary"
                                                            checked={selected} disabled={locked} onchange={on_category}
                                                            aria-label={format!("{} {}", module_name, category_name)}/>
                                                        <span>{&category.name}</span>
                                                        <span class="opacity-60">{format!("{}/{}", category.done, category.count)}</span>
                                                    </label>}
                                                })}
                                            </div>
                                        }
                                    </div>}
                                })}
                            </div>
                        } else {
                            <p class="text-sm opacity-60">{"正在读取题库目录…"}</p>
                        }
                        if let Some(message) = save_error.as_ref() {
                            <p role="alert" class="text-xs text-error">{message.clone()}</p>
                        }
                        <div class="flex justify-end gap-2">
                            <button type="button" class="btn btn-ghost" data-testid="study-plan-sheet-cancel" onclick={close_manage}>{"取消"}</button>
                            <button type="button" class="btn btn-primary" data-testid="study-plan-save"
                                disabled={*saving} onclick={save_plan}>{if *saving {"保存中…"} else {"保存计划"}}</button>
                        </div>
                    </div>
                </div>
            }
        </section>
    }
}
