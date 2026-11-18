//! Quantity question payloads and the restricted diagram contract.
use serde::{Deserialize, Serialize};

pub const QUIZ_QUESTIONS: usize = 10;
pub const QUIZ_SECONDS: u64 = 900;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedQuestion {
    pub target_id: String,
    pub kind: String,
    pub question: String,
    #[serde(default)]
    pub diagram_required: bool,
    pub options: [String; 4],
    pub correct_index: usize,
    pub explanation: String,
}

/// Check payload shape and diagram syntax; semantic correctness remains AI-reviewed.
pub fn validate_shape(question: &GeneratedQuestion) -> Result<(), String> {
    if [
        &question.target_id,
        &question.kind,
        &question.question,
        &question.explanation,
    ]
    .into_iter()
    .chain(question.options.iter())
    .any(|text| text.trim().is_empty())
    {
        return Err("required question fields must not be blank".into());
    }
    if question.correct_index >= question.options.len() {
        return Err("correct_index must be in 0..4".into());
    }
    if question.diagram_required && !question.question.contains("```svg") {
        return Err("几何题缺少配图，请重新出题".into());
    }
    let mut remaining = question.question.as_str();
    while let Some((prefix, tail)) = remaining.split_once("```svg") {
        if (!prefix.is_empty() && !prefix.ends_with('\n')) || !tail.starts_with('\n') {
            return Err("SVG 必须是独立代码块".into());
        }
        let (source, after) = tail.split_once("```").ok_or("SVG 代码块未闭合")?;
        if !source.ends_with('\n') || (!after.is_empty() && !after.starts_with('\n')) {
            return Err("SVG 必须是独立代码块".into());
        }
        validate_diagram(source.trim())?;
        remaining = after;
    }
    Ok(())
}

fn validate_diagram(source: &str) -> Result<(), String> {
    if !super::svg::safe_svg(source) {
        return Err("几何图不符合安全格式，请重新出题".into());
    }
    let doc = roxmltree::Document::parse(source).map_err(|_| "几何图不符合安全格式，请重新出题")?;
    if !doc.descendants().any(|node| {
        node.is_element()
            && [
                "line", "polyline", "polygon", "rect", "circle", "ellipse", "path",
            ]
            .contains(&node.tag_name().name())
    }) {
        return Err("几何图缺少图形，请重新出题".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question() -> GeneratedQuestion {
        GeneratedQuestion {
            target_id: "a".into(),
            kind: "formal".into(),
            question: "Which follows?".into(),
            diagram_required: false,
            options: ["A".into(), "B".into(), "C".into(), "D".into()],
            correct_index: 0,
            explanation: "A is true".into(),
        }
    }

    #[test]
    fn accepts_complete_payload_without_semantic_review() {
        let mut q = question();
        q.options[1] = q.options[0].clone();
        assert!(validate_shape(&q).is_ok());
    }

    #[test]
    fn accepts_safe_svg_with_gradients_and_styles() {
        let mut q = question();
        q.diagram_required = true;
        q.question = "几何题\n```svg\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\"><defs><linearGradient id=\"g\"><stop offset=\"0%\" stop-color=\"red\"/></linearGradient></defs><style>.shape { fill: url(#g); }</style><rect class=\"shape\" width=\"80\" height=\"80\" transform=\"translate(5 5)\"/></svg>\n```".into();
        assert!(validate_shape(&q).is_ok());
    }

    #[test]
    fn rejects_unsafe_svg_embedded_in_question() {
        let mut q = question();
        q.question =
            "几何题\n\n```svg\n<svg viewBox=\"0 0 100 100\"><script>alert(1)</script></svg>\n```"
                .into();
        assert!(validate_shape(&q).is_err());
    }

    #[test]
    fn diagram_required_needs_a_valid_svg() {
        let mut q = question();
        q.diagram_required = true;
        assert!(validate_shape(&q).is_err());
        q.question = "三角形 ABC\n\n```svg\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><polygon points=\"10,90 50,10 90,90\" fill=\"none\" stroke=\"black\"/><text x=\"50\" y=\"10\">A</text></svg>\n```".into();
        assert!(validate_shape(&q).is_ok());
    }

    #[test]
    fn diagram_must_be_a_standalone_markdown_block() {
        let mut q = question();
        q.diagram_required = true;
        q.question =
            "内联```svg\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"/>\n```"
                .into();
        assert!(validate_shape(&q).is_err());
    }

    #[test]
    fn required_diagram_cannot_be_empty() {
        let mut q = question();
        q.diagram_required = true;
        q.question =
            "几何题\n```svg\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"/>\n```"
                .into();
        assert!(validate_shape(&q).is_err());
    }

    #[test]
    fn rejects_blank_required_fields() {
        for field in 0..8 {
            let mut q = question();
            let text = match field {
                0 => &mut q.target_id,
                1 => &mut q.kind,
                2 => &mut q.question,
                3 => &mut q.explanation,
                i => &mut q.options[i - 4],
            };
            *text = " \n\t".into();
            assert!(validate_shape(&q).is_err(), "field {field}");
        }
    }

    #[test]
    fn checks_answer_index_range() {
        for index in [4, usize::MAX] {
            let mut q = question();
            q.correct_index = index;
            assert!(validate_shape(&q).is_err());
        }
        let mut q = question();
        q.correct_index = 3;
        assert!(validate_shape(&q).is_ok());
    }

    #[test]
    fn serde_requires_exactly_four_options_and_no_model() {
        let mut value = serde_json::to_value(question()).unwrap();
        value["options"] = serde_json::json!(["A", "B", "C"]);
        assert!(serde_json::from_value::<GeneratedQuestion>(value).is_err());
        let mut value = serde_json::to_value(question()).unwrap();
        value["model"] = serde_json::json!({});
        assert!(serde_json::from_value::<GeneratedQuestion>(value).is_err());
    }
}
