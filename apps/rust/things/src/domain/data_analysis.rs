//! Typed data-analysis question recipes; AI text is untrusted until verified.
use serde::{Deserialize, Serialize};

pub const QUIZ_QUESTIONS: usize = 20;
pub const QUIZ_GROUPS: usize = 4;
pub const QUIZ_PER_GROUP: usize = 5;
pub const QUIZ_SECONDS: u64 = 1200;

// These are statistical material topics, not question skills or subtopics.
pub const TOPICS: [&str; 12] = [
    "宏观经济",
    "农业",
    "工业能源",
    "交通物流与服务",
    "国内消费",
    "投资房地产",
    "对外经济",
    "财政金融",
    "居民民生",
    "科技教育",
    "文旅卫生体育",
    "资源环境安全",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Material {
    pub topic: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifiedQuestion {
    pub question: String,
    pub options: [String; 4],
    pub correct_index: usize,
    pub answer: String,
    pub explanation: String,
    pub skill: String,
    pub evidence: String,
}

/// Choose distinct untried topics for a parallel generation wave.
/// Retry identity is the topic itself, not an invented source URL or section.
/// Existing historical material JSON may still include legacy fields: serde
/// ignores them when deserializing the two-field Material above.
pub fn select_material_batch(
    ordered: &[Material],
    completed: &[Material],
    attempted: &std::collections::HashSet<String>,
    limit: usize,
) -> Vec<Material> {
    let mut seen = completed
        .iter()
        .map(|material| material.topic.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut selected = Vec::new();
    for material in ordered {
        if selected.len() == limit {
            break;
        }
        if attempted.contains(&material.topic) || !seen.insert(material.topic.clone()) {
            continue;
        }
        selected.push(material.clone());
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(topic: &str) -> Material {
        Material {
            topic: topic.to_owned(),
            text: "全年粮食产量71488万吨，比上年增加838万吨，增产1.2%。".into(),
        }
    }

    #[test]
    fn material_wire_has_only_topic_and_text_and_reads_legacy_history() {
        let material = material("农业");
        let json = serde_json::to_value(&material).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 2);
        assert_eq!(json["topic"], "农业");
        assert_eq!(json["text"], material.text);
        for removed in ["section", "subtopic", "source_url"] {
            assert!(json.get(removed).is_none());
        }
        // Existing SQLite groups_json is never rewritten; archived papers and
        // exact-question retries must continue decoding the old five-field JSON.
        let historic = serde_json::json!({
            "section": "二、农业",
            "topic": "农业",
            "subtopic": "粮食生产",
            "text": "历史材料",
            "source_url": "practice-theme-1"
        });
        let restored: Material = serde_json::from_value(historic).unwrap();
        assert_eq!(restored, Material {topic: "农业".into(), text: "历史材料".into()});
        assert_eq!(serde_json::to_value(&restored).unwrap().as_object().unwrap().len(), 2);
    }

    #[test]
    fn waves_select_distinct_topics_without_source_or_section() {
        let ordered = ["农业", "工业能源", "工业能源", "国内消费", "宏观经济"]
            .iter()
            .map(|topic| material(topic))
            .collect::<Vec<_>>();
        let first = select_material_batch(&ordered, &[], &Default::default(), 3);
        assert_eq!(first.iter().map(|m| m.topic.as_str()).collect::<Vec<_>>(),
            vec!["农业", "工业能源", "国内消费"]);
        let second = select_material_batch(
            &ordered,
            &[material("工业能源")],
            &std::collections::HashSet::from(["农业".to_owned()]),
            3,
        );
        assert_eq!(second.iter().map(|m| m.topic.as_str()).collect::<Vec<_>>(),
            vec!["国内消费", "宏观经济"]);
        assert!(select_material_batch(&ordered, &[], &Default::default(), 0).is_empty());
        assert!(TOPICS.len() >= QUIZ_GROUPS);
        assert_eq!(TOPICS.iter().collect::<std::collections::HashSet<_>>().len(), TOPICS.len());
        assert_eq!(QUIZ_GROUPS * QUIZ_PER_GROUP, QUIZ_QUESTIONS);
        assert_eq!(QUIZ_SECONDS, 1200);
    }
}
