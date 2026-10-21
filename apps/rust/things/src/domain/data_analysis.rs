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
#[serde(rename_all = "snake_case")]
pub enum Operation {
    BasePeriod,
    GrowthAmount,
    GrowthRate,
    AverageGrowthRate,
    AverageGrowthAmount,
    ShareChange,
    MixedGrowthRate,
    Share,
    Average,
    Difference,
    Ratio,
}

impl Operation {
    pub fn skill(&self) -> &'static str {
        match self {
            Self::BasePeriod => "基期量",
            Self::GrowthAmount => "增长量",
            Self::GrowthRate => "增长率",
            Self::AverageGrowthRate => "平均数增长率",
            Self::AverageGrowthAmount => "平均数增量",
            Self::ShareChange => "两期比重差",
            Self::MixedGrowthRate => "混合增长率",
            Self::Share => "比重",
            Self::Average => "平均数",
            Self::Difference => "差额",
            Self::Ratio => "倍数",
        }
    }

    fn result(&self, a: f64, b: f64) -> Option<f64> {
        let value = match self {
            Self::BasePeriod => a / (1.0 + b / 100.0),
            Self::GrowthAmount => a * (b / 100.0) / (1.0 + b / 100.0),
            Self::GrowthRate => (a / b - 1.0) * 100.0,
            Self::AverageGrowthRate => (a - b) / (1.0 + b / 100.0),
            Self::Share => a / b * 100.0,
            Self::Average | Self::Ratio => a / b,
            Self::Difference => a - b,
            Self::AverageGrowthAmount | Self::ShareChange | Self::MixedGrowthRate => return None,
        };
        (value.is_finite() && value.abs() < 1.0e15).then_some(value)
    }
}

/// Numbers are copied from the quote; never ask the model to calculate the
/// correct option. Both operands must occur within the cited source fragment.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionRecipe {
    pub question: String,
    pub evidence: String,
    pub operation: Operation,
    pub a: f64,
    pub b: f64,
    #[serde(default)]
    pub c: Option<f64>,
    #[serde(default)]
    pub d: Option<f64>,
    pub unit: String,
    pub explanation: String,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    MissingEvidence,
    MissingOperand,
    InvalidNumber,
    InvalidQuestion,
    Duplicate,
}

fn source_contains_numeric_token(text: &str, number: f64) -> bool {
    if !number.is_finite() {
        return false;
    }
    // The provider must copy a displayed number rather than fabricate digits.
    let magnitude = if number < 0.0
        && ["下降", "减少", "减产"]
            .iter()
            .any(|word| text.contains(word))
    {
        number.abs()
    } else {
        number
    };
    let variants = [
        format!("{magnitude}"),
        format!("{magnitude:.1}"),
        format!("{magnitude:.2}"),
        format!("{magnitude:.3}"),
    ];
    variants.into_iter().any(|v| {
        text.match_indices(&v).any(|(start, _)| {
            let left = text[..start].chars().next_back();
            let right = text[start + v.len()..].chars().next();
            !left.is_some_and(|c| c.is_ascii_digit() || c == '.')
                && !right.is_some_and(|c| c.is_ascii_digit() || c == '.')
        })
    })
}

/// A comprehensive true/false judgment needs independently verified claims,
/// which this numeric-only question engine does not support.
fn is_comprehensive_judgment(question: &str) -> bool {
    const EXCLUDED: [&str; 11] = [
        "能够推出的是",
        "可以推出的是",
        "可推出的是",
        "能够从上述资料中推出",
        "下列说法",
        "以下说法",
        "下列表述",
        "以下表述",
        "下列判断",
        "正确的是",
        "错误的是",
    ];
    EXCLUDED.iter().any(|term| question.contains(term))
}

/// Reject mismatched question intent even when the numbers are authentic.
/// In particular, asking "增加多少" must not be graded as a prior-year base.
fn question_matches_operation(question: &str, operation: &Operation) -> bool {
    let asks_change = [
        "增加多少",
        "增加约多少",
        "增长量",
        "增加了多少",
        "增长了多少",
        "减少多少",
        "减少约多少",
        "减少了多少",
        "增量",
        "比上年多多少",
        "比上年少多少",
        "比去年多多少",
        "比去年少多少",
    ]
    .iter()
    .any(|cue| question.contains(cue));
    if asks_change
        && !matches!(
            operation,
            Operation::GrowthAmount | Operation::AverageGrowthAmount | Operation::Difference
        )
    {
        return false;
    }
    match operation {
        Operation::BasePeriod => !asks_change,
        Operation::GrowthAmount => asks_change,
        Operation::AverageGrowthRate => {
            (question.contains("平均") || question.contains("人均") || question.contains("单位"))
                && (question.contains("增速")
                    || question.contains("增长率")
                    || question.contains("增幅"))
        }
        Operation::AverageGrowthAmount => {
            (question.contains("平均") || question.contains("人均") || question.contains("单位"))
                && asks_change
        }
        Operation::ShareChange => question.contains("比重") && question.contains("百分点"),
        Operation::MixedGrowthRate => {
            (question.contains("整体") || question.contains("合计") || question.contains("总"))
                && (question.contains("增长率")
                    || question.contains("增速")
                    || question.contains("增幅"))
        }
        _ => true,
    }
}

/// Precision is deliberately two decimals: the official public statistical
/// values can be rounded, so these are approximate multiple-choice exercises.
pub fn verify_recipe(
    material: &Material,
    recipe: &QuestionRecipe,
    position: usize,
) -> Result<VerifiedQuestion, ValidationError> {
    if is_comprehensive_judgment(&recipe.question)
        || !question_matches_operation(&recipe.question, &recipe.operation)
    {
        return Err(ValidationError::InvalidQuestion);
    }
    if recipe.question.chars().count() < 10
        || recipe.question.chars().count() > 240
        || recipe.explanation.chars().count() > 800
        || recipe.unit.chars().count() > 12
        || recipe.evidence.chars().count() < 15
        || !material.text.contains(recipe.evidence.trim())
    {
        return Err(ValidationError::MissingEvidence);
    }
    if !source_contains_numeric_token(&recipe.evidence, recipe.a)
        || !source_contains_numeric_token(&recipe.evidence, recipe.b)
    {
        return Err(ValidationError::MissingOperand);
    }
    if matches!(
        recipe.operation,
        Operation::BasePeriod | Operation::GrowthAmount
    ) {
        let negative = ["下降", "减少", "减产", "降低"]
            .iter()
            .any(|word| recipe.evidence.contains(word));
        let positive = ["增长", "增产", "上升", "提高"]
            .iter()
            .any(|word| recipe.evidence.contains(word));
        if negative && !positive && recipe.b > 0.0 || positive && !negative && recipe.b < 0.0 {
            return Err(ValidationError::InvalidNumber);
        }
    }
    if recipe.b == 0.0 || !recipe.a.is_finite() || !recipe.b.is_finite() {
        return Err(ValidationError::InvalidNumber);
    }
    if matches!(
        recipe.operation,
        Operation::BasePeriod | Operation::GrowthAmount
    ) && !(-95.0..=300.0).contains(&recipe.b)
    {
        return Err(ValidationError::InvalidNumber);
    }
    if matches!(recipe.operation, Operation::AverageGrowthRate) && recipe.b <= -100.0 {
        return Err(ValidationError::InvalidNumber);
    }
    let answer = match recipe.operation {
        Operation::AverageGrowthAmount | Operation::ShareChange | Operation::MixedGrowthRate => {
            let c = recipe.c.ok_or(ValidationError::MissingOperand)?;
            let d = recipe.d.ok_or(ValidationError::MissingOperand)?;
            if !c.is_finite()
                || !d.is_finite()
                || !source_contains_numeric_token(&recipe.evidence, c)
                || !source_contains_numeric_token(&recipe.evidence, d)
            {
                return Err(ValidationError::MissingOperand);
            }
            match recipe.operation {
                // current total, current population, total growth %, population growth %
                Operation::AverageGrowthAmount if recipe.b > 0.0 && c > -100.0 => {
                    recipe.a / recipe.b * (c - d) / (100.0 + c)
                }
                // current part, current whole, part growth %, whole growth %
                Operation::ShareChange if recipe.b > 0.0 && c > -100.0 && d > -100.0 => {
                    recipe.a / recipe.b * (c - d) / (1.0 + c / 100.0)
                }
                // base part 1, part1 growth %, base part 2, part2 growth %
                Operation::MixedGrowthRate if recipe.a >= 0.0 && c >= 0.0 && recipe.a + c > 0.0 => {
                    (recipe.a * recipe.b + c * d) / (recipe.a + c)
                }
                _ => return Err(ValidationError::InvalidNumber),
            }
        }
        _ => recipe
            .operation
            .result(recipe.a, recipe.b)
            .ok_or(ValidationError::InvalidNumber)?,
    };
    if !answer.is_finite() || answer.abs() > 1.0e15 {
        return Err(ValidationError::InvalidNumber);
    }
    if answer.abs() < 0.01 {
        return Err(ValidationError::InvalidNumber);
    }
    let correct_index = position % 4;
    // 国考约数选项：按量级取约三位有效数字，避免“70640.32万吨”这类
    // 机械保留两位小数的假精度；错误项拉开可判断的区间。
    let scale = 10_f64.powf(answer.abs().log10().floor() - 2.0).max(0.01);
    let values = [
        (answer / scale).round() * scale,
        (answer * 0.92 / scale).round() * scale,
        (answer * 1.08 / scale).round() * scale,
        (answer * 1.16 / scale).round() * scale,
    ];
    let decimals = if scale >= 1.0 {
        0
    } else if scale >= 0.1 {
        1
    } else {
        2
    };
    let mut candidates = [String::new(), String::new(), String::new(), String::new()];
    let mut wrong = 1;
    for (i, option) in candidates.iter_mut().enumerate() {
        let value = if i == correct_index {
            values[0]
        } else {
            let value = values[wrong];
            wrong += 1;
            value
        };
        *option = format!("{value:.decimals$}");
    }
    if candidates
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len()
        != 4
    {
        return Err(ValidationError::Duplicate);
    }
    let answer_text = candidates[correct_index].clone();
    let options = candidates.map(|value| format!("{value}{}", recipe.unit));
    Ok(VerifiedQuestion {
        question: recipe.question.clone(),
        options,
        correct_index,
        answer: format!("{answer_text}{}", recipe.unit),
        explanation: recipe.explanation.clone(),
        skill: recipe.operation.skill().to_owned(),
        evidence: recipe.evidence.clone(),
    })
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
    fn recipe() -> QuestionRecipe {
        QuestionRecipe {
            question: "已知粮食产量增长1.2%，求上年粮食产量约为多少万吨？".into(),
            evidence: material().text,
            operation: Operation::BasePeriod,
            a: 71488.0,
            b: 1.2,
            c: None,
            d: None,
            unit: "万吨".into(),
            explanation: "现期除以增长因子".into(),
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
    #[test]
    fn rejects_comprehensive_judgment_but_keeps_numeric_questions() {
        let source = material();
        assert!(verify_recipe(&source, &recipe(), 0).is_ok());
        for stem in [
            "根据上述资料，下列说法正确的是",
            "根据上述资料，能够推出的是",
            "关于粮食产量，以下表述错误的是",
        ] {
            let mut candidate = recipe();
            candidate.question = stem.to_owned();
            assert_eq!(
                verify_recipe(&source, &candidate, 0).unwrap_err(),
                ValidationError::InvalidQuestion
            );
        }
    }

    #[test]
    fn exam_choices_use_reasonable_precision_not_spurious_decimals() {
        let answer = verify_recipe(&material(), &recipe(), 0).unwrap();
        assert!(answer.options.iter().all(|v| !v.contains(".32")));
        assert_eq!(answer.options.len(), 4);
    }
    #[test]
    fn requires_real_evidence_and_two_source_numbers() {
        assert!(verify_recipe(&material(), &recipe(), 0).is_ok());
        let mut wrong = recipe();
        wrong.b = 33.3;
        assert_eq!(
            verify_recipe(&material(), &wrong, 0).unwrap_err(),
            ValidationError::MissingOperand
        );
        wrong = recipe();
        wrong.evidence = "2025年粮食产量71488万吨，比上年增长1.2%".into();
        assert_eq!(
            verify_recipe(&material(), &wrong, 0).unwrap_err(),
            ValidationError::MissingEvidence
        );
    }
    #[test]
    fn negative_growth_has_negative_amount_and_four_unique_options() {
        let m = Material {
            text: "2025年羊肉产量496万吨，下降4.2%。".into(),
            ..material()
        };
        let q = QuestionRecipe {
            question: "2025年羊肉产量同比增长量约为多少万吨？".into(),
            evidence: m.text.clone(),
            operation: Operation::GrowthAmount,
            a: 496.0,
            b: -4.2,
            c: None,
            d: None,
            unit: "万吨".into(),
            explanation: "现期乘增长率除增长因子".into(),
        };
        let mut q = q;
        q.b = 4.2;
        assert_eq!(
            verify_recipe(&m, &q, 0).unwrap_err(),
            ValidationError::InvalidNumber
        );
        q.b = -4.2;
        let verified = verify_recipe(&m, &q, 0).expect("negative rates are valid");
        assert!(verified.answer.starts_with('-'));
        assert_eq!(
            verified
                .options
                .iter()
                .filter(|o| o.starts_with('-'))
                .count(),
            4
        );
    }
    #[test]
    fn increase_question_cannot_be_graded_as_prior_year_stock() {
        let mut item = recipe();
        item.question = "2025年末存量比2024年末增加约多少万吨？".into();
        item.operation = Operation::BasePeriod;
        assert_eq!(
            verify_recipe(&material(), &item, 0).unwrap_err(),
            ValidationError::InvalidQuestion
        );
        item.operation = Operation::GrowthAmount;
        assert!(verify_recipe(&material(), &item, 0).is_ok());
    }

    #[test]
    fn average_growth_rate_uses_total_and_count_growth() {
        let mut item = recipe();
        item.question = "2025年的人均产量同比增长率约为多少？".into();
        item.operation = Operation::AverageGrowthRate;
        item.a = 1.2;
        item.b = 1.2;
        assert_eq!(item.operation.result(15.0, 5.0), Some((15.0 - 5.0) / 1.05));
    }

    #[test]
    fn compound_operations_require_four_source_operands_and_match_question_semantics() {
        let m=Material{
            text:"2022年，行业销售额1200亿元，同比增长10%；从业人数120万人，同比增长5%；甲行业增加值200亿元，同比增长20%，乙行业增加值800亿元，同比增长10%。".into(),
            ..material()
        };
        let mut q = QuestionRecipe {
            question: "2022年行业人均销售额比2021年增加约多少亿元？".into(),
            evidence: m.text.clone(),
            operation: Operation::AverageGrowthAmount,
            a: 1200.0,
            b: 120.0,
            c: Some(10.0),
            d: Some(5.0),
            unit: "亿元".into(),
            explanation: "平均数增量".into(),
        };
        assert!(verify_recipe(&m, &q, 0).is_ok());
        q.c = None;
        assert_eq!(
            verify_recipe(&m, &q, 0).unwrap_err(),
            ValidationError::MissingOperand
        );
        q.c = Some(20.0);
        q.operation = Operation::ShareChange;
        q.question = "2022年甲行业比重比2021年提高约多少个百分点？".into();
        q.a = 200.0;
        q.b = 1200.0;
        q.d = Some(10.0);
        q.unit = "个百分点".into();
        assert!(verify_recipe(&m, &q, 0).is_ok());
        q.operation = Operation::MixedGrowthRate;
        q.question = "2022年甲乙行业合计同比增长率约为多少？".into();
        q.a = 200.0;
        q.b = 20.0;
        q.c = Some(800.0);
        q.d = Some(10.0);
        q.unit = "%".into();
        assert!(verify_recipe(&m, &q, 0).is_ok());
    }
}
