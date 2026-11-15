//! Shared Question Bank AI request contract: scope URLs, draft keys and the
//! idempotent request identifier used by start, follow-up, retry and delete.
use super::*;
use hyz_things::domain::question_bank::AiReviewState;

pub(super) const ROOT: &str = "/api/v1/question-bank";

#[derive(serde::Serialize)]
pub(super) struct AiReviewStartRequest {
    pub(super) request_id: String,
    pub(super) instruction: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) attachments: Vec<String>,
}

#[derive(serde::Serialize)]
pub(super) struct AiReviewFollowUpRequest {
    pub(super) request_id: String,
    pub(super) message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) attachments: Vec<String>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
pub(super) struct AiRequestInfo {
    pub(super) request_id: String,
    pub(super) intent: String,
    pub(super) status: String,
    pub(super) error: Option<String>,
    pub(super) remote_uncertain: bool,
}

#[derive(serde::Deserialize)]
pub(super) struct AiRequestReceipt {
    #[serde(flatten)]
    pub(super) state: AiReviewState,
    pub(super) request: AiRequestInfo,
}

#[derive(serde::Serialize)]
pub(super) struct AiDeleteRequest {
    pub(super) confirm_uncertain: bool,
}

#[derive(serde::Serialize)]
pub(super) struct AiRetryRequest {
    pub(super) request_id: String,
    pub(super) confirm_uncertain: bool,
    pub(super) confirm_new_thread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) replacement_attachments: Option<Vec<String>>,
}

pub(super) fn definite_ai_rejection(error: &ApiError) -> bool {
    matches!(
        error.code(),
        Some(
            "question_bank_ai_invalid"
                | "question_bank_ai_busy"
                | "question_bank_ai_request_conflict"
                | "question_bank_ai_request_not_found"
                | "question_bank_ai_attachment_expired"
                | "question_bank_ai_confirmation_required"
        )
    )
}

pub(super) fn remember_ai_request(key: &str, id: &str) -> Result<(), String> {
    let storage = web_sys::window()
        .and_then(|window| window.session_storage().ok().flatten())
        .ok_or("浏览器会话存储不可用，无法安全记录请求；未发送。")?;
    storage
        .set_item(key, id)
        .map_err(|_| "无法保存请求标识；未发送。".to_owned())
}

pub(super) fn new_ai_request_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    web_sys::window()
        .ok_or("浏览器不可用")?
        .crypto()
        .map_err(|_| "无法获取安全随机数")?
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(|_| "无法获取安全随机数")?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

pub(super) fn streaming_markdown_complete(text: &str) -> bool {
    let fences = text
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("```") || line.starts_with("~~~")
        })
        .count();
    let dollar_count = text.chars().filter(|ch| *ch == '$').count();
    fences % 2 == 0
        && dollar_count % 2 == 0
        && (!text.contains("<svg") || text.contains("</svg>"))
        && text.matches("\\(").count() == text.matches("\\)").count()
        && text.matches("\\[").count() == text.matches("\\]").count()
}

pub(super) fn ai_scope_url(
    endpoint: &str,
    suffix: &str,
    scope: &str,
    query: Option<&str>,
) -> String {
    let mut params = Vec::new();
    if scope != "analysis" {
        params.push(format!("scope={scope}"));
    }
    if let Some(query) = query.filter(|value| !value.is_empty()) {
        params.push(query.to_owned());
    }
    if params.is_empty() {
        format!("{endpoint}{suffix}")
    } else {
        format!("{endpoint}{suffix}?{}", params.join("&"))
    }
}

pub(super) fn ai_draft_key(question_id: &str, scope: &str, kind: &str) -> String {
    match scope {
        "analysis" => format!("hyz.ai-review.draft.{question_id}.{kind}"),
        "grading" => format!("hyz.ai-review.draft.{question_id}.grading.{kind}"),
        _ => format!("hyz.ai-review.draft.{question_id}.{scope}.{kind}"),
    }
}

pub(super) fn ai_draft_read(key: &str) -> String {
    web_sys::window()
        .and_then(|window| window.session_storage().ok().flatten())
        .and_then(|storage| storage.get_item(key).ok().flatten())
        .unwrap_or_default()
}

pub(super) fn ai_draft_write(key: &str, value: &str) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.session_storage().ok().flatten())
    {
        if value.is_empty() {
            let _ = storage.remove_item(key);
        } else {
            let _ = storage.set_item(key, value);
        }
    }
}
