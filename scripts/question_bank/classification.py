"""Deterministic normalization of source module/type labels and topic families."""

from collections import defaultdict
from pathlib import Path

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


def normalize_classification(module, category, topics, *, raw_category=None):
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
