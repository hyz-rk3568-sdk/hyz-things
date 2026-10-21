//! 20 questions / 20 minutes. iPad-first 44:56 reading and answer panes.
use super::*;
use gloo_timers::{callback::Interval, future::TimeoutFuture};
use js_sys::Date;

#[derive(Properties, PartialEq)]
pub(crate) struct ComprehensiveProps {
    pub csrf: String,
}

#[function_component(ComprehensiveTraining)]
pub(crate) fn comprehensive_training(props: &ComprehensiveProps) -> Html {
    let status = use_state(|| None::<AnalysisStatus>);
    let progress = use_state(|| None::<PrepareProgress>);
    let run = use_state(|| None::<AnalysisRun>);
    let result = use_state(|| None::<AnalysisResult>);
    let current = use_state(|| 0usize);
    let busy = use_state(|| false);
    let error = use_state(|| None::<String>);
    let tick = use_state(|| 0_u64);
    let history = use_state(Vec::<ComprehensiveHistoryEntry>::new);
    let history_open = use_state(|| false);

    {
        let status = status.clone();
        let error = error.clone();
        let run = run.clone();
        let history = history.clone();
        use_effect_with((), move |_| {
            let history = history.clone();
            spawn_local(async move {
                match fetch_json_typed::<AnalysisStatus>(
                    &format!("{ANALYSIS_ROOT}/status"),
                    "综合训练状态",
                )
                .await
                {
                    Ok(v) => status.set(Some(v)),
                    Err(e) => error.set(Some(format!("{e}；综合出题需要管理员登录"))),
                }
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
        let run = run.clone();
        let current = current.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *busy {
                return;
            }
            busy.set(true);
            error.set(None);
            progress.set(None);
            let (progress, error, busy, run, current, csrf) = (
                progress.clone(),
                error.clone(),
                busy.clone(),
                run.clone(),
                current.clone(),
                csrf.clone(),
            );
            spawn_local(async move {
                let task = post_json_response_typed::<_, PrepareTask>(
                    &format!("{ANALYSIS_ROOT}/prepare"),
                    &csrf,
                    &EmptyRequest {},
                    "AI出题",
                )
                .await;
                let result = match task {
                    Err(err) => Err(err.to_string()),
                    Ok(task) => {
                        let mut outcome = Err("出题等待超时，可稍后重试".to_owned());
                        // Four material groups can require two 85-second Codex turns each.
                        // Keep the UI polling until the bounded backend job finishes.
                        for _ in 0..800 {
                            TimeoutFuture::new(1500).await;
                            let state = fetch_json_typed::<PrepareProgress>(
                                &format!("{ANALYSIS_ROOT}/prepare/{}", task.job_id),
                                "出题进度",
                            )
                            .await;
                            match state {
                                Ok(p) => {
                                    progress.set(Some(p.clone()));
                                    if p.state == "failed" {
                                        outcome = Err(p.message);
                                        break;
                                    }
                                    if p.state == "ready" {
                                        if let Some(id) = p.run_id {
                                            match fetch_json_typed::<AnalysisRun>(
                                                &format!("{ANALYSIS_ROOT}/runs/{id}"),
                                                "开始综合训练",
                                            )
                                            .await
                                            {
                                                Ok(r) => {
                                                    outcome = Ok(r);
                                                    break;
                                                }
                                                Err(e) => {
                                                    outcome = Err(e.to_string());
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    outcome = Err(e.to_string());
                                    break;
                                }
                            }
                        }
                        outcome
                    }
                };
                match result {
                    Ok(r) => {
                        current.set(0);
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item("hyz_comprehensive_active_run", &r.id);
                        }
                        run.set(Some(r));
                    }
                    Err(message) => error.set(Some(message)),
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
                    <p class="text-sm text-base-content/60">{"已交卷 · 题目依据官方统计文字资料生成，解析含 AI 建议，请以数据计算结果为准。"}</p>
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
                            <a class="p-3 text-xs underline" href={group.material.source_url.clone()} target="_blank" rel="noopener noreferrer">{"国家统计局 · 查看原文"}</a>
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
    html! {
        <section class="mx-auto max-w-3xl p-4 sm:p-6" data-testid="analysis-setup">
            <div class="rounded-box bg-base-100 p-5 sm:p-7">
                <h2 class="text-xl font-bold">{"资料分析综合训练"}</h2>
                <p class="mt-1 text-sm text-base-content/60">{"官方统计数据改编 · 4份仿真材料 × 每份5题 · 20分钟"}</p>
                <div class="mt-5 grid gap-3">
                    <button class="btn btn-outline w-full" type="button" onclick={{let history_open=history_open.clone();Callback::from(move |_|history_open.set(true))}}>{"查看练习历史"}</button>
                    <div class="rounded-box border border-base-content/15 p-4">
                        <p class="font-semibold">{"智能薄弱点强化"}</p>
                        <p class="mt-1 text-sm text-base-content/60">{"根据 Flashcard 错题考点及历史成绩，优先安排需要加强的领域。"}</p>
                        if let Some(info)=(*status).clone(){
                            <div class="mt-3 flex flex-wrap gap-2">{for info.weak_skills.iter().map(|x|html!{<span class="badge badge-outline">{x.clone()}</span>})}</div>
                        }
                    </div>
                    if let Some(info)=(*status).clone(){
                        <div class="text-sm text-base-content/65">
                            {format!("官方资料缓存 {} 篇 · 合格材料组 {} 组 · 出题引擎：{}",
                                info.cached_sources,info.verified_groups,
                                info.model.as_deref().unwrap_or("未配置"))}
                        </div>
                    }
                    if let Some(p)=(*progress).clone(){
                        <div class="rounded-box bg-base-200 p-3 text-sm" data-testid="analysis-progress">
                            <div class="font-semibold">{format!("{} · 已通过 {}/20",p.state,p.verified)}</div>
                            <p>{p.message}</p>
                        </div>
                    }
                    if let Some(err)=(*error).clone(){<p class="text-error text-sm" role="alert">{err}</p>}
                    <button type="button" class="btn btn-primary w-full" data-testid="analysis-start"
                        disabled={*busy || status.as_ref().as_ref().is_none_or(|s|!s.configured) || props.csrf.is_empty()}
                        onclick={start}>
                        {if *busy {"出题中，请稍候…"} else {"✨ 智能出题并开始"}}
                    </button>
                    <p class="text-xs text-base-content/60">
                        {"在 RK3568 的 /userdata/hyz-things/.env 配置 Codex WebSocket Token。电脑 192.168.8.202 需要运行已登录的 Codex App Server，未通过校验的题不会进入正式训练。"}
                    </p>
                </div>
            </div>
        </section>
    }
}
