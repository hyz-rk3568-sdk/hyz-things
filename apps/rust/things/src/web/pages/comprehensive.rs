//! 20 questions / 20 minutes. iPad-first 44:56 reading and answer panes.
use super::*;
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use js_sys::Date;
use serde::Serialize;

#[derive(Properties, PartialEq)]
pub(crate) struct ComprehensiveProps {
    pub csrf: String,
}

const COMPREHENSIVE_JOB_KEY: &str = "hyz_comprehensive_prepare_job";
const COMPREHENSIVE_READY_KEY: &str = "hyz_comprehensive_ready_run";
const MANUAL_SKILLS_KEY: &str = "hyz_comprehensive_manual_skills";

#[derive(Serialize)]
struct ComprehensivePrepareRequest {
    manual_skills: Vec<String>,
}

fn remember_preparation(id: Option<&str>) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        match id {
            Some(id) => {
                let _ = storage.set_item(COMPREHENSIVE_JOB_KEY, id);
            }
            None => {
                let _ = storage.remove_item(COMPREHENSIVE_JOB_KEY);
            }
        }
    }
}

async fn resume_preparation(
    id: String,
    progress: UseStateHandle<Option<PrepareProgress>>,
    ready_id: UseStateHandle<Option<String>>,
    preparing: UseStateHandle<bool>,
    error: UseStateHandle<Option<String>>,
) {
    preparing.set(true);
    for _ in 0..800 {
        match fetch_json_typed::<PrepareProgress>(
            &format!("{ANALYSIS_ROOT}/prepare/{id}"),
            "恢复综合训练出题进度",
        )
        .await
        {
            Ok(state) => {
                progress.set(Some(state.clone()));
                if state.state == "ready" {
                    if let Some(run_id) = state.run_id {
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item(COMPREHENSIVE_READY_KEY, &run_id);
                        }
                        remember_preparation(None);
                        ready_id.set(Some(run_id));
                        preparing.set(false);
                        return;
                    }
                    remember_preparation(None);
                    error.set(Some("出题完成但没有找到可进入的训练，请重新出题".into()));
                    preparing.set(false);
                    return;
                }
                if state.state == "failed" {
                    remember_preparation(None);
                    error.set(Some(state.message));
                    preparing.set(false);
                    return;
                }
            }
            Err(e) => {
                remember_preparation(None);
                preparing.set(false);
                error.set(Some(format!("恢复出题任务失败：{e}")));
                return;
            }
        }
        TimeoutFuture::new(1500).await;
    }
    preparing.set(false);
    error.set(Some("出题等待超时，可重新打开页面检查任务进度".into()));
}

#[function_component(ComprehensiveTraining)]
pub(crate) fn comprehensive_training(props: &ComprehensiveProps) -> Html {
    let status = use_state(|| None::<AnalysisStatus>);
    let progress = use_state(|| None::<PrepareProgress>);
    let run = use_state(|| None::<AnalysisRun>);
    let result = use_state(|| None::<AnalysisResult>);
    let current = use_state(|| 0usize);
    let busy = use_state(|| false);
    let preparing = use_state(|| false);
    let ready_id = use_state(|| None::<String>);
    let loading_status = use_state(|| true);
    let error = use_state(|| None::<String>);
    let tick = use_state(|| 0_u64);
    let history = use_state(Vec::<ComprehensiveHistoryEntry>::new);
    let history_open = use_state(|| false);
    let manual_skills = use_state(Vec::<String>::new);
    let skills_expanded = use_state(|| false);

    {
        let status = status.clone();
        let error = error.clone();
        let run = run.clone();
        let history = history.clone();
        let progress = progress.clone();
        let ready_id = ready_id.clone();
        let preparing = preparing.clone();
        let loading_status = loading_status.clone();
        let manual_skills = manual_skills.clone();
        use_effect_with((), move |_| {
            if let Some(raw) = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .and_then(|storage| storage.get_item(MANUAL_SKILLS_KEY).ok().flatten())
            {
                if let Ok(saved) = serde_json::from_str::<Vec<String>>(&raw) {
                    manual_skills.set(saved);
                }
            }
            let history = history.clone();
            spawn_local(async move {
                let status_result = fetch_json_typed::<AnalysisStatus>(
                    &format!("{ANALYSIS_ROOT}/status"),
                    "综合训练状态",
                )
                .await;
                let server_job = status_result
                    .as_ref()
                    .ok()
                    .and_then(|s| s.preparing_job_id.clone());
                match status_result {
                    Ok(v) => status.set(Some(v)),
                    Err(e) => error.set(Some(format!("{e}；综合出题需要管理员登录"))),
                }
                let storage = web_sys::window().and_then(|w| w.local_storage().ok().flatten());
                let saved_ready = storage
                    .as_ref()
                    .and_then(|s| s.get_item(COMPREHENSIVE_READY_KEY).ok().flatten());
                let saved_job = storage
                    .as_ref()
                    .and_then(|s| s.get_item(COMPREHENSIVE_JOB_KEY).ok().flatten());
                // A ready run is persisted in SQLite: even a backend restart
                // cannot accidentally start the timer or lose this prepared set.
                if let Some(id) = saved_ready {
                    ready_id.set(Some(id));
                } else if let Some(job) = saved_job.or(server_job) {
                    remember_preparation(Some(&job));
                    spawn_local(resume_preparation(
                        job,
                        progress,
                        ready_id,
                        preparing,
                        error.clone(),
                    ));
                }
                loading_status.set(false);
                if let Ok(items) = fetch_json_typed::<Vec<ComprehensiveHistoryEntry>>(
                    &format!("{ANALYSIS_ROOT}/history"),
                    "综合训练历史",
                )
                .await
                {
                    history.set(items);
                }
                // Restore a started run after iPad Safari reload.
                if let Some(id) = web_sys::window()
                    .and_then(|w| w.local_storage().ok().flatten())
                    .and_then(|s| s.get_item("hyz_comprehensive_active_run").ok().flatten())
                {
                    if let Ok(v) = fetch_json_typed::<AnalysisRun>(
                        &format!("{ANALYSIS_ROOT}/runs/{id}"),
                        "恢复综合训练",
                    )
                    .await
                    {
                        run.set(Some(v));
                    }
                }
            });
            || ()
        });
    }
    {
        let tick = tick.clone();
        use_effect_with((), move |_| {
            let handle = Interval::new(1000, move || tick.set(Date::now() as u64));
            move || drop(handle)
        });
    }
    let submit = {
        let run = run.clone();
        let result = result.clone();
        let error = error.clone();
        let csrf = props.csrf.clone();
        let busy = busy.clone();
        let history = history.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            let Some(active) = (*run).clone() else { return };
            busy.set(true);
            let result = result.clone();
            let error = error.clone();
            let busy = busy.clone();
            let csrf = csrf.clone();
            let history = history.clone();
            spawn_local(async move {
                let endpoint = format!("{ANALYSIS_ROOT}/runs/{}/submit", active.id);
                match post_json_response_typed::<_, AnalysisResult>(
                    &endpoint,
                    &csrf,
                    &EmptyRequest {},
                    "提交综合训练",
                )
                .await
                {
                    Ok(out) => {
                        result.set(Some(out));
                        if let Ok(items) = fetch_json_typed::<Vec<ComprehensiveHistoryEntry>>(
                            &format!("{ANALYSIS_ROOT}/history"),
                            "刷新练习历史",
                        )
                        .await
                        {
                            history.set(items);
                        }
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.remove_item("hyz_comprehensive_active_run");
                        }
                    }
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        })
    };
    {
        let submit = submit.clone();
        let deadline = run.as_ref().as_ref().and_then(|r| r.deadline_at_ms);
        let ended = result.is_some();
        use_effect_with((deadline, ended, *tick), move |(deadline, ended, _)| {
            if !ended && deadline.is_some_and(|v| Date::now() >= v as f64) {
                submit.emit(());
            }
            || ()
        });
    }
    let start = {
        let progress = progress.clone();
        let error = error.clone();
        let busy = busy.clone();
        let preparing = preparing.clone();
        let ready_id = ready_id.clone();
        let csrf = props.csrf.clone();
        let manual_skills = manual_skills.clone();
        Callback::from(move |_| {
            if *busy || *preparing || ready_id.is_some() {
                return;
            }
            busy.set(true);
            error.set(None);
            progress.set(None);
            let progress = progress.clone();
            let error = error.clone();
            let busy = busy.clone();
            let preparing = preparing.clone();
            let ready_id = ready_id.clone();
            let csrf = csrf.clone();
            let selected = (*manual_skills).clone();
            spawn_local(async move {
                match post_json_response_typed::<_, PrepareTask>(
                    &format!("{ANALYSIS_ROOT}/prepare"),
                    &csrf,
                    &ComprehensivePrepareRequest {
                        manual_skills: selected,
                    },
                    "准备综合训练",
                )
                .await
                {
                    Ok(task) => {
                        remember_preparation(Some(&task.job_id));
                        busy.set(false);
                        resume_preparation(task.job_id, progress, ready_id, preparing, error).await;
                    }
                    Err(e) => {
                        busy.set(false);
                        error.set(Some(e.to_string()));
                    }
                }
            });
        })
    };
    // Opening the prepared paper is the *only* action which starts 20 minutes.
    // A background generation result can remain ready for hours without timing.
    let enter_ready = {
        let run = run.clone();
        let current = current.clone();
        let ready_id = ready_id.clone();
        let busy = busy.clone();
        let error = error.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            let Some(id) = (*ready_id).clone() else {
                return;
            };
            busy.set(true);
            let run = run.clone();
            let current = current.clone();
            let ready_id = ready_id.clone();
            let busy = busy.clone();
            let error = error.clone();
            spawn_local(async move {
                match fetch_json_typed::<AnalysisRun>(
                    &format!("{ANALYSIS_ROOT}/runs/{id}"),
                    "进入综合训练",
                )
                .await
                {
                    Ok(active) => {
                        current.set(0);
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item("hyz_comprehensive_active_run", &active.id);
                        }
                        remember_preparation(None);
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.remove_item(COMPREHENSIVE_READY_KEY);
                        }
                        ready_id.set(None);
                        run.set(Some(active));
                    }
                    Err(e) => error.set(Some(format!("进入训练失败：{e}"))),
                }
                busy.set(false);
            });
        })
    };
    let toggle_pause = {
        let run = run.clone();
        let busy = busy.clone();
        let error = error.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            let Some(active) = (*run).clone() else { return };
            busy.set(true);
            let run = run.clone();
            let busy = busy.clone();
            let error = error.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let endpoint = format!("{ANALYSIS_ROOT}/runs/{}/pause", active.id);
                match post_json_response_typed::<_, AnalysisRun>(
                    &endpoint,
                    &csrf,
                    &EmptyRequest {},
                    "暂停或继续综合训练",
                )
                .await
                {
                    Ok(next) => run.set(Some(next)),
                    Err(e) => error.set(Some(e.to_string())),
                }
                busy.set(false);
            });
        })
    };
    if let Some(score) = (*result).clone() {
        return html! {
            <section class="mx-auto max-w-5xl space-y-5 px-4 py-5" data-testid="analysis-result">
                <div class="rounded-box bg-base-100 p-5">
                    <h2 class="text-xl font-bold">{"综合训练成绩"}</h2>
                    <p class="mt-3 text-3xl font-black">{format!("{} / {} 题正确",score.correct,score.total)}</p>
                    <p class="text-sm text-base-content/60">{"已交卷 · 题目为 Codex 根据 hyz 做题套路生成的模拟训练数据与解析。"}</p>
                </div>
                <details class="rounded-box bg-base-100 p-4">
                    <summary class="cursor-pointer font-semibold">{"查看本套4份训练材料"}</summary>
                    {for score.materials.iter().enumerate().map(|(i,m)|html!{
                        <article class="mt-4 border-t border-base-content/10 pt-3"><h3 class="font-semibold">{format!("材料{} · {}",i+1,m.subtopic)}</h3>
                        <p class="mt-2 whitespace-pre-wrap text-sm leading-7">{m.text.clone()}</p></article>
                    })}
                </details>
                {for score.questions.iter().map(|q|html!{
                    <article class="rounded-box bg-base-100 p-4" key={q.number}>
                        <div class="flex justify-between"><strong>{format!("第{}题 · {} / {}",q.number,q.topic,q.skill)}</strong><span>{if q.chosen==Some(q.correct_index){"正确"}else{"错误/未答"}}</span></div>
                        <p class="mt-2">{format!("正确答案：{} · {}",["A","B","C","D"][q.correct_index],q.answer)}</p>
                        <p class="mt-2 text-sm font-semibold">{q.question.clone()}</p>
                    <p class="text-sm mt-2">{format!("你的选择：{}；正确答案：{}",q.chosen.map(|i|["A","B","C","D"][i]).unwrap_or("未答"),["A","B","C","D"][q.correct_index])}</p>
                    <div class="mt-2 text-sm">{for q.options.iter().enumerate().map(|(i,v)|html!{<p>{format!("{}. {}",["A","B","C","D"][i],v)}</p>})}</div>
                    <p class="text-sm mt-2">{q.explanation.clone()}</p>
                        <details class="mt-2 text-xs"><summary>{"查看原始数据证据"}</summary><p>{q.evidence.clone()}</p></details>
                    </article>
                })}
                <button class="btn btn-outline" onclick={{
                    let result=result.clone();let run=run.clone();let history_open=history_open.clone();
                    Callback::from(move |_|{result.set(None);run.set(None);history_open.set(true);})
                }}>{"查看练习历史"}</button>
                <button class="btn btn-primary" onclick={{
                    let run=run.clone();let result=result.clone();let progress=progress.clone();
                    Callback::from(move |_|{run.set(None);result.set(None);progress.set(None);})
                }}>{"再出一套"}</button>
            </section>
        };
    }
    if *history_open {
        let back = {
            let history_open = history_open.clone();
            Callback::from(move |_| history_open.set(false))
        };
        let open_result = {
            let result = result.clone();
            let error = error.clone();
            let history_open = history_open.clone();
            let run = run.clone();
            Callback::from(move |id: String| {
                let result = result.clone();
                let error = error.clone();
                let history_open = history_open.clone();
                let run = run.clone();
                spawn_local(async move {
                    match fetch_json_typed::<AnalysisResult>(
                        &format!("{ANALYSIS_ROOT}/runs/{id}/result"),
                        "历史成绩详情",
                    )
                    .await
                    {
                        Ok(value) => {
                            run.set(None);
                            history_open.set(false);
                            result.set(Some(value));
                        }
                        Err(e) => error.set(Some(e.to_string())),
                    }
                });
            })
        };
        return html! {
            <section class="mx-auto max-w-4xl space-y-4 p-5" data-testid="analysis-history">
                <div class="flex items-center justify-between">
                    <h2 class="text-xl font-bold">{"综合训练历史"}</h2>
                    <button class="btn btn-outline btn-sm" onclick={back}>{"返回训练"}</button>
                </div>
                {if history.is_empty(){html!{<p class="text-sm text-base-content/60">{"还没有已交卷的综合训练。"}</p>}}else{Html::default()}}
                {for history.iter().map(|entry|{
                    let id=entry.id.clone();let open_result=open_result.clone();
                    let stamp=Date::new(&JsValue::from_f64(entry.started_at_ms as f64));
                    let date=format!("{}-{:02}-{:02} {:02}:{:02}",
                        stamp.get_full_year(),stamp.get_month()+1,stamp.get_date(),stamp.get_hours(),stamp.get_minutes());
                    html!{
                        <article class="rounded-box bg-base-100 p-4" key={entry.id.clone()}>
                            <div class="flex flex-wrap items-center justify-between gap-3">
                                <div><strong>{format!("{} · {}/{}题",date,entry.correct,entry.total)}</strong>
                                <p class="text-xs text-base-content/60">{format!("作答用时 {}分{}秒 · {}",entry.elapsed_ms/60000,(entry.elapsed_ms/1000)%60,entry.topics.join("、"))}</p></div>
                                <button class="btn btn-primary btn-sm" onclick={Callback::from(move |_|open_result.emit(id.clone()))}>{"查看原题与解析"}</button>
                            </div>
                        </article>
                    }
                })}
                {if let Some(msg)=(*error).clone(){html!{<p class="text-error text-sm">{msg}</p>}}else{Html::default()}}
            </section>
        };
    }
    if let Some(active) = (*run).clone() {
        if active.groups.len() != 4 || active.answers.len() != 20 {
            return html! {<p class="p-5 text-error">{"服务端返回的综合训练题量无效，请重新出题。"}</p>};
        }
        if active.paused_remaining_ms.is_some() {
            return html! {
                <section class="mx-auto max-w-2xl p-6" data-testid="analysis-paused">
                    <div class="rounded-box bg-base-100 p-8 text-center">
                        <h2 class="text-2xl font-bold">{"训练已暂停"}</h2>
                        <p class="mt-3 text-base-content/60">{"已隐藏材料和题目，暂停期间不能答题。刷新页面仍保持暂停。"}</p>
                        <p class="my-5 font-mono text-3xl">{format!("{:02}:{:02}",active.paused_remaining_ms.unwrap_or(0)/60000,(active.paused_remaining_ms.unwrap_or(0)/1000)%60)}</p>
                        <button class="btn btn-primary" disabled={*busy} onclick={toggle_pause}>{"继续计时"}</button>
                        {if let Some(msg)=(*error).clone(){html!{<p class="mt-4 text-error text-sm">{msg}</p>}}else{Html::default()}}
                    </div>
                </section>
            };
        }
        let index = (*current).min(19);
        let group_index = index / 5;
        let local_index = index % 5;
        let group = &active.groups[group_index];
        let q = &group.questions[local_index];
        let remaining = active
            .deadline_at_ms
            .map(|ms| ((ms as f64 - Date::now()) / 1000.0).ceil().max(0.0) as u64)
            .unwrap_or(1200);
        let clock = format!("{:02}:{:02}", remaining / 60, remaining % 60);
        let next = {
            let current = current.clone();
            Callback::from(move |_| current.set((*current + 1).min(19)))
        };
        let previous = {
            let current = current.clone();
            Callback::from(move |_| current.set(current.saturating_sub(1)))
        };
        let answers=(0..4).map(|i|{
            let id=active.id.clone();let run=run.clone();let csrf=props.csrf.clone();
            let error=error.clone();let selected=active.answers[index]==Some(i);
            html!{
                <button type="button" class={if selected {"btn btn-primary justify-start min-h-14 h-auto text-left"}else{"btn btn-outline justify-start min-h-14 h-auto text-left"}}
                    data-testid="analysis-choice" disabled={remaining==0} onclick={Callback::from(move |_|{
                        let run=run.clone();let csrf=csrf.clone();let id=id.clone();let error=error.clone();
                        spawn_local(async move {
                            let endpoint=format!("{ANALYSIS_ROOT}/runs/{id}/answers/{}",index+1);
                            match post_json_typed(&endpoint,&csrf,&SaveAnswer{choice:i},"保存答案").await{
                                Ok(_)=>{
                                    if let Some(mut copy)=(*run).clone(){
                                        if copy.id==id {copy.answers[index]=Some(i);run.set(Some(copy));}
                                    }
                                }
                                Err(e)=>error.set(Some(e.to_string())),
                            }
                        });
                    })}>
                    {format!("{}. {}",["A","B","C","D"][i],q.options[i])}
                </button>
            }
        });
        return html! {
            <section class="mx-auto flex h-[calc(100dvh-2rem)] max-w-[1500px] flex-col px-3 py-2 sm:px-5" data-testid="analysis-run">
                <header class="mb-3 flex flex-wrap items-center justify-between gap-2 border-b border-base-content/10 pb-3">
                    <div class="text-sm font-bold">{"资料分析综合训练"} <span class="text-base-content/60">{format!("第 {}/20 题 · 材料 {}/4",index+1,group_index+1)}</span></div>
                    <div class="flex items-center gap-2">
                        <button type="button" class={if remaining<=60 {"btn btn-ghost font-mono text-xl text-error"}else{"btn btn-ghost font-mono text-xl"}} data-testid="analysis-clock" title="点击暂停计时" disabled={*busy || remaining==0} onclick={toggle_pause}>{format!("⏸ {clock}")}</button>
                        <button type="button" class="btn btn-sm btn-error btn-outline" disabled={*busy} onclick={{let submit=submit.clone();Callback::from(move |_|submit.emit(()))}}>{"交卷"}</button>
                    </div>
                </header>
                <InkOverlay question_index={index} persist_key={format!("{}-workspace-{}", active.id, index)} responsive={true}>
                    <div class="grid h-full min-h-0 grid-cols-1 gap-3 lg:grid-cols-[44%_minmax(0,1fr)]" data-testid="analysis-shared-workspace">
                        <div class="flex min-h-0 min-w-0 flex-col overflow-auto rounded-box bg-base-100" data-testid="analysis-material">
                            <div class="p-3 text-sm font-semibold">{format!("{} · {}",group.material.section,group.material.subtopic)}</div>
                            <div class="min-h-0 flex-1 overflow-auto">
                                <div class="whitespace-pre-wrap p-4 text-base leading-8">{group.material.text.clone()}</div>
                            </div>
                            <a class="p-3 text-xs underline" href={group.material.source_url.clone()} target="_blank" rel="noopener noreferrer">{"查看统计口径参考资料（非模拟数据来源）"}</a>
                        </div>
                        <div class="min-h-0 min-w-0 overflow-auto rounded-box bg-base-100" data-testid="analysis-question">
                            <div class="space-y-4 p-4">
                                <div class="flex items-center justify-between text-xs text-base-content/60"><span>{format!("本材料第 {}/5 题",local_index+1)}</span><span>{q.skill.clone()}</span></div>
                                <h2 class="text-lg font-semibold leading-relaxed">{q.question.clone()}</h2>
                                <div class="grid gap-3">{for answers}</div>
                            </div>
                        </div>
                    </div>
                </InkOverlay>
                {if let Some(msg)=(*error).clone(){html!{<div class="text-error text-xs">{msg}</div>}}else{Html::default()}}
                <footer class="flex items-center justify-between gap-3 py-3" data-testid="analysis-navigation">
                    <button type="button" class="btn btn-outline btn-sm" disabled={index==0} onclick={previous}>{"上一题"}</button>
                    <div class="flex flex-wrap justify-center gap-1">
                        {for (0..4).map(|g|{
                            let current=current.clone();
                            html!{<button type="button" class={if g==group_index{"btn btn-primary btn-xs"}else{"btn btn-ghost btn-xs"}} onclick={Callback::from(move |_|current.set(g*5))}>{format!("材料{}",g+1)}</button>}
                        })}
                        {for (group_index*5..group_index*5+5).map(|i|{
                            let current=current.clone();
                            let chosen=active.answers[i].is_some();
                            html!{<button type="button" class={if i==index{"btn btn-primary btn-xs"}else if chosen{"btn btn-success btn-outline btn-xs"}else{"btn btn-ghost btn-xs"}} onclick={Callback::from(move |_|current.set(i))}>{i+1}</button>}
                        })}
                    </div>
                    <button type="button" class="btn btn-primary btn-sm" disabled={index==19} onclick={next}>{"下一题"}</button>
                </footer>
            </section>
        };
    }
    let toggle_manual = {
        let manual_skills = manual_skills.clone();
        Callback::from(move |skill: String| {
            let mut chosen = (*manual_skills).clone();
            if let Some(index) = chosen.iter().position(|v| v == &skill) {
                chosen.remove(index);
            } else if chosen.len() < 12 {
                chosen.push(skill);
            }
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                if let Ok(raw) = serde_json::to_string(&chosen) {
                    let _ = storage.set_item(MANUAL_SKILLS_KEY, &raw);
                }
            }
            manual_skills.set(chosen);
        })
    };
    let toggle_selector = {
        let skills_expanded = skills_expanded.clone();
        Callback::from(move |_| skills_expanded.set(!*skills_expanded))
    };
    html! {
        <section class="mx-auto max-w-3xl p-4 sm:p-6" data-testid="analysis-setup">
            <div class="rounded-box bg-base-100 p-5 sm:p-7">
                <h2 class="text-xl font-bold">{"资料分析综合训练"}</h2>
                <p class="mt-1 text-sm text-base-content/60">{"参考官方统计口径构造模拟数据 · 4份材料 × 每份5题 · 20分钟"}</p>
                <div class="mt-5 grid gap-3">
                    <button class="btn btn-outline w-full" type="button" onclick={{let history_open=history_open.clone();Callback::from(move |_|history_open.set(true))}}>{"查看练习历史"}</button>
                    <div class="rounded-box border border-base-content/15 p-4 space-y-3">
                        <p class="font-semibold">{"错题分类 · 智能薄弱点强化"}</p>
                        <p class="text-sm text-base-content/60">{"直接使用 flashcards/资料/错题/ 下的分类；自动推荐结合错题数量、复习记录和训练表现。"}</p>
                        if let Some(info)=(*status).clone(){
                            <div class="flex flex-wrap gap-2">{for info.weak_skills.iter().map(|x|html!{<span class="badge badge-outline">{x.clone()}</span>})}</div>
                        }
                        <div class="flex items-center justify-between gap-3">
                            <strong class="text-sm">{format!("手动强化考点（已选{}项，最多12项）",manual_skills.len())}</strong>
                            <button type="button" class="btn btn-outline btn-sm" data-testid="analysis-skill-expand"
                                onclick={toggle_selector}>{if *skills_expanded{"收起"}else{"添加考点"}}</button>
                        </div>
                        <div class="flex flex-wrap gap-2">
                            {for manual_skills.iter().map(|name|html!{
                                <span class="badge badge-primary badge-outline">{name.clone()}</span>
                            })}
                        </div>
                        if *skills_expanded {
                            if let Some(info)=(*status).clone(){
                                <div class="flex max-h-60 flex-wrap gap-2 overflow-auto rounded-box bg-base-200 p-3" data-testid="analysis-skill-options">
                                    if info.available_skills.is_empty() {
                                        <p class="text-xs text-base-content/60">{"尚未发现资料/错题分类，请先同步 Flashcard。"}</p>
                                    }
                                    {for info.available_skills.iter().map(|name|{
                                        let chosen=manual_skills.contains(name);
                                        let title=name.clone();let select=toggle_manual.clone();
                                        html!{<button type="button" data-testid="analysis-skill-toggle"
                                            class={if chosen{"btn btn-primary btn-sm"}else{"btn btn-outline btn-sm"}}
                                            aria-pressed={chosen.to_string()}
                                            disabled={!chosen && manual_skills.len()>=12}
                                            onclick={Callback::from(move |_|select.emit(title.clone()))}>{name.clone()}</button>}
                                    })}
                                </div>
                            }
                        }
                        <p class="text-xs text-base-content/60">{"勾选的名称就是错题文件夹名称。Codex 会读取相应目录的 Flashcard；选择保存在本设备浏览器中。"}</p>
                    </div>
                    if let Some(info)=(*status).clone(){
                        <p class="text-sm text-base-content/65">{format!("出题引擎：{} · 每次主动出题均生成新材料和20道新题",
                            info.model.as_deref().unwrap_or("未配置"))}</p>
                    }
                    if let Some(p)=(*progress).clone(){
                        <div class="rounded-box bg-base-200 p-3 text-sm" data-testid="analysis-progress">
                            <div class="font-semibold">{format!("{} · 已生成 {}/20",p.state,p.verified)}</div>
                            <p>{p.message}</p>
                        </div>
                    }
                    if let Some(err)=(*error).clone(){<p class="text-error text-sm" role="alert">{err}</p>}
                    if ready_id.is_some() {
                        <div class="rounded-box border border-success/40 bg-success/5 p-4" data-testid="analysis-ready">
                            <p class="font-semibold">{"20题已经准备好"}</p>
                            <p class="mt-1 text-sm text-base-content/70">{"倒计时尚未开始。点击进入训练时才启动20分钟计时。"}</p>
                        </div>
                        <button type="button" class="btn btn-primary w-full" data-testid="analysis-enter" disabled={*busy} onclick={enter_ready}>
                            {if *busy {"进入中…"}else{"进入训练 · 开始20分钟计时"}}
                        </button>
                    } else {
                        <button type="button" class="btn btn-primary w-full" data-testid="analysis-start"
                            disabled={*loading_status || *busy || *preparing || status.as_ref().as_ref().is_none_or(|s|!s.configured) || props.csrf.is_empty()}
                            onclick={start}>
                            {if *busy || *preparing {"正在出题中…（可以切换页面）"} else {"✨ 智能出题"}}
                        </button>
                    }
                    <p class="text-xs text-base-content/60">
                        {"在 RK3568 的 /userdata/hyz-things/.env 配置 Codex WebSocket Token。电脑 192.168.8.202 需要运行已登录的 Codex App Server，未通过校验的题不会进入正式训练。"}
                    </p>
                </div>
            </div>
        </section>
    }
}
