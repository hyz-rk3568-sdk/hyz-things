//! 申论 AI 批改适配层。输入与图片控件复用 Question Bank AiComposer，
//! 评分、转写、历史和来源答案继续使用现有 ShenlunAiService。
use super::*;
use hyz_things::domain::question_bank::{
    ConfirmShenlunTranscription, SaveShenlunDraft, ShenlunAttempt, ShenlunJob,
    ShenlunJobRequest, ShenlunSourceAnswer,
};

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunAiPanelProps {
    pub(super) attempt: ShenlunAttempt,
    pub(super) csrf: String,
    pub(super) on_confirm: Callback<ShenlunAttempt>,
    pub(super) attempts: Vec<ShenlunAttempt>,
    pub(super) is_essay: bool,
    pub(super) mode: String,
    pub(super) regrade_signal: u32,
    pub(super) delete_signal: u32,
    pub(super) on_new_attempt: Callback<()>,
}

fn transcription_text(job: &ShenlunJob) -> String {
    job.result.as_ref()
        .and_then(|result| result.get("pages"))
        .and_then(serde_json::Value::as_array)
        .map(|pages|pages.iter().filter_map(|page|
            page.get("text").and_then(serde_json::Value::as_str)
        ).collect::<Vec<_>>().join("\n\n")).unwrap_or_default()
}

fn grade_score(job: &ShenlunJob) -> String {
    let Some(result)=job.result.as_ref() else { return "暂无有效批改结果".into(); };
    match (
        result.get("score").and_then(serde_json::Value::as_f64),
        result.get("max_score").and_then(serde_json::Value::as_f64)
    ) {
        (Some(score), Some(max)) => format!("练习参考分 {score}/{max}（非官方）"),
        _ => "未评分练习诊断：题目分值待确认".into(),
    }
}

fn highlighted_answer(text: &str, result: &serde_json::Value) -> Html {
    let mut ranges = Vec::new();
    if let Some(items)=result.get("items").and_then(serde_json::Value::as_array) {
        for item in items {
            let range=item.get("start").and_then(serde_json::Value::as_u64)
                .and_then(|value|usize::try_from(value).ok())
                .zip(item.get("end").and_then(serde_json::Value::as_u64)
                    .and_then(|value|usize::try_from(value).ok()));
            if let (Some((start,end)),Some(quote))=(range,item.get("quote").and_then(serde_json::Value::as_str)) {
                if start<end && text.get(start..end)==Some(quote) { ranges.push((start,end)); }
            }
        }
    }
    ranges.sort_unstable();
    let mut result_html=Vec::new();
    let mut cursor=0_usize;
    for (start,end) in ranges {
        if start<cursor {continue;}
        if let Some(prefix)=text.get(cursor..start) {
            result_html.push(html!{<span>{prefix.to_owned()}</span>});
        }
        if let Some(quote)=text.get(start..end) {
            result_html.push(html!{<mark class="rounded-sm bg-warning/25" data-testid="shenlun-quote-highlight">{quote.to_owned()}</mark>});
        }
        cursor=end;
    }
    if let Some(rest)=text.get(cursor..) {
        result_html.push(html!{<span>{rest.to_owned()}</span>});
    }
    html!{<div class="whitespace-pre-wrap rounded-box border border-base-content/10 p-3 text-sm" data-testid="shenlun-highlighted-original">{for result_html}</div>}
}

#[function_component(ShenlunAiPanel)]
pub(super) fn shenlun_ai_panel(props: &ShenlunAiPanelProps) -> Html {
    let jobs = use_state(Vec::<ShenlunJob>::new);
    let refresh = use_state(||0_u32);
    let task_busy = use_state(||false);
    let error = use_state(||None::<String>);
    let draft = use_state(||props.attempt.text.clone());
    let images = use_state(Vec::<PendingImage>::new);
    let uploading = use_state(||false);
    let transcribed = use_state(String::new);
    let show_transcription = use_state(||false);
    let selected_job = use_state(||None::<String>);
    let delete_open = use_state(||false);
    let source_answer = use_state(||None::<ShenlunSourceAnswer>);
    let confirm_uncertain = use_state(||false);
    let composer_ref = use_node_ref();

    let attempt_ids = props.attempts.iter().map(|record|record.id.clone()).collect::<Vec<_>>();
    {
        let jobs=jobs.clone();
        let error=error.clone();
        use_effect_with((attempt_ids, *refresh),move |(ids,_)| {
            let alive=Rc::new(Cell::new(true));
            let guard=alive.clone();
            let ids=ids.clone();
            spawn_local(async move {
                let mut collected=Vec::<ShenlunJob>::new();
                for id in ids {
                    let url=format!("/api/v1/question-bank/shenlun/attempts/{id}/jobs");
                    match fetch_json::<Vec<ShenlunJob>>(&url,"申论 AI 历史").await {
                        Ok(mut records) => collected.append(&mut records),
                        Err(message) => { if guard.get() {error.set(Some(message));} return; }
                    }
                }
                if guard.get() {
                    collected.sort_by(|a,b| b.created_at_ms.cmp(&a.created_at_ms).then_with(||b.id.cmp(&a.id)));
                    jobs.set(collected);
                }
            });
            move || alive.set(false)
        });
    }
    {
        let draft=draft.clone();
        let images=images.clone();
        let transcribed=transcribed.clone();
        let show_transcription=show_transcription.clone();
        let selected_job=selected_job.clone();
        let error=error.clone();
        use_effect_with(props.attempt.id.clone(),move |_|{
            draft.set(String::new());
            images.set(Vec::new());
            transcribed.set(String::new());
            show_transcription.set(false);
            selected_job.set(None);
            error.set(None);
            ||()
        });
    }
    {
        let draft=draft.clone();
        let text=props.attempt.text.clone();
        use_effect_with((props.attempt.id.clone(),props.attempt.revision),move |_|{
            draft.set(text.clone());
            ||()
        });
    }
    let latest_grade=jobs.iter().find(|job|
        job.attempt_id==props.attempt.id && job.kind=="grade" && job.status=="complete"
        && job.source_hash==props.attempt.source_hash
        && job.attempt_revision==props.attempt.revision).cloned();
    let running=jobs.iter().find(|job|job.status=="running").cloned();
    {
        let refresh=refresh.clone();
        use_effect_with(running.is_some(),move |active|{
            let interval=if *active {
                let refresh=refresh.clone();
                Some(gloo_timers::callback::Interval::new(1600,move ||refresh.set(refresh.wrapping_add(1))))
            }else{None};
            move || drop(interval)
        });
    }
    let latest_transcription=jobs.iter().find(|job|
        job.attempt_id==props.attempt.id && job.kind=="transcribe" && job.status=="complete"
        && job.attempt_revision==props.attempt.revision).cloned();
    {
        let transcribed=transcribed.clone();
        let show_transcription=show_transcription.clone();
        let current_text=(*draft).clone();
        use_effect_with(latest_transcription.as_ref().map(|job|job.id.clone()),move |_|{
            if let Some(job)=latest_transcription {
                let extracted=transcription_text(&job);
                if !extracted.trim().is_empty() {
                    transcribed.set(if current_text.trim().is_empty() {extracted} else {
                        format!("{current_text}\n\n{extracted}")
                    });
                    show_transcription.set(true);
                }
            }
            ||()
        });
    }

    let start_job = {
        let csrf=props.csrf.clone();
        let attempt=props.attempt.clone();
        let jobs=jobs.clone();
        let task_busy=task_busy.clone();
        let error=error.clone();
        let refresh=refresh.clone();
        let confirm_uncertain=confirm_uncertain.clone();
        Callback::from(move |(kind,attachments):(String,Vec<String>)|{
            if *task_busy || jobs.iter().any(|job|job.status=="running") {return;}
            task_busy.set(true);
            error.set(None);
            let job_request=match super::shenlun_training::new_shenlun_attempt_id(){
                Ok(request_id)=>ShenlunJobRequest {
                    request_id,kind,attachments,confirm_uncertain:*confirm_uncertain
                },
                Err(message)=>{task_busy.set(false);error.set(Some(message));return;}
            };
            let endpoint=format!("/api/v1/question-bank/shenlun/attempts/{}/jobs",attempt.id);
            let csrf=csrf.clone();
            let task_busy=task_busy.clone();
            let error=error.clone();
            let refresh=refresh.clone();
            let jobs=jobs.clone();
            spawn_local(async move {
                let result:Result<ShenlunJob,String>=post_json_response(
                    &endpoint,&csrf,&job_request,"申论 AI 任务"
                ).await;
                match result {
                    Ok(job)=>{
                        let mut current=(*jobs).clone();
                        current.retain(|item|item.id!=job.id);
                        current.insert(0,job);
                        jobs.set(current);
                        refresh.set(refresh.wrapping_add(1));
                    }
                    Err(message)=>error.set(Some(message)),
                }
                task_busy.set(false);
            });
        })
    };
    let submit_grade = {
        let attempt=props.attempt.clone();
        let text=draft.clone();
        let csrf=props.csrf.clone();
        let images=images.clone();
        let uploading=uploading.clone();
        let task_busy=task_busy.clone();
        let error=error.clone();
        let on_confirm=props.on_confirm.clone();
        let jobs=jobs.clone();
        let refresh=refresh.clone();
        let confirm_uncertain=confirm_uncertain.clone();
        let start_job=start_job.clone();
        Callback::from(move |_:MouseEvent| {
            if *task_busy || *uploading {return;}
            if images.len()>0 {
                if attempt.status!="draft" {
                    error.set(Some("图片转写需要新批改，请先在三点菜单中选择「新批改」。".into()));
                    return;
                }
                let attachments:Vec<String>=images.iter().map(|image|image.id.clone()).collect();
                if *text!=attempt.text && !text.trim().is_empty() {
                    // A mixed text+image answer must preserve its typed part before
                    // transcription, whose human confirmation appends the image text.
                    task_busy.set(true);
                    error.set(None);
                    let typed=(*text).clone();
                    let attempt=attempt.clone();
                    let csrf=csrf.clone();
                    let on_confirm=on_confirm.clone();
                    let task_busy=task_busy.clone();
                    let error=error.clone();
                    let jobs=jobs.clone();
                    let refresh=refresh.clone();
                    let confirm_uncertain=*confirm_uncertain;
                    spawn_local(async move {
                        let result:Result<ShenlunJob,String>=async {
                            let saved:ShenlunAttempt=post_json_response(
                                &format!("/api/v1/question-bank/shenlun/attempts/{}/draft",attempt.id),
                                &csrf,&SaveShenlunDraft{expected_revision:attempt.revision,text:typed},
                                "保存图片批改中的文字"
                            ).await?;
                            on_confirm.emit(saved.clone());
                            let request_id=super::shenlun_training::new_shenlun_attempt_id()?;
                            post_json_response(
                                &format!("/api/v1/question-bank/shenlun/attempts/{}/jobs",saved.id),
                                &csrf,&ShenlunJobRequest{
                                    request_id,kind:"transcribe".into(),attachments,
                                    confirm_uncertain,
                                },"识别手写图片"
                            ).await
                        }.await;
                        match result {
                            Ok(job)=>{
                                let mut history=(*jobs).clone();
                                history.retain(|item|item.id!=job.id);
                                history.insert(0,job);
                                jobs.set(history);
                                refresh.set(refresh.wrapping_add(1));
                            }
                            Err(message)=>error.set(Some(message)),
                        }
                        task_busy.set(false);
                    });
                } else {
                    start_job.emit(("transcribe".into(),attachments));
                }
                return;
            }
            if text.trim().is_empty(){
                error.set(Some("请先输入作答内容，或上传手写图片。".into()));
                return;
            }
            if attempt.status=="submitted" {
                if *text!=attempt.text {
                    error.set(Some("已批改原稿不可修改，请通过「新批改」创建新的作答快照。".into()));
                } else {
                    start_job.emit(("grade".into(),Vec::new()));
                }
                return;
            }
            task_busy.set(true);
            error.set(None);
            let attempt=attempt.clone();
            let text=(*text).clone();
            let csrf=csrf.clone();
            let task_busy=task_busy.clone();
            let error=error.clone();
            let on_confirm=on_confirm.clone();
            let jobs=jobs.clone();
            let refresh=refresh.clone();
            let confirm_uncertain=confirm_uncertain.clone();
            spawn_local(async move {
                let result:Result<(ShenlunAttempt,ShenlunJob),String>=async {
                    let saved=if text!=attempt.text {
                        let body=SaveShenlunDraft{expected_revision:attempt.revision,text};
                        post_json_response::<_,ShenlunAttempt>(
                            &format!("/api/v1/question-bank/shenlun/attempts/{}/draft",attempt.id),
                            &csrf,&body,"保存批改输入"
                        ).await?
                    }else{attempt.clone()};
                    let submitted:ShenlunAttempt=post_json_response(
                        &format!("/api/v1/question-bank/shenlun/attempts/{}/submit",saved.id),
                        &csrf,&EmptyRequest{},"冻结批改原稿"
                    ).await?;
                    // The source snapshot has already been frozen. Surface it
                    // even when the subsequent Codex request fails, so users
                    // can retry grading without resubmitting or losing text.
                    on_confirm.emit(submitted.clone());
                    let request_id=super::shenlun_training::new_shenlun_attempt_id()?;
                    let request=ShenlunJobRequest{
                        request_id,kind:"grade".into(),attachments:Vec::new(),
                        confirm_uncertain:*confirm_uncertain,
                    };
                    let job:ShenlunJob=post_json_response(
                        &format!("/api/v1/question-bank/shenlun/attempts/{}/jobs",submitted.id),
                        &csrf,&request,"申论 AI 批改"
                    ).await?;
                    Ok((submitted,job))
                }.await;
                match result {
                    Ok((_submitted,job))=>{
                        let mut current=(*jobs).clone();
                        current.retain(|item|item.id!=job.id);
                        current.insert(0,job);
                        jobs.set(current);
                        refresh.set(refresh.wrapping_add(1));
                    }
                    Err(message)=>error.set(Some(message)),
                }
                task_busy.set(false);
            });
        })
    };
    let start_ocr = {
        let images=images.clone();
        let start_job=start_job.clone();
        let error=error.clone();
        let attempt=props.attempt.clone();
        Callback::from(move |_|{
            if attempt.status!="draft" {
                error.set(Some("此稿已完成提交；使用三点菜单「新批改」开始新的图片转写。".into()));
            } else if images.is_empty(){
                error.set(Some("请选择需要转写的图片。".into()));
            } else {
                start_job.emit(("transcribe".into(),images.iter().map(|image|image.id.clone()).collect()));
            }
        })
    };
    let confirm_text = {
        let attempt=props.attempt.clone();
        let transcription=latest_transcription.clone();
        let transcribed=transcribed.clone();
        let csrf=props.csrf.clone();
        let task_busy=task_busy.clone();
        let error=error.clone();
        let on_confirm=props.on_confirm.clone();
        let images=images.clone();
        let draft=draft.clone();
        let show_transcription=show_transcription.clone();
        Callback::from(move |_| {
            let Some(job)=transcription.as_ref() else {return;};
            if transcribed.trim().is_empty() || *task_busy {return;}
            let body=ConfirmShenlunTranscription{
                job_id:job.id.clone(),expected_revision:attempt.revision,text:(*transcribed).clone()
            };
            task_busy.set(true);
            let task_busy=task_busy.clone();
            let error=error.clone();
            let on_confirm=on_confirm.clone();
            let images=images.clone();
            let draft=draft.clone();
            let show_transcription=show_transcription.clone();
            let csrf=csrf.clone();
            let url=format!("/api/v1/question-bank/shenlun/attempts/{}/transcription/confirm",attempt.id);
            spawn_local(async move {
                let result:Result<ShenlunAttempt,String>=post_json_response(
                    &url,&csrf,&body,"确认申论手写转写"
                ).await;
                match result {
                    Ok(saved)=>{
                        draft.set(saved.text.clone());
                        on_confirm.emit(saved);
                        images.set(Vec::new());
                        show_transcription.set(false);
                    }
                    Err(message)=>error.set(Some(message)),
                }
                task_busy.set(false);
            });
        })
    };
    let stop = {
        let running=running.clone();
        let csrf=props.csrf.clone();
        let refresh=refresh.clone();
        let error=error.clone();
        Callback::from(move |_|{
            let Some(job)=running.as_ref() else {return;};
            let csrf=csrf.clone();
            let refresh=refresh.clone();
            let error=error.clone();
            let url=format!("/api/v1/question-bank/shenlun/jobs/{}/stop",job.id);
            spawn_local(async move {
                let result:Result<ShenlunJob,String>=post_json_response(
                    &url,&csrf,&EmptyRequest{},"停止申论 AI"
                ).await;
                match result {
                    Ok(_)=>refresh.set(refresh.wrapping_add(1)),
                    Err(message)=>error.set(Some(message)),
                }
            });
        })
    };
    let fetch_source = {
        let id=props.attempt.id.clone();
        let source_answer=source_answer.clone();
        let error=error.clone();
        Callback::from(move |_:()|{
            let source_answer=source_answer.clone();
            let error=error.clone();
            let url=format!("/api/v1/question-bank/shenlun/attempts/{id}/source-answer");
            spawn_local(async move {
                match fetch_json::<ShenlunSourceAnswer>(&url,"来源参考答案").await {
                    Ok(answer)=>source_answer.set(Some(answer)),
                    Err(message)=>error.set(Some(message)),
                }
            });
        })
    };
    {
        let fetch_source=fetch_source.clone();
        use_effect_with((props.mode.clone(),props.attempt.id.clone()),move |(mode,_)|{
            if mode=="source" {fetch_source.emit(());}
            ||()
        });
    }
    {
        let prior=use_mut_ref(||props.regrade_signal);
        let start_job=start_job.clone();
        let attempt=props.attempt.clone();
        let error=error.clone();
        use_effect_with(props.regrade_signal,move |signal|{
            if *signal!=*prior.borrow() {
                if attempt.status=="submitted" {
                    start_job.emit(("grade".into(),Vec::new()));
                } else {
                    error.set(Some("还没有完成本题的首次批改，请输入答案并发送。".into()));
                }
            }
            *prior.borrow_mut()=*signal;
            ||()
        });
    }
    {
        let prior=use_mut_ref(||props.delete_signal);
        let delete_open=delete_open.clone();
        use_effect_with(props.delete_signal,move |signal|{
            if *signal!=*prior.borrow() {delete_open.set(true);}
            *prior.borrow_mut()=*signal;
            ||()
        });
    }
    let display_job=(*selected_job).as_ref().and_then(|id|jobs.iter().find(|job|job.id==*id)).cloned()
        .or_else(||latest_grade.clone());
    let delete = {
        let selected=display_job.clone();
        let csrf=props.csrf.clone();
        let refresh=refresh.clone();
        let delete_open=delete_open.clone();
        let selected_job=selected_job.clone();
        let error=error.clone();
        let task_busy=task_busy.clone();
        Callback::from(move |_|{
            let Some(job)=selected.as_ref() else {
                delete_open.set(false);
                error.set(Some("请先在历史记录中选择要删除的 AI 批改。".into()));
                return;
            };
            if matches!(job.status.as_str(),"running"|"unknown") {
                error.set(Some("运行中或状态不确定的任务须先停止或确认状态，不能直接删除。".into()));
                return;
            }
            task_busy.set(true);
            let csrf=csrf.clone();
            let refresh=refresh.clone();
            let delete_open=delete_open.clone();
            let selected_job=selected_job.clone();
            let error=error.clone();
            let task_busy=task_busy.clone();
            let url=format!("/api/v1/question-bank/shenlun/jobs/{}/delete",job.id);
            spawn_local(async move {
                let result:Result<serde_json::Value,String>=post_json_response(
                    &url,&csrf,&EmptyRequest{},"删除当前 AI 批改"
                ).await;
                match result {
                    Ok(_)=>{
                        delete_open.set(false);
                        selected_job.set(None);
                        refresh.set(refresh.wrapping_add(1));
                    },
                    Err(message)=>error.set(Some(message)),
                }
                task_busy.set(false);
            });
        })
    };
    let oninput={
        let draft=draft.clone();
        Callback::from(move |event:InputEvent|{
            let input:web_sys::HtmlTextAreaElement=event.target_unchecked_into();
            draft.set(input.value());
        })
    };
    let on_transcribed={
        let transcribed=transcribed.clone();
        Callback::from(move |event:InputEvent|{
            let input:web_sys::HtmlTextAreaElement=event.target_unchecked_into();
            transcribed.set(input.value());
        })
    };
    let compose_endpoint=format!("/api/v1/question-bank/practice/questions/{}/ai/attachments",props.attempt.question_id);
    let is_running=running.is_some();
    html!{
        <section class="mt-3 space-y-3" data-testid="shenlun-ai-panel">
            if props.mode=="grade" {
                <div class="flex items-center justify-between">
                    <h3 class="text-sm font-semibold">{"AI 批改"}</h3>
                    <span class="text-xs text-base-content/55">
                        {if is_running {"正在批改"} else if latest_grade.is_some() {"已有评分报告"} else {"文字或手写图片"}}
                    </span>
                </div>
                {ai_composer(AiComposer {
                    id:"shenlun-grade-input",input_ref:composer_ref.clone(),
                    compose_testid:"shenlun-grade-compose",input_testid:"shenlun-answer-input",
                    button_testid:if is_running {"shenlun-grade-stop"} else {"shenlun-grade-start"},
                    value:draft.as_str(),max_length:50000,read_only:props.attempt.status=="submitted",placeholder:"粘贴申论答案，或上传手写图片…",
                    button_label:if is_running {"停止批改"} else {"发送批改"},
                    is_stop:is_running,
                    disabled:*task_busy || *uploading || (!is_running && (props.attempt.status=="submitted" || (draft.trim().is_empty() && images.is_empty()))),
                    endpoint:compose_endpoint.clone(),csrf:props.csrf.clone(),
                    attachments:images.clone(),uploading:uploading.clone(),
                    picker_disabled:*task_busy || is_running,picker_testid:"shenlun-grade-images",
                    oninput,onkeydown:Callback::from(|_:KeyboardEvent|{}),
                    onclick:if is_running {stop.clone()} else {submit_grade.clone()}
                })}
                if images.len()>0 {
                    <p class="text-xs text-base-content/55">{"含图片的批改会先转写，核对确认文字后才能评分，不根据模糊字迹猜测。"}
                    </p>
                }
                if let Some(job)=latest_grade.as_ref() {
                    <h4 class="font-semibold" data-testid="shenlun-score">{grade_score(job)}</h4>
                }
            } else if props.mode=="ocr" {
                <h3 class="text-sm font-semibold">{"OCR 图片转写"}</h3>
                <p class="text-xs text-base-content/60">{"只接受图片。Codex 先读原图，必要时使用 OCR 辅助；确认前不进入评分。"}</p>
                <AiImagePicker endpoint={compose_endpoint.clone()} csrf={props.csrf.clone()}
                    attachments={images.clone()} uploading={uploading.clone()}
                    disabled={*task_busy || is_running} testid="shenlun-ocr-images">
                    <span class="text-sm">{"上传手写图片"}</span>
                </AiImagePicker>
                <button type="button" class="btn btn-primary btn-sm" data-testid="shenlun-transcribe-start"
                    disabled={*task_busy || *uploading || is_running || images.is_empty()}
                    onclick={start_ocr}>{"转写图片"}</button>
            } else if props.mode=="source" {
                <h3 class="text-sm font-semibold">{"来源参考答案"}</h3>
                if let Some(answer)=(*source_answer).as_ref() {
                    <div class="rounded-box border border-base-content/10 p-3" data-testid="shenlun-source-answer">
                        <p class="text-xs opacity-60">{answer.provenance.clone()}</p>
                        <div class="whitespace-pre-wrap text-sm">{answer.text.clone()}</div>
                    </div>
                } else {
                    <p class="text-sm text-base-content/60">{"仅在本题已有完成批改时解锁题库来源答案。"}</p>
                }
            } else if props.mode=="history" {
                <h3 class="text-sm font-semibold">{"本题 AI 批改历史"}</h3>
                <p class="text-xs opacity-60">{"按题目保留每次答案快照和评分。选择记录查看，删除不会影响原题和其他批改。"}</p>
                <div class="flex flex-wrap gap-2" data-testid="shenlun-grade-history">
                    {for jobs.iter().filter(|job| job.kind=="grade").map(|job|{
                        let id=job.id.clone();
                        let selected_job=selected_job.clone();
                        html!{
                            <button key={id.clone()} type="button"
                                class={classes!("btn","btn-sm",if (*selected_job).as_ref()==Some(&id) {"btn-primary"} else {"btn-outline"})}
                                data-testid="shenlun-job-history"
                                onclick={Callback::from(move |_|selected_job.set(Some(id.clone())))}>
                                {format!("{} · {}",grade_score(job),job.status)}
                            </button>
                        }
                    })}
                </div>
            }

            if *show_transcription && matches!(props.mode.as_str(),"ocr"|"grade") {
                <div class="space-y-2 rounded-box border border-primary/30 p-3" data-testid="shenlun-transcription-confirm">
                    <h4 class="font-semibold">{"逐页校对转写内容"}</h4>
                    <p class="text-xs opacity-60">{"请对照原图，修正所有辨认不清的文字；确认后仍可切换到批改。"}</p>
                    <textarea class="textarea textarea-bordered min-h-40 w-full" rows="7"
                        data-testid="shenlun-transcribed-text" value={(*transcribed).clone()}
                        oninput={on_transcribed}/>
                    <button type="button" class="btn btn-primary btn-sm"
                        data-testid="shenlun-transcription-confirm-button"
                        disabled={*task_busy || transcribed.trim().is_empty()}
                        onclick={confirm_text}>{"确认转写文本"}</button>
                </div>
            }

            if let Some(job)=display_job.as_ref() {
                if job.kind=="grade" && matches!(props.mode.as_str(),"grade"|"history") {
                    if let Some(result)=job.result.as_ref() {
                        <article class="space-y-2 rounded-box border border-base-content/10 p-3" data-testid="shenlun-result">
                            <h4 class="font-semibold">{grade_score(job)}</h4>
                            if let Some(original)=props.attempts.iter().find(|record|record.id==job.attempt_id) {
                                <h5 class="text-sm font-semibold">{"原稿命中位置"}</h5>
                                {highlighted_answer(&original.text,result)}
                            }
                            <p class="text-sm">{result.get("summary").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                            if let Some(items)=result.get("items").and_then(serde_json::Value::as_array){
                                {for items.iter().map(|item|html!{
                                    <div class="rounded border border-base-content/10 p-2 text-sm">
                                        <strong>{item.get("criterion").and_then(serde_json::Value::as_str).unwrap_or("")}</strong>
                                        <span class="ml-2 opacity-70">{item.get("coverage").and_then(serde_json::Value::as_str).unwrap_or("")}</span>
                                        <p>{"我的原句："}{item.get("quote").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                                        <p>{"材料依据："}{item.get("evidence").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                                        <p>{"建议："}{item.get("suggestion").and_then(serde_json::Value::as_str).unwrap_or("")}</p>
                                    </div>
                                })}
                            }
                        </article>
                    }
                }
            }
            if let Some(job)=running.as_ref() {
                <div role="status" class="flex items-center gap-2 text-xs text-base-content/60">
                    <span class="loading loading-spinner loading-xs"></span>
                    {format!("{} 正在执行…",job.kind)}
                </div>
            }
            if jobs.iter().any(|job|job.status=="unknown") {
                <label class="flex items-center gap-2 text-xs text-warning">
                    <input type="checkbox" class="checkbox checkbox-sm" checked={*confirm_uncertain}
                        onchange={Callback::from({let confirm_uncertain=confirm_uncertain.clone();
                            move |event:Event|{
                                let input:web_sys::HtmlInputElement=event.target_unchecked_into();
                                confirm_uncertain.set(input.checked());
                            }
                        })}/>
                    {"远端状态无法确认；允许新的执行可能重复消耗额度，我已知悉。"}
                </label>
            }
            if let Some(message)=(*error).as_ref(){
                <p role="alert" class="text-sm text-error">{message.clone()}</p>
            }
            if *delete_open {
                <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 p-4"
                    role="dialog" aria-modal="true" aria-label="确认删除 AI 批改">
                    <div class="w-full max-w-sm space-y-3 rounded-box bg-base-100 p-5 shadow-xl">
                        <h3 class="font-semibold">{"删除当前 AI 批改记录？"}</h3>
                        <p class="text-sm">{"只删除当前选中的 AI 批改及其报告，不删除原卷、其他记录和作答快照。"}</p>
                        <div class="flex justify-end gap-2">
                            <button type="button" class="btn btn-ghost btn-sm"
                                onclick={Callback::from({let delete_open=delete_open.clone();move |_|delete_open.set(false)})}>{"取消"}</button>
                            <button type="button" class="btn btn-error btn-sm" data-testid="shenlun-delete-confirm"
                                disabled={*task_busy || display_job.is_none()} onclick={delete}>{"确认删除"}</button>
                        </div>
                    </div>
                </div>
            }
        </section>
    }
}
