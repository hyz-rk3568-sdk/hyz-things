//! 申论 OCR 是临时 Codex Markdown 回答；批改、解析与追问复用 QuestionAiReview。
use super::*;
use hyz_things::domain::question_bank::ShenlunSourceAnswer;

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunAiPanelProps {
    pub(super) question_id: String,
    pub(super) csrf: String,
}

#[derive(serde::Serialize)]
struct OcrRequest {
    attachments: Vec<String>,
}

#[derive(serde::Deserialize)]
struct OcrResponse {
    markdown: String,
}

#[function_component(ShenlunAiPanel)]
pub(super) fn shenlun_ai_panel(props: &ShenlunAiPanelProps) -> Html {
    let busy = use_state(|| false);
    let error = use_state(|| None::<String>);
    let result = use_state(|| None::<String>);
    let images = use_state(Vec::<PendingImage>::new);
    let uploading = use_state(|| false);
    let start_ocr = {
        let images = images.clone();
        let busy = busy.clone();
        let error = error.clone();
        let result = result.clone();
        let csrf = props.csrf.clone();
        let question_id = props.question_id.clone();
        Callback::from(move |_| {
            if *busy || *uploading || images.is_empty() {
                return;
            }
            busy.set(true);
            error.set(None);
            result.set(None);
            let body = OcrRequest {
                attachments: images.iter().map(|image| image.id.clone()).collect(),
            };
            let endpoint = format!("/api/v1/question-bank/shenlun/questions/{question_id}/ocr");
            let busy = busy.clone();
            let error = error.clone();
            let result = result.clone();
            let images = images.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                match post_json_response::<_, OcrResponse>(
                    &endpoint, &csrf, &body, "手写图片转写",
                ).await {
                    Ok(response) => {
                        result.set(Some(response.markdown));
                        images.set(Vec::new());
                    }
                    Err(message) => error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let image_endpoint = format!(
        "/api/v1/question-bank/practice/questions/{}/ai/attachments",
        props.question_id
    );
    html! {
        <section class="mt-3 space-y-3" data-testid="shenlun-ai-panel">
            <h3 class="font-semibold">{"手写图片 OCR"}</h3>
            <p class="text-sm text-base-content/60">
                {"临时 Codex 识别：直接返回 Markdown 原文，不创建批改记录、不要求确认，也不会自动评分。"}
            </p>
            <AiImagePicker endpoint={image_endpoint} csrf={props.csrf.clone()}
                attachments={images.clone()} uploading={uploading.clone()}
                disabled={*busy || *uploading} testid="shenlun-ocr-images">
                <span class="text-sm">{"上传手写图片"}</span>
            </AiImagePicker>
            <button type="button" class="btn btn-primary btn-sm" data-testid="shenlun-transcribe-start"
                disabled={*busy || *uploading || images.is_empty()} onclick={start_ocr}>
                {if *busy {"正在识别…"} else {"转写为 Markdown"}}
            </button>
            if let Some(markdown) = (*result).as_ref() {
                <article class="study-markdown space-y-2 rounded-box border border-base-content/10 p-3"
                    data-testid="shenlun-ocr-markdown">
                    {render_question_bank_user_markdown(markdown)}
                </article>
            }
            if let Some(message) = (*error).as_ref() {
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
    let answer = use_state(|| None::<ShenlunSourceAnswer>);
    let error = use_state(|| None::<String>);
    {
        let answer = answer.clone();
        let error = error.clone();
        use_effect_with(props.question_id.clone(), move |question_id| {
            let alive = Rc::new(Cell::new(true));
            let guard = alive.clone();
            let endpoint =
                format!("/api/v1/question-bank/shenlun/questions/{question_id}/source-answer");
            spawn_local(async move {
                match fetch_json::<ShenlunSourceAnswer>(&endpoint, "题库来源参考答案").await {
                    Ok(value) if guard.get() => answer.set(Some(value)),
                    Err(message) if guard.get() => error.set(Some(message)),
                    _ => (),
                }
            });
            move || alive.set(false)
        });
    }
    html! {
        <section class="mt-3 space-y-3" data-testid="shenlun-source-panel">
            <h3 class="font-semibold">{"题库来源参考答案"}</h3>
            if let Some(value) = (*answer).as_ref() {
                <article class="study-markdown" data-testid="shenlun-source-answer">
                    <p class="text-xs opacity-60">{value.provenance.clone()}</p>
                    {render_question_bank_markdown(&value.text)}
                </article>
            } else {
                <p class="text-sm text-base-content/60">{"本题完成 AI 批改后可查看来源参考答案。"}</p>
            }
            if let Some(message) = (*error).as_ref() {
                <p role="alert" class="text-sm text-error">{message.clone()}</p>
            }
        </section>
    }
}
