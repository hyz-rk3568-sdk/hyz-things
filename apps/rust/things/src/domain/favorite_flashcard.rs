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
        target_deck: "资料分析/乘除变化/错题",
        relative_path: "资料分析/乘除变化/错题/cards.md",
        tags: &["资料分析", "错题", "乘除变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/假设代入思想/错题",
        relative_path: "资料分析/假设代入思想/错题/cards.md",
        tags: &["资料分析", "错题", "假设代入思想"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/分数比较大小/错题",
        relative_path: "资料分析/分数比较大小/错题/cards.md",
        tags: &["资料分析", "错题", "分数比较大小"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/增长贡献率和拉动增长率/错题",
        relative_path: "资料分析/增长贡献率和拉动增长率/错题/cards.md",
        tags: &["资料分析", "错题", "增长贡献率和拉动增长率"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/年均变化/错题",
        relative_path: "资料分析/年均变化/错题/cards.md",
        tags: &["资料分析", "错题", "年均变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/线段混合/错题",
        relative_path: "资料分析/线段混合/错题/cards.md",
        tags: &["资料分析", "错题", "线段混合"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "资料分析/计算方法及误差分析/错题",
        relative_path: "资料分析/计算方法及误差分析/错题/cards.md",
        tags: &["资料分析", "错题", "计算方法及误差分析"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/形式推理/直言命题/错题",
        relative_path: "判断推理/形式推理/直言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "直言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/形式推理/联言选言假言命题/错题",
        relative_path: "判断推理/形式推理/联言选言假言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "联言选言假言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/形式推理/组合排列/错题",
        relative_path: "判断推理/形式推理/组合排列/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "组合排列"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "判断推理/形式推理/特殊题型/错题",
        relative_path: "判断推理/形式推理/特殊题型/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "特殊题型"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/经济利润/错题",
        relative_path: "数量关系/经济利润/错题/cards.md",
        tags: &["数量关系", "错题", "经济利润"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/工程效率/错题",
        relative_path: "数量关系/工程效率/错题/cards.md",
        tags: &["数量关系", "错题", "工程效率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/行程/相对运动/错题",
        relative_path: "数量关系/行程/相对运动/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "相对运动"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/行程/综合应用/错题",
        relative_path: "数量关系/行程/综合应用/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "综合应用"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/溶液/错题",
        relative_path: "数量关系/溶液/错题/cards.md",
        tags: &["数量关系", "错题", "溶液"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/最值/错题",
        relative_path: "数量关系/最值/错题/cards.md",
        tags: &["数量关系", "错题", "最值"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/排列组合/基础/错题",
        relative_path: "数量关系/排列组合/基础/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "基础"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/排列组合/插空与同素分堆/错题",
        relative_path: "数量关系/排列组合/插空与同素分堆/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "插空与同素分堆"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/排列组合/特殊套路/错题",
        relative_path: "数量关系/排列组合/特殊套路/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "特殊套路"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/概率/错题",
        relative_path: "数量关系/概率/错题/cards.md",
        tags: &["数量关系", "错题", "概率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "数量关系/周期/错题",
        relative_path: "数量关系/周期/错题/cards.md",
        tags: &["数量关系", "错题", "周期"],
    },
];

/// A validated deck path. Historic generator decks keep their existing paths,
/// while ordinary review cards use <module>/<category>/错题/cards.md.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub module: &'static str,
    pub target_deck: String,
    pub relative_path: String,
    pub tags: Vec<String>,
    pub ai_generation: bool,
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
        && !category.ends_with(' ') && !category.ends_with('.')
        && !category.chars().any(|ch| {
            ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
}

pub fn resolve_target(target: &str) -> Option<ResolvedTarget> {
    // Accept historic abbreviations only for resolving legacy generators and imports.
    // New creation and UI lists always use canonical full module names.
    let normalized = if let Some(tail) = target.strip_prefix("资料/") {
        format!("资料分析/{tail}")
    } else if let Some(tail) = target.strip_prefix("判断/") {
        format!("判断推理/{tail}")
    } else if let Some(tail) = target.strip_prefix("数量/") {
        format!("数量关系/{tail}")
    } else if let Some(tail) = target.strip_prefix("言语/") {
        format!("言语理解与表达/{tail}")
    } else {
        target.to_owned()
    };
    let target = normalized.as_str();
    if let Some(known) = TARGETS.iter().find(|entry| entry.target_deck == target) {
        return Some(ResolvedTarget {
            module: known.module,
            target_deck: target.to_owned(),
            relative_path: known.relative_path.to_owned(),
            tags: known.tags.iter().map(|tag| (*tag).to_owned()).collect(),
            ai_generation: true,
        });
    }
    let parts: Vec<_> = target.split('/').collect();
    let [root, category, "错题"] = parts.as_slice() else {
        return None;
    };
    let module = [
        "资料分析",
        "判断推理",
        "数量关系",
        "言语理解与表达",
        "常识判断",
        "政治理论",
    ]
    .into_iter()
    .find(|module| module_root(module) == Some(*root))?;
    if !valid_category(category) {
        return None;
    }
    Some(ResolvedTarget {
        module,
        target_deck: target.to_owned(),
        relative_path: format!("{target}/cards.md"),
        tags: vec![(*root).to_owned(), (*category).to_owned(), "错题".into()],
        ai_generation: false,
    })
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
    let root = module_root(module_name).expect("all recognized question-bank modules have a deck root");
    let category = category.unwrap_or("").trim();
    let fallback = if category.is_empty() { "未分类" } else { category };
    if !valid_category(fallback) {
        return rejected("原题二级 category 含有不安全的目录字符");
    }
    let normal = format!("{root}/{fallback}/错题");
    let candidates = TARGETS.iter().filter(|target| target.module == module_name).collect::<Vec<_>>();
    let exact = candidates.iter().filter(|target| {
        let parts = target.target_deck.split('/').collect::<Vec<_>>();
        category == parts[1..parts.len()-1].join("/") || category == parts[parts.len()-2]
    }).copied().collect::<Vec<_>>();
    let likely = if !exact.is_empty() {
        exact
    } else if module_name == "数量关系" && matches!(category, "" | "数学运算" | "数量关系" | "未分类") {
        candidates
    } else if module_name == "判断推理" && matches!(category, "" | "逻辑判断" | "形式推理" | "判断推理" | "未分类") {
        candidates
    } else if module_name == "数量关系" && matches!(category, "行程" | "行程问题") {
        candidates.into_iter().filter(|t| t.target_deck.starts_with("数量关系/行程/")).collect()
    } else if module_name == "数量关系" && matches!(category, "排列组合" | "排列组合问题") {
        candidates.into_iter().filter(|t| t.target_deck.starts_with("数量关系/排列组合/")).collect()
    } else {
        Vec::new()
    };
    let mut targets = vec![normal.clone()];
    let mut ai_targets = Vec::new();
    for known in likely {
        if !targets.iter().any(|value| value == known.target_deck) {
            targets.push(known.target_deck.into());
        }
        ai_targets.push(known.target_deck.into());
    }
    FavoriteEligibility {
        eligible: true,
        reason: None,
        recommended_target: Some(normal),
        allowed_targets: targets,
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
    fn generic_category_supports_every_question_bank_module_without_ai() {
        for (module, root) in [
            ("资料分析", "资料"), ("数量关系", "数量"),
            ("判断推理", "判断"), ("言语理解与表达", "言语"),
            ("常识判断", "常识判断"), ("政治理论", "政治理论"),
        ] {
            let e = eligibility(Some(module), Some("综合分析与审题"));
            assert!(e.eligible);
            assert!(e.allowed_targets.contains(&format!("{root}/综合分析与审题/错题")));
            assert!(resolve_target(&format!("{root}/综合分析与审题/错题")).is_some());
        }
    }

    #[test]
    fn old_ai_decks_remain_selectable_and_can_coexist_with_non_ai_cards() {
        let e = eligibility(Some("判断推理"), Some("直言命题"));
        assert!(e.allowed_targets.contains(&"判断推理/直言命题/错题".to_owned()));
        assert!(e.allowed_targets.contains(&"判断推理/形式推理/直言命题/错题".to_owned()));
        assert!(e.ai_targets.contains(&"判断推理/形式推理/直言命题/错题".to_owned()));
    }

    #[test]
    fn rejects_untrusted_target_segments() {
        for target in ["资料分析/../错题", "资料分析/a/b/错题", "资料分析/.hidden/错题",
            "数量关系/hello\\world/错题", "数量关系/题目/方法", "数量关系/题目/错题/extra"] {
            assert!(resolve_target(target).is_none(), "{target}");
        }
    }

    #[test]
    fn broad_math_can_explicitly_select_old_generator_decks() {
        let e = eligibility(Some("数量关系"), Some("数学运算"));
        assert_eq!(e.ai_targets.len(), 11);
        assert!(e.requires_type_confirmation);
    }
}
