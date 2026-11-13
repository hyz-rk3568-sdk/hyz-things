//! 申论专用 AI 工作区：独立任务、原图优先的人工确认与按需答案。
use super::*;
use hyz_things::domain::question_bank::{
    ConfirmShenlunTranscription, ShenlunAttempt, ShenlunJob, ShenlunJobRequest, ShenlunSourceAnswer,
};

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunAiPanelProps {
    pub(super) attempt: ShenlunAttempt,
    pub(super) csrf: String,
    pub(super) on_confirm: Callback<ShenlunAttempt>,
}

#[derive(Clone, PartialEq)]
struct LocalImage {
    id: String,
    name: String,
    preview: Option<String>,
}

fn transcription_text(job: &ShenlunJob) -> String {
    job.result
        .as_ref()
        .and_then(|result| result.get("pages"))
        .and_then(serde_json::Value::as_array)
        .map(|pages| {
            pages
                .iter()
                .filter_map(|page| page.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

fn grade_score(job: &ShenlunJob) -> String {
    let Some(result) = job.result.as_ref() else { return "暂无有效批改结果".into(); };
    let Some(max) = result.get("max_score").and_then(serde_json::Value::as_f64) else {
        return "未评分练习诊断：题目分值不确定".into();
    };
    let Some(score) = result.get("score").and_then(serde_json::Value::as_f64) else {
        return "未评分练习诊断".into();
    };
    format!("练习参考分 {score}/{max}（非官方）")
}

#[function_component(ShenlunAiPanel)]
pub(super) fn shenlun_ai_panel(props: &ShenlunAiPanelProps) -> Html {
    let jobs = use_state(Vec::<ShenlunJob>::new);
    let images = use_state(Vec::<LocalImage>::new);
    let image_busy = use_state(|| false);
    let task_busy = use_state(|| false);
    let error = use_state(|| None::<String>);
    let poll = use_state(|| 0_u32);
    let transcribed = use_state(String::new);
    let source_answer = use_state(|| None::<ShenlunSourceAnswer>);
    let showing_answer = use_state(|| false);
    let selected_history = use_state(|| None::<String>);
    let attempt_id = props.attempt.id.clone();
    {
        let jobs = jobs.clone();
        let error = error.clone();
        let attempt_id = attempt_id.clone();
        use_effect_with((attempt_id, *poll), move |(id, _)| {
            let alive = Rc::new(Cell::new(true));
            let live = alive.clone();
            let id = id.clone();
            spawn_local(async move {
                match fetch_json::<Vec<ShenlunJob>>(
                    &format!("/api/v1/question-bank/shenlun/attempts/{id}/jobs"),
                    "申论 AI 历史",
                ).await {
                    Ok(value) if live.get() => jobs.set(value),
                    Err(message) if live.get() => error.set(Some(message)),
                    _ => {}
                }
            });
            move || alive.set(false)
        });
    }
    let is_running = jobs.iter().any(|job| job.status == "running");
    {
        let poll = poll.clone();
        use_effect_with(is_running, move |active| {
            let interval = if *active {
                let poll = poll.clone();
                Some(gloo_timers::callback::Interval::new(1800, move || {
                    poll.set(poll.wrapping_add(1));
                }))
            } else {
                None
            };
            move || drop(interval)
        });
    }
    let latest_transcription = jobs.iter().find(|job| {
        job.kind == "transcribe" && job.status == "complete"
            && job.attempt_revision == props.attempt.revision
    }).cloned();
    {
        let transcribed = transcribed.clone();
        use_effect_with(latest_transcription.as_ref().map(|job| job.id.clone()), move |_| {
            if let Some(job) = latest_transcription {
                transcribed.set(transcription_text(&job));
            } else {
                transcribed.set(String::new());
            }
            || ()
        });
    }
    let latest_grade = jobs.iter().find(|job| {
        job.kind == "grade" && job.status == "complete"
            && job.attempt_revision == props.attempt.revision
            && job.source_hash == props.attempt.source_hash
    }).cloned();
    let running = jobs.iter().find(|job| job.status == "running").cloned();
    let start = {
        let attempt = props.attempt.clone();
        let images = images.clone();
        let csrf = props.csrf.clone();
        let jobs = jobs.clone();
        let error = error.clone();
        let task_busy = task_busy.clone();
        let poll = poll.clone();
        Callback::from(move |kind: String| {
            if *task_busy || jobs.iter().any(|job| job.status == "running") { return; }
            let request_id = match super::shenlun_training::new_shenlun_attempt_id() {
                Ok(id) => id, Err(message) => { error.set(Some(message)); return; }
            };
            let attachments = if kind == "transcribe" {
                images.iter().map(|image| image.id.clone()).collect()
            } else { Vec::new() };
            task_busy.set(true);
            error.set(None);
            let jobs=jobs.clone();
            let error=error.clone();
            let task_busy=task_busy.clone();
            let poll=poll.clone();
            let endpoint=format!("/api/v1/question-bank/shenlun/attempts/{}/jobs",attempt.id);
            let csrf=csrf.clone();
            spawn_local(async move {
                let request=ShenlunJobRequest { request_id, kind, attachments };
                let response: Result<ShenlunJob,String>=post_json_response(&endpoint,&csrf,&request,"启动申论 AI").await;
                match response {
                    Ok(job) => {
                        let mut next=(*jobs).clone();
                        next.retain(|item| item.id != job.id);
                        next.insert(0,job);
                        jobs.set(next);
                        poll.set(poll.wrapping_add(1));
                    }
                    Err(message)=>error.set(Some(message)),
                }
                task_busy.set(false);
            });
        })
    };
    let select_image = {
        let images=images.clone();
        let image_busy=image_busy.clone();
        let error=error.clone();
        let csrf=props.csrf.clone();
        let question_id=props.attempt.question_id.clone();
        Callback::from(move |event: Event| {
            let input: web_sys::HtmlInputElement=event.target_unchecked_into();
            let picked: Vec<web_sys::File>=input.files()
                .map(|files| (0..files.length()).filter_map(|i| files.get(i)).collect())
                .unwrap_or_default();
            input.set_value("");
            if *image_busy || picked.is_empty() { return; }
            if picked.len()+images.len()>3 {
                error.set(Some("每批最多选择 3 张图片，可分批处理多页".into()));return;
            }
            for file in &picked {
                if !matches!(file.type_().as_str(),"image/jpeg"|"image/png")
                    || file.size()>8.0*1024.0*1024.0 {
                    error.set(Some("仅支持单张不超过 8 MiB 的 JPG/PNG".into()));return;
                }
            }
            image_busy.set(true);
            let images=images.clone();
            let image_busy=image_busy.clone();
            let error=error.clone();
            let csrf=csrf.clone();
            let question_id=question_id.clone();
            spawn_local(async move {
                let mut list=(*images).clone();
                for file in picked {
                    match upload_question_ai_image(&format!("/api/v1/question-bank/practice/questions/{}/ai/attachments",question_id),&csrf,file.clone()).await {
                        Ok(id)=>{
                            let preview=wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await
                                .ok()
                                .map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec())
                                .map(|bytes| {
                                    use base64::Engine as _;
                                    format!("data:{};base64,{}",file.type_(),base64::engine::general_purpose::STANDARD.encode(bytes))
                                });
                            list.push(LocalImage{id,name:file.name(),preview});
                        }
                        Err(message)=>{error.set(Some(message));break;}
                    }
                }
                images.set(list);
                image_busy.set(false);
            });
        })
    };
    let clear_images = {
        let images=images.clone();
        Callback::from(move |_| images.set(Vec::new()))
    };
    let confirm_text = {
        let transcribed=transcribed.clone();
        let latest=jobs.iter().find(|job| job.kind=="transcribe" && job.status=="complete"
            && job.attempt_revision==props.attempt.revision).cloned();
        let attempt=props.attempt.clone();
        let on_confirm=props.on_confirm.clone();
        let csrf=props.csrf.clone();
        let task_busy=task_busy.clone();
        let error=error.clone();
        let images=images.clone();
        Callback::from(move |_| {
            let Some(job)=latest.as_ref() else { return; };
            if transcribed.trim().is_empty() || *task_busy {return;}
            let body=ConfirmShenlunTranscription{
                job_id:job.id.clone(),
                expected_revision:attempt.revision,
                text:(*transcribed).clone(),
            };
            task_busy.set(true);
            let task_busy=task_busy.clone();
            let on_confirm=on_confirm.clone();
            let error=error.clone();
            let images=images.clone();
            let csrf=csrf.clone();
            let url=format!("/api/v1/question-bank/shenlun/attempts/{}/transcription/confirm",attempt.id);
            spawn_local(async move {
                let response:Result<ShenlunAttempt,String>=post_json_response(&url,&csrf,&body,"确认手写转写").await;
                match response {
                    Ok(result)=>{images.set(Vec::new());on_confirm.emit(result);},
                    Err(message)=>error.set(Some(message)),
                }
                task_busy.set(false);
            });
        })
    };
    let edit_transcribed = {
        let transcribed=transcribed.clone();
        Callback::from(move |event:InputEvent| {
            let input:web_sys::HtmlTextAreaElement=event.target_unchecked_into();
            transcribed.set(input.value());
        })
    };
    let stop_job = {
        let running=running.clone();
        let csrf=props.csrf.clone();
        let poll=poll.clone();
        let error=error.clone();
        Callback::from(move |_| {
            let Some(job)=running.as_ref() else{return;};
            let url=format!("/api/v1/question-bank/shenlun/jobs/{}/stop",job.id);
            let csrf=csrf.clone();
            let poll=poll.clone();
            let error=error.clone();
            spawn_local(async move {
                let result:Result<ShenlunJob,String>=post_json_response(&url,&csrf,&EmptyRequest{},"停止申论 AI").await;
                match result {
                    Ok(_)=>poll.set(poll.wrapping_add(1)),
                    Err(message)=>error.set(Some(message)),
                }
            });
        })
    };
    let reveal_source = {
        let id=props.attempt.id.clone();
        let source_answer=source_answer.clone();
        let showing_answer=showing_answer.clone();
        let error=error.clone();
        Callback::from(move |_|{
            showing_answer.set(true);
            let url=format!("/api/v1/question-bank/shenlun/attempts/{id}/source-answer");
            let source_answer=source_answer.clone();
            let error=error.clone();
            spawn_local(async move {
                match fetch_json::<ShenlunSourceAnswer>(&url,"来源参考答案").await {
                    Ok(value)=>source_answer.set(Some(value)),
                    Err(message)=>error.set(Some(message)),
                }
            });
        })
    };
    let choose_history = {
        let selected_history=selected_history.clone();
        Callback::from(move |id:String| selected_history.set(Some(id)))
    };
    let display_job=selected_history.as_ref()
        .and_then(|id| jobs.iter().find(|job|job.id==*id)).cloned()
        .or_else(||latest_grade.clone());
    html! {
        <section class="mt-5 space-y-3 border-t border-base-300 pt-4" data-testid="shenlun-ai-panel">
            <header class="flex flex-wrap items-center gap-2">
                <h3 class="font-semibold">{"申论 AI 批改"}</h3>
                if let Some(job)=running.as_ref() {
                    <span class="text-xs opacity-70">{format!("{} 正在运行",job.kind)}</span>
                    <button type="button" class="btn btn-error btn-outline btn-xs" onclick={stop_job}>{"停止"}</button>
                }
            </header>
            if props.attempt.status == "draft" {
                <div class="space-y-2 rounded-lg border border-base-300 p-3" data-testid="shenlun-transcribe">
                    <p class="text-sm">{"手写拍照转文字：Codex 先看原图。上传后按页序确认；不确认不参与批改。"}</p>
                    <div class="flex flex-wrap items-center gap-2">
                        <label class="btn btn-outline btn-sm">
                            {"选择图片（最多 3 张）"}
                            <input type="file" class="hidden" accept="image/png,image/jpeg" multiple=true
                                disabled={*image_busy || is_running} onchange={select_image}/>
                        </label>
                        <button type="button" class="btn btn-ghost btn-sm" onclick={clear_images} disabled={*image_busy}>{"清空图片"}</button>
                    </div>
                    <div class="flex flex-wrap gap-2">
                        {for images.iter().map(|image|html! {
                            <div key={image.id.clone()} class="w-24 overflow-hidden rounded-lg border border-base-300 p-1">
                                if let Some(preview)=image.preview.as_ref() {
                                    <img class="h-24 w-full object-contain" src={preview.clone()} alt={image.name.clone()}/>
                                }
                                <p class="truncate text-xs">{image.name.clone()}</p>
                            </div>
                        })}
                    </div>
                    <button type="button" class="btn btn-primary btn-sm" data-testid="shenlun-transcribe-start"
                        disabled={*image_busy || *task_busy || is_running || images.is_empty()}
                        onclick={Callback::from({let start=start.clone();move |_|start.emit("transcribe".into())})}>
                        {"开始忠实转写"}
                    </button>
                    if let Some(job)=latest_transcription.as_ref() {
                        <p class="text-xs opacity-70">{"请逐页对照原图核对；不确定文字必须手动确认。"}</p>
                        if let Some(pages)=job.result.as_ref().and_then(|value|value.get("pages"))
                            .and_then(serde_json::Value::as_array) {
                            {for pages.iter().map(|page|html!{
                                <div class="text-xs">
                                    <span class="font-semibold">{format!("第 {} 页",
                                        page.get("page").and_then(serde_json::Value::as_u64).unwrap_or(0))}</span>
                                    if let Some(uncertain)=page.get("uncertain").and_then(serde_json::Value::as_array) {
                                        {for uncertain.iter().map(|item|html!{
                                            <p class="text-warning">{format!("疑似「{}」：{}",
                                                item.get("text").and_then(serde_json::Value::as_str).unwrap_or(""),
                                                item.get("reason").and_then(serde_json::Value::as_str).unwrap_or(""))}</p>
                                        })}
                                    }
                                </div>
                            })}
                        }
                        <textarea class="textarea textarea-bordered min-h-44 w-full" rows="10"
                            aria-label="人工校对手写转写" data-testid="shenlun-transcribed-edit"
                            value={(*transcribed).clone()} oninput={edit_transcribed}/>
                        <button type="button" class="btn btn-primary btn-sm" data-testid="shenlun-transcribe-confirm"
                            disabled={*task_busy || transcribed.trim().is_empty()} onclick={confirm_text}>
                            {"我已对照原图确认文本"}
                        </button>
                    }
                </div>
            } else {
                <div class="flex flex-wrap gap-2" data-testid="shenlun-grade-controls">
                    <button type="button" class="btn btn-primary btn-sm" data-testid="shenlun-grade-start"
                        disabled={*task_busy || is_running || latest_grade.is_some()}
                        onclick={Callback::from({let start=start.clone();move |_|start.emit("grade".into())})}>
                        {"AI 批改"}
                    </button>
                    if latest_grade.is_some() {
                        <button type="button" class="btn btn-outline btn-sm"
                            onclick={Callback::from({let start=start.clone();move |_|start.emit("grade".into())})}
                            disabled={*task_busy || is_running}>{"重新批改"}</button>
                        <button type="button" class="btn btn-outline btn-sm" onclick={reveal_source.clone()}>
                            {"来源参考答案（免费）"}
                        </button>
                        <button type="button" class="btn btn-outline btn-sm"
                            onclick={Callback::from({let start=start.clone();move |_|start.emit("independent_answer".into())})}
                            disabled={*task_busy || is_running}>{"AI 独立参考答案"}</button>
                        <button type="button" class="btn btn-outline btn-sm"
                            onclick={Callback::from({let start=start.clone();move |_|start.emit("rewrite".into())})}
                            disabled={*task_busy || is_running}>{"我的答案优化版"}</button>
                        <button type="button" class="btn btn-outline btn-sm"
                            onclick={Callback::from({let start=start.clone();move |_|start.emit("full_essay".into())})}
                            disabled={*task_busy || is_running}>{"按需生成完整范文"}</button>
                    }
                </div>
                if *showing_answer {
                    if let Some(answer)=(*source_answer).as_ref() {
                        <details open=true class="rounded-lg border border-base-300 p-3" data-testid="shenlun-source-answer">
                            <summary class="font-semibold">{answer.provenance.clone()}</summary>
                            <pre class="mt-2 whitespace-pre-wrap text-sm">{answer.text.clone()}</pre>
                        </details>
                    } else {
                        <p class="text-xs opacity-70">{"正在读取已有来源答案…"}</p>
                    }
                }
                if let Some(job)=latest_grade.as_ref() {
                    <h4 class="font-semibold" data-testid="shenlun-score">{grade_score(job)}</h4>
                }
                <div class="flex flex-wrap gap-1">
                    {for jobs.iter().filter(|job|job.status=="complete").take(20).map(|job|{
                        let id=job.id.clone();
                        let select=choose_history.clone();
                        html! {
                            <button key={id.clone()} type="button" class="btn btn-ghost btn-xs"
                                data-testid="shenlun-job-history" onclick={Callback::from(move |_|select.emit(id.clone()))}>
                                {format!("{} · {}",job.kind,job.status)}
                            </button>
                        }
                    })}
                </div>
                if let Some(job)=display_job.as_ref() {
                    if let Some(result)=job.result.as_ref() {
                        <article class="rounded-lg border border-base-300 p-3" data-testid="shenlun-result">
                            if job.kind=="grade" {
                                <p class="font-semibold">{grade_score(job)}</p>
                                <p class="my-2 text-sm">{result.get("summary").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                                if let Some(items)=result.get("items").and_then(serde_json::Value::as_array) {
                                    {for items.iter().map(|item| html!{
                                        <div class="mb-3 rounded border border-base-300 p-2 text-sm">
                                            <p class="font-semibold">{format!("{} · {} · {}/{}",
                                                item.get("criterion").and_then(serde_json::Value::as_str).unwrap_or(""),
                                                item.get("coverage").and_then(serde_json::Value::as_str).unwrap_or(""),
                                                item.get("earned").unwrap_or(&serde_json::Value::Null),
                                                item.get("weight").unwrap_or(&serde_json::Value::Null))}</p>
                                            <p>{"我的原句："}{item.get("quote").and_then(serde_json::Value::as_str).unwrap_or("无命中原句")}</p>
                                            <p>{"材料依据："}{item.get("evidence").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                                            <p>{"分析："}{item.get("reason").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                                            <p>{"修改建议："}{item.get("suggestion").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                                        </div>
                                    })}
                                }
                            } else if matches!(job.kind.as_str(),"independent_answer"|"rewrite"|"full_essay") {
                                <p class="mb-2 text-xs opacity-70">{format!("{} · AI 生成，非官方",job.kind)}</p>
                                <pre class="whitespace-pre-wrap text-sm">{result.get("text").and_then(serde_json::Value::as_str).unwrap_or("")}</pre>
                                if let Some(edits)=result.get("edits").and_then(serde_json::Value::as_array) {
                                    {for edits.iter().map(|item|html!{
                                        <p class="mt-2 text-xs">{format!("原句：{} → 改写：{}（{}）",
                                            item.get("before").and_then(serde_json::Value::as_str).unwrap_or(""),
                                            item.get("after").and_then(serde_json::Value::as_str).unwrap_or(""),
                                            item.get("reason").and_then(serde_json::Value::as_str).unwrap_or(""))}</p>
                                    })}
                                }
                            }
                        </article>
                    }
                }
            }
            if let Some(message)=(*error).as_ref() {<p class="text-error text-xs" role="alert">{message}</p>}
            if let Some(failed)=jobs.iter().find(|job|matches!(job.status.as_str(),"failed"|"unknown"|"stopped")) {
                <p class="text-warning text-xs">{format!("{}：{}；可以重新发起新请求，旧稿保留。",
                    failed.kind,failed.error.as_deref().unwrap_or(&failed.status))}</p>
            }
            <p class="text-xs opacity-60">{"评分仅供练习，来源答案及 AI 生成稿不代表官方阅卷标准。二稿完全可选。"}</p>
        </section>
    }
}
