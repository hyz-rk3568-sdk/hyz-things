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

#[derive(Clone, PartialEq, serde::Deserialize)]
struct OcrEvent {
    kind: String,
    payload: serde_json::Value,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
struct OcrResponse {
    markdown: String,
    events: Vec<OcrEvent>,
}

/// Use the same command/tool item renderer as the Question Bank AI review.
fn ocr_tools(events: &[OcrEvent]) -> Vec<(String, String, String, String)> {
    let mut tools: Vec<(String, String, String, String)> = Vec::new();
    for event in events {
        let id = event
            .payload
            .get("item_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        match event.kind.as_str() {
            "tool_started" => tools.push((
                id,
                event
                    .payload
                    .get("tool")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("工具")
                    .to_owned(),
                event
                    .payload
                    .pointer("/details/command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                String::new(),
            )),
            "command_output_delta" | "tool_output_delta" => {
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.0 == id) {
                    tool.3.push_str(
                        event
                            .payload
                            .get("text")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(""),
                    );
                }
            }
            "tool_progress" => {
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.0 == id) {
                    tool.3.push_str(
                        event
                            .payload
                            .get("message")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(""),
                    );
                    tool.3.push('\n');
                }
            }
            "tool_finished" => {
                if let Some(tool) = tools.iter_mut().rev().find(|tool| tool.0 == id) {
                    tool.1 = format!(
                        "{} · {}",
                        tool.1,
                        event
                            .payload
                            .get("status")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("完成")
                    );
                }
            }
            _ => (),
        }
    }
    tools
}

#[function_component(ShenlunAiPanel)]
pub(super) fn shenlun_ai_panel(props: &ShenlunAiPanelProps) -> Html {
    let busy = use_state(|| false);
    let error = use_state(|| None::<String>);
    let result = use_state(|| None::<OcrResponse>);
    let images = use_state(Vec::<PendingImage>::new);
    let uploading = use_state(|| false);
    let ocr_input_ref = use_node_ref();
    let start_ocr = {
        let images = images.clone();
        let busy = busy.clone();
        let error = error.clone();
        let result = result.clone();
        let uploading = uploading.clone();
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
                match post_json_response::<_, OcrResponse>(&endpoint, &csrf, &body, "手写图片转写")
                    .await
                {
                    Ok(response) => {
                        result.set(Some(response));
                        images.set(Vec::new());
                    }
                    Err(message) => error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let tools = (*result)
        .as_ref()
        .map(|response| ocr_tools(&response.events))
        .unwrap_or_default();
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
            {ai_composer(AiComposer {
                id: "shenlun-ocr-input",
                input_ref: ocr_input_ref,
                compose_testid: "shenlun-ocr-composer",
                input_testid: "shenlun-ocr-input",
                button_testid: "shenlun-transcribe-start",
                value: "",
                max_length: 0,
                read_only: true,
                placeholder: "仅上传手写图片，不支持文字输入…",
                button_label: if *busy {"正在识别…"} else {"转写为 Markdown"},
                is_stop: false,
                disabled: *busy || *uploading || images.is_empty(),
                endpoint: image_endpoint,
                csrf: props.csrf.clone(),
                attachments: images.clone(),
                uploading: uploading.clone(),
                picker_disabled: *busy || *uploading,
                picker_testid: "shenlun-ocr-images",
                oninput: Callback::noop(),
                onkeydown: Callback::noop(),
                onclick: start_ocr,
            })}
            if let Some(response) = (*result).as_ref() {
                if !tools.is_empty() {
                    <section class="py-1" data-testid="shenlun-ocr-tool-activity">
                        <p class="text-xs text-base-content/60">{"工具调用 · 本次临时转写"}</p>
                        <div class="mt-2 space-y-3 border-l border-base-content/15 pl-3 text-xs text-base-content/60">
                            {for tools.iter().map(|(_, label, command, output)| {
                                super::ai_review_activity::tool_execution_item(
                                    label, command, output, "shenlun-ocr"
                                )
                            })}
                        </div>
                    </section>
                }
                <article class="study-markdown space-y-2 rounded-box border border-base-content/10 p-3"
                    data-testid="shenlun-ocr-markdown">
                    {render_question_bank_user_markdown(&response.markdown)}
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
    let loading = use_state(|| true);
    {
        let loading = loading.clone();
        let answer = answer.clone();
        let error = error.clone();
        use_effect_with(props.question_id.clone(), move |question_id| {
            loading.set(true);
            answer.set(None);
            error.set(None);
            let alive = Rc::new(Cell::new(true));
            let guard = alive.clone();
            let endpoint =
                format!("/api/v1/question-bank/shenlun/questions/{question_id}/source-answer");
            spawn_local(async move {
                match fetch_json::<ShenlunSourceAnswer>(&endpoint, "题库来源参考答案").await
                {
                    Ok(value) if guard.get() => answer.set(Some(value)),
                    Err(message) if guard.get() => error.set(Some(message)),
                    _ => (),
                }
                if guard.get() {
                    loading.set(false);
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
            } else if *loading {
                <p class="text-sm text-base-content/60" role="status">{"正在加载题库参考答案…"}</p>
            } else if error.is_none() {
                <p class="text-sm text-base-content/60">{"题库暂无可展示的参考答案。"}</p>
            }
            if let Some(message) = (*error).as_ref() {
                <p role="alert" class="text-sm text-error">{message.clone()}</p>
            }
        </section>
    }
}
