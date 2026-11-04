//! Two parallel destinations under each exam module: 默认 and AI出题.
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
        target_deck: "资料分析/AI出题/乘除变化/错题",
        relative_path: "资料分析/AI出题/乘除变化/错题/cards.md",
        tags: &["资料分析", "错题", "乘除变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/AI出题/假设代入思想/错题",
        relative_path: "资料分析/AI出题/假设代入思想/错题/cards.md",
        tags: &["资料分析", "错题", "假设代入思想"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/AI出题/分数比较大小/错题",
        relative_path: "资料分析/AI出题/分数比较大小/错题/cards.md",
        tags: &["资料分析", "错题", "分数比较大小"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/AI出题/增长贡献率和拉动增长率/错题",
        relative_path: "资料分析/AI出题/增长贡献率和拉动增长率/错题/cards.md",
        tags: &["资料分析", "错题", "增长贡献率和拉动增长率"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/AI出题/年均变化/错题",
        relative_path: "资料分析/AI出题/年均变化/错题/cards.md",
        tags: &["资料分析", "错题", "年均变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/AI出题/线段混合/错题",
        relative_path: "资料分析/AI出题/线段混合/错题/cards.md",
        tags: &["资料分析", "错题", "线段混合"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/AI出题/计算方法及误差分析/错题",
        relative_path: "资料分析/AI出题/计算方法及误差分析/错题/cards.md",
        tags: &["资料分析", "错题", "计算方法及误差分析"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/AI出题/形式推理/直言命题/错题",
        relative_path: "判断推理/AI出题/形式推理/直言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "直言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/AI出题/形式推理/联言选言假言命题/错题",
        relative_path: "判断推理/AI出题/形式推理/联言选言假言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "联言选言假言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/AI出题/形式推理/组合排列/错题",
        relative_path: "判断推理/AI出题/形式推理/组合排列/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "组合排列"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/AI出题/形式推理/特殊题型/错题",
        relative_path: "判断推理/AI出题/形式推理/特殊题型/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "特殊题型"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/经济利润/错题",
        relative_path: "数量关系/AI出题/经济利润/错题/cards.md",
        tags: &["数量关系", "错题", "经济利润"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/工程效率/错题",
        relative_path: "数量关系/AI出题/工程效率/错题/cards.md",
        tags: &["数量关系", "错题", "工程效率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/行程/相对运动/错题",
        relative_path: "数量关系/AI出题/行程/相对运动/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "相对运动"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/行程/综合应用/错题",
        relative_path: "数量关系/AI出题/行程/综合应用/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "综合应用"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/溶液/错题",
        relative_path: "数量关系/AI出题/溶液/错题/cards.md",
        tags: &["数量关系", "错题", "溶液"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/最值/错题",
        relative_path: "数量关系/AI出题/最值/错题/cards.md",
        tags: &["数量关系", "错题", "最值"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/排列组合/基础/错题",
        relative_path: "数量关系/AI出题/排列组合/基础/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "基础"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/排列组合/插空与同素分堆/错题",
        relative_path: "数量关系/AI出题/排列组合/插空与同素分堆/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "插空与同素分堆"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/排列组合/特殊套路/错题",
        relative_path: "数量关系/AI出题/排列组合/特殊套路/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "特殊套路"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/概率/错题",
        relative_path: "数量关系/AI出题/概率/错题/cards.md",
        tags: &["数量关系", "错题", "概率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/AI出题/周期/错题",
        relative_path: "数量关系/AI出题/周期/错题/cards.md",
        tags: &["数量关系", "错题", "周期"],
    },
];

/// Explicit, parallel creation modes; independent of where existing method cards live.
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

fn module_root(module: &str) -> Option<&'static str> {
    match module {
        "资料分析" => Some("资料分析"),
        "数量关系" => Some("数量关系"),
        "判断推理" => Some("判断推理"),
        "言语理解与表达" => Some("言语理解与表达"),
        "常识判断" => Some("常识判断"),
        "政治理论" => Some("政治理论"),
        _ => None,
    }
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

fn canonical_root(path: &str) -> String {
    for (short, full) in [
        ("资料/", "资料分析/"), ("判断/", "判断推理/"),
        ("数量/", "数量关系/"), ("言语/", "言语理解与表达/"),
    ] {
        if let Some(rest) = path.strip_prefix(short) {
            return format!("{full}{rest}");
        }
    }
    path.to_owned()
}

/// New writes require an explicit mode directory. Existing abbreviated AI paths
/// remain readable for sync, deduplication, and current training decks.
pub fn resolve_target(path: &str) -> Option<ResolvedTarget> {
    let canonical = canonical_root(path);
    if let Some(known) = AI_TARGETS.iter().find(|entry| entry.target_deck == canonical) {
        return Some(ResolvedTarget {
            module: known.module,
            target_deck: canonical,
            relative_path: known.relative_path.to_owned(),
            tags: known.tags.iter().map(|tag| (*tag).into()).collect(),
            mode: FlashcardCreationMode::AiGeneration,
        });
    }
    let parts: Vec<_> = canonical.split('/').collect();
    if let [root, "默认", category, "错题"] = parts.as_slice() {
        let module = [
            "政治理论", "常识判断", "言语理解与表达", "判断推理", "数量关系", "资料分析",
        ].into_iter().find(|module| module_root(module) == Some(*root))?;
        if !valid_category(category) { return None; }
        return Some(ResolvedTarget {
            module,
            target_deck: canonical.clone(),
            relative_path: format!("{canonical}/cards.md"),
            tags: vec![(*root).into(), (*category).into(), "错题".into()],
            mode: FlashcardCreationMode::Default,
        });
    }
    // For already existing on-disk AI cards, try the new explicit AI taxonomy,
    // but keep the original physical filename if it is used as a source path.
    if parts.len() >= 3 && parts[1] != "默认" && parts[1] != "AI出题" {
        let candidate = format!("{}/AI出题/{}", parts[0], parts[1..].join("/"));
        if let Some(known) = AI_TARGETS.iter().find(|entry| entry.target_deck == candidate) {
            return Some(ResolvedTarget {
                module: known.module,
                target_deck: canonical,
                relative_path: format!("{path}/cards.md"),
                tags: known.tags.iter().map(|tag| (*tag).into()).collect(),
                mode: FlashcardCreationMode::AiGeneration,
            });
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteEligibility {
    pub eligible: bool,
    pub reason: Option<String>,
    pub recommended_target: Option<String>,
    pub allowed_targets: Vec<String>,
    pub requires_type_confirmation: bool,
    #[serde(default)]
    pub ai_targets: Vec<String>,
}

pub fn eligibility(module: Option<&str>, category: Option<&str>) -> FavoriteEligibility {
    let Some(module_name) = super::question_bank::primary_module(module) else {
        return rejected("原题没有可识别的一级模块");
    };
    let category = category.unwrap_or("").trim();
    let category = if category.is_empty() { "未分类" } else { category };
    if !valid_category(category) {
        return rejected("原题二级 category 含有不安全的目录字符");
    }
    let default = format!("{module_name}/默认/{category}/错题");
    let candidates = AI_TARGETS.iter().filter(|target| target.module == module_name).collect::<Vec<_>>();
    let exact = candidates.iter().filter(|target| {
        let parts = target.target_deck.split('/').collect::<Vec<_>>();
        category == parts[2..parts.len() - 1].join("/")
            || category == parts[parts.len() - 2]
    }).copied().collect::<Vec<_>>();
    let ai_matches = if !exact.is_empty() {
        exact
    } else if module_name == "数量关系"
        && matches!(category, "数学运算" | "数量关系" | "未分类")
    {
        candidates
    } else if module_name == "判断推理"
        && matches!(category, "逻辑判断" | "形式推理" | "判断推理" | "未分类")
    {
        candidates
    } else if module_name == "数量关系" && matches!(category, "行程" | "行程问题") {
        candidates.into_iter().filter(|t| t.target_deck.starts_with("数量关系/AI出题/行程/")).collect()
    } else if module_name == "数量关系" && matches!(category, "排列组合" | "排列组合问题") {
        candidates.into_iter().filter(|t| t.target_deck.starts_with("数量关系/AI出题/排列组合/")).collect()
    } else {
        Vec::new()
    };
    let ai_targets = ai_matches.into_iter().map(|target| target.target_deck.to_owned()).collect::<Vec<_>>();
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
        eligible: false, reason: Some(reason.into()), recommended_target: None,
        allowed_targets: Vec::new(), requires_type_confirmation: false,
        ai_targets: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_default_category_decks_for_every_exam_module() {
        for module in ["政治理论","常识判断","言语理解与表达","判断推理","数量关系","资料分析"] {
            let preview = eligibility(Some(module), Some("综合分析与审题"));
            let path = format!("{module}/默认/综合分析与审题/错题");
            assert_eq!(preview.recommended_target.as_deref(), Some(path.as_str()));
            assert_eq!(resolve_target(&path).unwrap().mode, FlashcardCreationMode::Default);
        }
    }

    #[test]
    fn ai_and_default_have_explicit_independent_directories() {
        let preview = eligibility(Some("数量关系"), Some("经济利润"));
        assert_eq!(preview.recommended_target.as_deref(), Some("数量关系/默认/经济利润/错题"));
        assert_eq!(preview.ai_targets, vec!["数量关系/AI出题/经济利润/错题"]);
        assert_eq!(resolve_target(&preview.ai_targets[0]).unwrap().mode, FlashcardCreationMode::AiGeneration);
        assert_eq!(resolve_target("数量/经济利润/错题").unwrap().mode, FlashcardCreationMode::AiGeneration);
    }

    #[test]
    fn generic_review_categories_not_supported_by_ai_still_work() {
        let preview = eligibility(Some("判断推理"), Some("论证推理"));
        assert!(preview.eligible);
        assert!(preview.ai_targets.is_empty());
        assert_eq!(preview.allowed_targets, vec!["判断推理/默认/论证推理/错题"]);
    }

    #[test]
    fn disallows_traversal_and_invalid_categories() {
        for path in [
            "资料分析/默认/../错题", "资料分析/默认/a/b/错题",
            "判断推理/默认/.secret/错题", "数量关系/默认/a\\b/错题",
            "数量关系/AI出题/未知/错题",
        ] {
            assert!(resolve_target(path).is_none(), "{path}");
        }
    }

    #[test]
    fn coarse_ai_category_requires_confirmation() {
        let preview = eligibility(Some("数量关系"), Some("数学运算"));
        assert_eq!(preview.ai_targets.len(), 11);
        assert!(preview.requires_type_confirmation);
    }
}
