//! Typed data-analysis question recipes; AI text is untrusted until verified.
use serde::{Deserialize, Serialize};

pub const SOURCE_URL: &str = "https://www.stats.gov.cn/sj/zxfb/202602/t20260228_1962662.html";
/// Verifiable National Bureau of Statistics annual bulletins; year labels are
/// metadata for examination-style text, not fabricated values.
pub const OFFICIAL_SOURCES: [(u16, &str); 4] = [
    (
        2022,
        "https://www.stats.gov.cn/sj/zxfb/202302/t20230228_1919011.html",
    ),
    (
        2023,
        "https://www.stats.gov.cn/sj/zxfb/202402/t20240228_1947915.html",
    ),
    (
        2024,
        "https://www.stats.gov.cn/sj/zxfb/202502/t20250228_1958817.html",
    ),
    (2025, SOURCE_URL),
];
pub const QUIZ_QUESTIONS: usize = 20;
pub const QUIZ_GROUPS: usize = 4;
pub const QUIZ_PER_GROUP: usize = 5;
pub const QUIZ_SECONDS: u64 = 1200;

/// Single-level subject taxonomy for exam-style simulated passages.
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
    pub source_url: String,
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

/// Choose distinct subjects for a generation wave.
/// The strict pass also prefers independent source identities; the fallback
/// may repeat a source, but *never* a subject within the same paper.
pub fn select_material_batch(
    ordered: &[Material],
    completed: &[Material],
    attempted: &std::collections::HashSet<String>,
    strict_diversity: bool,
    limit: usize,
) -> Vec<Material> {
    let mut topics = completed
        .iter()
        .map(|material| material.topic.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut sources = completed
        .iter()
        .map(|material| material.source_url.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut selected = Vec::new();
    for material in ordered {
        if selected.len() == limit {
            break;
        }
        if topics.contains(&material.topic)
            || attempted.contains(&material.topic)
            || (strict_diversity && sources.contains(&material.source_url))
        {
            continue;
        }
        topics.insert(material.topic.clone());
        sources.insert(material.source_url.clone());
        selected.push(material.clone());
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    fn material() -> Material {
        Material {
            topic: "农业".into(),
            text: "全年粮食产量71488万吨，比上年增加838万吨，增产1.2%。".into(),
            source_url: SOURCE_URL.into(),
        }
    }
    #[test]
    fn parallel_material_wave_is_diverse_bounded_and_deterministic() {
        let candidates = (0..7)
            .map(|n| Material {
                source_url: format!("year-{}", n / 2),
                topic: format!("topic-{}", n / 2),
                ..material()
            })
            .collect::<Vec<_>>();
        let batch = select_material_batch(
            &candidates,
            &[],
            &std::collections::HashSet::new(),
            true,
            QUIZ_GROUPS,
        );
        assert_eq!(batch.len(), QUIZ_GROUPS);
        assert_eq!(
            batch
                .iter()
                .map(|m| &m.source_url)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            QUIZ_GROUPS
        );
        assert_eq!(
            batch
                .iter()
                .map(|m| &m.topic)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            QUIZ_GROUPS
        );
        assert_eq!(
            batch,
            select_material_batch(
                &candidates,
                &[],
                &std::collections::HashSet::new(),
                true,
                QUIZ_GROUPS
            )
        );
    }

    #[test]
    fn failed_wave_replenishes_without_reusing_identity() {
        let source = material();
        let candidates = vec![
            Material {
                source_url: "year-a".into(),
                topic: "agriculture".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-a".into(),
                topic: "industry".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-b".into(),
                topic: "trade".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-c".into(),
                topic: "investment".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-b".into(),
                topic: "health".into(),
                ..source.clone()
            },
        ];
        let attempted = std::collections::HashSet::from(["agriculture".to_owned()]);
        let completed = vec![candidates[2].clone()];
        let strict = select_material_batch(&candidates, &completed, &attempted, true, 3);
        assert_eq!(strict, vec![candidates[1].clone(), candidates[3].clone()]);
        let fallback = select_material_batch(&candidates, &completed, &attempted, false, 3);
        assert_eq!(
            fallback,
            vec![
                candidates[1].clone(),
                candidates[3].clone(),
                candidates[4].clone()
            ]
        );
        assert!(select_material_batch(&candidates, &completed, &attempted, false, 0).is_empty());
    }

    #[test]
    fn single_topic_taxonomy_is_stable() {
        assert_eq!(TOPICS.len(), 12);
        assert_eq!(OFFICIAL_SOURCES.len(), 4);
        assert!(OFFICIAL_SOURCES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(QUIZ_GROUPS * QUIZ_PER_GROUP, QUIZ_QUESTIONS);
        assert_eq!(QUIZ_SECONDS, 1200);
    }
    #[test]
    fn a_material_uses_only_topic_and_reads_historic_json() {
        let historic = r#"{"section":"二、农业","topic":"农业","subtopic":"粮食生产","text":"测试材料","source_url":"practice-theme-1"}"#;
        let parsed: Material = serde_json::from_str(historic).unwrap();
        assert_eq!(parsed.topic, "农业");
        let json = serde_json::to_value(&parsed).unwrap();
        assert_eq!(json["topic"], "农业");
        assert!(json.get("section").is_none());
        assert!(json.get("subtopic").is_none());
        assert_eq!(json.as_object().unwrap().len(), 3);
    }

    #[test]
    fn fallback_cannot_choose_the_same_topic_twice() {
        let mut candidates = vec![material()];
        candidates.push(Material {
            topic: "农业".into(),
            source_url: "new-source".into(),
            ..material()
        });
        candidates.push(Material {
            topic: "工业能源".into(),
            ..material()
        });
        let result = select_material_batch(
            &candidates, &[], &std::collections::HashSet::new(), false, 4
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].topic, "农业");
        assert_eq!(result[1].topic, "工业能源");
    }

}
