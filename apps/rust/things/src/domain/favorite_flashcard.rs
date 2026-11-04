//! Independent source roots: 真题练习 (default) and 用户整理 (AI source).
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
        target_deck: "用户整理/资料分析/乘除变化/错题",
        relative_path: "用户整理/资料分析/乘除变化/错题/cards.md",
        tags: &["资料分析", "错题", "乘除变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "用户整理/资料分析/假设代入思想/错题",
        relative_path: "用户整理/资料分析/假设代入思想/错题/cards.md",
        tags: &["资料分析", "错题", "假设代入思想"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "用户整理/资料分析/分数比较大小/错题",
        relative_path: "用户整理/资料分析/分数比较大小/错题/cards.md",
        tags: &["资料分析", "错题", "分数比较大小"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "用户整理/资料分析/增长贡献率和拉动增长率/错题",
        relative_path: "用户整理/资料分析/增长贡献率和拉动增长率/错题/cards.md",
        tags: &["资料分析", "错题", "增长贡献率和拉动增长率"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "用户整理/资料分析/年均变化/错题",
        relative_path: "用户整理/资料分析/年均变化/错题/cards.md",
        tags: &["资料分析", "错题", "年均变化"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "用户整理/资料分析/线段混合/错题",
        relative_path: "用户整理/资料分析/线段混合/错题/cards.md",
        tags: &["资料分析", "错题", "线段混合"],
    },
    FavoriteTarget {
        module: "资料分析",
        target_deck: "用户整理/资料分析/计算方法及误差分析/错题",
        relative_path: "用户整理/资料分析/计算方法及误差分析/错题/cards.md",
        tags: &["资料分析", "错题", "计算方法及误差分析"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "用户整理/判断推理/形式推理/直言命题/错题",
        relative_path: "用户整理/判断推理/形式推理/直言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "直言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "用户整理/判断推理/形式推理/联言选言假言命题/错题",
        relative_path: "用户整理/判断推理/形式推理/联言选言假言命题/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "联言选言假言命题"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "用户整理/判断推理/形式推理/组合排列/错题",
        relative_path: "用户整理/判断推理/形式推理/组合排列/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "组合排列"],
    },
    FavoriteTarget {
        module: "判断推理",
        target_deck: "用户整理/判断推理/形式推理/特殊题型/错题",
        relative_path: "用户整理/判断推理/形式推理/特殊题型/错题/cards.md",
        tags: &["判断推理", "错题", "形式推理", "特殊题型"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/经济利润/错题",
        relative_path: "用户整理/数量关系/经济利润/错题/cards.md",
        tags: &["数量关系", "错题", "经济利润"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/工程效率/错题",
        relative_path: "用户整理/数量关系/工程效率/错题/cards.md",
        tags: &["数量关系", "错题", "工程效率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/行程/相对运动/错题",
        relative_path: "用户整理/数量关系/行程/相对运动/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "相对运动"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/行程/综合应用/错题",
        relative_path: "用户整理/数量关系/行程/综合应用/错题/cards.md",
        tags: &["数量关系", "错题", "行程", "综合应用"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/溶液/错题",
        relative_path: "用户整理/数量关系/溶液/错题/cards.md",
        tags: &["数量关系", "错题", "溶液"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/最值/错题",
        relative_path: "用户整理/数量关系/最值/错题/cards.md",
        tags: &["数量关系", "错题", "最值"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/排列组合/基础/错题",
        relative_path: "用户整理/数量关系/排列组合/基础/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "基础"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/排列组合/插空与同素分堆/错题",
        relative_path: "用户整理/数量关系/排列组合/插空与同素分堆/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "插空与同素分堆"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/排列组合/特殊套路/错题",
        relative_path: "用户整理/数量关系/排列组合/特殊套路/错题/cards.md",
        tags: &["数量关系", "错题", "排列组合", "特殊套路"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/概率/错题",
        relative_path: "用户整理/数量关系/概率/错题/cards.md",
        tags: &["数量关系", "错题", "概率"],
    },
    FavoriteTarget {
        module: "数量关系",
        target_deck: "用户整理/数量关系/周期/错题",
        relative_path: "用户整理/数量关系/周期/错题/cards.md",
        tags: &["数量关系", "错题", "周期"],
    },
];

/// Explicit, parallel creation modes; independent of where existing method cards live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashcardCreationMode {
    Practice,
    UserOrganized,
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
        ("资料/", "资料分析/"),
        ("判断/", "判断推理/"),
        ("数量/", "数量关系/"),
        ("言语/", "言语理解与表达/"),
    ] {
        if let Some(rest) = path.strip_prefix(short) {
            return format!("{full}{rest}");
        }
    }
    path.to_owned()
}

/// Parse a bounded destination. The application also checks observed directories.
pub fn resolve_web_target(path: &str) -> Option<ResolvedTarget> {
    let parts = path.split('/').collect::<Vec<_>>();
    if !(4..=9).contains(&parts.len()) || parts.last() != Some(&"错题") {
        return None;
    }
    let module = module_root(parts[1])?;
    let categories = &parts[2..parts.len() - 1];
    if categories
        .iter()
        .any(|part| !valid_category(part) || *part == "错题")
    {
        return None;
    }
    let (mode, filename) = match parts[0] {
        "真题练习" if categories.len() == 1 => (FlashcardCreationMode::Practice, "cards.md"),
        "用户整理" => (FlashcardCreationMode::UserOrganized, "cards.md"),
        _ => return None,
    };
    let mut tags = vec![parts[1].to_owned()];
    tags.extend(categories.iter().map(|part| (*part).to_owned()));
    tags.push("错题".into());
    Some(ResolvedTarget {
        module,
        target_deck: path.to_owned(),
        relative_path: format!("{path}/{filename}"),
        tags,
        mode,
    })
}

/// Resolve current source files plus known pre-migration AI decks for reading.
/// Only resolve_web_target authorizes writes.
pub fn resolve_target(path: &str) -> Option<ResolvedTarget> {
    if path.split('/').count() == 4 {
        if let Some(target) = resolve_web_target(path) {
            return Some(target);
        }
    }
    // Migrated, nested method decks are registered AI source directories.
    if let Some(known) = AI_TARGETS.iter().find(|entry| entry.target_deck == path) {
        return Some(ResolvedTarget {
            module: known.module,
            target_deck: path.to_owned(),
            relative_path: format!("{path}/cards.md"),
            tags: known.tags.iter().map(|tag| (*tag).into()).collect(),
            mode: FlashcardCreationMode::UserOrganized,
        });
    }
    let canonical = canonical_root(path);
    let legacy = canonical.replace("/AI出题/", "/");
    let candidate = format!("用户整理/{legacy}");
    let candidate = if candidate.ends_with("/错题") {
        candidate
    } else {
        format!("{candidate}/错题")
    };
    let known = AI_TARGETS
        .iter()
        .find(|entry| entry.target_deck == candidate)?;
    Some(ResolvedTarget {
        module: known.module,
        target_deck: canonical,
        relative_path: format!("{path}/cards.md"),
        tags: known.tags.iter().map(|tag| (*tag).into()).collect(),
        mode: FlashcardCreationMode::UserOrganized,
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
    let category = category.unwrap_or("").trim();
    let category = if category.is_empty() {
        "未分类"
    } else {
        category
    };
    if !valid_category(category) {
        return rejected("原题二级 category 含有不安全的目录字符");
    }
    let practice = format!("真题练习/{module_name}/{category}/错题");
    let organized = format!("用户整理/{module_name}/{category}/错题");
    FavoriteEligibility {
        eligible: true,
        reason: None,
        recommended_target: Some(practice.clone()),
        allowed_targets: vec![practice, organized],
        requires_type_confirmation: false,
        ai_targets: Vec::new(),
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
    fn web_destination_defaults_to_practice_and_allows_user_organized_for_every_module() {
        for module in [
            "政治理论",
            "常识判断",
            "言语理解与表达",
            "判断推理",
            "数量关系",
            "资料分析",
        ] {
            let preview = eligibility(Some(module), Some("综合分析与审题"));
            let practice = format!("真题练习/{module}/综合分析与审题/错题");
            let organized = format!("用户整理/{module}/综合分析与审题/错题");
            assert_eq!(
                preview.recommended_target.as_deref(),
                Some(practice.as_str())
            );
            assert_eq!(
                preview.allowed_targets,
                [practice.clone(), organized.clone()]
            );
            assert_eq!(
                resolve_web_target(&practice).unwrap().mode,
                FlashcardCreationMode::Practice
            );
            assert_eq!(
                resolve_web_target(&organized).unwrap().mode,
                FlashcardCreationMode::UserOrganized
            );
        }
    }

    #[test]
    fn known_ai_sources_remain_readable_after_migration() {
        assert_eq!(
            resolve_target("数量/经济利润/错题").unwrap().module,
            "数量关系"
        );
        assert_eq!(
            resolve_target("判断推理/AI出题/形式推理/直言命题/错题")
                .unwrap()
                .module,
            "判断推理"
        );
        assert_eq!(
            resolve_target("用户整理/数量关系/经济利润/错题")
                .unwrap()
                .module,
            "数量关系"
        );
        assert_eq!(
            resolve_target("用户整理/判断推理/形式推理/直言命题/错题")
                .unwrap()
                .mode,
            FlashcardCreationMode::UserOrganized
        );
    }

    #[test]
    fn rejects_traversal_old_creation_modes_and_unregistered_destinations() {
        for path in [
            "真题练习/资料分析/../错题",
            "真题练习/资料分析/a/b/错题",
            "用户整理/判断推理/.secret/错题",
            "用户整理/数量关系/a\\\\b/错题",
            "真题收录/数量关系/工程效率/错题",
            "用户整理/数量关系/工程效率",
            "数量关系/默认/工程效率/错题",
            "数量关系/AI出题/未知/错题",
        ] {
            assert!(resolve_web_target(path).is_none(), "{path}");
        }
    }
}
