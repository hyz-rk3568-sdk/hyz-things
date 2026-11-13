//! Pure, deterministic validators for untrusted 申论 model output.
//! No AI-produced total is accepted. Invalid citations never become highlighted evidence.
use crate::domain::question_bank::MaterialView;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const SKILL_VERSION: &str = "hyz-shenlun-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RubricPoint {
    pub id: String,
    pub criterion: String,
    pub material_id: String,
    pub evidence: String,
    pub weight: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RubricResponse {
    items: Vec<RubricPoint>,
    notes: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GradeResponse {
    evaluations: Vec<GradeEvaluation>,
    summary: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GradeEvaluation {
    id: String,
    coverage: String,
    quote: String,
    start: Option<usize>,
    end: Option<usize>,
    reason: String,
    suggestion: String,
}

fn contains_evidence(materials: &[MaterialView], point: &RubricPoint) -> bool {
    materials.iter().any(|material| {
        material.id == point.material_id
            && !point.evidence.trim().is_empty()
            && material.markdown.contains(&point.evidence)
    })
}

/// Reject any rubric that invents points or exceeds the stated available score.
pub fn validate_rubric(
    value: &Value,
    materials: &[MaterialView],
    max_score: Option<f64>,
) -> Result<Vec<RubricPoint>, String> {
    let parsed: RubricResponse =
        serde_json::from_value(value.clone()).map_err(|_| "评分点池 JSON 格式无效")?;
    if parsed.items.is_empty() || parsed.items.len() > 30 || parsed.notes.len() > 4_000 {
        return Err("评分点数量或说明不合法".into());
    }
    let mut seen = HashSet::new();
    let mut sum = 0_f64;
    for point in &parsed.items {
        if point.id.len() > 64
            || point.id.is_empty()
            || !seen.insert(point.id.clone())
            || point.criterion.trim().is_empty()
            || point.criterion.len() > 600
            || point.evidence.len() > 1_000
            || !contains_evidence(materials, point)
            || !point.weight.is_finite()
            || point.weight <= 0.0
            || point.weight > 1000.0
        {
            return Err("评分点重复、证据无法定位或权重无效".into());
        }
        sum += point.weight;
    }
    if !sum.is_finite()
        || max_score.is_some_and(|limit| {
            !limit.is_finite() || limit <= 0.0 || limit > 1000.0 || sum > limit + 0.00001
        })
    {
        return Err("评分点权重超出题目分值".into());
    }
    Ok(parsed.items)
}

/// The proposed grade contains semantic coverage only. Rust computes the total,
/// checks UTF-8 byte ranges against the student's exact confirmed text, and
/// refuses hallucinated references, omitted points, and duplicate point IDs.
pub fn validated_grade(
    value: &Value,
    rubric: &[RubricPoint],
    answer: &str,
    max_score: Option<f64>,
) -> Result<Value, String> {
    let parsed: GradeResponse =
        serde_json::from_value(value.clone()).map_err(|_| "批改 JSON 格式无效")?;
    if parsed.evaluations.len() != rubric.len() || parsed.summary.len() > 8_000 {
        return Err("批改未完整覆盖评分点或点评过长".into());
    }
    let mut seen = HashSet::new();
    let mut score = 0_f64;
    let mut items = Vec::new();
    for evaluation in parsed.evaluations {
        let point = rubric
            .iter()
            .find(|point| point.id == evaluation.id)
            .ok_or("批改包含未知评分点")?;
        if !seen.insert(evaluation.id.clone())
            || evaluation.reason.len() > 3_000
            || evaluation.suggestion.len() > 3_000
            || evaluation.reason.trim().is_empty()
        {
            return Err("批改重复或点评无效".into());
        }
        let ratio = match evaluation.coverage.as_str() {
            "full" => 1.0,
            "partial" => 0.5,
            "missing" | "unsupported" => 0.0,
            _ => return Err("覆盖状态无效".into()),
        };
        match (evaluation.start, evaluation.end) {
            (Some(start), Some(end))
                if start < end && answer.get(start..end) == Some(evaluation.quote.as_str()) => {}
            (None, None) if evaluation.quote.is_empty() => {
                if matches!(
                    evaluation.coverage.as_str(),
                    "full" | "partial" | "unsupported"
                ) {
                    return Err("命中或错误断言缺少考生原句".into());
                }
            }
            _ => return Err("考生原句与字节范围不符".into()),
        }
        if matches!(evaluation.coverage.as_str(), "full" | "partial") && evaluation.quote.is_empty()
        {
            return Err("命中项不能使用空引用".into());
        }
        let earned = (point.weight * ratio * 100.0).round() / 100.0;
        score += earned;
        items.push(json!({
            "id": evaluation.id, "criterion": point.criterion,
            "material_id": point.material_id, "evidence": point.evidence,
            "weight": point.weight, "earned": earned,
            "coverage": evaluation.coverage, "quote": evaluation.quote,
            "start": evaluation.start, "end": evaluation.end,
            "reason": evaluation.reason, "suggestion": evaluation.suggestion
        }));
    }
    let score = (score * 100.0).round() / 100.0;
    let valid_max = max_score.filter(|max| max.is_finite() && *max > 0.0 && *max <= 1_000.0);
    if valid_max.is_some_and(|max| score > max + 0.001) {
        return Err("计算分数超出满分".into());
    }
    Ok(json!({
        "provisional": true,
        "score": valid_max.map(|_| score),
        "max_score": valid_max,
        "rubric_version": SKILL_VERSION,
        "summary": parsed.summary,
        "items": items,
        "unscored_reason": if valid_max.is_none() {
            Some("题目未提供可信满分，只给练习诊断")
        } else {
            None
        }
    }))
}

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
    Ok(json!({ "pages": pages, "confirmed": false }))
}

pub fn validate_generated(value: &Value, original: Option<&str>) -> Result<Value, String> {
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or("生成内容缺失")?;
    if text.trim().is_empty() || text.chars().count() > 50_000 {
        return Err("生成内容为空或超出长度限制".into());
    }
    let edits = value
        .get("edits")
        .and_then(Value::as_array)
        .ok_or("改写项无效")?;
    if edits.len() > 100
        || edits.iter().any(|edit| {
            let before = edit.get("before").and_then(Value::as_str);
            let after = edit.get("after").and_then(Value::as_str);
            let reason = edit.get("reason").and_then(Value::as_str);
            before.is_none_or(|s| s.len() > 1000)
                || after.is_none_or(|s| s.len() > 2000)
                || reason.is_none_or(|s| s.len() > 1000)
        })
    {
        return Err("改写项超出限定".into());
    }
    if let Some(original) = original {
        if edits.is_empty()
            || edits.iter().any(|edit| {
                edit.get("before")
                    .and_then(Value::as_str)
                    .is_none_or(|before| before.is_empty() || !original.contains(before))
            })
        {
            return Err("改写对照未引用真实原稿".into());
        }
    } else if !edits.is_empty() {
        return Err("独立答案不得含学生原稿改写对照".into());
    }
    Ok(json!({"text":text, "edits":edits, "provenance":"AI 生成，非官方"}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn materials() -> Vec<MaterialView> {
        vec![MaterialView {
            id: "m1".into(),
            name: "资料一".into(),
            markdown: "推进基层治理，需要明确责任。".into(),
        }]
    }
    fn points() -> Vec<RubricPoint> {
        vec![RubricPoint {
            id: "p1".into(),
            criterion: "责任主体".into(),
            material_id: "m1".into(),
            evidence: "明确责任".into(),
            weight: 10.0,
        }]
    }

    #[test]
    fn rubric_requires_real_source_evidence_and_unique_weights() {
        let valid = json!({"items":[{"id":"p1","criterion":"责任主体","material_id":"m1",
            "evidence":"明确责任","weight":10}],"notes":""});
        assert_eq!(
            validate_rubric(&valid, &materials(), Some(20.0))
                .unwrap()
                .len(),
            1
        );
        assert!(validate_rubric(&valid, &[], Some(20.0)).is_err());
        let mut bad = valid;
        bad["items"][0]["evidence"] = json!("虚构出处");
        assert!(validate_rubric(&bad, &materials(), Some(20.0)).is_err());
    }

    #[test]
    fn score_uses_true_utf8_quotes_and_deterministic_math() {
        let answer = "我认为，要明确责任。";
        let start = answer.find("明确责任").unwrap();
        let good = json!({"evaluations":[{"id":"p1","coverage":"partial",
            "quote":"明确责任","start":start,"end":start+"明确责任".len(),
            "reason":"缺少实施主体","suggestion":"补充谁来负责"}],"summary":"方向正确"});
        let grade = validated_grade(&good, &points(), answer, Some(20.0)).unwrap();
        assert_eq!(grade["score"], 5.0);
        assert_eq!(grade["max_score"], 20.0);
        assert_eq!(
            validated_grade(&good, &points(), answer, None).unwrap()["score"],
            Value::Null
        );
        let mut forged = good;
        forged["evaluations"][0]["quote"] = json!("不在原稿中的句子");
        assert!(validated_grade(&forged, &points(), answer, Some(20.0)).is_err());
    }

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
    fn independent_answer_must_not_contain_student_rewrites() {
        let good = json!({"text":"独立参考答案","edits":[]});
        assert!(validate_generated(&good, None).is_ok());
        assert!(validate_generated(&good, Some("原稿")).is_err());
        let edit = json!({"text":"优化答案","edits":[{"before":"原稿","after":"新稿","reason":"补充观点"}]});
        assert!(validate_generated(&edit, Some("原稿")).is_ok());
        assert!(validate_generated(&edit, None).is_err());
    }
}
