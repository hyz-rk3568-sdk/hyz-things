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

/// Original section title is preserved separately from training topic.
pub const SECTIONS: [(&str, &str); 12] = [
    ("一、综合", "宏观经济"),
    ("二、农业", "农业"),
    ("三、工业和建筑业", "工业能源"),
    ("四、服务业", "交通物流与服务"),
    ("五、国内贸易", "国内消费"),
    ("六、固定资产投资", "投资房地产"),
    ("七、对外经济", "对外经济"),
    ("八、财政金融", "财政金融"),
    ("九、居民收入消费和社会保障", "居民民生"),
    ("十、科学技术和教育", "科技教育"),
    ("十一、文化旅游、卫生健康和体育", "文旅卫生体育"),
    ("十二、资源、环境和应急管理", "资源环境安全"),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Material {
    pub section: String,
    pub topic: String,
    pub subtopic: String,
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

/// Pick at most \`limit\` distinct material groups for a parallel generation wave.
///
/// The first pass requires independent official years and topics; the fallback
/// pass relaxes diversity, never the unique source/section identity. Rejected
/// candidates are not retried indefinitely.
pub fn select_material_batch(
    ordered: &[Material],
    completed: &[Material],
    attempted: &std::collections::HashSet<(String, String)>,
    strict_diversity: bool,
    limit: usize,
) -> Vec<Material> {
    let mut identities = completed
        .iter()
        .map(|m| (m.source_url.clone(), m.section.clone()))
        .collect::<std::collections::HashSet<_>>();
    let mut years = completed
        .iter()
        .map(|m| m.source_url.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut topics = completed
        .iter()
        .map(|m| m.topic.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut selected = Vec::new();
    for material in ordered {
        if selected.len() == limit {
            break;
        }
        let identity = (material.source_url.clone(), material.section.clone());
        if identities.contains(&identity) || attempted.contains(&identity) {
            continue;
        }
        if strict_diversity
            && (years.contains(&material.source_url) || topics.contains(&material.topic))
        {
            continue;
        }
        identities.insert(identity);
        years.insert(material.source_url.clone());
        topics.insert(material.topic.clone());
        selected.push(material.clone());
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    fn material() -> Material {
        Material {
            section: "二、农业".into(),
            topic: "农业".into(),
            subtopic: "粮食生产".into(),
            text: "全年粮食产量71488万吨，比上年增加838万吨，增产1.2%。".into(),
            source_url: SOURCE_URL.into(),
        }
    }
    #[test]
    fn parallel_material_wave_is_diverse_bounded_and_deterministic() {
        let candidates = (0..7)
            .map(|n| Material {
                source_url: format!("year-{}", n / 2),
                section: format!("section-{n}"),
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
                section: "one".into(),
                topic: "agriculture".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-a".into(),
                section: "two".into(),
                topic: "industry".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-b".into(),
                section: "three".into(),
                topic: "trade".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-c".into(),
                section: "four".into(),
                topic: "investment".into(),
                ..source.clone()
            },
            Material {
                source_url: "year-b".into(),
                section: "five".into(),
                topic: "health".into(),
                ..source.clone()
            },
        ];
        let attempted = std::collections::HashSet::from([("year-a".to_owned(), "one".to_owned())]);
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
    fn section_taxonomy_is_stable() {
        assert_eq!(SECTIONS.len(), 12);
        assert_eq!(OFFICIAL_SOURCES.len(), 4);
        assert!(OFFICIAL_SOURCES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(QUIZ_GROUPS * QUIZ_PER_GROUP, QUIZ_QUESTIONS);
        assert_eq!(QUIZ_SECONDS, 1200);
    }
}
