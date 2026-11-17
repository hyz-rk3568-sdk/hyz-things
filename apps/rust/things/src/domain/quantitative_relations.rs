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
    let invalid = || "几何图不符合安全格式，请重新出题".to_owned();
    if source.is_empty() || source.len() > 32_768 || source.contains("<!") || source.contains("<?")
    {
        return Err(invalid());
    }
    let doc = roxmltree::Document::parse(source).map_err(|_| invalid())?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg"
        || root.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
    {
        return Err(invalid());
    }
    let numeric = |value: &str| {
        value
            .parse::<f64>()
            .is_ok_and(|number| number.is_finite() && number.abs() <= 10_000.0)
    };
    let list = |value: &str| {
        let parts = value
            .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        !parts.is_empty() && parts.len() <= 2048 && parts.iter().all(|part| numeric(part))
    };
    let mut count = 0;
    let mut has_shape = false;
    for node in root.descendants().filter(|node| node.is_element()) {
        count += 1;
        if count > 256
            || node
                .ancestors()
                .filter(|ancestor| ancestor.is_element())
                .count()
                > 16
            || node.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
        {
            return Err(invalid());
        }
        let name = node.tag_name().name();
        has_shape |= ["line", "polyline", "polygon", "rect", "circle", "ellipse"].contains(&name);
        if ![
            "svg", "g", "line", "polyline", "polygon", "rect", "circle", "ellipse", "text",
            "tspan", "title", "desc",
        ]
        .contains(&name)
            || (name == "svg" && node != root)
        {
            return Err(invalid());
        }
        for attr in node.attributes() {
            if attr.namespace().is_some() {
                return Err(invalid());
            }
            let value = attr.value();
            let valid = match attr.name() {
                "viewBox" if node == root => {
                    let parts = value
                        .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>();
                    parts.len() == 4
                        && list(value)
                        && parts[2].parse::<f64>().is_ok_and(|v| v > 0.0)
                        && parts[3].parse::<f64>().is_ok_and(|v| v > 0.0)
                }
                "x" | "y" | "x1" | "y1" | "x2" | "y2" | "cx" | "cy" | "r" | "rx" | "ry"
                | "width" | "height" | "stroke-width" | "font-size" | "opacity"
                | "fill-opacity" | "stroke-opacity" => numeric(value),
                "fill" | "stroke" => {
                    value == "none"
                        || value == "currentColor"
                        || value == "transparent"
                        || (value.starts_with('#')
                            && (4..=9).contains(&value.len())
                            && value[1..].bytes().all(|b| b.is_ascii_hexdigit()))
                        || ((3..=20).contains(&value.len())
                            && value.bytes().all(|b| b.is_ascii_alphabetic()))
                }
                "text-anchor" => ["start", "middle", "end"].contains(&value),
                "dominant-baseline" => {
                    ["auto", "middle", "central", "hanging", "alphabetic"].contains(&value)
                }
                "stroke-linecap" => ["butt", "round", "square"].contains(&value),
                "stroke-linejoin" => ["miter", "round", "bevel"].contains(&value),
                "font-weight" => [
                    "normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900",
                ]
                .contains(&value),
                "font-family" => ["sans-serif", "serif", "monospace"].contains(&value),
                "points" if name == "polygon" || name == "polyline" => list(value),
                _ => false,
            };
            if !valid {
                return Err(invalid());
            }
        }
    }
    if root.attribute("viewBox").is_none() || !has_shape {
        return Err(invalid());
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
