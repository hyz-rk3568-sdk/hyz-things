use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiPracticeModule {
    DataAnalysis,
    LogicJudgment,
    QuantitativeRelations,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiPracticeQuestionId {
    pub module: AiPracticeModule,
    pub session_id: String,
    pub question: usize,
}

impl AiPracticeQuestionId {
    pub fn is_valid(&self) -> bool {
        crate::domain::flashcards::FlashcardId::new(self.session_id.clone()).is_ok()
            && self.question
                < match self.module {
                    AiPracticeModule::DataAnalysis => 20,
                    AiPracticeModule::LogicJudgment | AiPracticeModule::QuantitativeRelations => 10,
                }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiPracticeMessage {
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiPracticeAgentState {
    pub status: String,
    pub messages: Vec<AiPracticeMessage>,
    pub partial: String,
    pub error: Option<String>,
}

pub fn favorite_card_category(path: Option<&str>) -> (String, String) {
    let fallback = || ("未分类".into(), "未分类".into());
    let Some(path) = path else {
        return fallback();
    };
    let path = path.strip_prefix("flashcards/").unwrap_or(path);
    let Some((deck, _)) = path.rsplit_once('/') else {
        return fallback();
    };
    let mut parts = deck.split('/');
    let Some(source) = parts
        .next()
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
    else {
        return fallback();
    };
    let rest = parts.collect::<Vec<_>>();
    if rest
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return fallback();
    }
    let category = if rest.is_empty() {
        "未分类".into()
    } else {
        rest.join("/")
    };
    (source.into(), category)
}

#[cfg(test)]
mod category_tests {
    use super::*;
    #[test]
    fn favorite_categories_follow_the_card_deck() {
        assert_eq!(
            favorite_card_category(Some("flashcards/用户整理/数量关系/工程/错题/cards.md")),
            ("用户整理".into(), "数量关系/工程/错题".into())
        );
        assert_eq!(
            favorite_card_category(Some("资料/增长率/cards.md")),
            ("资料".into(), "增长率".into())
        );
        assert_eq!(
            favorite_card_category(None),
            ("未分类".into(), "未分类".into())
        );
    }
}
