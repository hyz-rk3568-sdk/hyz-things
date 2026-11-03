"""Deterministic normalization of source module/type labels and topic families."""

from collections import defaultdict
from pathlib import Path
import re

import yaml


RULES = yaml.safe_load(Path(__file__).with_name("classification.yaml").read_text())
STANDARD_MODULES = tuple(RULES["standard_modules"])
STANDARD_CATEGORIES = RULES["standard_categories"]


def _present(value):
    return isinstance(value, str) and bool(value.strip())


def _category_targets():
    targets = defaultdict(set)
    for module, categories in STANDARD_CATEGORIES.items():
        for category in categories:
            targets[category].add((module, category))
        for alias, canonical in RULES["category_aliases"].get(module, {}).items():
            targets[alias].add((module, canonical))
    return targets


CATEGORY_TARGETS = _category_targets()
RULE_VERSION = "hyz-classification/v2"


def structure_evidence(module, stem, options, *, has_material=False):
    """Recognize question *formats*; never treat a free-standing topic as a type."""
    if not stem or not options or not module:
        return {}
    if re.search(r"!\[|<img\b", stem + options, re.I):
        if (
            module == "判断推理"
            and "问号" in stem
            and "四个选项" in stem
            and re.search(r"!\[|<img\b", stem, re.I)
            and len(re.findall(r"(?m)^- [A-D]\.\s*(?:!\[|<img\b)", options)) == 4
        ):
            return {"图形推理": "graphic_question_image_pattern"}
        return {}
    choices = re.findall(
        r"(?m)^- [A-H]\.\s*(.*?)(?=\n\s*- [A-H]\.\s*|\Z)", options, re.S
    )
    if len(choices) < 4:
        return {}
    result = {}
    if module == "判断推理":
        prompt = re.split(
            r"根据(?:上述|以上|该|这些)?定义|按照(?:上述|以上)?定义", stem
        )[0]
        if (
            len(prompt) >= 20
            and re.search(r"[^。？！\n]{2,45}(?:是|指|称为)[^。？！\n]{8,}", prompt)
            and re.search(
                r"(?:根据|按照)(?:上述|以上|该|这些)?定义[^\n]{0,100}(?:属于|符合|不属于|不符合|有几项|正确|错误|是)",
                stem,
            )
        ):
            result["定义判断"] = "definition_question_pattern"
        if re.search(
            r"\S+\s*对于\s*[（(]\s*[）)]\s*相当于\s*[（(]\s*[）)]\s*对于\s*\S+",
            stem,
        ) and all("；" in choice or ";" in choice for choice in choices):
            result["类比推理"] = "analogy_fill_relation_pattern"
        if (
            len(stem) < 70
            and "\n" not in stem
            and not re.search(r"根据.{0,8}定义|以下|下列|上述|对话|论证", stem)
            and (re.search(r"[：:∶]", stem) or "相当于" in stem)
            and all(re.search(r"[：:∶]|相当于", c) for c in choices)
            and not any(len(c) > 95 for c in choices)
        ):
            result["类比推理"] = "analogy_relation_pattern"
        if (
            len(stem) >= 45
            and "定义判断" not in result
            and re.search(r"如果.{0,10}为真", stem)
            and re.search(r"(?:最能|能够|可以|支持|削弱|加强|推断|推出)", stem[-100:])
        ):
            result.setdefault("逻辑判断", "if_true_argument_pattern")
        if (
            len(stem) >= 45
            and "定义判断" not in result
            and re.search(
                r"(?:最能|最有助于|最不能)(?:削弱|加强|支持|质疑|反驳|解释|论证)|(?:论证|结论|观点).{0,25}(?:前提|假设)|(?:以下|下列)哪项.{0,20}(?:推断|推出)",
                stem[-120:],
            )
        ):
            result["逻辑判断"] = "argument_question_pattern"
    elif module == "言语理解与表达":
        if len(re.findall(r"_{2,}|[＿]{2,}|【\s*】", stem)) >= 2 and all(
            len(c) < 55 for c in choices
        ):
            result["逻辑填空"] = "multi_blank_word_choice_pattern"
        if re.search(
            r"(?:将|把)?(?:以上|下列|上述|下面).{0,12}句(?:子|话).{0,20}(?:重新排列|排序)|语句排序",
            stem,
        ):
            result["语句表达"] = "sentence_order_pattern"
        if (
            len(stem) >= 90
            and not re.search(r"_{2,}|[＿]{2,}|【\s*】", stem)
            and re.search(
                r"(?:这段|本文|文段|这篇).{0,12}(?:文字|话|文章).{0,20}(?:意在|旨在|主要|强调|说明|表明)",
                stem,
            )
        ):
            result["片段阅读"] = "passage_main_idea_pattern"
    elif module == "数量关系":
        compact = re.sub(r"\s+", "", stem)
        if (
            len(compact) < 90
            and re.fullmatch(r"[-\d.,，、\s]+[（(][？?\s]*[）)]", stem)
            and all(re.fullmatch(r"[\s\d.,%％/负-]+", c.strip()) for c in choices)
        ):
            result["数字推理"] = "number_sequence_pattern"
    elif module == "资料分析" and has_material:
        # These match the *question asked*, never incidental words in the material.
        if re.search(
            r"(?:以下|下列).{0,12}(?:说法|表述).{0,15}(?:正确|错误|不正确|不符合)", stem
        ):
            result["综合分析"] = "material_statement_pattern"
        elif re.search(r"(?:增长率|增长量|增速|同比增长|环比增长|较上年增长)", stem):
            result["增长"] = "material_growth_question_pattern"
        elif re.search(r"(?:所占比重|占.{1,25}(?:比重|比例)|比重为|比重约)", stem):
            result["比重"] = "material_share_question_pattern"
        elif re.search(r"(?:平均|人均|每人|每家|每单位)", stem):
            result["平均数"] = "material_average_question_pattern"
        elif re.search(
            r"(?:是.{1,25}的?几倍|是.{1,25}的?多少倍|倍数约|为.{1,25}的?几倍)", stem
        ):
            result["倍数"] = "material_multiple_question_pattern"
    return result


def material_boundary_module(module, category):
    """Keep the pre-normalization module used to attach shared material."""
    if _present(module):
        return module.strip()
    if _present(category):
        return RULES["material_boundary_category_modules"].get(category.strip())
    return None


def _module_target(label):
    if label in STANDARD_MODULES:
        return label, None
    alias = RULES["module_aliases"].get(label)
    if alias:
        return alias["module"], alias.get("category")
    alias = RULES["type_aliases"].get(label)
    if alias:
        return alias["module"], alias.get("category")
    return None, None


def _category_target(label):
    targets = CATEGORY_TARGETS.get(label, set())
    if len(targets) == 1:
        return next(iter(targets)), None
    if len(targets) > 1:
        return None, "ambiguous_category_alias"
    return None, None


def _topic_matches(module, topics):
    categories = set()
    evidence = defaultdict(list)
    for topic in topics:
        if not isinstance(topic, str):
            continue
        for target_module, by_category in RULES["topic_rules"].items():
            if topic in RULES.get("weak_topics", {}).get(target_module, ()):
                continue
            for category, matchers in by_category.items():
                matched = topic in matchers.get("exact", []) or any(
                    topic.startswith(prefix) for prefix in matchers.get("prefixes", [])
                )
                if matched and target_module == module:
                    categories.add(category)
                    evidence[category].append(topic)
    return categories, evidence


def _explicit_topic_target(module, label):
    """Resolve a source type label through the same exact topic dictionary."""
    modules = [module] if module else list(RULES["topic_rules"])
    targets = set()
    evidence = defaultdict(list)
    for target_module in modules:
        categories, matches = _topic_matches(target_module, [label])
        targets.update((target_module, category) for category in categories)
        for category, values in matches.items():
            evidence[(target_module, category)].extend(values)
    if len(targets) == 1:
        target = next(iter(targets))
        return target, evidence[target], None
    if len(targets) > 1:
        return None, [], "ambiguous_explicit_category"
    return None, [], None


def normalize_classification(
    module,
    category,
    topics,
    *,
    raw_category=None,
    stem="",
    options="",
    has_material=False,
):
    """Return normalized labels and an audit trail without changing topics.

    Unknown modules are never inferred from topics. An explicit question type
    can establish its module when the source section has no recognized module.
    Conflicting explicit module/type labels leave category unset for review.
    """
    module_label = module.strip() if _present(module) else None
    category_label = (
        raw_category.strip()
        if _present(raw_category)
        else category.strip()
        if _present(category)
        else None
    )
    topics = list(topics or [])
    conflicts = []
    module_target, implied_category = _module_target(module_label)
    explicit_target = None
    explicit_conflict = False
    normalized_category = None
    category_evidence = []
    module_from_category_label = False

    if category_label in RULES.get("weak_topics", {}).get(module_target, ()):
        category_label = None
    if category_label:
        explicit_target, alias_conflict = _category_target(category_label)
        if alias_conflict:
            conflicts.append(alias_conflict)
            explicit_conflict = True
        elif not explicit_target:
            category_module, category_type = _module_target(category_label)
            if category_module:
                if category_module != module_target and module_target:
                    conflicts.append("explicit_category_module_mismatch")
                    explicit_conflict = True
                elif module_target is None:
                    module_target = category_module
                    module_from_category_label = True
                if category_type:
                    explicit_target = (category_module, category_type)
            else:
                explicit_target, category_evidence, explicit_type_conflict = (
                    _explicit_topic_target(module_target, category_label)
                )
                if explicit_type_conflict:
                    conflicts.append(explicit_type_conflict)
                    explicit_conflict = True
                elif not explicit_target:
                    conflicts.append("unknown_explicit_category")
                    explicit_conflict = True

    category_basis = "missing"
    if module_target is None and explicit_target:
        module_target = explicit_target[0]
        module_basis = "explicit_category"
    elif module_target and module_from_category_label:
        module_basis = "explicit_category_module"
    elif module_target:
        module_basis = (
            "standard_module" if module_label in STANDARD_MODULES else "module_alias"
        )
    else:
        module_basis = "missing"

    if module_label and module_target is None:
        conflicts.append("unknown_module")
    if explicit_target:
        explicit_module, explicit_category = explicit_target
        if module_target and explicit_module != module_target:
            conflicts.append("explicit_category_module_mismatch")
            explicit_conflict = True
            category_basis = "conflict"
        else:
            module_target = explicit_module
            category_basis = "explicit_category"
            normalized_category = explicit_category
            if not category_evidence:
                category_evidence = [category_label]
    elif category_label and "unknown_explicit_category" in conflicts:
        category_basis = "unverified_explicit_category"
    elif category_label and explicit_conflict:
        category_basis = "conflict"
    else:
        if not explicit_conflict and implied_category and module_target:
            if implied_category in STANDARD_CATEGORIES.get(module_target, []):
                normalized_category = implied_category
                category_basis = "module_type_alias"
                category_evidence = [module_label]
            else:
                conflicts.append("invalid_module_type_alias")
        elif not explicit_conflict and module_target:
            candidates, topic_evidence = _topic_matches(module_target, topics)
            if len(candidates) == 1:
                normalized_category = next(iter(candidates))
                category_basis = "topics"
                category_evidence = topic_evidence[normalized_category]
            elif len(candidates) > 1:
                conflicts.append("ambiguous_topic_categories")
            else:
                other_categories = set()
                for other_module in RULES["topic_rules"]:
                    if other_module == module_target:
                        continue
                    other, _ = _topic_matches(other_module, topics)
                    other_categories.update((other_module, item) for item in other)
                if other_categories:
                    conflicts.append("topic_module_mismatch")

    structural = structure_evidence(
        module_target, stem, options, has_material=has_material
    )
    if len(structural) > 1:
        conflicts.append("strong_evidence_conflict")
        normalized_category = None
        category_basis = "conflict"
    elif structural:
        structural_category, structural_rule = next(iter(structural.items()))
        if (
            module_target == "资料分析"
            and normalized_category
            and normalized_category != structural_category
        ):
            conflicts.append("material_structure_topic_disagreement")
        elif (explicit_target and explicit_target[1] != structural_category) or (
            implied_category and implied_category != structural_category
        ):
            conflicts.append("strong_evidence_conflict")
            normalized_category = None
            category_basis = "conflict"
        elif not explicit_conflict:
            if normalized_category and normalized_category != structural_category:
                conflicts.append("topic_category_mismatch")
            for topic in topics:
                if topic in RULES.get("weak_topics", {}).get(module_target, ()):
                    for guessed_category, matchers in RULES["topic_rules"][
                        module_target
                    ].items():
                        if (
                            topic in matchers.get("exact", ())
                            and guessed_category != structural_category
                        ):
                            conflicts.append("topic_category_mismatch")
            normalized_category = structural_category
            category_basis = "structure"
            category_evidence = [structural_rule]
    if normalized_category and normalized_category not in STANDARD_CATEGORIES.get(
        module_target, []
    ):
        conflicts.append("category_not_standard_for_module")
        normalized_category = None
        category_basis = "conflict"

    return {
        "module": module_target,
        "category": normalized_category,
        "module_basis": module_basis,
        "category_basis": category_basis,
        "category_evidence": category_evidence,
        "conflicts": sorted(set(conflicts)),
    }
