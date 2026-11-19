//! Independent logic question payloads; Codex owns correctness and semantic review.
use serde::{Deserialize, Serialize};

pub const QUIZ_QUESTIONS: usize = 10;
pub const QUIZ_SECONDS: u64 = 600;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedQuestion {
    pub target_id: String,
    pub kind: String,
    pub question: String,
    pub options: [String; 4],
    pub correct_index: usize,
    pub explanation: String,
}

/// Check payload shape only, not the correctness or meaning of a question.
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

pub fn generation_card_path(path: &str) -> bool {
    let Some(deck) = path
        .strip_prefix("用户整理/判断推理/形式推理/")
        .and_then(|value| value.strip_suffix("/cards.md"))
    else {
        return false;
    };
    let category = deck.strip_suffix("/错题").unwrap_or(deck);
    ["直言命题", "联言选言假言命题", "组合排列", "特殊题型"].contains(&category)
}

#[cfg(test)]
mod generation_path_tests {
    use super::*;
    #[test]
    fn generation_accepts_only_user_organized_formal_decks() {
        assert!(generation_card_path(
            "用户整理/判断推理/形式推理/直言命题/cards.md"
        ));
        assert!(generation_card_path(
            "用户整理/判断推理/形式推理/直言命题/错题/cards.md"
        ));
        for path in [
            "判断/形式推理/直言命题/cards.md",
            "判断推理/AI出题/形式推理/直言命题/cards.md",
            "真题练习/判断推理/形式推理/直言命题/错题/cards.md",
            "用户整理/判断推理/形式推理/未知/cards.md",
            "用户整理/判断推理/形式推理/直言命题/其他/cards.md",
        ] {
            assert!(!generation_card_path(path), "{path}");
        }
    }
}
