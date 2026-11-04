//! Two parallel flashcard destinations: fixed AI generation decks and category review decks.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavoriteTarget {
    pub module: &'static str,
    pub target_deck: &'static str,
    pub relative_path: &'static str,
    pub tags: &'static [&'static str],
}

pub const AI_TARGETS: &[FavoriteTarget] = &[
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/乘除变化/错题",
        relative_path: "资料/乘除变化/错题/cards.md",
        tags: &["资料分析", "错题", "乘除变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/假设代入思想/错题",
        relative_path: "资料/假设代入思想/错题/cards.md",
        tags: &["资料分析", "错题", "假设代入思想"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/分数比较大小/错题",
        relative_path: "资料/分数比较大小/错题/cards.md",
        tags: &["资料分析", "错题", "分数比较大小"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/增长贡献率和拉动增长率/错题",
        relative_path: "资料/增长贡献率和拉动增长率/错题/cards.md",
        tags: &["资料分析", "错题", "增长贡献率和拉动增长率"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/年均变化/错题",
        relative_path: "资料/年均变化/错题/cards.md",
        tags: &["资料分析", "错题", "年均变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/线段混合/错题",
        relative_path: "资料/线段混合/错题/cards.md",
        tags: &["资料分析", "错题", "线段混合"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料/计算方法及误差分析/错题",
        relative_path: "资料/计算方法及误差分析/错题/cards.md",
        tags: &["资料分析", "错题", "计算方法及误差分析"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/直言命题/错题",
        relative_path: "判断/形式推理/直言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "直言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/联言选言假言命题/错题",
        relative_path: "判断/形式推理/联言选言假言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "联言选言假言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/组合排列/错题",
        relative_path: "判断/形式推理/组合排列/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "组合排列"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断/形式推理/特殊题型/错题",
        relative_path: "判断/形式推理/特殊题型/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "特殊题型"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/经济利润/错题",
        relative_path: "数量/经济利润/错题/cards.md",
        tags: &["数量关系", "错题", "经济利润"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/工程效率/错题",
        relative_path: "数量/工程效率/错题/cards.md",
        tags: &["数量关系", "错题", "工程效率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/行程/相对运动/错题",
        relative_path: "数量/行程/相对运动/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "相对运动"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/行程/综合应用/错题",
        relative_path: "数量/行程/综合应用/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "综合应用"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/溶液/错题",
        relative_path: "数量/溶液/错题/cards.md",
        tags: &["数量关系", "错题", "溶液"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/最值/错题",
        relative_path: "数量/最值/错题/cards.md",
        tags: &["数量关系", "错题", "最值"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/排列组合/基础/错题",
        relative_path: "数量/排列组合/基础/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "基础"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/排列组合/插空与同素分堆/错题",
        relative_path: "数量/排列组合/插空与同素分堆/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "插空与同素分堆"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/排列组合/特殊套路/错题",
        relative_path: "数量/排列组合/特殊套路/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "特殊套路"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/概率/错题",
        relative_path: "数量/概率/错题/cards.md",
        tags: &["数量关系", "错题", "概率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量/周期/错题",
        relative_path: "数量/周期/错题/cards.md",
        tags: &["数量关系", "错题", "周期"],
    },
];

/// Default cards use the canonical full exam-module name; AI training keeps its
/// dedicated, independently supported deck paths. Neither mode is a migration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashcardCreationMode {
    Default,
    AiGeneration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub module: &'static str,
    pub target_deck: String,
    pub relative_path: String,
    pub tags: Vec<String>,
    pub mode: FlashcardCreationMode,
}

fn valid_category(category: &str) -> bool {
    !category.is_empty()
        && category.len() <= 180
        && category != "."
        && category != ".."
        && !category.starts_with('.')
        && !category.ends_with(' ')
        && !category.ends_with('.')
        && !category.chars().any(|ch| {
            ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}

/// Resolve a destination to a bounded category directory or a registered AI deck.
pub fn resolve_target(target: &str) -> Option<ResolvedTarget> {
    if let Some(known) = AI_TARGETS.iter().find(|entry| entry.target_deck == target) {
        return Some(ResolvedTarget {
            module: known.module,
            target_deck: target.to_owned(),
            relative_path: known.relative_path.to_owned(),
            tags: known.tags.iter().map(|tag| (*tag).to_owned()).collect(),
            mode: FlashcardCreationMode::AiGeneration,
        });
    }
    let parts = target.split('/').collect::<Vec<_>>();
    let [module, category, "错题"] = parts.as_slice() else {
        return None;
    };
    let module = [
        "资料分析", "判断推理", "数量关系", "言语理解与表达",
        "常识判断", "政治理论",
    ].into_iter().find(|candidate| candidate == module)?;
    if !valid_category(category) {
        return None;
    }
    Some(ResolvedTarget {
        module,
        target_deck: target.to_owned(),
        relative_path: format!("{target}/cards.md"),
        tags: vec![module.into(), (*category).into(), "错题".into()],
        mode: FlashcardCreationMode::Default,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteEligibility {
    pub eligible: bool,
    pub reason: Option<String>,
    /// Full-name module/category/错题; not selected until the user confirms it.
    pub recommended_target: Option<String>,
    pub allowed_targets: Vec<String>,
    pub requires_type_confirmation: bool,
    /// Independently offered AI generation targets; empty for unsupported categories.
    #[serde(default)]
    pub ai_targets: Vec<String>,
}

pub fn eligibility(module: Option<&str>, category: Option<&str>) -> FavoriteEligibility {
    let Some(module_name) = super::question_bank::primary_module(module) else {
        return rejected("原题没有可识别的一级模块");
    };
    let category = category.unwrap_or("").trim();
    let fallback = if category.is_empty() { "未分类" } else { category };
    if !valid_category(fallback) {
        return rejected("原题二级 category 含有不安全的目录字符");
    }
    // Default is available for every recognized module, regardless of AI generation coverage.
    let default = format!("{module_name}/{fallback}/错题");
    let candidates = AI_TARGETS.iter()
        .filter(|target| target.module == module_name)
        .collect::<Vec<_>>();
    let exact = candidates.iter().filter(|target| {
        let parts = target.target_deck.split('/').collect::<Vec<_>>();
        category == parts[1..parts.len() - 1].join("/")
            || category == parts[parts.len() - 2]
    }).copied().collect::<Vec<_>>();
    let ai_matches = if !exact.is_empty() {
        exact
    } else if module_name == "数量关系"
        && matches!(category, "" | "数学运算" | "数量关系" | "未分类")
    {
        candidates
    } else if module_name == "判断推理"
        && matches!(category, "" | "逻辑判断" | "形式推理" | "判断推理" | "未分类")
    {
        candidates
    } else if module_name == "数量关系" && matches!(category, "行程" | "行程问题") {
        candidates.into_iter().filter(|t| t.target_deck.starts_with("数量/行程/")).collect()
    } else if module_name == "数量关系"
        && matches!(category, "排列组合" | "排列组合问题")
    {
        candidates.into_iter().filter(|t| t.target_deck.starts_with("数量/排列组合/")).collect()
    } else {
        Vec::new()
    };
    let ai_targets = ai_matches.iter().map(|target| target.target_deck.to_owned()).collect::<Vec<_>>();
    let mut allowed_targets = vec![default.clone()];
    allowed_targets.extend(ai_targets.iter().cloned());
    FavoriteEligibility {
        eligible: true,
        reason: None,
        recommended_target: Some(default),
        allowed_targets,
        requires_type_confirmation: ai_targets.len() > 1,
        ai_targets,
    }
}

fn rejected(reason: &str) -> FavoriteEligibility {
    FavoriteEligibility {
        eligible: false,
        reason: Some(reason.into()),
        recommended_target: None,
        allowed_targets: Vec::new(),
        requires_type_confirmation: false,
        ai_targets: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_decks_are_full_module_names_for_every_category() {
        for module in ["资料分析", "数量关系", "判断推理", "言语理解与表达", "常识判断", "政治理论"] {
            let e = eligibility(Some(module), Some("综合分析与审题"));
            let target = format!("{module}/综合分析与审题/错题");
            assert_eq!(e.recommended_target.as_deref(), Some(target.as_str()));
            assert_eq!(resolve_target(&target).unwrap().mode, FlashcardCreationMode::Default);
            assert!(e.ai_targets.is_empty());
        }
    }

    #[test]
    fn ai_and_default_are_parallel_targets_even_when_category_names_match() {
        let e = eligibility(Some("资料分析"), Some("乘除变化"));
        assert_eq!(e.recommended_target.as_deref(), Some("资料分析/乘除变化/错题"));
        assert_eq!(e.ai_targets, vec!["资料/乘除变化/错题"]);
        assert_eq!(resolve_target(&e.ai_targets[0]).unwrap().mode, FlashcardCreationMode::AiGeneration);
        assert_eq!(resolve_target(e.recommended_target.as_ref().unwrap()).unwrap().mode, FlashcardCreationMode::Default);
    }

    #[test]
    fn unsupported_ai_category_does_not_disable_default_review() {
        let e = eligibility(Some("判断推理"), Some("论证推理"));
        assert!(e.eligible);
        assert_eq!(e.ai_targets, Vec::<String>::new());
        assert_eq!(e.allowed_targets, vec!["判断推理/论证推理/错题"]);
    }

    #[test]
    fn rejects_untrusted_target_segments() {
        for target in [
            "资料分析/../错题", "资料分析/a/b/错题", "资料分析/.hidden/错题",
            "数量关系/hello\\world/错题", "数量关系/题目/方法",
            "数量关系/题目/错题/extra", "资料/非法分类/错题",
        ] {
            assert!(resolve_target(target).is_none(), "{target}");
        }
    }

    #[test]
    fn broad_math_may_choose_either_mode() {
        let e = eligibility(Some("数量关系"), Some("数学运算"));
        assert_eq!(e.ai_targets.len(), 11);
        assert_eq!(e.allowed_targets.len(), 12);
        assert!(e.requires_type_confirmation);
    }
}
