//! Typed data-analysis question recipes; AI text is untrusted until verified.
use serde::{Deserialize, Serialize};

pub const QUIZ_QUESTIONS: usize = 20;
pub const QUIZ_GROUPS: usize = 4;
pub const QUIZ_PER_GROUP: usize = 5;
pub const QUIZ_SECONDS: u64 = 1200;

// Statistical material topics and representative indicators, separate from question skills.
// Keep labels stable: they are stored in historical papers and used for recent-topic balancing.
pub const TOPICS: [(&str, &str); 19] = [
    ("宏观经济", "GDP、三次产业、经济增长、物价"),
    ("人口与就业", "人口结构、出生人口、就业人数、工资"),
    ("农业农村", "粮食、农作物、畜牧、水产、农业产值"),
    ("工业制造", "工业增加值、工业企业、制造业产量"),
    ("建筑业", "建筑企业、建筑业产值、施工面积"),
    ("能源资源", "煤炭、电力、石油、天然气、能源消费"),
    ("生态环境", "环境治理、碳排放、水资源、污染防治"),
    ("投资房地产", "固定资产投资、房地产开发、商品房销售"),
    ("消费零售", "社会消费品零售、电商、商品销售"),
    ("交通物流", "铁路、公路、航空、货运、客运、快递"),
    ("通信互联网", "电信、互联网用户、软件产业、数字经济"),
    ("对外贸易", "进出口、贸易顺差、外商投资"),
    ("财政金融", "财政收支、税收、银行、贷款、保险"),
    (
        "居民收入与社会保障",
        "人均收入、居民消费、养老保险、社会救助",
    ),
    ("科技创新", "研发投入、科研人员、专利、技术成果"),
    ("教育", "学校数量、招生、在校生、毕业生"),
    ("文化旅游体育", "旅游人次、旅游收入、文化产业、体育产业"),
    ("医疗卫生", "医院、医护人员、医疗机构、诊疗人次"),
    (
        "企业经营与产业发展",
        "企业数量、营业收入、利润、产业化率、新兴产业",
    ),
];

pub fn topic_scope(topic: &str) -> Option<&'static str> {
    TOPICS
        .iter()
        .find_map(|(name, scope)| (*name == topic).then_some(*scope))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Material {
    pub topic: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelatedCardSnapshot {
    pub front: String,
    pub back: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_card: Option<RelatedCardSnapshot>,
}

impl VerifiedQuestion {
    pub fn related_card_for(
        &self,
        card_id: &str,
        source_hash: &str,
        current: RelatedCardSnapshot,
    ) -> Option<RelatedCardSnapshot> {
        self.related_card.clone().or_else(|| {
            (self.card_id.as_deref() == Some(card_id)
                && self.source_hash.as_deref() == Some(source_hash))
            .then_some(current)
        })
    }
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
    fn legacy_review_recovers_only_the_same_card_version() {
        let mut question: VerifiedQuestion = serde_json::from_value(serde_json::json!({
            "question":"题目", "options":["1","2","3","4"], "correct_index":0,
            "answer":"1", "explanation":"解析", "skill":"基期量", "evidence":"统计数据",
            "card_id":"card-1", "source_hash":"original-hash"
        }))
        .unwrap();
        let card = RelatedCardSnapshot {
            front: "原题方法".into(),
            back: "原题解法".into(),
        };
        assert_eq!(
            question.related_card_for("card-1", "original-hash", card.clone()),
            Some(card.clone())
        );
        assert_eq!(
            question.related_card_for("card-1", "new-hash", card.clone()),
            None
        );
        assert_eq!(
            question.related_card_for("another-card", "original-hash", card.clone()),
            None
        );
        question.source_hash = None;
        assert_eq!(
            question.related_card_for("card-1", "original-hash", card.clone()),
            None
        );
        question.related_card = Some(card.clone());
        assert_eq!(
            question.related_card_for(
                "another-card",
                "new-hash",
                RelatedCardSnapshot {
                    front: "改过的卡片".into(),
                    back: "改过的解法".into(),
                }
            ),
            Some(card)
        );
    }

    #[test]
    fn saved_question_preserves_its_related_method_card_snapshot() {
        let value = serde_json::json!({
            "question":"上一年的业务量？", "options":["1","2","3","4"],
            "correct_index":0, "answer":"1", "explanation":"解析", "skill":"基期量", "evidence":"统计数据",
            "related_card":{"front":"基期量方法", "back":"$B=A/(1+r)$"}
        });
        let question: VerifiedQuestion = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(question).unwrap()["related_card"],
            value["related_card"]
        );
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
        assert_eq!(
            restored,
            Material {
                topic: "农业".into(),
                text: "历史材料".into()
            }
        );
        assert_eq!(
            serde_json::to_value(&restored)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn waves_select_distinct_topics_without_source_or_section() {
        let ordered = ["农业", "工业能源", "工业能源", "国内消费", "宏观经济"]
            .iter()
            .map(|topic| material(topic))
            .collect::<Vec<_>>();
        let first = select_material_batch(&ordered, &[], &Default::default(), 3);
        assert_eq!(
            first.iter().map(|m| m.topic.as_str()).collect::<Vec<_>>(),
            vec!["农业", "工业能源", "国内消费"]
        );
        let second = select_material_batch(
            &ordered,
            &[material("工业能源")],
            &std::collections::HashSet::from(["农业".to_owned()]),
            3,
        );
        assert_eq!(
            second.iter().map(|m| m.topic.as_str()).collect::<Vec<_>>(),
            vec!["国内消费", "宏观经济"]
        );
        assert!(select_material_batch(&ordered, &[], &Default::default(), 0).is_empty());
        assert_eq!(TOPICS.len(), 19);
        assert!(TOPICS.len() >= QUIZ_GROUPS);
        assert_eq!(
            TOPICS
                .iter()
                .map(|(topic, _)| *topic)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            TOPICS.len()
        );
        assert!(TOPICS
            .iter()
            .all(|(topic, scope)| !topic.is_empty() && !scope.is_empty()));
        assert_eq!(
            topic_scope("医疗卫生"),
            Some("医院、医护人员、医疗机构、诊疗人次")
        );
        assert_eq!(topic_scope("不存在的主题"), None);
        assert_eq!(QUIZ_GROUPS * QUIZ_PER_GROUP, QUIZ_QUESTIONS);
        assert_eq!(QUIZ_SECONDS, 1200);
    }
}
