"""Auditable source qualifications and structural coverage, never certification."""

from collections import Counter
import csv
import json

from classification import (
    RULES,
    STANDARD_CATEGORIES,
    STANDARD_MODULES,
    normalize_classification,
)


BLOCKING = {
    "missing_stem",
    "missing_options",
    "missing_answer",
    "missing_material",
    "missing_image",
    "answer_unverified",
    "material_link_unverified",
    "source_conflict",
    "evidence_only",
    "content_unverified",
    "question_count_unverified",
}


def read_evidence(source):
    manifest = source / "catalog/manifest.jsonl"
    catalog = (
        {x["path"]: x for x in map(json.loads, manifest.read_text().splitlines())}
        if manifest.exists()
        else {}
    )
    ledger = source / "reports/GAP-CURRENT-CONFIRMED-20261030.csv"
    gaps = {}
    if ledger.exists():
        for row in csv.DictReader(ledger.open()):
            gaps.setdefault(row["paper_path"], []).append(row)
    return catalog, gaps


def qualify(paper, source_record, gaps, source_hash):
    paper_issues = []
    raw = paper.provenance["frontmatter"]
    title = paper.metadata["title"]
    paper.metadata["role"] = "evidence" if raw.get("从属佐证版") is True else "primary"
    if paper.metadata["role"] == "evidence":
        paper_issues.append("evidence_only")
    if "综合知识" in title and "行政职业能力" not in title and "行测" not in title:
        paper_issues.append("source_conflict")
    expected = source_record.get("declared_questions")
    if expected is not None and int(expected) != len(paper.questions):
        paper_issues.append("question_count_unverified")
    if "解析公式尚待核验" in source_record.get("issues", []):
        paper_issues.append("content_unverified")
    if "答案" in str(raw.get("校验状态", "")) and "待核" in str(
        raw.get("校验状态", "")
    ):
        # Keep the uncertainty conservative until a reliable per-question ledger exists.
        paper_issues.append("answer_unverified")
    for q in paper.questions:
        issues = q.metadata["issues"]
        issues.extend(paper_issues)
        for gap in gaps:
            if (
                int(gap["question_number"]) == q.number
                and gap["paper_sha256_at_audit"] == source_hash
            ):
                issue = {
                    "A1": "missing_stem",
                    "A2": "missing_options",
                    "B2": "missing_answer",
                }.get(gap["issue_type"])
                if issue:
                    issues.append(issue)
                    q.provenance.setdefault("confirmed_gaps", []).append(gap)
        if (
            any("选调" in s for s in q.provenance.get("source", []))
            and "选调" not in title
        ):
            issues.append("source_conflict")
        issues[:] = sorted(set(issues))
    paper.metadata["issues"] = sorted(set(paper_issues))


def paper_statistics(paper):
    counts = Counter(
        papers=1,
        question_positions=len(paper.questions),
        materials=len(paper.materials),
    )
    counts[paper.metadata.get("role", "primary") + "_papers"] += 1
    for q in paper.questions:
        issues = set(q.metadata["issues"])
        counts.update(issues)
        if not issues & BLOCKING:
            counts["structurally_answerable"] += 1
        if not issues:
            counts["ready"] += 1
        for key in ("module", "category", "topics", "passage_source"):
            if q.metadata[key]:
                counts[key + "_covered"] += 1
        if q.metadata["materials"]:
            counts["questions_with_materials"] += 1
        counts["material_references"] += len(q.metadata["materials"])
        if not q.explanation:
            counts["missing_explanation"] += 1
    numbers = {q.number for q in paper.questions}
    missing = sorted(set(range(1, max(numbers) + 1)) - numbers)
    return dict(counts), missing


def inventory_text(decisions, source_commit):
    counts = Counter(x["decision"] for x in decisions)
    years = Counter(str(x.get("year")) for x in decisions if x["decision"] == "include")
    return "\n".join(
        [
            "# 来源清点",
            "",
            f"固定来源 commit：`{source_commit}`。默认不设年份上下限。",
            "",
            f"纳入 {counts['include']} 份行测候选；排除 {counts['exclude']} 份其他科目；待核 {counts['review']} 份。",
            "",
            "文件级决定见 inventory.json；候选纳入不等于官方认证或可作答验收。",
            "",
            "| 年份 | 行测候选卷 |",
            "| --- | ---: |",
            *[f"| {year} | {count} |" for year, count in sorted(years.items())],
            "",
        ]
    )


def classification_snapshot(documents):
    modules = Counter()
    categories = Counter()
    module_categories = {}
    positions = 0
    topics_covered = 0
    nonstandard_modules = 0
    misplaced_type_modules = 0
    nonstandard_categories = 0
    conflict_questions = 0
    type_modules = {
        label for label, value in RULES["type_aliases"].items() if value.get("category")
    }
    type_modules.update(
        label
        for label, value in RULES["module_aliases"].items()
        if value.get("category")
    )
    for questions in documents.values():
        for question in questions:
            positions += 1
            module = question.get("module")
            category = question.get("category")
            module_key = module or "<null>"
            category_key = category or "<null>"
            modules[module_key] += 1
            categories[category_key] += 1
            module_categories.setdefault(module_key, Counter())[category_key] += 1
            topics_covered += bool(question.get("topics"))
            if module and module not in STANDARD_MODULES:
                nonstandard_modules += 1
                misplaced_type_modules += module in type_modules
            if category and (
                module not in STANDARD_CATEGORIES
                or category not in STANDARD_CATEGORIES.get(module, ())
            ):
                nonstandard_categories += 1
            normalized = normalize_classification(
                module, category, question.get("topics", [])
            )
            conflict_questions += bool(normalized["conflicts"])
    covered = positions - categories.get("<null>", 0)
    return {
        "papers": len(documents),
        "question_positions": positions,
        "module_counts": dict(sorted(modules.items())),
        "category_counts": dict(sorted(categories.items())),
        "module_category_counts": {
            module: dict(sorted(values.items()))
            for module, values in sorted(module_categories.items())
        },
        "module_covered": positions - modules.get("<null>", 0),
        "category_covered": covered,
        "category_null": positions - covered,
        "category_coverage": covered / positions if positions else 0,
        "topics_covered": topics_covered,
        "topics_empty": positions - topics_covered,
        "nonstandard_modules": nonstandard_modules,
        "misplaced_type_modules": misplaced_type_modules,
        "nonstandard_categories": nonstandard_categories,
        "classification_conflict_questions": conflict_questions,
    }


def classification_report(before_documents, after_documents, records):
    before = classification_snapshot(before_documents)
    after = classification_snapshot(after_documents)
    before_index = {
        (path, q["number"]): q
        for path, questions in before_documents.items()
        for q in questions
    }
    after_index = {
        (path, q["number"]): q
        for path, questions in after_documents.items()
        for q in questions
    }
    module_changes = 0
    module_resolved = 0
    module_cleared = 0
    category_changes = 0
    for key in before_index.keys() & after_index.keys():
        old_module = before_index[key].get("module")
        new_module = after_index[key].get("module")
        module_changes += old_module != new_module
        module_resolved += (
            old_module not in STANDARD_MODULES and new_module in STANDARD_MODULES
        )
        module_cleared += bool(old_module and old_module not in STANDARD_MODULES) and (
            new_module is None
        )
        category_changes += before_index[key].get("category") != after_index[key].get(
            "category"
        )
    record_index = {(row["path"], row["number"]): row for row in records}
    relocations = []
    topic_inferences = []
    proposed_conflicts = []
    conflict_reasons = Counter()
    cleared_unknown_modules = []
    raw_modules = Counter()
    raw_categories = Counter()
    for row in records:
        if row.get("raw_module"):
            raw_modules[row["raw_module"]] += 1
        else:
            raw_modules["<null>"] += 1
        if row.get("raw_category"):
            raw_categories[row["raw_category"]] += 1
        else:
            raw_categories["<null>"] += 1
        if row.get("module") in STANDARD_MODULES:
            key = (row["path"], row["number"])
            previous = before_index.get(key)
            current = after_index.get(key)
            if previous is not None and current is not None:
                changed = previous.get("module") != current.get("module")
            else:
                changed = bool(
                    (row.get("raw_module") and row["raw_module"] != row["module"])
                    or row.get("module_basis") == "explicit_category"
                )
            if changed:
                relocations.append(row)
        if row.get("category_basis") == "topics":
            topic_inferences.append(row)
        if row.get("conflicts"):
            proposed_conflicts.append(row)
            conflict_reasons.update(row["conflicts"])
        if row.get("raw_module") and row.get("module") is None:
            cleared_unknown_modules.append(row)
    unclassified = []
    for (path, number), question in sorted(after_index.items()):
        if question.get("category") is not None:
            continue
        source = record_index.get((path, number), {})
        unclassified.append(
            {
                "path": path,
                "number": number,
                "module": question.get("module"),
                "category": None,
                "topics": question.get("topics", []),
                "raw_module": source.get("raw_module"),
                "raw_category": source.get("raw_category"),
                "reason": source.get("conflicts") or ["no_unique_category_rule"],
            }
        )
    summary = {
        "before": before,
        "after": after,
        "module_change_count": module_changes,
        "module_resolved_count": module_resolved,
        "unknown_module_cleared_count": module_cleared,
        "category_change_count": category_changes,
        "module_relocation_count": len(relocations),
        "topics_inferred_count": len(topic_inferences),
        "remaining_category_null_count": len(unclassified),
        "conflict_count": len(proposed_conflicts),
        "conflict_reason_counts": dict(sorted(conflict_reasons.items())),
        "report": "reports/classification.json",
    }
    details = {
        "schema": "hyz-question-bank-classification-audit/v1",
        "summary": summary,
        "raw_module_counts": dict(sorted(raw_modules.items())),
        "raw_category_counts": dict(sorted(raw_categories.items())),
        "module_relocations": relocations,
        "unknown_module_cleared": cleared_unknown_modules,
        "topics_inferred": topic_inferences,
        "unclassified": unclassified,
        "conflicts": proposed_conflicts,
    }
    return summary, details


def summary_text(report):
    counts = report.get("source_content_counts", {})
    total = counts.get("question_positions", 0)
    lines = [
        "# 导入质量报告",
        "",
        f"来源 commit：`{report.get('source_commit')}`；转换器指纹：`{report.get('importer_sha256')}`。",
        "",
        f"处理 {report['converted_papers']} 卷；解析失败 {len(report['parse_failures'])} 卷；冲突 {len(report['conflicts'])} 卷；图片异常 {len(report['image_errors'])} 处。",
        "",
        "**统计口径**：题位包括缺题占位；问题标签可重叠。structurally_answerable 仅通过结构与已知作答缺损检查，仍可能题序待核；ready 额外要求无已知待核标签。两者均不代表官方认证。",
        "",
        "| 项目 | 数量 |",
        "| --- | ---: |",
        *[f"| {key} | {value} |" for key, value in sorted(counts.items())],
        "",
        "## 标签覆盖",
        "",
        "| 字段 | 覆盖题位 | 覆盖率 |",
        "| --- | ---: | ---: |",
    ]
    for key in ("module", "category", "topics", "passage_source"):
        count = counts.get(key + "_covered", 0)
        lines.append(
            f"| {key} | {count} | {count / total:.2%} |"
            if total
            else f"| {key} | 0 | — |"
        )
    classification = report.get("classification", {})
    before = classification.get("before", {})
    after = classification.get("after", {})
    lines += [
        "",
        "## 模块与题型分类",
        "",
        f"category 覆盖：{before.get('category_covered', 0)} → {after.get('category_covered', 0)} / {after.get('question_positions', 0)}（{before.get('category_coverage', 0):.2%} → {after.get('category_coverage', 0):.2%}）。",
        f"module 变化 {classification.get('module_change_count', 0)} 题；category 变化 {classification.get('category_change_count', 0)} 题；题型 module 归位 {classification.get('module_relocation_count', 0)} 题；topics 推导 {classification.get('topics_inferred_count', 0)} 题。",
        f"剩余 category: null {classification.get('remaining_category_null_count', 0)} 题；分类冲突 {classification.get('conflict_count', 0)} 题。完整未分类与冲突清单见 `reports/classification.json`。",
        "",
        "不使用 AI 补题或标注。原始模块范围和显式类别映射的依据保存在 provenance.jsonl；未知标签保持 null/空列表。",
        "",
        "逐题异常见 issues.jsonl；来源声明、缺失题号、原始答案标签、原始考点与来源见 provenance.jsonl。历史来源记录随版本保留在 reports/sources/。",
        "",
        "UUID 当前仅校验/保留，不生成；首次设备同步尚未实施。未修改设备、Samba 或 Flashcards。",
        "",
    ]
    return "\n".join(lines)
