//! 申论 OCR 与题库来源答案。批改/追问统一复用 QuestionAiReview。
//! 不创建评分任务、不显示评分历史，也不在这里生成 Markdown 报告。
use super::*;
use hyz_things::domain::question_bank::{
    ConfirmShenlunTranscription, ShenlunAttempt, ShenlunJob, ShenlunJobRequest,
    ShenlunSourceAnswer,
};

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunAiPanelProps {
    pub(super) attempt: ShenlunAttempt,
    pub(super) csrf: String,
    pub(super) on_confirm: Callback<ShenlunAttempt>,
}

fn transcription_text(job: &ShenlunJob) -> String {
    job.result.as_ref()
        .and_then(|result| result.get("pages"))
        .and_then(serde_json::Value::as_array)
        .map(|pages| pages.iter().filter_map(|page|
            page.get("text").and_then(serde_json::Value::as_str)
        ).collect::<Vec<_>>().join("\n\n"))
        .unwrap_or_default()
}

#[function_component(ShenlunAiPanel)]
pub(super) fn shenlun_ai_panel(props: &ShenlunAiPanelProps) -> Html {
    let jobs=use_state(Vec::<ShenlunJob>::new);
    let refresh=use_state(||0_u32);
    let busy=use_state(||false);
    let error=use_state(||None::<String>);
    let images=use_state(Vec::<PendingImage>::new);
    let uploading=use_state(||false);
    let transcribed=use_state(String::new);
    let show_confirmation=use_state(||false);
    {
        let jobs=jobs.clone();
        let error=error.clone();
        use_effect_with((props.attempt.id.clone(),*refresh),move |(id,_)|{
            let alive=Rc::new(Cell::new(true));
            let guard=alive.clone();
            let endpoint=format!("/api/v1/question-bank/shenlun/attempts/{id}/jobs");
            spawn_local(async move {
                match fetch_json::<Vec<ShenlunJob>>(&endpoint,"手写转写状态").await {
                    Ok(items) if guard.get()=>jobs.set(items),
                    Err(message) if guard.get()=>error.set(Some(message)),
                    _=>()
                }
            });
            move ||alive.set(false)
        });
    }
    let running=jobs.iter().find(|job|job.kind=="transcribe" && job.status=="running").cloned();
    {
        let refresh=refresh.clone();
        use_effect_with(running.is_some(),move |active|{
            let timer=if *active {
                let refresh=refresh.clone();
                Some(gloo_timers::callback::Interval::new(1600,move ||refresh.set(refresh.wrapping_add(1))))
            }else{None};
            move ||drop(timer)
        });
    }
    let transcription=jobs.iter().find(|job|job.kind=="transcribe" && job.status=="complete").cloned();
    {
        let transcribed=transcribed.clone();
        let show_confirmation=show_confirmation.clone();
        let text=props.attempt.text.clone();
        use_effect_with(transcription.as_ref().map(|job|job.id.clone()),move |_|{
            if let Some(job)=transcription {
                let extracted=transcription_text(&job);
                if !extracted.trim().is_empty(){
                    transcribed.set(if text.trim().is_empty(){extracted}else{format!("{text}\n\n{extracted}")});
                    show_confirmation.set(true);
                }
            }
            ||()
        });
    }
    let start_ocr={
        let images=images.clone();
        let busy=busy.clone();
        let error=error.clone();
        let csrf=props.csrf.clone();
        let attempt=props.attempt.clone();
        let refresh=refresh.clone();
        Callback::from(move |_|{
            if *busy || images.is_empty(){return;}
            if attempt.status!="draft" {
                error.set(Some("当前已确认的原稿不可覆盖，请在新题目中重新转写。".into()));
                return;
            }
            let request_id=match super::shenlun_training::new_shenlun_attempt_id(){
                Ok(id)=>id,Err(message)=>{error.set(Some(message));return;}
            };
            busy.set(true);error.set(None);
            let busy=busy.clone();let error=error.clone();let refresh=refresh.clone();
            let endpoint=format!("/api/v1/question-bank/shenlun/attempts/{}/jobs",attempt.id);
            let csrf=csrf.clone();
            let attachment_ids=images.iter().map(|image|image.id.clone()).collect::<Vec<_>>();
            spawn_local(async move {
                let body=ShenlunJobRequest{request_id,kind:"transcribe".into(),attachments:attachment_ids,confirm_uncertain:false};
                let result:Result<ShenlunJob,String>=post_json_response(&endpoint,&csrf,&body,"申论图片转写").await;
                match result {
                    Ok(_)=>refresh.set(refresh.wrapping_add(1)),
                    Err(message)=>error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let stop_ocr={
        let job=running.clone();
        let csrf=props.csrf.clone();
        let refresh=refresh.clone();
        let error=error.clone();
        Callback::from(move |_|{
            let Some(job)=job.as_ref() else{return;};
            let endpoint=format!("/api/v1/question-bank/shenlun/jobs/{}/stop",job.id);
            let csrf=csrf.clone();let refresh=refresh.clone();let error=error.clone();
            spawn_local(async move {
                let result:Result<ShenlunJob,String>=post_json_response(
                    &endpoint,&csrf,&EmptyRequest{},"停止手写转写").await;
                match result {
                    Ok(_)=>refresh.set(refresh.wrapping_add(1)),
                    Err(message)=>error.set(Some(message)),
                }
            });
        })
    };
    let confirm={
        let transcription=transcription.clone();
        let transcribed=transcribed.clone();
        let busy=busy.clone();
        let csrf=props.csrf.clone();
        let attempt=props.attempt.clone();
        let on_confirm=props.on_confirm.clone();
        let show_confirmation=show_confirmation.clone();
        let refresh=refresh.clone();
        let images=images.clone();
        let error=error.clone();
        Callback::from(move |_|{
            let Some(job)=transcription.as_ref() else{return;};
            if *busy || transcribed.trim().is_empty(){return;}
            busy.set(true);
            let payload=ConfirmShenlunTranscription{
                job_id:job.id.clone(),expected_revision:attempt.revision,text:(*transcribed).clone()
            };
            let endpoint=format!("/api/v1/question-bank/shenlun/attempts/{}/transcription/confirm",attempt.id);
            let csrf=csrf.clone();let busy=busy.clone();let error=error.clone();
            let on_confirm=on_confirm.clone();let images=images.clone();
            let show_confirmation=show_confirmation.clone();let refresh=refresh.clone();
            spawn_local(async move {
                let result:Result<ShenlunAttempt,String>=post_json_response(&endpoint,&csrf,&payload,"确认手写转写").await;
                match result {
                    Ok(saved)=>{
                        on_confirm.emit(saved);
                        show_confirmation.set(false);
                        images.set(Vec::new());
                        refresh.set(refresh.wrapping_add(1));
                    }
                    Err(message)=>error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let image_endpoint=format!("/api/v1/question-bank/practice/questions/{}/ai/attachments",props.attempt.question_id);
    html!{
        <section class="mt-3 space-y-3" data-testid="shenlun-ai-panel">
                <h3 class="font-semibold">{"手写图片转写"}</h3>
                <p class="text-sm text-base-content/60">{"先核对原图，确认转写文本后可复制到「AI 批改」中发送。转写本身不会评分。"}</p>
                <AiImagePicker endpoint={image_endpoint} csrf={props.csrf.clone()}
                    attachments={images.clone()} uploading={uploading.clone()}
                    disabled={*busy || *uploading || running.is_some()} testid="shenlun-ocr-images">
                    <span class="text-sm">{"上传手写图片"}</span>
                </AiImagePicker>
                <button type="button" class="btn btn-primary btn-sm" data-testid="shenlun-transcribe-start"
                    disabled={*busy || *uploading || running.is_some() || images.is_empty()}
                    onclick={start_ocr}>{"转写图片"}</button>
                if let Some(job)=running.as_ref(){
                    <div class="flex items-center gap-2 text-sm" role="status">
                        <span class="loading loading-spinner loading-xs"></span>{"正在转写…"}
                        <button type="button" class="btn btn-ghost btn-xs" onclick={stop_ocr}>{"停止"}</button>
                    </div>
                    <span class="hidden">{job.id.clone()}</span>
                }
                if *show_confirmation{
                    <div class="space-y-2 rounded-box border border-primary/30 p-3" data-testid="shenlun-transcription-confirm">
                        <h4 class="font-semibold">{"核对逐页转写"}</h4>
                        <p class="text-xs opacity-60">{"请对照原图逐句修正；确认前不能作为评分依据。"}</p>
                        <textarea class="textarea textarea-bordered min-h-40 w-full" rows="8"
                            data-testid="shenlun-transcribed-text" value={(*transcribed).clone()}
                            oninput={Callback::from({let transcribed=transcribed.clone();move |event:InputEvent|{
                                let textarea:web_sys::HtmlTextAreaElement=event.target_unchecked_into();
                                transcribed.set(textarea.value());
                            }})}/>
                        <button type="button" class="btn btn-primary btn-sm"
                            data-testid="shenlun-transcription-confirm-button"
                            disabled={*busy || transcribed.trim().is_empty()} onclick={confirm}>{"确认转写"}</button>
                    </div>
                }
                if !props.attempt.text.trim().is_empty(){
                    <div class="space-y-2">
                        <h4 class="text-sm font-semibold">{"已确认文本（复制后在 AI 批改中发送）"}</h4>
                        <div class="whitespace-pre-wrap rounded-box border border-base-content/10 p-3 text-sm"
                            data-testid="shenlun-ocr-confirmed-text">{props.attempt.text.clone()}</div>
                    </div>
                }
            if let Some(message)=(*error).as_ref(){
                <p role="alert" class="text-sm text-error">{message.clone()}</p>
            }
        </section>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunSourceAnswerPanelProps {
    pub(super) question_id: String,
}

#[function_component(ShenlunSourceAnswerPanel)]
pub(super) fn shenlun_source_answer_panel(props: &ShenlunSourceAnswerPanelProps) -> Html {
    let answer=use_state(||None::<ShenlunSourceAnswer>);
    let error=use_state(||None::<String>);
    {
        let answer=answer.clone();
        let error=error.clone();
        use_effect_with(props.question_id.clone(),move |question_id|{
            let alive=Rc::new(Cell::new(true));
            let guard=alive.clone();
            let endpoint=format!("/api/v1/question-bank/shenlun/questions/{question_id}/source-answer");
            spawn_local(async move {
                match fetch_json::<ShenlunSourceAnswer>(&endpoint,"题库来源参考答案").await {
                    Ok(value) if guard.get()=>answer.set(Some(value)),
                    Err(message) if guard.get()=>error.set(Some(message)),
                    _=>()
                }
            });
            move ||alive.set(false)
        });
    }
    html! {
        <section class="mt-3 space-y-3" data-testid="shenlun-source-panel">
            <h3 class="font-semibold">{"题库来源参考答案"}</h3>
            if let Some(value)=(*answer).as_ref(){
                <article class="study-markdown" data-testid="shenlun-source-answer">
                    <p class="text-xs opacity-60">{value.provenance.clone()}</p>
                    {render_question_bank_markdown(&value.text)}
                </article>
            } else {
                <p class="text-sm text-base-content/60">{"本题完成 AI 批改后可查看来源参考答案。"}</p>
            }
            if let Some(message)=(*error).as_ref(){
                <p role="alert" class="text-sm text-error">{message.clone()}</p>
            }
        </section>
    }
}
