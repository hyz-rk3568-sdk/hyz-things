//! Five-question data lookup drills using the same validated materials as comprehensive training.
use super::*;
use gloo_timers::future::TimeoutFuture;
use hyz_things::domain::data_analysis::Material;
use js_sys::Date;
use serde::{Deserialize, Serialize};

const LOCATING_ROOT: &str = "/api/v1/study/data-analysis/locating";
const LOCATING_ACTIVE_KEY: &str = "hyz_locating_active_run";

#[derive(Properties, PartialEq)]
pub(crate) struct LocatingProps {
    pub csrf: String,
}

#[derive(Clone, Debug, Deserialize)]
struct LocatingTopicDto {
    topic: String,
    subtopics: Vec<String>,
}

#[derive(Serialize)]
struct LocatingStartDto {
    topic: Option<String>,
    subtopic: Option<String>,
    allow_review: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct LocatingQuestionDto {
    question: String,
}

#[derive(Clone, Debug, Deserialize)]
struct LocatingAnswerDto {
    correct: bool,
    elapsed_ms: u32,
    selected_start: usize,
    expected_start: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct LocatingRunDto {
    id: String,
    status: String,
    material: Material,
    questions: Vec<LocatingQuestionDto>,
    answers: Vec<Option<LocatingAnswerDto>>,
}

#[derive(Serialize)]
struct LocatingClickDto {
    start: usize,
    end: usize,
    elapsed_ms: u32,
}

fn remember_locating_run(id: Option<&str>) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        match id {
            Some(id) => {
                let _ = storage.set_item(LOCATING_ACTIVE_KEY, id);
            }
            None => {
                let _ = storage.remove_item(LOCATING_ACTIVE_KEY);
            }
        }
    }
}

/// Return byte-offset spans so even identical numeric strings in separate
/// paragraphs are independently clickable and scored by their exact position.
fn number_parts(text: &str) -> Vec<(usize, usize, bool)> {
    let bytes = text.as_bytes();
    let mut output = Vec::new();
    let mut cursor = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            if cursor < index {
                output.push((cursor, index, false));
            }
            let start = index;
            index += 1;
            while index < bytes.len() {
                if bytes[index].is_ascii_digit() {
                    index += 1;
                } else if (bytes[index] == b'.' || bytes[index] == b',')
                    && index + 1 < bytes.len()
                    && bytes[index + 1].is_ascii_digit()
                {
                    index += 1;
                } else {
                    break;
                }
            }
            output.push((start, index, true));
            cursor = index;
        } else {
            index += 1;
        }
    }
    if cursor < text.len() {
        output.push((cursor, text.len(), false));
    }
    output
}

fn material_with_numbers(text: &str, on_digit: Callback<(usize, usize)>, disabled: bool) -> Html {
    html! {
        <div class="whitespace-pre-wrap text-base leading-8">
            {for number_parts(text).into_iter().map(|(start,end,numeric)|{
                let value=text[start..end].to_owned();
                if numeric {
                    let callback=on_digit.clone();
                    html! {
                        <button type="button" class="inline rounded-sm px-px text-inherit underline decoration-dotted decoration-base-content/25 underline-offset-4 hover:bg-primary/15 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary" data-testid="locating-number"
                            disabled={disabled} onclick={Callback::from(move |_|callback.emit((start,end)))}>
                            {value}
                        </button>
                    }
                } else {
                    html!{<span>{value}</span>}
                }
            })}
        </div>
    }
}

#[function_component(LocatingTraining)]
pub(crate) fn locating_training(props: &LocatingProps) -> Html {
    let topics = use_state(Vec::<LocatingTopicDto>::new);
    let topic = use_state(String::new);
    let subtopic = use_state(String::new);
    let review = use_state(|| false);
    let run = use_state(|| None::<LocatingRunDto>);
    let progress = use_state(|| None::<PrepareProgress>);
    let loading = use_state(|| false);
    let error = use_state(|| None::<String>);
    let started_at = use_state(Date::now);

    {
        let topics = topics.clone();
        let run = run.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                if let Ok(value) = fetch_json_typed::<Vec<LocatingTopicDto>>(
                    &format!("{LOCATING_ROOT}/topics"),
                    "找数主题",
                )
                .await
                {
                    topics.set(value);
                }
                if let Some(id) = web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|s| s.get_item(LOCATING_ACTIVE_KEY).ok().flatten())
                {
                    if let Ok(value) = fetch_json_typed::<LocatingRunDto>(
                        &format!("{LOCATING_ROOT}/runs/{id}"),
                        "恢复找数训练",
                    )
                    .await
                    {
                        run.set(Some(value));
                    } else {
                        remember_locating_run(None);
                    }
                }
            });
            || ()
        });
    }

    let start = {
        let run = run.clone();
        let progress = progress.clone();
        let error = error.clone();
        let loading = loading.clone();
        let started_at = started_at.clone();
        let csrf = props.csrf.clone();
        let topic = topic.clone();
        let subtopic = subtopic.clone();
        let review = review.clone();
        Callback::from(move |_| {
            if *loading {
                return;
            }
            loading.set(true);
            error.set(None);
            progress.set(None);
            let query = LocatingStartDto {
                topic: (!topic.is_empty()).then(|| (*topic).clone()),
                subtopic: (!subtopic.is_empty()).then(|| (*subtopic).clone()),
                allow_review: *review,
            };
            let run = run.clone();
            let progress = progress.clone();
            let error = error.clone();
            let loading = loading.clone();
            let csrf = csrf.clone();
            let started_at = started_at.clone();
            spawn_local(async move {
                let task = post_json_response_typed::<_, PrepareTask>(
                    &format!("{LOCATING_ROOT}/prepare"),
                    &csrf,
                    &query,
                    "准备找数训练",
                )
                .await;
                let outcome = match task {
                    Err(e) => Err(e.to_string()),
                    Ok(task) => {
                        let mut answer = Err("找数材料准备超时".to_owned());
                        for _ in 0..800 {
                            TimeoutFuture::new(1500).await;
                            match fetch_json_typed::<PrepareProgress>(
                                &format!("{ANALYSIS_ROOT}/prepare/{}", task.job_id),
                                "找数出题进度",
                            )
                            .await
                            {
                                Err(e) => {
                                    answer = Err(e.to_string());
                                    break;
                                }
                                Ok(state) => {
                                    progress.set(Some(state.clone()));
                                    if state.state == "failed" {
                                        answer = Err(state.message);
                                        break;
                                    }
                                    if state.state == "ready" {
                                        if let Some(id) = state.run_id {
                                            answer = fetch_json_typed::<LocatingRunDto>(
                                                &format!("{LOCATING_ROOT}/runs/{id}"),
                                                "开始找数训练",
                                            )
                                            .await
                                            .map_err(|e| e.to_string());
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        answer
                    }
                };
                match outcome {
                    Ok(live) => {
                        remember_locating_run(Some(&live.id));
                        started_at.set(Date::now());
                        run.set(Some(live));
                    }
                    Err(e) => error.set(Some(e)),
                }
                loading.set(false);
            });
        })
    };

    let submit = {
        let run = run.clone();
        let loading = loading.clone();
        let error = error.clone();
        let csrf = props.csrf.clone();
        let started_at = started_at.clone();
        Callback::from(move |(start, end): (usize, usize)| {
            if *loading {
                return;
            }
            let Some(active) = (*run).clone() else { return };
            let Some(index) = active.answers.iter().position(Option::is_none) else {
                return;
            };
            if index >= active.questions.len() || active.status != "active" {
                return;
            }
            let elapsed = (Date::now() - *started_at).max(0.0).min(300_000.0) as u32;
            loading.set(true);
            let run = run.clone();
            let loading = loading.clone();
            let error = error.clone();
            let csrf = csrf.clone();
            let started_at = started_at.clone();
            spawn_local(async move {
                let endpoint = format!("{LOCATING_ROOT}/runs/{}/answers/{}", active.id, index + 1);
                match post_json_response_typed::<_, LocatingRunDto>(
                    &endpoint,
                    &csrf,
                    &LocatingClickDto {
                        start,
                        end,
                        elapsed_ms: elapsed,
                    },
                    "提交数字定位",
                )
                .await
                {
                    Ok(live) => {
                        if live.status == "complete" {
                            remember_locating_run(None);
                        }
                        started_at.set(Date::now());
                        run.set(Some(live));
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                loading.set(false);
            });
        })
    };

    if let Some(live) = (*run).clone() {
        if live.questions.len() != 5 || live.answers.len() != 5 {
            return html! {<p class="p-5 text-error">{"服务器返回的找数题量无效"}</p>};
        }
        let current = live.answers.iter().position(Option::is_none);
        if current.is_none() || live.status == "complete" {
            let correct = live.answers.iter().flatten().filter(|a| a.correct).count();
            let restart = {
                let run = run.clone();
                let progress = progress.clone();
                let error = error.clone();
                Callback::from(move |_| {
                    remember_locating_run(None);
                    run.set(None);
                    progress.set(None);
                    error.set(None);
                })
            };
            return html! {
                <section class="mx-auto max-w-3xl space-y-4 p-5" data-testid="locating-result">
                    <div class="rounded-box bg-base-100 p-5">
                        <h2 class="text-xl font-bold">{"找数训练成绩"}</h2>
                        <p class="mt-2 text-3xl font-black">{format!("{correct}/5 题定位正确")}</p>
                        <p class="text-sm text-base-content/60">{"这份材料已计入曝光记录；下次新材料训练不会重复抽取同一统计章节。"}</p>
                    </div>
                    <div class="rounded-box bg-base-100 p-4">
                        {for live.answers.iter().enumerate().filter_map(|(i,answer)|answer.as_ref().map(|a|{
                            html!{
                                <div class="flex items-center justify-between gap-3 border-b border-base-content/10 py-3" key={i}>
                                    <span class="text-sm">{format!("第{}题 · {}",i+1,if a.correct{"定位正确"}else{"定位错误"})}</span>
                                    <span class="font-mono text-sm">{format!("{:.1} 秒",a.elapsed_ms as f64/1000.0)}</span>
                                </div>
                            }
                        }))}
                    </div>
                    <button class="btn btn-primary" onclick={restart}>{"再练一篇"}</button>
                </section>
            };
        }
        let index = current.unwrap_or(0);
        return html! {
            <section class="mx-auto flex h-[calc(100dvh-2rem)] max-w-[1500px] flex-col px-3 py-2 sm:px-5" data-testid="locating-run">
                <header class="mb-3 flex flex-wrap items-center justify-between gap-2 border-b border-base-content/10 pb-3">
                    <strong>{format!("资料分析 · 找数训练　 第 {}/5 题",index+1)}</strong>
                    <span class="text-sm text-base-content/60">{"只找数，不计算；直接点击原文数字"}</span>
                </header>
                <div class="grid min-h-0 flex-1 grid-cols-1 gap-3 lg:grid-cols-[56%_minmax(0,1fr)]">
                    <article class="min-h-0 overflow-auto rounded-box bg-base-100 p-4" data-testid="locating-material">
                        <div class="mb-3 text-sm font-semibold">
                            {format!("{} · {}",live.material.topic,live.material.subtopic)}
                        </div>
                        {material_with_numbers(&live.material.text,submit,*loading)}
                        <a class="mt-5 block text-xs underline" href={live.material.source_url.clone()}
                            target="_blank" rel="noopener noreferrer">{"国家统计局 · 查看官方来源"}</a>
                    </article>
                    <aside class="min-h-0 overflow-auto rounded-box bg-base-100 p-5">
                        <div class="text-xs text-base-content/60">{format!("本材料第 {} 题",index+1)}</div>
                        <h2 class="mt-3 text-lg font-bold leading-relaxed">{live.questions[index].question.clone()}</h2>
                        <p class="mt-4 text-sm text-base-content/65">{"根据年份、主体和指标定位数字，点击材料中的数字即可提交。"}</p>
                        <div class="mt-5 space-y-2 text-xs text-base-content/55">
                            {for live.answers.iter().take(index).enumerate().filter_map(|(i,a)|a.as_ref().map(|answer|{
                                html!{<p>{format!("第{}题：{} · {:.1}秒",i+1,if answer.correct{"正确"}else{"错误"},answer.elapsed_ms as f64/1000.0)}</p>}
                            }))}
                        </div>
                        {if let Some(message)=(*error).clone(){html!{<p class="mt-3 text-sm text-error">{message}</p>}}else{Html::default()}}
                    </aside>
                </div>
            </section>
        };
    }
    let matching_subtopics = topics
        .iter()
        .find(|item| item.topic == *topic)
        .map(|item| item.subtopics.clone())
        .unwrap_or_default();
    let change_topic = {
        let topic = topic.clone();
        let subtopic = subtopic.clone();
        Callback::from(move |event: Event| {
            topic.set(event.target_unchecked_into::<HtmlSelectElement>().value());
            subtopic.set(String::new());
        })
    };
    let change_subtopic = {
        let subtopic = subtopic.clone();
        Callback::from(move |event: Event| {
            subtopic.set(event.target_unchecked_into::<HtmlSelectElement>().value())
        })
    };
    let change_review = {
        let review = review.clone();
        Callback::from(move |event: Event| {
            review.set(event.target_unchecked_into::<HtmlInputElement>().checked())
        })
    };
    html! {
        <section class="mx-auto max-w-3xl p-4 sm:p-6" data-testid="locating-setup">
            <div class="rounded-box bg-base-100 p-5 sm:p-7">
                <h2 class="text-xl font-bold">{"资料分析 · 找数专项"}</h2>
                <p class="mt-1 text-sm text-base-content/65">{"官方数据经 AI 改写校验 · 每篇5题 · 建立并复用数据地图"}</p>
                <div class="mt-5 grid gap-4">
                    <label class="grid gap-2 text-sm">
                        <span>{"大主题"}</span>
                        <select class="select select-bordered w-full" value={(*topic).clone()} onchange={change_topic}>
                            <option value="">{"全部主题"}</option>
                            {for topics.iter().map(|item|html!{
                                <option value={item.topic.clone()}>{item.topic.clone()}</option>
                            })}
                        </select>
                    </label>
                    <label class="grid gap-2 text-sm">
                        <span>{"小主题"}</span>
                        <select class="select select-bordered w-full" value={(*subtopic).clone()} onchange={change_subtopic}>
                            <option value="">{"全部小主题"}</option>
                            {for matching_subtopics.iter().map(|value|html!{<option value={value.clone()}>{value.clone()}</option>})}
                        </select>
                    </label>
                    <label class="flex items-center gap-3 text-sm">
                        <input type="checkbox" class="checkbox checkbox-primary" checked={*review} onchange={change_review} />
                        <span>{"复练已见过的材料（默认关闭）"}</span>
                    </label>
                    <p class="text-xs text-base-content/60">
                        {"新材料模式跨综合训练与找数训练去重；如已用完会明确提示，不会悄悄重复。" }
                    </p>
                    {if let Some(state)=(*progress).clone(){
                        html!{<p class="rounded-box bg-base-200 p-3 text-sm">{format!("{} · {}",state.state,state.message)}</p>}
                    }else{Html::default()}}
                    {if let Some(message)=(*error).clone(){html!{<p class="text-sm text-error">{message}</p>}}else{Html::default()}}
                    <button class="btn btn-primary w-full" disabled={*loading || props.csrf.is_empty()} onclick={start}>
                        {if *loading{"正在准备找数材料…"}else{"开始5题找数训练"}}
                    </button>
                </div>
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_values_still_have_distinct_byte_offsets() {
        let text = "A产量20亿元，B产量20亿元。";
        let numbers: Vec<_> = number_parts(text).into_iter().filter(|p| p.2).collect();
        assert_eq!(numbers.len(), 2);
        assert_ne!(numbers[0].0, numbers[1].0);
        assert_eq!(&text[numbers[0].0..numbers[0].1], "20");
    }
}
