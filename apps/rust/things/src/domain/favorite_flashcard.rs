//! Fixed creation targets shared by the favorites UI and generation eligibility.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteTarget {
    pub module: &'static str,
    pub target_deck: &'static str,
    pub relative_path: &'static str,
    pub tags: &'static [&'static str],
}

pub const TARGETS: &[FavoriteTarget] = &[
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/乘除变化/错题",
        relative_path: "资料/乘除变化/错题/cards.md",
        tags: &["资料", "乘除变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/假设代入思想/错题",
        relative_path: "资料/假设代入思想/错题/cards.md",
        tags: &["资料", "假设代入思想"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/分数比较大小/错题",
        relative_path: "资料/分数比较大小/错题/cards.md",
        tags: &["资料", "分数比较大小"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/增长贡献率和拉动增长率/错题",
        relative_path: "资料/增长贡献率和拉动增长率/错题/cards.md",
        tags: &["资料", "增长贡献率和拉动增长率"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/年均变化/错题",
        relative_path: "资料/年均变化/错题/cards.md",
        tags: &["资料", "年均变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/线段混合/错题",
        relative_path: "资料/线段混合/错题/cards.md",
        tags: &["资料", "线段混合"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/计算方法及误差分析/错题",
        relative_path: "资料/计算方法及误差分析/错题/cards.md",
        tags: &["资料", "计算方法及误差分析"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/直言命题/错题",
        relative_path: "判断/形式推理/直言命题/错题/cards.md",
        tags: &["判断", "形式推理", "直言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/联言选言假言命题/错题",
        relative_path: "判断/形式推理/联言选言假言命题/错题/cards.md",
        tags: &["判断", "形式推理", "联言选言假言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/组合排列/错题",
        relative_path: "判断/形式推理/组合排列/错题/cards.md",
        tags: &["判断", "形式推理", "组合排列"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/特殊题型/错题",
        relative_path: "判断/形式推理/特殊题型/错题/cards.md",
        tags: &["判断", "形式推理", "特殊题型"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/经济利润/错题",
        relative_path: "数量/经济利润/错题/cards.md",
        tags: &["数量", "经济利润"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/工程效率/错题",
        relative_path: "数量/工程效率/错题/cards.md",
        tags: &["数量", "工程效率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/行程/相对运动/错题",
        relative_path: "数量/行程/相对运动/错题/cards.md",
        tags: &["数量", "行程", "相对运动"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/行程/综合应用/错题",
        relative_path: "数量/行程/综合应用/错题/cards.md",
        tags: &["数量", "行程", "综合应用"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/溶液/错题",
        relative_path: "数量/溶液/错题/cards.md",
        tags: &["数量", "溶液"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/最值/错题",
        relative_path: "数量/最值/错题/cards.md",
        tags: &["数量", "最值"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/排列组合/基础/错题",
        relative_path: "数量/排列组合/基础/错题/cards.md",
        tags: &["数量", "排列组合", "基础"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/排列组合/插空与同素分堆/错题",
        relative_path: "数量/排列组合/插空与同素分堆/错题/cards.md",
        tags: &["数量", "排列组合", "插空与同素分堆"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/排列组合/特殊套路/错题",
        relative_path: "数量/排列组合/特殊套路/错题/cards.md",
        tags: &["数量", "排列组合", "特殊套路"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/概率/错题",
        relative_path: "数量/概率/错题/cards.md",
        tags: &["数量", "概率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/周期/错题",
        relative_path: "数量/周期/错题/cards.md",
        tags: &["数量", "周期"],
    },
];

pub fn resolve_target(target: &str) -> Option<&'static FavoriteTarget> {
    TARGETS.iter().find(|entry| entry.target_deck == target)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteEligibility {
    pub eligible: bool,
    pub reason: Option<String>,
    pub recommended_target: Option<String>,
    pub allowed_targets: Vec<String>,
    pub requires_type_confirmation: bool,
}

pub fn eligibility(module: Option<&str>, category: Option<&str>) -> FavoriteEligibility {
    let Some(module_name) = super::question_bank::primary_module(module) else {
        return rejected("当前模块暂不支持创建 Flashcard");
    };
    let targets = TARGETS
        .iter()
        .filter(|target| target.module == module_name)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return rejected("当前模块暂不支持创建 Flashcard");
    }
    let category = category.unwrap_or("").trim();
    if module_name == "判断推理"
        && [module.unwrap_or(""), category].iter().any(|value| {
            ["图形推理", "定义判断", "类比推理", "论证推理", "逻辑论证"]
                .iter()
                .any(|unsupported| value.split('/').any(|part| part == *unsupported))
        })
    {
        return rejected("当前仅支持四类形式推理，暂不支持该题型");
    }
    if module_name == "数量关系" && category == "数字推理" {
        return rejected("当前数量关系生成器暂不支持数字推理");
    }
    let exact = targets.iter().find(|target| {
        let parts = target.target_deck.split('/').collect::<Vec<_>>();
        category == parts[1..parts.len() - 1].join("/") || category == parts[parts.len() - 2]
    });
    if let Some(target) = exact {
        return FavoriteEligibility {
            eligible: true,
            reason: None,
            recommended_target: Some(target.target_deck.into()),
            allowed_targets: vec![target.target_deck.into()],
            requires_type_confirmation: false,
        };
    }
    let prefix = match (module_name, category) {
        ("数量关系", "行程" | "行程问题") => Some("数量/行程/"),
        ("数量关系", "排列组合" | "排列组合问题") => Some("数量/排列组合/"),
        ("数量关系", "" | "数学运算" | "数量关系" | "数量" | "未分类") => None,
        ("数量关系", _) => return rejected("当前数量关系生成器暂不支持该具体题型"),
        ("判断推理", "" | "逻辑判断" | "形式推理" | "判断推理" | "未分类") => {
            None
        }
        ("判断推理", _) => return rejected("当前仅支持四类形式推理，请检查原题分类"),
        _ => None,
    };
    let targets = targets
        .into_iter()
        .filter(|target| prefix.is_none_or(|prefix| target.target_deck.starts_with(prefix)))
        .collect::<Vec<_>>();
    FavoriteEligibility {
        eligible: true,
        reason: None,
        recommended_target: None,
        allowed_targets: targets
            .iter()
            .map(|target| target.target_deck.into())
            .collect(),
        requires_type_confirmation: true,
    }
}

fn rejected(reason: &str) -> FavoriteEligibility {
    FavoriteEligibility {
        eligible: false,
        reason: Some(reason.into()),
        recommended_target: None,
        allowed_targets: Vec::new(),
        requires_type_confirmation: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_precise_math_type_is_rejected_while_broad_math_needs_confirmation() {
        assert!(!eligibility(Some("数量关系"), Some("几何问题")).eligible);
        let broad = eligibility(Some("数量关系"), Some("数学运算"));
        assert!(broad.eligible && broad.requires_type_confirmation);
        assert_eq!(broad.allowed_targets.len(), 11);
    }

    #[test]
    fn rejects_known_unsupported_secondary_categories() {
        assert!(!eligibility(Some("数量关系"), Some("数字推理")).eligible);
        assert!(!eligibility(Some("判断推理"), Some("论证推理")).eligible);
        assert!(!eligibility(Some("言语"), None).eligible);
    }

    #[test]
    fn exact_supported_category_recommends_its_fixed_target() {
        let eligibility = eligibility(Some("数量关系"), Some("工程效率"));
        assert_eq!(
            eligibility.recommended_target.as_deref(),
            Some("数量/工程效率/错题")
        );
        assert_eq!(eligibility.allowed_targets, vec!["数量/工程效率/错题"]);
        assert!(!eligibility.requires_type_confirmation);
    }
}
