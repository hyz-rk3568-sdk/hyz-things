"""Human-approved, fingerprint-bound question classification overrides."""

import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import uuid

import yaml

from classification import RULE_VERSION, STANDARD_CATEGORIES


SCHEMA = "hyz-question-bank-classification-overrides/v1"
PROPOSAL_FIELDS = frozenset(
    {
        "uuid",
        "paper_uuid",
        "question_hash",
        "source_hash",
        "classification_rule_version",
        "module",
        "category",
        "reviewed_source",
        "review_status",
        "rationale",
        "status",
    }
)
FINGERPRINT_FIELDS = (
    "paper_uuid",
    "question_hash",
    "source_hash",
    "classification_rule_version",
)
SHA256 = re.compile(r"[0-9a-f]{64}\Z")


class _UniqueLoader(yaml.SafeLoader):
    def construct_mapping(self, node, deep=False):
        result = {}
        for key_node, value_node in node.value:
            key = self.construct_object(key_node, deep=deep)
            try:
                if key in result:
                    raise ValueError(f"duplicate YAML key: {key}")
            except TypeError as error:
                raise ValueError("invalid YAML mapping key") from error
            result[key] = self.construct_object(value_node, deep=deep)
        return result


def _text(value, field):
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"missing or invalid {field}")
    return value.strip()


def _uuid(value, field):
    text = _text(value, field)
    try:
        parsed = uuid.UUID(text)
    except ValueError as error:
        raise ValueError(f"invalid {field}") from error
    if parsed.version != 7 or str(parsed) != text:
        raise ValueError(f"invalid {field}: expected canonical UUIDv7")
    return text


def _hash(value, field):
    text = _text(value, field)
    if not SHA256.fullmatch(text):
        raise ValueError(f"invalid {field}: expected SHA-256")
    return text


def _version(value):
    if value != RULE_VERSION:
        raise ValueError("stale classification classification_rule_version")
    return value


def _normalize_proposal(row):
    if not isinstance(row, dict) or set(row) not in (
        PROPOSAL_FIELDS,
        PROPOSAL_FIELDS | {"proposal_digest"},
    ):
        raise ValueError("proposal must contain only classification and review fields")
    result = {
        "uuid": _uuid(row["uuid"], "uuid"),
        "paper_uuid": _uuid(row["paper_uuid"], "paper_uuid"),
        "question_hash": _hash(row["question_hash"], "question_hash"),
        "source_hash": _hash(row["source_hash"], "source_hash"),
        "classification_rule_version": _version(row["classification_rule_version"]),
        "module": _text(row["module"], "module"),
        "category": _text(row["category"], "category"),
        "reviewed_source": _text(row["reviewed_source"], "reviewed_source"),
        "rationale": _text(row["rationale"], "rationale"),
        "review_status": row["review_status"],
        "status": row["status"],
    }
    if result["uuid"] == result["paper_uuid"]:
        raise ValueError("question and paper UUIDs must differ")
    if result["category"] not in STANDARD_CATEGORIES.get(result["module"], ()):
        raise ValueError("nonstandard module/category pair")
    if result["review_status"] != "reviewed" or result["status"] != "suggested":
        raise ValueError("proposal must be a reviewed suggestion, not an approval")
    payload = json.dumps(
        result, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    )
    digest = hashlib.sha256(payload.encode("utf-8")).hexdigest()
    if "proposal_digest" in row and row["proposal_digest"] != digest:
        raise ValueError("proposal digest mismatch")
    result["proposal_digest"] = digest
    return result


def _check_uuid_collisions(pairs):
    question_ids = set()
    paper_ids = set()
    for question_id, paper_id in pairs:
        question_ids.add(question_id)
        paper_ids.add(paper_id)
    if question_ids & paper_ids:
        raise ValueError("question UUID duplicates a paper UUID")


def validate_proposals(rows, audit_rows):
    """Match AI suggestions against a fresh UUID audit; never approve them."""
    if not isinstance(rows, list) or not isinstance(audit_rows, list):
        raise ValueError("proposals and audit_rows must be lists")
    indexed = {}
    for audit in audit_rows:
        if not isinstance(audit, dict):
            raise ValueError("invalid audit row")
        identifier = _uuid(audit.get("question_uuid", audit.get("uuid")), "audit UUID")
        if "uuid" in audit and _uuid(audit["uuid"], "audit UUID") != identifier:
            raise ValueError(f"conflicting audit UUID for {identifier}")
        reasons = audit.get("conflict_reasons", [])
        if not isinstance(reasons, list) or any(
            not isinstance(reason, str)
            or (
                reason.startswith(("missing_", "duplicate_"))
                and any(field in reason for field in ("uuid", "hash", "version"))
            )
            for reason in reasons
        ):
            raise ValueError(f"unreliable audit identity for {identifier}")
        _uuid(audit.get("paper_uuid"), "audit paper_uuid")
        _hash(audit.get("question_hash"), "audit question_hash")
        _hash(audit.get("source_hash"), "audit source_hash")
        _version(audit.get("classification_rule_version"))
        if identifier in indexed:
            raise ValueError(f"duplicate audit UUID {identifier}")
        indexed[identifier] = audit
    _check_uuid_collisions(
        (identifier, row["paper_uuid"]) for identifier, row in indexed.items()
    )
    normalized = []
    seen = set()
    for row in rows:
        item = _normalize_proposal(row)
        identifier = item["uuid"]
        if identifier in seen:
            raise ValueError(f"duplicate proposal UUID {identifier}")
        seen.add(identifier)
        audit = indexed.get(identifier)
        if audit is None:
            raise ValueError(f"missing audit UUID {identifier}")
        for field in FINGERPRINT_FIELDS:
            if item[field] != audit.get(field):
                raise ValueError(f"stale proposal {field} for {identifier}")
        normalized.append(item)
    return normalized


def _approved_record(row):
    fields = PROPOSAL_FIELDS | {"proposal_digest", "reviewer", "approval_rationale"}
    if not isinstance(row, dict) or set(row) != fields or row["status"] != "approved":
        raise ValueError("invalid approved override record")
    suggestion = {field: row[field] for field in PROPOSAL_FIELDS}
    suggestion["status"] = "suggested"
    suggestion["proposal_digest"] = row["proposal_digest"]
    normalized = _normalize_proposal(suggestion)
    normalized["status"] = "approved"
    normalized["reviewer"] = _text(row["reviewer"], "reviewer")
    normalized["approval_rationale"] = _text(
        row["approval_rationale"], "approval_rationale"
    )
    return normalized


def load_overrides(path):
    """Read approved UUID records; an absent store contains no approvals."""
    path = Path(path)
    if path.is_symlink():
        raise ValueError("override store must not be a symlink")
    if not path.exists():
        return {}
    try:
        data = yaml.load(path.read_text(encoding="utf-8"), Loader=_UniqueLoader)
    except yaml.YAMLError as error:
        raise ValueError("invalid classification override YAML") from error
    if (
        not isinstance(data, dict)
        or set(data) != {"schema", "overrides"}
        or data["schema"] != SCHEMA
        or not isinstance(data["overrides"], list)
    ):
        raise ValueError("invalid classification override store")
    records = {}
    for row in data["overrides"]:
        record = _approved_record(row)
        identifier = record["uuid"]
        if identifier in records:
            raise ValueError(f"duplicate approved UUID {identifier}")
        records[identifier] = record
    _check_uuid_collisions((row["uuid"], row["paper_uuid"]) for row in records.values())
    return records


def _write_if_changed(path, records):
    content = yaml.safe_dump(
        {"schema": SCHEMA, "overrides": [records[key] for key in sorted(records)]},
        allow_unicode=True,
        sort_keys=False,
    )
    if path.exists() and path.read_text(encoding="utf-8") == content:
        return
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            "w", encoding="utf-8", dir=path.parent, prefix=path.name + ".", delete=False
        ) as stream:
            temporary = Path(stream.name)
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def approve_proposals(proposals, approvals, overrides_path):
    """Store only explicitly approved, digest-matched suggestions as YAML."""
    if not isinstance(proposals, list) or not isinstance(approvals, list):
        raise ValueError("proposals and approvals must be lists")
    path = Path(overrides_path)
    if path.suffix not in (".yaml", ".yml") or path.is_symlink():
        raise ValueError("override store must be a regular YAML path")
    suggestions = {}
    for row in proposals:
        if not isinstance(row, dict) or "proposal_digest" not in row:
            raise ValueError("approval requires validated proposal digest")
        suggestion = _normalize_proposal(row)
        identifier = suggestion["uuid"]
        if identifier in suggestions:
            raise ValueError(f"duplicate proposal UUID {identifier}")
        suggestions[identifier] = suggestion
    records = load_overrides(path)
    seen = set()
    for approval in approvals:
        if not isinstance(approval, dict) or set(approval) != {
            "uuid",
            "proposal_digest",
            "reviewer",
            "rationale",
            "status",
        }:
            raise ValueError(
                "approval requires uuid, digest, reviewer, rationale, status"
            )
        identifier = _uuid(approval["uuid"], "approval UUID")
        if identifier in seen:
            raise ValueError(f"duplicate approval UUID {identifier}")
        seen.add(identifier)
        suggestion = suggestions.get(identifier)
        if suggestion is None:
            raise ValueError(f"missing proposal UUID {identifier}")
        if approval["proposal_digest"] != suggestion["proposal_digest"]:
            raise ValueError(f"stale approval digest for {identifier}")
        if approval["status"] != "approved":
            raise ValueError("approval must explicitly have status approved")
        record = _approved_record(
            {
                **suggestion,
                "status": "approved",
                "reviewer": _text(approval["reviewer"], "reviewer"),
                "approval_rationale": _text(
                    approval["rationale"], "approval rationale"
                ),
            }
        )
        if identifier in records and records[identifier] != record:
            raise ValueError(f"conflicting approved override for {identifier}")
        records[identifier] = record
    _check_uuid_collisions((row["uuid"], row["paper_uuid"]) for row in records.values())
    if not approvals:
        return records
    _write_if_changed(path, records)
    return records
