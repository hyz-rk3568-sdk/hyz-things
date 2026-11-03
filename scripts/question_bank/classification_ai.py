#!/usr/bin/env python3
"""Offline UUID lookup and two-stage AI classification review interface."""

import argparse
import json
from pathlib import Path
import re
import sys

import yaml

from classification_audit import META, UNIT, audit_bank, paper_questions
from classification_overrides import approve_proposals, validate_proposals
from importer import classification_mask, digest, write_if_changed


PROPOSALS = "reports/classification_proposals.jsonl"
UNRESOLVED = "reports/classification_unresolved.jsonl"


def _jsonl(rows):
    return "".join(
        json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows
    )


def _read_jsonl(path):
    return [
        json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()
    ]


def lookup_question(root, question_uuid):
    """Find the unique marker in the local bank; return only the pre-submit prompt."""
    root = Path(root).resolve()
    target = f"<!-- hyz-question-id: {question_uuid} -->"
    found = []
    for path in sorted((root / "papers").rglob("*.md")):
        if path.is_symlink() or not path.resolve().is_relative_to(root):
            raise ValueError("unsafe paper path")
        text = path.read_text()
        if target in text:
            found.append((path, text))
    if len(found) != 1:
        raise ValueError("UUID must exist exactly once; repair identity before review")
    path, text = found[0]
    relative = path.relative_to(root).as_posix()
    manifest = root / "manifest.json"
    if manifest.exists():
        mapping = {
            entry["target"]: entry["source_sha256"]
            for entry in json.loads(manifest.read_text())["papers"].values()
        }
    else:
        mapping = {}
    source_hash = mapping.get(relative, digest(classification_mask(text)))
    matches = [
        row
        for row in paper_questions(text, relative, source_hash)
        if row["question_uuid"] == question_uuid
    ]
    if len(matches) != 1:
        raise ValueError("UUID marker is malformed or ambiguous")
    row = matches[0]
    if row["paper_uuid"] is None or row["_errors"]:
        raise ValueError("UUID metadata requires repair")
    units = list(UNIT.finditer(text))
    materials = {}
    for i, unit in enumerate(units):
        chunk = text[
            unit.end() : units[i + 1].start() if i + 1 < len(units) else len(text)
        ]
        if unit[2]:
            materials[unit[2]] = re.sub(
                r"<!-- hyz-material-id: [^\n]+ -->", "", chunk
            ).strip()
        elif int(unit[1]) == row["question_number"]:
            meta = META.search(chunk)
            refs = yaml.safe_load(meta[1])["materials"]
    return {
        "question_uuid": question_uuid,
        "paper_uuid": row["paper_uuid"],
        "paper_path": relative,
        "question_number": row["question_number"],
        "module": row["module"],
        "current_category": row["current_category"],
        "topics": row["topics"],
        "question_hash": row["question_hash"],
        "source_hash": row["source_hash"],
        "stem": row["_stem"],
        "options": row["_options"],
        "materials": [materials[ref] for ref in refs],
    }


def import_suggestions(root, rows):
    """Validate AI output against fresh content and store *pending* proposals."""
    root = Path(root).resolve()
    audit = audit_bank(root)
    indexed = {r["question_uuid"]: r for r in audit["rows"] if r["question_uuid"]}
    seen = set()
    proposed, unresolved = [], []
    required = {
        "question_uuid",
        "paper_uuid",
        "question_hash",
        "source_hash",
        "classification_rule_version",
        "module",
        "category",
        "can_determine",
        "rationale",
        "unable_reason",
    }
    for row in rows:
        if not isinstance(row, dict) or set(row) != required:
            raise ValueError("AI result requires exact versioned classification fields")
        identifier = row["question_uuid"]
        if identifier in seen:
            raise ValueError("duplicate AI suggestion UUID")
        seen.add(identifier)
        current = indexed.get(identifier)
        if current is None or any("uuid" in r for r in current["conflict_reasons"]):
            raise ValueError("missing or duplicate question UUID in current bank")
        for field in (
            "paper_uuid",
            "question_hash",
            "source_hash",
            "classification_rule_version",
        ):
            if row[field] != current[field]:
                raise ValueError(f"stale AI suggestion {field}")
        if not current["needs_ai_review"]:
            raise ValueError("question has a reliable deterministic classification")
        if type(row["can_determine"]) is not bool:
            raise ValueError("can_determine must be a boolean")
        if not row["can_determine"]:
            if (
                row["module"] is not None
                or row["category"] is not None
                or not isinstance(row["unable_reason"], str)
                or not row["unable_reason"].strip()
            ):
                raise ValueError(
                    "indeterminate result requires null labels and a reason"
                )
            unresolved.append(row)
            continue
        if row["unable_reason"] is not None:
            raise ValueError("determinate result cannot include unable_reason")
        proposed.append(
            {
                "uuid": identifier,
                "paper_uuid": row["paper_uuid"],
                "question_hash": row["question_hash"],
                "source_hash": row["source_hash"],
                "classification_rule_version": row["classification_rule_version"],
                "module": row["module"],
                "category": row["category"],
                "reviewed_source": "ai",
                "review_status": "reviewed",
                "rationale": row["rationale"],
                "status": "suggested",
            }
        )
    validated = validate_proposals(proposed, [indexed[row["uuid"]] for row in proposed])
    write_if_changed(root / PROPOSALS, _jsonl(validated))
    write_if_changed(root / UNRESOLVED, _jsonl(unresolved))
    return validated


def approve_suggestions(root, approvals):
    """Require separate, explicit human approval of current proposals."""
    root = Path(root).resolve()
    proposal_path = root / PROPOSALS
    proposals = _read_jsonl(proposal_path)
    audit = audit_bank(root)
    indexed = {r["question_uuid"]: r for r in audit["rows"] if r["question_uuid"]}
    validated = validate_proposals(proposals, [indexed[r["uuid"]] for r in proposals])
    return approve_proposals(
        validated, approvals, root / "classification_overrides.yaml"
    )


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bank", type=Path, default=Path("question_bank"))
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument(
        "--lookup", help="unique UUID; print prompt, options, and materials"
    )
    mode.add_argument(
        "--import-suggestions",
        type=Path,
        help="AI results JSONL to validate as pending",
    )
    mode.add_argument(
        "--approve", type=Path, help="human approvals JSONL (separate file)"
    )
    args = parser.parse_args(argv)
    try:
        if args.lookup:
            output = lookup_question(args.bank, args.lookup)
        elif args.import_suggestions:
            output = {
                "pending": len(
                    import_suggestions(args.bank, _read_jsonl(args.import_suggestions))
                )
            }
        else:
            output = {
                "approved_overrides": len(
                    approve_suggestions(args.bank, _read_jsonl(args.approve))
                )
            }
        print(json.dumps(output, ensure_ascii=False, sort_keys=True, indent=2))
        return 0
    except (OSError, ValueError) as error:
        print(f"classification-ai: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
