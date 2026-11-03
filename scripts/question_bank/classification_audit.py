#!/usr/bin/env python3
"""Read-only corpus-wide classification audit; writes require explicit CLI switches."""

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import re
import sys
import uuid

import yaml

from classification import (
    RULE_VERSION,
    STANDARD_CATEGORIES,
    STANDARD_MODULES,
    normalize_classification,
)
from identity import MARKER, without_ids
from importer import classification_mask, digest, json_text, write_if_changed
from verify import validate_document


UNIT = re.compile(r"(?m)^## (?:Question (\d+)|Material (M\d+))\s*$")
META = re.compile(r"```yaml\n(.*?)\n```", re.S)
SECTION = re.compile(r"(?m)^### (Stem|Options|Answer|Explanation|Notes)\s*$")


def valid_id(value):
    if not isinstance(value, str):
        return False
    try:
        parsed = uuid.UUID(value)
    except ValueError:
        return False
    return parsed.version == 7 and str(parsed) == value


def marker(chunk, kind):
    found = MARKER.findall(chunk)
    matching = [value for tag, value in found if tag == kind]
    malformed = chunk.count(f"<!-- hyz-{kind}-id:") != len(matching)
    if len(matching) != 1 or not valid_id(matching[0]):
        return (
            None,
            "invalid_" + kind + "_uuid"
            if matching or malformed
            else "missing_" + kind + "_uuid",
        )
    return matching[0], None


def sections(chunk):
    heads = list(SECTION.finditer(chunk))
    return {
        head[1]: chunk[
            head.end() : heads[i + 1].start() if i + 1 < len(heads) else len(chunk)
        ].strip()
        for i, head in enumerate(heads)
    }


def question_hash(fields, material_contents):
    content = {
        "stem": fields.get("Stem", ""),
        "options": fields.get("Options", ""),
        "answer": fields.get("Answer", ""),
        "explanation": fields.get("Explanation", ""),
        "notes": fields.get("Notes", ""),
        "materials": material_contents,
    }
    return digest(
        json.dumps(content, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    )


def paper_questions(text, path, source_hash):
    units = list(UNIT.finditer(text))
    paper_id, paper_error = marker(text[: units[0].start()] if units else text, "paper")
    materials = {}
    for i, unit in enumerate(units):
        if unit[2]:
            raw = text[
                unit.end() : units[i + 1].start() if i + 1 < len(units) else len(text)
            ]
            materials[unit[2]] = without_ids(raw).strip()
    for i, unit in enumerate(units):
        if not unit[1]:
            continue
        raw = text[
            unit.end() : units[i + 1].start() if i + 1 < len(units) else len(text)
        ]
        meta = META.search(raw)
        if meta is None:
            raise ValueError(f"missing YAML metadata: {path}, question {unit[1]}")
        data = yaml.safe_load(meta[1])
        if not isinstance(data, dict):
            raise ValueError(f"invalid YAML metadata: {path}, question {unit[1]}")
        fields = sections(raw[meta.end() :])
        if not all(k in fields for k in ("Stem", "Options", "Answer")):
            raise ValueError(f"missing section: {path}, question {unit[1]}")
        question_id, question_error = marker(raw[: meta.start()], "question")
        refs = data.get("materials", [])
        if not isinstance(refs, list) or any(ref not in materials for ref in refs):
            raise ValueError(f"invalid materials: {path}, question {unit[1]}")
        yield {
            "question_uuid": question_id,
            "paper_uuid": paper_id,
            "question_number": int(unit[1]),
            "module": data.get("module"),
            "current_category": data.get("category"),
            "topics": data.get("topics", []),
            "question_hash": question_hash(
                fields, [(ref, materials[ref]) for ref in refs]
            ),
            "content_signature": question_hash(
                {"Stem": fields["Stem"], "Options": fields["Options"]},
                [(ref, materials[ref]) for ref in refs],
            ),
            "source_hash": source_hash,
            "paper_path": path,
            "_stem": fields["Stem"],
            "_options": fields["Options"],
            "_has_material": bool(refs),
            "_errors": [error for error in (paper_error, question_error) if error],
        }


def provenance_index(root):
    path = root / "reports/provenance.jsonl"
    if not path.is_file():
        return {}
    result = {}
    with path.open() as stream:
        for line in stream:
            row = json.loads(line)
            result[(row["path"], row["source_sha256"])] = {
                q["number"]: q for q in row.get("questions", [])
            }
    return result


def audit_bank(root):
    """Evaluate every present question; do not create reports or edit Markdown."""
    root = Path(root).resolve()
    manifest_path = root / "manifest.json"
    manifest = (
        json.loads(manifest_path.read_text())
        if manifest_path.is_file()
        else {"papers": {}}
    )
    source_by_path = {
        row["target"]: (source_path, row["source_sha256"])
        for source_path, row in manifest["papers"].items()
    }
    provenance = provenance_index(root)
    override_path = root / "classification_overrides.yaml"
    if override_path.exists():
        from classification_overrides import load_overrides

        overrides = load_overrides(override_path)
    else:
        overrides = {}
    rows = []
    paper_count = 0
    for paper in sorted((root / "papers").rglob("*.md")):
        if paper.is_symlink() or not paper.resolve().is_relative_to(root):
            raise ValueError(f"unsafe paper path: {paper}")
        text = paper.read_text()
        path = paper.relative_to(root).as_posix()
        source_path, source_hash = source_by_path.get(path, (None, None))
        if source_hash is None:
            source_hash = digest(classification_mask(text))
        raw_labels = provenance.get((source_path, source_hash), {})
        paper_count += 1
        for row in paper_questions(text, path, source_hash):
            raw = raw_labels.get(row["question_number"], {})
            decision = normalize_classification(
                raw.get("raw_module") or row["module"],
                None,
                row["topics"],
                raw_category=raw.get("raw_category"),
                stem=row.pop("_stem"),
                options=row.pop("_options"),
                has_material=row.pop("_has_material"),
            )
            errors = row.pop("_errors")
            current = row["current_category"]
            proposed = decision["category"]
            module = row["module"]
            if module is not None and module not in STANDARD_MODULES:
                errors.append("invalid_module")
            if current is not None and current not in STANDARD_CATEGORIES.get(
                module, ()
            ):
                errors.append("invalid_module_category")
            if (
                current != proposed
                and current is not None
                and "strong_evidence_conflict" not in decision["conflicts"]
            ):
                errors.append(
                    "category_content_mismatch"
                    if decision["category_basis"] == "structure"
                    else "unverified_category"
                )
            errors.extend(decision["conflicts"])
            row.update(
                suggested_module=decision["module"],
                suggested_category=proposed,
                classification_basis=decision["category_basis"],
                classification_rule_version=RULE_VERSION,
                evidence=decision["category_evidence"],
                conflict_reasons=sorted(set(errors)),
            )
            approved = overrides.get(row["question_uuid"])
            if approved:
                if any(
                    approved.get(key) != row.get(key)
                    for key in (
                        "paper_uuid",
                        "question_hash",
                        "source_hash",
                        "classification_rule_version",
                    )
                ):
                    row["conflict_reasons"].append("stale_override")
                    row["conflict_reasons"] = sorted(set(row["conflict_reasons"]))
                else:
                    row["suggested_module"] = approved["module"]
                    row["suggested_category"] = approved["category"]
                    row["classification_basis"] = "reviewed_override"
                    row["evidence"] = ["approved_uuid_override"]
            rows.append(row)
    ids = defaultdict(list)
    paper_ids = defaultdict(set)
    content_groups = defaultdict(list)
    for row in rows:
        year = re.search(r"/(20\d{2})/", row["paper_path"])
        if year:
            content_groups[(year[1], row["content_signature"])].append(row)
        if row["question_uuid"]:
            ids[row["question_uuid"]].append(row)
        if row["paper_uuid"]:
            paper_ids[row["paper_uuid"]].add(row["paper_path"])
    duplicate_count = 0
    for group in ids.values():
        if len(group) > 1:
            duplicate_count += len(group)
            for row in group:
                row["conflict_reasons"].append("duplicate_question_uuid")
    for group in content_groups.values():
        if len({row["paper_path"] for row in group}) < 2:
            continue
        if len({row["module"] for row in group}) > 1:
            reason = "cross_paper_module_disagreement"
        elif len({row["current_category"] for row in group}) > 1:
            reason = "cross_paper_category_disagreement"
        else:
            continue
        for row in group:
            row["conflict_reasons"].append(reason)
    for row in rows:
        if len(paper_ids[row["paper_uuid"]]) > 1:
            row["conflict_reasons"].append("duplicate_paper_uuid")
        row["conflict_reasons"] = sorted(set(row["conflict_reasons"]))
        stale = "stale_override" in row["conflict_reasons"]
        invalid_id = any("uuid" in reason for reason in row["conflict_reasons"])
        if not stale and not invalid_id and overrides.get(row["question_uuid"]):
            row["status"] = (
                "reviewed"
                if (row["module"], row["current_category"])
                == (row["suggested_module"], row["suggested_category"])
                else "conflict"
            )
        elif (set(row["conflict_reasons"]) - {"topic_category_mismatch"}) or (
            row["module"],
            row["current_category"],
        ) != (row["suggested_module"], row["suggested_category"]):
            row["status"] = "conflict"
        else:
            row["status"] = (
                "verified" if row["current_category"] is not None else "unclassified"
            )
        row["needs_ai_review"] = (
            row["status"] == "unclassified"
            or stale
            or invalid_id
            or (row["status"] == "conflict" and row["suggested_category"] is None)
            or "strong_evidence_conflict" in row["conflict_reasons"]
            or "material_structure_topic_disagreement" in row["conflict_reasons"]
            or "cross_paper_module_disagreement" in row["conflict_reasons"]
            or (
                "cross_paper_category_disagreement" in row["conflict_reasons"]
                and row["classification_basis"]
                not in ("structure", "reviewed_override")
            )
        )
    categories = Counter((r["current_category"] or "<null>") for r in rows)
    modules = Counter((r["module"] or "<null>") for r in rows)
    suggested_categories = Counter((r["suggested_category"] or "<null>") for r in rows)
    statuses = Counter(r["status"] for r in rows)
    blocked = {
        "duplicate_question_uuid",
        "duplicate_paper_uuid",
        "invalid_question_uuid",
        "invalid_paper_uuid",
        "missing_question_uuid",
        "missing_paper_uuid",
        "stale_override",
        "cross_paper_module_disagreement",
        "material_structure_topic_disagreement",
    }

    def auto_safe(row):
        reasons = set(row["conflict_reasons"])
        if reasons & blocked:
            return False
        if "cross_paper_category_disagreement" in reasons and row[
            "classification_basis"
        ] not in ("structure", "reviewed_override"):
            return False
        return True

    correct_existing = sum(
        row["current_category"] is not None
        and row["suggested_category"] is not None
        and row["current_category"] != row["suggested_category"]
        and auto_safe(row)
        for row in rows
    )
    new_categories = sum(
        row["current_category"] is None
        and row["suggested_category"] is not None
        and auto_safe(row)
        for row in rows
    )
    clear_categories = sum(
        row["current_category"] is not None
        and row["suggested_category"] is None
        and auto_safe(row)
        for row in rows
    )
    duplicate_paper_count = sum(len(paths) > 1 for paths in paper_ids.values())
    proposed_covered = len(rows) - suggested_categories["<null>"]
    summary = {
        "schema": "hyz-classification-audit/v2",
        "classification_rule_version": RULE_VERSION,
        "papers": paper_count,
        "questions": len(rows),
        "module_counts": dict(sorted(modules.items())),
        "category_counts": dict(sorted(categories.items())),
        "suggested_category_counts": dict(sorted(suggested_categories.items())),
        "status_counts": dict(sorted(statuses.items())),
        "category_covered": len(rows) - categories["<null>"],
        "category_coverage": (len(rows) - categories["<null>"]) / len(rows)
        if rows
        else 0,
        "proposed_category_covered": proposed_covered,
        "proposed_category_coverage": proposed_covered / len(rows) if rows else 0,
        "existing_classifications_to_correct": correct_existing,
        "new_automatic_classifications": new_categories,
        "classifications_to_clear_for_safety": clear_categories,
        "correctable": correct_existing + new_categories + clear_categories,
        "unclassified": statuses["unclassified"],
        "conflicts": statuses["conflict"],
        "needs_ai_review": sum(r["needs_ai_review"] for r in rows),
        "missing_question_uuids": sum(r["question_uuid"] is None for r in rows),
        "duplicate_question_uuids": duplicate_count,
        "duplicate_paper_uuids": duplicate_paper_count,
        "missing_paper_uuids": sum(r["paper_uuid"] is None for r in rows),
        "conflict_reason_counts": dict(
            sorted(Counter(v for r in rows for v in r["conflict_reasons"]).items())
        ),
    }
    return {"summary": summary, "rows": rows}


def write_reports(root, audit):
    """Opt-in report generation. Reports contain identity metadata, never answers."""
    root = Path(root)
    rows = audit["rows"]

    def lines(items):
        return "".join(
            json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
            + "\n"
            for row in items
        )

    write_if_changed(root / "reports/classification_review.jsonl", lines(rows))
    write_if_changed(
        root / "reports/classification_queue.jsonl",
        lines(r for r in rows if r["needs_ai_review"]),
    )
    write_if_changed(
        root / "reports/classification_summary.json", json_text(audit["summary"])
    )


def _patch_metadata(text, edits):
    pieces = []
    cursor = 0
    units = list(UNIT.finditer(text))
    for i, unit in enumerate(units):
        if not unit[1] or int(unit[1]) not in edits:
            continue
        end = units[i + 1].start() if i + 1 < len(units) else len(text)
        meta = META.search(text, unit.end(), end)
        if meta is None:
            raise ValueError("metadata disappeared during edit")
        replacement = meta[0]
        module, category = edits[int(unit[1])]
        for key, value in (("module", module), ("category", category)):
            replacement, count = re.subn(
                rf"(?m)^{key}:.*$",
                f"{key}: {value if value is not None else 'null'}",
                replacement,
            )
            if count != 1:
                raise ValueError(f"metadata {key} is missing or duplicated")
        pieces.extend((text[cursor : meta.start()], replacement))
        cursor = meta.end()
    pieces.append(text[cursor:])
    return "".join(pieces)


def apply_changes(root, audit):
    """Apply only validated metadata changes; reject identity anomalies or manual edits."""
    root = Path(root).resolve()
    rows = audit["rows"]
    if any("uuid" in reason for row in rows for reason in row["conflict_reasons"]):
        raise ValueError(
            "UUID anomaly: repair identities before applying classification"
        )
    if any("stale_override" in row["conflict_reasons"] for row in rows):
        raise ValueError(
            "stale override: re-review content before applying classification"
        )
    manifest_path = root / "manifest.json"
    if not manifest_path.exists():
        raise ValueError("manifest required to protect importer three-way comparison")
    manifest = json.loads(manifest_path.read_text())
    targets = {item["target"]: item for item in manifest["papers"].values()}
    per_file = defaultdict(dict)
    for row in rows:
        if row["status"] == "verified" or row["status"] == "reviewed":
            continue
        if "cross_paper_module_disagreement" in row["conflict_reasons"]:
            continue
        if "cross_paper_category_disagreement" in row["conflict_reasons"] and row[
            "classification_basis"
        ] not in ("structure", "reviewed_override"):
            continue
        if "material_structure_topic_disagreement" in row["conflict_reasons"]:
            continue
        if "strong_evidence_conflict" in row["conflict_reasons"]:
            category = None
        else:
            category = row["suggested_category"]
        module = (
            row["suggested_module"]
            if row["suggested_module"] in STANDARD_MODULES
            else row["module"]
        )
        if (row["module"], row["current_category"]) == (module, category):
            continue
        per_file[row["paper_path"]][row["question_number"]] = (module, category)
    pending = []
    for relative, edits in sorted(per_file.items()):
        path = root / relative
        if path.is_symlink() or not path.resolve().is_relative_to(root):
            raise ValueError("unsafe paper path")
        old = path.read_text()
        paper_rows = [row for row in rows if row["paper_path"] == relative]
        ids = {
            r["question_number"]: (
                r["question_uuid"],
                r["question_hash"],
                r["module"],
                r["current_category"],
            )
            for r in paper_questions(old, relative, paper_rows[0]["source_hash"])
        }
        if ids != {
            r["question_number"]: (
                r["question_uuid"],
                r["question_hash"],
                r["module"],
                r["current_category"],
            )
            for r in paper_rows
        }:
            raise ValueError("paper content changed during audit")
        entry = targets.get(relative)
        if entry is None:
            raise ValueError("untracked paper cannot update importer state")
        old_hash = digest(without_ids(old))
        if entry.get("managed_sha256", entry.get("generated_sha256")) != old_hash:
            raise ValueError(
                "manual change or stale importer baseline; refusing metadata overwrite"
            )
        new = _patch_metadata(old, edits)
        if classification_mask(old) != classification_mask(new):
            raise ValueError("classification change touched non-classification content")
        validate_document(new)
        entry["managed_sha256"] = digest(without_ids(new))
        pending.append((path, old, new))
    for path, old, new in pending:
        if path.read_text() != old:
            raise ValueError("paper changed during classification update")
    for path, old, new in pending:
        write_if_changed(root / f"reports/backups/classification/{digest(old)}.md", old)
        write_if_changed(path, new)
    if pending:
        write_if_changed(
            root
            / f"reports/backups/classification/manifest-{digest(manifest_path.read_bytes())}.json",
            manifest_path.read_bytes(),
        )
        write_if_changed(manifest_path, json_text(manifest))
    return sum(len(edits) for edits in per_file.values())


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bank", type=Path, default=Path("question_bank"))
    parser.add_argument(
        "--write-reports", action="store_true", help="write JSONL audit and AI queue"
    )
    parser.add_argument(
        "--apply", action="store_true", help="apply classification-only corrections"
    )
    args = parser.parse_args(argv)
    if args.apply and not args.write_reports:
        parser.error("--apply requires --write-reports for a re-audit")
    try:
        audit = audit_bank(args.bank)
        changed = None
        if args.apply:
            changed = apply_changes(args.bank, audit)
            audit = audit_bank(args.bank)
        if args.write_reports:
            write_reports(args.bank, audit)
        summary = dict(audit["summary"])
        if changed is not None:
            summary["applied_changes"] = changed
        print(json_text(summary), end="")
        return 0
    except (OSError, ValueError, yaml.YAMLError) as error:
        print(f"classification-audit: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
