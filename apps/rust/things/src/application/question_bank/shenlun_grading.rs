//! OCR output validation only.
//! 申论批改通过 Question Bank 的原生 AI 对话输出 Markdown，不在 Rust 中计算分数。
use serde_json::{json, Value};

pub const SKILL_VERSION: &str = "hyz-shenlun-v1";

pub fn validate_transcription(value: &Value, expected_pages: usize) -> Result<Value, String> {
    let pages = value
        .get("pages")
        .and_then(Value::as_array)
        .ok_or("转写缺少逐页结果")?;
    if pages.len() != expected_pages || pages.is_empty() || pages.len() > 4 {
        return Err("转写页序或页数不匹配".into());
    }
    let mut total = 0_usize;
    for (index, page) in pages.iter().enumerate() {
        if page.get("page").and_then(Value::as_u64) != Some((index + 1) as u64) {
            return Err("转写页码无序".into());
        }
        let text = page
            .get("text")
            .and_then(Value::as_str)
            .ok_or("转写缺少正文")?;
        total += text.chars().count();
        let uncertain = page
            .get("uncertain")
            .and_then(Value::as_array)
            .ok_or("转写缺少不确定段落列表")?;
        if uncertain.len() > 100
            || uncertain.iter().any(|region| {
                region
                    .get("text")
                    .and_then(Value::as_str)
                    .is_none_or(|part| part.len() > 300)
                    || region
                        .get("reason")
                        .and_then(Value::as_str)
                        .is_none_or(|reason| reason.len() > 500)
            })
        {
            return Err("不确定段落结构无效".into());
        }
    }
    if total == 0 || total > 50_000 {
        return Err("转写内容为空或超限".into());
    }
    Ok(json!({"pages":pages,"confirmed":false}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_never_skips_or_reorders_pages() {
        let good = json!({"pages":[
            {"page":1,"text":"第一页","uncertain":[]},
            {"page":2,"text":"第二页","uncertain":[{"text":"字迹","reason":"模糊"}]}
        ]});
        assert!(validate_transcription(&good, 2).is_ok());
        assert!(validate_transcription(&good, 1).is_err());
    }

    #[test]
    fn transcription_rejects_empty_pages_and_bad_uncertainty() {
        let empty = json!({"pages":[{"page":1,"text":"","uncertain":[]}]});
        assert!(validate_transcription(&empty, 1).is_err());
        let bad = json!({"pages":[{"page":1,"text":"原文","uncertain":[{"text":"不确定"}]}]});
        assert!(validate_transcription(&bad, 1).is_err());
    }
}
