//! 资料分析四选一速算。题目、判题均在浏览器内完成，不修改 Flashcard/设备数据。
use super::*;
use js_sys::Date;
use gloo_timers::callback::Interval;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CalculationKind {
    Division,
    Multiplication,
    Percent,
}

impl CalculationKind {
    const ALL: [Self; 3] = [Self::Division, Self::Multiplication, Self::Percent];

    fn label(self) -> &'static str {
        match self {
            Self::Division => "三位 ÷ 两位",
            Self::Multiplication => "三位 × 两位",
            Self::Percent => "增长量 · 份数法",
        }
    }

    fn test_id(self) -> &'static str {
        match self {
            Self::Division => "calc-kind-div",
            Self::Multiplication => "calc-kind-mul",
            Self::Percent => "calc-kind-percent",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Self::Division => "分子前三位、分母两位试商；用选项差异决定心算止步。",
            Self::Multiplication => "高位拆乘、先确定量级，再根据选项差异决定是否补位。",
            Self::Percent => {
                "份数法：增速约为 +1/n 时，现期约 n+1 份，增量为 +1 份；增速约为 -1/n 时，现期约 n-1 份，增量为 -1 份。按选项补位。"
            }
        }
    }
}

#[derive(Clone, Debug)]
struct CalculationQuestion {
    expression: String,
    options: [String; 4],
    correct_index: usize,
    exact_result: String,
    #[cfg(test)]
    numerator: u64,
    #[cfg(test)]
    denominator: u64,
    #[cfg(test)]
    is_negative: bool,
    kind: CalculationKind,
    tip: &'static str,
}

#[derive(Clone, Debug)]
struct CalculationAnswer {
    choice: usize,
    elapsed_ms: u32,
}

#[derive(Clone)]
struct CalculationSession {
    questions: Vec<CalculationQuestion>,
    answers: Vec<Option<CalculationAnswer>>,
    index: usize,
    started_question_at: f64,
    finished: bool,
}

// 一次练习使用同一个确定性 PRNG；可按固定 seed 重放和测试，不依赖系统服务。
fn next_random(seed: &mut u64) -> u64 {
    if *seed == 0 {
        *seed = 0x9e37_79b9_7f4a_7c15;
    }
    *seed ^= *seed >> 12;
    *seed ^= *seed << 25;
    *seed ^= *seed >> 27;
    (*seed).wrapping_mul(0x2545_f491_4f6c_dd1d)
}

fn random_range(seed: &mut u64, low: u64, high: u64) -> u64 {
    low + next_random(seed) % (high - low + 1)
}

fn power_ten(exp: u32) -> u64 {
    10_u64.pow(exp)
}

// 精确有理数：对循环小数不冒充有精确的有限小数选项。
// 出题阶段拒绝无法有限表示的除式，正确选项绝不人为四舍五入。
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn exact_decimal_parts(num: u64, den: u64) -> Option<(u64, u32)> {
    if den == 0 {
        return None;
    }
    let common = gcd(num, den);
    let mut denominator = den / common;
    let mut twos = 0_u32;
    let mut fives = 0_u32;
    while denominator % 2 == 0 {
        denominator /= 2;
        twos += 1;
    }
    while denominator % 5 == 0 {
        denominator /= 5;
        fives += 1;
    }
    if denominator != 1 {
        return None;
    }
    let places = twos.max(fives);
    let scale = power_ten(places);
    Some((num.checked_mul(scale)? / den, places))
}

fn format_decimal(value: u64, places: u32) -> String {
    if places == 0 {
        value.to_string()
    } else {
        let scale = power_ten(places);
        format!(
            "{}.{:0width$}",
            value / scale,
            value % scale,
            width = places as usize,
        )
    }
}

fn exact_decimal(num: u64, den: u64) -> String {
    let (value, places) = exact_decimal_parts(num, den).expect("只能将有限小数表示成精确数值");
    format_decimal(value, places)
}

// 用于出题器校验：只比较前两位或前三位，绝不截断正确选项的显示值。
fn significant_prefix(value: &str, count: usize) -> String {
    value
        .chars()
        .filter(|digit| digit.is_ascii_digit())
        .skip_while(|digit| *digit == '0')
        .take(count)
        .collect()
}

fn order_of_magnitude(mut number: u64) -> u32 {
    let mut order = 0;
    while number >= 10 {
        number /= 10;
        order += 1;
    }
    order
}

// 干扰项保留真实答案的量级和小数位数，低位随机生成；
// 不使用等差选项。选项前 2/3 位已经足以区分，不需要真的算到末位。
fn candidate_options(seed: &mut u64, correct: u64, places: u32) -> Option<([String; 4], usize)> {
    let prefix_digits = if random_range(seed, 0, 1) == 0 { 2 } else { 3 };
    let order = order_of_magnitude(correct);
    let actual_digits = order + 1;
    let needed_digits = prefix_digits.min(actual_digits);
    let factor = power_ten(actual_digits - needed_digits);
    let target_prefix = correct / factor;
    let magnitude_lower_bound = power_ten(needed_digits - 1);
    let magnitude_upper_bound = power_ten(needed_digits) - 1;

    // 干扰项的最高两/三位来自周边不同数字，余下位数独立随机；
    // 例如 123.12 可以遇到 121.22、125.45、127.08。
    let mut prefixes = Vec::new();
    let max_offset = if needed_digits < 3 { 4 } else { 6 };
    for offset in -max_offset..=max_offset {
        if offset == 0 {
            continue;
        }
        let candidate = target_prefix as i64 + offset;
        if (magnitude_lower_bound as i64..=magnitude_upper_bound as i64).contains(&candidate) {
            prefixes.push(candidate as u64);
        }
    }
    if prefixes.len() < 3 {
        return None;
    }
    for i in (1..prefixes.len()).rev() {
        let j = random_range(seed, 0, i as u64) as usize;
        prefixes.swap(i, j);
    }
    let mut distractors = Vec::new();
    for prefix in prefixes {
        let lower_digits = if factor > 1 {
            random_range(seed, 0, factor - 1)
        } else {
            0
        };
        let candidate = prefix * factor + lower_digits;
        // 题目所有干扰项与真实答案的相对差距严格小于 10%。
        if candidate * 10 <= correct * 9 || candidate * 10 >= correct * 11 {
            continue;
        }
        distractors.push(candidate);
        if distractors.len() == 3 {
            break;
        }
    }
    if distractors.len() != 3 {
        return None;
    }
    let mut candidates = [correct; 4];
    for (index, value) in distractors.into_iter().enumerate() {
        candidates[index] = value;
    }
    for i in (1..candidates.len()).rev() {
        let j = random_range(seed, 0, i as u64) as usize;
        candidates.swap(i, j);
    }
    let correct_index = candidates.iter().position(|value| *value == correct)?;
    let options = candidates.map(|value| format_decimal(value, places));
    let distinct_prefixes = options
        .iter()
        .map(|value| significant_prefix(value, 3))
        .collect::<std::collections::HashSet<_>>();
    (distinct_prefixes.len() == 4).then_some((options, correct_index))
}

// 略过容易直接背倒数的规整增速。单位为 0.1%，如 137 表示 13.7%。
fn is_share_anchor(per_mille: u64) -> bool {
    per_mille % 10 == 0
        || matches!(
            per_mille,
            111 | 125 | 143 | 147 | 167 | 250 | 286 | 333 | 375 | 429 | 625 | 667 | 750
        )
}

fn make_question(seed: &mut u64, kind: CalculationKind) -> CalculationQuestion {
    for _ in 0..4096 {
        let a = random_range(seed, 205, 985);
        let b = random_range(seed, 12, 87);
        let (expression, numerator, denominator, is_negative) = match kind {
            CalculationKind::Division => (format!("{a} ÷ {b}"), a, b, false),
            CalculationKind::Multiplication => (format!("{a} × {b}"), a * b, 1, false),
            CalculationKind::Percent => {
                // 从整数基期量构造正/负增速下的现期量，精确解保留符号。
                // 负增长始终大于 -100%，因此分母 1+r 始终为正。
                let is_negative = random_range(seed, 0, 1) == 0;
                let limit = if is_negative { 499 } else { 899 };
                let p = random_range(seed, 101, limit); // 单位 0.1%
                if is_share_anchor(p) {
                    continue;
                }
                let base = random_range(seed, 225, 525);
                let growth_factor = if is_negative { 1000 - p } else { 1000 + p };
                let current_scaled = base * growth_factor;
                (
                    format!(
                        "现期量 {}，增长率 {}{}%，求增长量",
                        format_decimal(current_scaled, 3),
                        if is_negative { "-" } else { "" },
                        format_decimal(p, 1),
                    ),
                    current_scaled * p,
                    1000 * growth_factor,
                    is_negative,
                )
            }
        };
        let Some((correct, places)) = exact_decimal_parts(numerator, denominator) else {
            continue;
        };
        let Some((options, correct_index)) = candidate_options(seed, correct, places) else {
            continue;
        };
        let exact_result = exact_decimal(numerator, denominator);
        return CalculationQuestion {
            expression,
            options: options.map(|value| {
                if is_negative {
                    format!("-{value}")
                } else {
                    value
                }
            }),
            correct_index,
            exact_result: if is_negative {
                format!("-{exact_result}")
            } else {
                exact_result
            },
            #[cfg(test)]
            numerator,
            #[cfg(test)]
            denominator,
            #[cfg(test)]
            is_negative,
            kind,
            tip: kind.tip(),
        };
    }
    panic!("所选计算类型未能生成满足精确答案与选项可辨识规则的题目")
}

fn question_set(
    seed: &mut u64,
    kinds: &[CalculationKind],
    count: usize,
) -> Vec<CalculationQuestion> {
    let mut questions: Vec<_> = (0..count)
        .map(|index| {
            let kind = if index < kinds.len() {
                kinds[index]
            } else {
                kinds[random_range(seed, 0, (kinds.len() - 1) as u64) as usize]
            };
            make_question(seed, kind)
        })
        .collect();
    // 启用的每种计算类型每轮至少出现一次，打散出现顺序。
    for index in (1..questions.len()).rev() {
        let other = random_range(seed, 0, index as u64) as usize;
        questions.swap(index, other);
    }
    questions
}

#[derive(Properties, PartialEq)]
struct CalculationMetricProps {
    label: AttrValue,
    value: String,
}

#[function_component(CalculationMetric)]
fn calculation_metric(props: &CalculationMetricProps) -> Html {
    html! {
        <article class="rounded-box border border-base-content/10 bg-base-100/60 p-4">
            <span class="text-xs text-base-content/65">{props.label.clone()}</span>
            <strong class="mt-1 block font-mono text-2xl font-black">{props.value.clone()}</strong>
        </article>
    }
}

#[derive(Properties, PartialEq)]
struct CalculationTrainingProps {
    route_state: UseStateHandle<PortalRoute>,
    on_immersive_change: Callback<bool>,
}

pub(crate) fn render_calculation_training(
    route_state: UseStateHandle<PortalRoute>,
    on_immersive_change: Callback<bool>,
) -> Html {
    html! { <CalculationTraining route_state={route_state} on_immersive_change={on_immersive_change} /> }
}

#[function_component(CalculationTraining)]
fn calculation_training(props: &CalculationTrainingProps) -> Html {
    let enabled = use_state(|| [true, true, true]);
    let count = use_state(|| 20_usize);
    let session = use_state(|| None::<CalculationSession>);
    let clock_tick = use_state(|| 0_u64);
    let is_training_immersive = session.as_ref().is_some_and(|state| !state.finished);
    {
        let notify = props.on_immersive_change.clone();
        use_effect_with(is_training_immersive, move |active| {
            notify.emit(*active);
            || ()
        });
    }

    {
        let clock_tick = clock_tick.clone();
        use_effect_with(is_training_immersive, move |active| {
            let ticker = (*active).then(|| {
                let clock_tick = clock_tick.clone();
                Interval::new(1_000, move || clock_tick.set(Date::now() as u64))
            });
            move || drop(ticker)
        });
    }
    let back = {
        let route_state = props.route_state.clone();
        let session = session.clone();
        Callback::from(move |_| {
            if session.is_some() {
                session.set(None);
            } else {
                navigate_route(
                    &route_state,
                    PortalRoute::study(StudyView::Dashboard, None, Vec::new()),
                    false,
                );
            }
        })
    };
    let restart = {
        let session = session.clone();
        Callback::from(move |_| session.set(None))
    };
    let start = {
        let enabled = *enabled;
        let count = *count;
        let session = session.clone();
        Callback::from(move |_| {
            let kinds: Vec<_> = CalculationKind::ALL
                .into_iter()
                .enumerate()
                .filter_map(|(index, kind)| enabled[index].then_some(kind))
                .collect();
            if kinds.is_empty() {
                return;
            }
            let mut seed = (Date::now() as u64) ^ 0x9e37_79b9_7f4a_7c15;
            let questions = question_set(&mut seed, &kinds, count);
            session.set(Some(CalculationSession {
                answers: vec![None; questions.len()],
                questions,
                index: 0,
                started_question_at: Date::now(),
                finished: false,
            }));
        })
    };

    let current = (*session).clone();
    let content = if let Some(live) = current {
        if live.finished {
            let answered: Vec<_> = live
                .answers
                .iter()
                .enumerate()
                .filter_map(|(index, answer)| answer.as_ref().map(|answer| (index, answer)))
                .collect();
            let total = answered.len();
            let correct = answered
                .iter()
                .filter(|(index, answer)| answer.choice == live.questions[*index].correct_index)
                .count();
            let elapsed: u64 = answered
                .iter()
                .map(|(_, answer)| answer.elapsed_ms as u64)
                .sum();
            html! {
                <section class="mx-auto grid w-full max-w-3xl gap-5" data-testid="calculation-summary">
                    <div class="rounded-box border border-base-content/10 bg-base-100 p-6">
                        <p class="text-xs font-bold tracking-widest text-primary">{"训练结果"}</p>
                        <h1 class="mt-2 text-3xl font-black">{"本轮完成"}</h1>
                        <div class="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-3">
                            <CalculationMetric label="正确率" value={if total == 0 { "0%".to_owned() } else { format!("{:.0}%", correct as f64 * 100.0 / total as f64) }} />
                            <CalculationMetric label="已做题目" value={format!("{total} / {}", live.questions.len())} />
                            <CalculationMetric label="平均用时" value={if total == 0 { "—".to_owned() } else { format!("{:.1} 秒", elapsed as f64 / total as f64 / 1000.0) }} />
                        </div>
                    </div>
                    <div class="grid gap-3">
                        <h2 class="text-lg font-bold">{"错题回看"}</h2>
                        {for answered.iter().filter(|(index, answer)| answer.choice != live.questions[*index].correct_index).map(|(index, answer)| {
                            let question = &live.questions[*index];
                            html! {
                                <article class="rounded-box border border-base-content/10 bg-base-100 p-4">
                                    <p class="font-semibold">{format!("第 {} 题 · {}", index + 1, question.expression)}</p>
                                    <p class="mt-2 text-sm">{format!("你的选择：{}　正确选项：{}", question.options[answer.choice], question.exact_result)}</p>
                                    <p class="mt-1 text-xs text-base-content/65">{question.tip}</p>
                                </article>
                            }
                        })}
                        if correct == total {
                            <p class="text-sm text-base-content/60">{"本轮没有错题。"}</p>
                        }
                    </div>
                    <div class="flex flex-wrap justify-center gap-3">
                        <button type="button" class="btn btn-primary" data-testid="calculation-restart" onclick={restart}>{"再练一轮"}</button>
                        <button type="button" class="btn btn-ghost" onclick={back.clone()}>{"返回专项训练"}</button>
                    </div>
                </section>
            }
        } else {
            let question = live.questions[live.index].clone();
            let answered = live.answers[live.index].clone();
            let progress = live.index + 1;
            let buttons = question.options.iter().enumerate().map(|(choice, option)| {
                let session = session.clone();
                let is_selected = answered.as_ref().is_some_and(|record| record.choice == choice);
                let class = if answered.is_some() && question.correct_index == choice {
                    "btn btn-success h-20 min-h-20 text-2xl font-semibold"
                } else if is_selected {
                    "btn btn-error h-20 min-h-20 text-2xl font-semibold"
                } else {
                    "btn btn-outline h-20 min-h-20 text-2xl font-semibold"
                };
                html! {
                    <button
                        type="button"
                        class={class}
                        data-testid="calculation-answer"
                        disabled={answered.is_some()}
                        onclick={Callback::from(move |_| {
                            let Some(mut live) = (*session).clone() else { return };
                            if live.finished || live.answers[live.index].is_some() { return; }
                            let index = live.index;
                            live.answers[index] = Some(CalculationAnswer {
                                choice,
                                elapsed_ms: (Date::now() - live.started_question_at).max(0.0) as u32,
                            });
                            session.set(Some(live));
                        })}
                    >
                        {format!("{}. {}", ["A", "B", "C", "D"][choice], option)}
                    </button>
                }
            });
            let next = {
                let session = session.clone();
                Callback::from(move |_| {
                    let Some(mut live) = (*session).clone() else {
                        return;
                    };
                    if live.answers[live.index].is_none() {
                        return;
                    }
                    if live.index + 1 == live.questions.len() {
                        live.finished = true;
                    } else {
                        live.index += 1;
                        live.started_question_at = Date::now();
                    }
                    session.set(Some(live));
                })
            };
            let finish = {
                let session = session.clone();
                Callback::from(move |_| {
                    if let Some(mut live) = (*session).clone() {
                        live.finished = true;
                        session.set(Some(live));
                    }
                })
            };
            html! {
                <section class="calculation-workspace mx-auto flex h-full min-h-0 w-full max-w-5xl flex-col gap-2" data-testid="calculation-question">
                    <div class="min-h-0 flex-1">
                        <InkOverlay question_index={live.index} responsive={true}>
                            <div class="h-full min-h-0 overflow-y-auto overscroll-contain p-1" data-testid="calculation-question-scroll">
                                <div class="calculation-problem grid min-w-0 content-start gap-4" data-testid="calculation-problem">
                                    <div class="flex items-center justify-between gap-3 text-sm text-base-content/60">
                                        <span>{format!("第 {progress} / {} 题", live.questions.len())}</span>
                                        <span>{question.kind.label()}</span>
                                    </div>
                                    <div class="grid min-h-36 place-items-center rounded-box border border-base-content/10 bg-base-100 p-5 text-center">
                                        <h1 class="break-words text-3xl font-black leading-snug sm:text-5xl" data-testid="calculation-expression">
                                            {if question.kind == CalculationKind::Percent {
                                                format!("{}？", question.expression)
                                            } else {
                                                format!("{} = ?", question.expression)
                                            }}
                                        </h1>
                                    </div>
                                    <div class="grid grid-cols-2 gap-3">{for buttons}</div>
                                    if let Some(record) = answered {
                                        <div class="rounded-box border border-base-content/10 bg-base-100 p-4" role="status" data-testid="calculation-feedback">
                                            <strong class={if record.choice == question.correct_index { "text-success" } else { "text-error" }}>
                                                {if record.choice == question.correct_index { "回答正确" } else { "回答错误" }}
                                            </strong>
                                            <p class="mt-1">{format!("正确答案：{}；用时 {:.1} 秒", question.exact_result, record.elapsed_ms as f64 / 1000.0)}</p>
                                            <p class="mt-1 text-sm text-base-content/65">{question.tip}</p>
                                        </div>
                                    }
                                </div>
                            </div>
                        </InkOverlay>
                    </div>
                    <footer class="flex shrink-0 flex-col gap-2 py-1" data-testid="calculation-controls">
                        if live.answers[live.index].is_some() {
                            <button type="button" class="btn btn-primary w-full" data-testid="calculation-next" onclick={next}>
                                {if progress == live.questions.len() { "查看结果" } else { "下一题" }}
                            </button>
                        }
                        <button type="button" class="btn btn-ghost btn-sm self-center" data-testid="calculation-finish" onclick={finish}>{"提前结束并查看成绩"}</button>
                    </footer>
                </section>
            }
        }
    } else {
        let flags = *enabled;
        html! {
            <section class="mx-auto grid w-full max-w-3xl gap-5" data-testid="calculation-setup">
                <div class="rounded-box border border-base-content/10 bg-base-100 p-5 sm:p-7">
                    <h1 class="text-2xl font-black">{"资料分析 · 速算训练"}</h1>
                    <p class="mt-2 text-sm text-base-content/65">{"正确选项保留真实结果，凭选项前两三位判断，无需心算到最后一位。"}</p>
                    <fieldset class="mt-5">
                        <legend class="mb-3 font-bold">{"计算类型"}</legend>
                        <div class="grid gap-3 sm:grid-cols-3">
                            {for CalculationKind::ALL.into_iter().enumerate().map(|(index, kind)| {
                                let enabled = enabled.clone();
                                html! {
                                    <label class="flex min-h-12 items-center gap-2 rounded-box border border-base-content/10 p-3">
                                        <input type="checkbox" class="checkbox checkbox-primary" data-testid={kind.test_id()} checked={flags[index]} onchange={Callback::from(move |_| {
                                            let mut flags = *enabled;
                                            flags[index] = !flags[index];
                                            enabled.set(flags);
                                        })} />
                                        <span class="text-sm">{kind.label()}</span>
                                    </label>
                                }
                            })}
                        </div>
                    </fieldset>
                    <label class="mt-5 grid max-w-48 gap-2">
                        <span class="font-bold">{"本轮题数"}</span>
                        <select class="select select-bordered w-full" value={count.to_string()} onchange={{
                            let count = count.clone();
                            Callback::from(move |event: Event| {
                                let value = event.target_unchecked_into::<HtmlSelectElement>().value();
                                if let Ok(value @ (10 | 20 | 50)) = value.parse::<usize>() {
                                    count.set(value);
                                }
                            })
                        }}>
                            {for [10, 20, 50].into_iter().map(|value| html! {
                                <option value={value.to_string()} selected={*count == value}>{format!("{value} 题")}</option>
                            })}
                        </select>
                    </label>
                    if !flags.iter().any(|checked| *checked) {
                        <p class="mt-3 text-sm text-error">{"至少选择一种计算类型。"}</p>
                    }
                    <button type="button" class="btn btn-primary mt-6 w-full" data-testid="calculation-start" onclick={start} disabled={!flags.iter().any(|checked| *checked)}>{"开始训练"}</button>
                </div>
            </section>
        }
    };

    let _ = *clock_tick;
    let active_info = session.as_ref().filter(|s| !s.finished).map(|s| (
        s.questions[s.index].kind.label(),
        s.index + 1,
        s.questions.len(),
        ((Date::now() - s.started_question_at).max(0.0) / 1_000.0) as u64,
    ));
    html! {
        <div class={if is_training_immersive { format!("calculation-layout {}",super::training::FIXED_TRAINING_LAYOUT) } else { "calculation-layout min-h-dvh bg-base-200 px-4 py-5 sm:px-8".to_owned() }} data-testid="calculation-layout" oncontextmenu={Callback::from(|event: web_sys::MouseEvent| event.prevent_default())}>
            <div class={if is_training_immersive { "calculation-header mx-auto flex w-full shrink-0 flex-wrap items-center justify-between gap-3 border-b border-base-content/10 pb-2" } else { "calculation-header mx-auto mb-5 flex w-full items-center justify-between" }}>
                <div class="flex min-w-0 items-center gap-3">
                    <button type="button" class="btn btn-ghost btn-sm" data-testid="calculation-back" onclick={back}>
                        {if is_training_immersive { "← 返回专项训练" } else { "← 返回学习中心" }}
                    </button>
                    if let Some((kind,_,_,_)) = active_info {
                        <div class="hidden min-w-0 sm:block">
                            <strong class="block text-sm">{"速算训练"}</strong>
                            <span class="text-xs text-base-content/60">{kind}</span>
                        </div>
                    }
                </div>
                if let Some((_,number,total,elapsed)) = active_info {
                    <div class="flex items-center gap-3" data-testid="calculation-progress-header">
                        <span class="text-xs text-base-content/65">{format!("第 {number}/{total} 题")}</span>
                        <div class="rounded-lg border border-primary/25 bg-base-100 px-3 py-1 text-right">
                            <span class="block text-[0.6rem] text-base-content/60">{"本题已用时"}</span>
                            <strong class="font-mono text-lg font-bold tabular-nums" data-testid="calculation-elapsed">
                                {format!("{:02}:{:02}", elapsed / 60, elapsed % 60)}
                            </strong>
                        </div>
                    </div>
                } else {
                    <span class="text-xs font-semibold uppercase tracking-widest text-base-content/60">{"STUDY / CALCULATION"}</span>
                }
            </div>
            if is_training_immersive {
                <div class="min-h-0 flex-1 overflow-hidden">{content}</div>
            } else {
                {content}
            }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn growth_training_uses_non_anchor_positive_and_negative_rates() {
        let mut seed = 20261020;
        let mut positive = 0;
        let mut negative = 0;
        for _ in 0..500 {
            let question = make_question(&mut seed, CalculationKind::Percent);
            assert!(question.expression.contains("现期量"));
            assert!(question.expression.contains("增长率"));
            assert!(question.expression.contains("增长量"));
            assert!(!question.expression.contains(" ÷ "));
            let rate = question
                .expression
                .split("增长率 ")
                .nth(1)
                .expect("must contain growth rate")
                .split('%')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(rate.abs() > 10.0);
            assert!(rate > -100.0);
            let per_mille = (rate.abs() * 10.0).round() as u64;
            assert!(!is_share_anchor(per_mille), "{}", question.expression);
            assert_eq!(question.is_negative, rate < 0.0);
            assert_eq!(question.exact_result.starts_with('-'), rate < 0.0);
            assert!(question
                .options
                .iter()
                .all(|option| option.starts_with('-') == (rate < 0.0)));
            // 正/负增速统一使用现期 × r/(1+r)，精确答案必须带正确符号。
            let current = question
                .expression
                .split("现期量 ")
                .nth(1)
                .unwrap()
                .split('，')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            let expected = current * (rate / 100.0) / (1.0 + rate / 100.0);
            let actual = question.exact_result.parse::<f64>().unwrap();
            assert!(
                (expected - actual).abs() < 1e-7,
                "{}: expected {expected}, got {actual}",
                question.expression
            );
            if rate < 0.0 {
                negative += 1;
            } else {
                positive += 1;
            }
        }
        assert!(
            positive >= 100 && negative >= 100,
            "{positive} positive, {negative} negative"
        );
    }

    #[test]
    fn all_answer_choices_stay_within_ten_percent() {
        let mut seed = 2026;
        for kind in CalculationKind::ALL {
            for _ in 0..1000 {
                let question = make_question(&mut seed, kind);
                let correct = question.exact_result.parse::<f64>().unwrap();
                for option in &question.options {
                    let value = option.parse::<f64>().unwrap();
                    assert!(
                        (value - correct).abs() < correct.abs() * 0.1,
                        "{} vs {}",
                        value,
                        correct
                    );
                }
            }
        }
    }

    #[test]
    fn every_kind_has_one_exact_answer_and_options_differ_by_three_digits() {
        let mut has_two_digit_question = false;
        let mut has_three_digit_question = false;
        for kind in CalculationKind::ALL {
            let mut seed = 1428;
            for _ in 0..800 {
                let question = make_question(&mut seed, kind);
                assert_eq!(
                    question.options[question.correct_index],
                    question.exact_result
                );
                let magnitude = exact_decimal(question.numerator, question.denominator);
                let expected = if question.is_negative {
                    format!("-{magnitude}")
                } else {
                    magnitude
                };
                assert_eq!(question.exact_result, expected);
                let unique: std::collections::HashSet<_> = question.options.iter().collect();
                assert_eq!(unique.len(), 4);
                let three: std::collections::HashSet<_> = question
                    .options
                    .iter()
                    .map(|value| significant_prefix(value, 3))
                    .collect();
                assert_eq!(three.len(), 4, "{}", question.expression);
                let two: std::collections::HashSet<_> = question
                    .options
                    .iter()
                    .map(|value| significant_prefix(value, 2))
                    .collect();
                has_two_digit_question |= two.len() == 4;
                has_three_digit_question |= two.len() != 4;
                assert!(question
                    .options
                    .iter()
                    .all(|value| value.starts_with('-') == question.is_negative));
                if kind == CalculationKind::Percent {
                    assert!(question.expression.contains('%'));
                } else {
                    assert!(!question.is_negative);
                }
            }
        }
        assert!(has_two_digit_question);
        assert!(has_three_digit_question);
    }

    #[test]
    fn exact_results_are_not_truncated_or_rounded() {
        assert_eq!(exact_decimal(438 * 27, 1), "11826");
        assert_eq!(exact_decimal(438 * 13, 100), "56.94");
        assert_eq!(exact_decimal(438 * 100, 120), "365");
        assert_eq!(exact_decimal(5, 4), "1.25");
        assert_eq!(exact_decimal_parts(736, 24), None);
        assert_eq!(exact_decimal_parts(12312, 100), Some((12312, 2)));
        assert_eq!(significant_prefix("123.12", 3), "123");
        assert_eq!(significant_prefix("125.45", 3), "125");
        assert_eq!(significant_prefix("121.22", 3), "121");
    }

    #[test]
    fn correct_choice_is_not_pinned_to_a_position() {
        let mut seed = 19;
        let mut positions = [0; 4];
        for _ in 0..256 {
            let question = make_question(&mut seed, CalculationKind::Division);
            positions[question.correct_index] += 1;
        }
        assert!(positions.into_iter().all(|count| count >= 30));
    }

    #[test]
    fn every_enabled_kind_is_included_and_percent_only_is_possible() {
        let mut seed = 123;
        let questions = question_set(&mut seed, &[CalculationKind::Percent], 100);
        assert!(questions
            .iter()
            .all(|question| question.kind == CalculationKind::Percent));
        let all = question_set(&mut seed, &CalculationKind::ALL, 20);
        for kind in CalculationKind::ALL {
            assert!(all.iter().any(|question| question.kind == kind));
        }
    }
}
