//! 资料分析四选一速算。题目、判题均在浏览器内完成，不修改 Flashcard/设备数据。
use super::*;
use js_sys::Date;

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
            Self::Percent => "百分式",
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
            Self::Division => "分子取前三位、分母取前两位试商；依据选项间距决定是否补下一位。",
            Self::Multiplication => "从高位拆乘，先确定量级，再根据选项间距补位。",
            Self::Percent => "以 10% 和 1% 为锚点拆算；能从选项定界就及时停止。",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GapMode {
    Wide,
    Narrow,
    Mixed,
}

impl GapMode {
    fn label(self) -> &'static str {
        match self {
            Self::Wide => "大间距",
            Self::Narrow => "小间距",
            Self::Mixed => "随机间距",
        }
    }

    fn test_id(self) -> &'static str {
        match self {
            Self::Wide => "calc-gap-wide",
            Self::Narrow => "calc-gap-small",
            Self::Mixed => "calc-gap-mixed",
        }
    }
}

#[derive(Clone, Debug)]
struct CalculationQuestion {
    expression: String,
    options: [String; 4],
    correct_index: usize,
    rounded_result: String,
    kind: CalculationKind,
    gap: GapMode,
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

// 有理数 num/den 的最高有效数字数量级（10^order），纯整数运算。
fn order_of_magnitude(num: u64, den: u64) -> i32 {
    assert!(num > 0 && den > 0);
    let mut order = 0;
    if num >= den {
        let mut divisor = den;
        while divisor <= num / 10 {
            divisor *= 10;
            order += 1;
        }
    } else {
        let mut numerator = num;
        while numerator < den {
            numerator *= 10;
            order -= 1;
        }
    }
    order
}

// 用精确整数分数进行四舍五入；不把浮点近似值用于正确项判定。
fn rounded_units(num: u64, den: u64, exponent: i32) -> i64 {
    if exponent >= 0 {
        let divisor = den * power_ten(exponent as u32);
        ((num + divisor / 2) / divisor) as i64
    } else {
        let scaled = num * power_ten((-exponent) as u32);
        ((scaled + den / 2) / den) as i64
    }
}

fn format_units(units: i64, exponent: i32) -> String {
    if exponent >= 0 {
        (units * power_ten(exponent as u32) as i64).to_string()
    } else {
        let places = (-exponent) as usize;
        let scale = power_ten(places as u32) as i64;
        format!(
            "{}.{:0width$}",
            units / scale,
            units % scale,
            width = places
        )
    }
}

fn make_question(
    seed: &mut u64,
    kind: CalculationKind,
    requested_gap: GapMode,
) -> CalculationQuestion {
    let a = random_range(seed, 205, 985);
    let b = random_range(seed, 12, 87);
    let p = random_range(seed, 5, 42);
    let (expression, numerator, denominator) = match kind {
        CalculationKind::Division => (format!("{a} ÷ {b}"), a, b),
        CalculationKind::Multiplication => (format!("{a} × {b}"), a * b, 1),
        CalculationKind::Percent => match random_range(seed, 0, 2) {
            0 => (format!("{a} × {p}%"), a * p, 100),
            1 => (format!("{a} ÷ (1 + {p}%)"), a * 100, 100 + p),
            _ => (format!("{a} × {p}% ÷ (1 + {p}%)"), a * p, 100 + p),
        },
    };
    let gap = if requested_gap == GapMode::Mixed {
        if random_range(seed, 0, 1) == 0 {
            GapMode::Wide
        } else {
            GapMode::Narrow
        }
    } else {
        requested_gap
    };
    // 所有选项同单位、同小数位。小间距约为第三位有效数间隔，
    // 大间距扩大十倍，不向考生提示“算几位”。
    let exponent = order_of_magnitude(numerator, denominator) - 2;
    let correct_units = rounded_units(numerator, denominator, exponent);
    let separation = if gap == GapMode::Wide { 10 } else { 1 };
    let left = random_range(seed, 0, 3) as i64;
    let mut candidates: [(i64, String); 4] = std::array::from_fn(|i| {
        let units = correct_units + (i as i64 - left) * separation;
        (units, format_units(units, exponent))
    });
    // 均匀洗牌，不能把正确答案固定在中间或固定选项。
    for i in (1..4).rev() {
        let j = random_range(seed, 0, i as u64) as usize;
        candidates.swap(i, j);
    }
    let correct_index = candidates
        .iter()
        .position(|(units, _)| *units == correct_units)
        .expect("正确答案必须在四个选项中");
    let options = candidates.map(|(_, label)| label);
    CalculationQuestion {
        expression,
        options,
        correct_index,
        rounded_result: format_units(correct_units, exponent),
        kind,
        gap,
        tip: kind.tip(),
    }
}

fn question_set(
    seed: &mut u64,
    kinds: &[CalculationKind],
    gap: GapMode,
    count: usize,
) -> Vec<CalculationQuestion> {
    let mut questions: Vec<_> = (0..count)
        .map(|index| {
            let kind = if index < kinds.len() {
                kinds[index]
            } else {
                kinds[random_range(seed, 0, (kinds.len() - 1) as u64) as usize]
            };
            make_question(seed, kind, gap)
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
}

pub(crate) fn render_calculation_training(route_state: UseStateHandle<PortalRoute>) -> Html {
    html! { <CalculationTraining route_state={route_state} /> }
}

#[function_component(CalculationTraining)]
fn calculation_training(props: &CalculationTrainingProps) -> Html {
    let enabled = use_state(|| [true, true, true]);
    let gap_mode = use_state(|| GapMode::Mixed);
    let count = use_state(|| 20_usize);
    let session = use_state(|| None::<CalculationSession>);

    let back = {
        let route_state = props.route_state.clone();
        Callback::from(move |_| {
            navigate_route(
                &route_state,
                PortalRoute::study(StudyView::Dashboard, None, Vec::new()),
                false,
            )
        })
    };
    let restart = {
        let session = session.clone();
        Callback::from(move |_| session.set(None))
    };
    let start = {
        let enabled = *enabled;
        let gap = *gap_mode;
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
            let questions = question_set(&mut seed, &kinds, gap, count);
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
            let render_gap = |gap: GapMode| {
                let subset: Vec<_> = answered
                    .iter()
                    .filter(|(index, _)| live.questions[*index].gap == gap)
                    .collect();
                let right = subset
                    .iter()
                    .filter(|(index, answer)| answer.choice == live.questions[*index].correct_index)
                    .count();
                let ms: u64 = subset
                    .iter()
                    .map(|(_, answer)| answer.elapsed_ms as u64)
                    .sum();
                html! {
                    <div class="rounded-box border border-base-content/10 bg-base-200/40 p-3">
                        <p class="text-sm text-base-content/65">{gap.label()}</p>
                        <p class="font-semibold">{format!("{} / {} 正确", right, subset.len())}</p>
                        <p class="text-xs text-base-content/60">
                            {if subset.is_empty() { "暂无数据".to_owned() } else { format!("平均 {:.1} 秒", ms as f64 / subset.len() as f64 / 1000.0) }}
                        </p>
                    </div>
                }
            };
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
                        <div class="mt-4 grid grid-cols-2 gap-3">{render_gap(GapMode::Wide)}{render_gap(GapMode::Narrow)}</div>
                    </div>
                    <div class="grid gap-3">
                        <h2 class="text-lg font-bold">{"错题回看"}</h2>
                        {for answered.iter().filter(|(index, answer)| answer.choice != live.questions[*index].correct_index).map(|(index, answer)| {
                            let question = &live.questions[*index];
                            html! {
                                <article class="rounded-box border border-base-content/10 bg-base-100 p-4">
                                    <p class="font-semibold">{format!("第 {} 题 · {}", index + 1, question.expression)}</p>
                                    <p class="mt-2 text-sm">{format!("你的选择：{}　正确选项：{}", question.options[answer.choice], question.rounded_result)}</p>
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
                        <button type="button" class="btn btn-ghost" onclick={back.clone()}>{"返回学习"}</button>
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
                <section class="mx-auto grid w-full max-w-3xl gap-4" data-testid="calculation-question">
                    <div class="flex items-center justify-between gap-3 text-sm text-base-content/60">
                        <span>{format!("第 {progress} / {} 题", live.questions.len())}</span>
                        <span>{question.kind.label()}</span>
                    </div>
                    <div class="grid min-h-36 place-items-center rounded-box border border-base-content/10 bg-base-100 p-5 text-center">
                        <h1 class="break-words text-3xl font-black leading-snug sm:text-5xl" data-testid="calculation-expression">
                            {format!("{} ≈ ?", question.expression)}
                        </h1>
                    </div>
                    <div class="grid grid-cols-2 gap-3">{for buttons}</div>
                    if let Some(record) = answered {
                        <div class="rounded-box border border-base-content/10 bg-base-100 p-4" role="status" data-testid="calculation-feedback">
                            <strong class={if record.choice == question.correct_index { "text-success" } else { "text-error" }}>
                                {if record.choice == question.correct_index { "回答正确" } else { "回答错误" }}
                            </strong>
                            <p class="mt-1">{format!("正确答案：{}；用时 {:.1} 秒", question.rounded_result, record.elapsed_ms as f64 / 1000.0)}</p>
                            <p class="mt-1 text-sm text-base-content/65">{question.tip}</p>
                        </div>
                        <button type="button" class="btn btn-primary w-full" data-testid="calculation-next" onclick={next}>
                            {if progress == live.questions.len() { "查看结果" } else { "下一题" }}
                        </button>
                    }
                    <button type="button" class="btn btn-ghost justify-self-center" data-testid="calculation-finish" onclick={finish}>{"提前结束并查看成绩"}</button>
                </section>
            }
        }
    } else {
        let flags = *enabled;
        let gap = *gap_mode;
        html! {
            <section class="mx-auto grid w-full max-w-3xl gap-5" data-testid="calculation-setup">
                <div class="rounded-box border border-base-content/10 bg-base-100 p-5 sm:p-7">
                    <h1 class="text-2xl font-black">{"资料分析 · 速算训练"}</h1>
                    <p class="mt-2 text-sm text-base-content/65">{"看题、心算、四选一。精度由选项区分度决定，不要求输入精确结果。"}</p>
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
                    <fieldset class="mt-5">
                        <legend class="mb-3 font-bold">{"选项间距"}</legend>
                        <div class="grid grid-cols-3 gap-2">
                            {for [GapMode::Wide, GapMode::Narrow, GapMode::Mixed].into_iter().map(|mode| {
                                let gap_mode = gap_mode.clone();
                                html! {
                                    <button
                                        type="button"
                                        class={if gap == mode { "btn btn-primary" } else { "btn btn-outline" }}
                                        data-testid={mode.test_id()}
                                        aria-pressed={(gap == mode).to_string()}
                                        onclick={Callback::from(move |_| gap_mode.set(mode))}
                                    >{mode.label()}</button>
                                }
                            })}
                        </div>
                        <p class="mt-2 text-xs text-base-content/60">{"大间距如 1.23 / 1.33；小间距如 123 / 124；随机只变选项间距。"}</p>
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

    html! {
        <div class="min-h-dvh bg-base-200 px-4 py-5 sm:px-8" data-testid="calculation-layout">
            <div class="mx-auto mb-5 flex w-full max-w-3xl items-center justify-between">
                <button type="button" class="btn btn-ghost btn-sm" data-testid="calculation-back" onclick={back}>{"← 返回学习"}</button>
                <span class="text-xs font-semibold uppercase tracking-widest text-base-content/60">{"STUDY / CALCULATION"}</span>
            </div>
            {content}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_and_gap_has_exactly_one_rounded_answer() {
        for kind in CalculationKind::ALL {
            for gap in [GapMode::Wide, GapMode::Narrow, GapMode::Mixed] {
                let mut seed = 7;
                for _ in 0..1000 {
                    let question = make_question(&mut seed, kind, gap);
                    let unique: std::collections::HashSet<_> = question.options.iter().collect();
                    assert_eq!(unique.len(), 4);
                    assert_eq!(
                        question
                            .options
                            .iter()
                            .filter(|v| **v == question.rounded_result)
                            .count(),
                        1
                    );
                    assert_eq!(
                        question.options[question.correct_index],
                        question.rounded_result
                    );
                    assert!(question.options.iter().all(|v| !v.starts_with('-')));
                    if kind == CalculationKind::Percent {
                        assert!(question.expression.contains('%'));
                    }
                }
            }
        }
    }

    #[test]
    fn exact_rounding_does_not_depend_on_float_or_option_position() {
        assert_eq!(rounded_units(387, 31, -1), 125);
        assert_eq!(format_units(125, -1), "12.5");
        assert_eq!(rounded_units(438 * 27, 1, 2), 118);
        assert_eq!(format_units(118, 2), "11800");
        assert_eq!(rounded_units(438 * 13, 100, 0), 57);
        let mut seed = 19;
        let mut positions = [0; 4];
        for _ in 0..256 {
            let question = make_question(&mut seed, CalculationKind::Division, GapMode::Narrow);
            positions[question.correct_index] += 1;
        }
        assert!(positions.into_iter().all(|n| n >= 30));
    }

    #[test]
    fn percent_only_questions_and_mixed_gaps_are_supported() {
        let mut seed = 123;
        let questions = question_set(&mut seed, &[CalculationKind::Percent], GapMode::Mixed, 100);
        assert!(questions.iter().all(|q| q.kind == CalculationKind::Percent));
        assert!(questions.iter().any(|q| q.gap == GapMode::Wide));
        assert!(questions.iter().any(|q| q.gap == GapMode::Narrow));
        let all = question_set(&mut seed, &CalculationKind::ALL, GapMode::Mixed, 20);
        for kind in CalculationKind::ALL {
            assert!(all.iter().any(|q| q.kind == kind));
        }
    }
    #[test]
    fn exact_answers_keep_every_naturally_required_digit() {
        assert_eq!(exact_decimal(438 * 27, 1), "11826");
        assert_eq!(exact_decimal(438 * 13, 100), "56.94");
        assert_eq!(exact_decimal(438 * 100, 120), "365");
        assert_eq!(exact_decimal(5, 4), "1.25");
    }

    #[test]
    fn all_distractors_are_distinguishable_within_three_leading_digits() {
        let mut has_two_digit_question = false;
        let mut has_three_digit_question = false;
        for kind in CalculationKind::ALL {
            let mut seed = 1428;
            for _ in 0..800 {
                let question = make_question(&mut seed, kind);
                assert_eq!(question.options[question.correct_index], question.exact_result);
                assert_eq!(
                    question.exact_result,
                    exact_decimal(question.numerator, question.denominator)
                );
                let prefixes: std::collections::HashSet<_> = question
                    .options
                    .iter()
                    .map(|value| significant_prefix(value, 3))
                    .collect();
                assert_eq!(prefixes.len(), 4, "{}", question.expression);
                let two: std::collections::HashSet<_> = question
                    .options
                    .iter()
                    .map(|value| significant_prefix(value, 2))
                    .collect();
                has_two_digit_question |= two.len() == 4;
                has_three_digit_question |= two.len() != 4;
                assert!(!question.expression.contains("≈"));
            }
        }
        assert!(has_two_digit_question);
        assert!(has_three_digit_question);
    }

}
