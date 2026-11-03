from pathlib import Path
import tempfile
import unittest

import yaml

from classification import RULE_VERSION
from classification_overrides import (
    approve_proposals,
    load_overrides,
    validate_proposals,
)


QUESTION_ID = "019a3d8e-1b32-7a60-9c83-f34217a6e210"
PAPER_ID = "019a3d8e-1b32-7a60-9c83-f34217a6e201"


def audit(**changes):
    row = {
        "question_uuid": QUESTION_ID,
        "paper_uuid": PAPER_ID,
        "question_hash": "a" * 64,
        "source_hash": "b" * 64,
        "classification_rule_version": RULE_VERSION,
        "status": "conflict",
        "conflict_reasons": ["strong_evidence_conflict"],
    }
    row.update(changes)
    return row


def proposal(**changes):
    row = {
        "uuid": QUESTION_ID,
        "paper_uuid": PAPER_ID,
        "question_hash": "a" * 64,
        "source_hash": "b" * 64,
        "classification_rule_version": RULE_VERSION,
        "module": "判断推理",
        "category": "定义判断",
        "reviewed_source": "题目原始试卷第 1 题",
        "review_status": "reviewed",
        "rationale": "原题问根据上述定义选择。",
        "status": "suggested",
    }
    row.update(changes)
    return row


def approval(suggestion, **changes):
    row = {
        "uuid": suggestion["uuid"],
        "proposal_digest": suggestion["proposal_digest"],
        "reviewer": "human-reviewer",
        "rationale": "与原始试卷和题目定义核对。",
        "status": "approved",
    }
    row.update(changes)
    return row


class ClassificationOverridesTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.path = Path(directory.name) / "classification_overrides.yaml"

    def test_missing_store_is_empty_and_malformed_store_is_rejected(self):
        self.assertEqual(load_overrides(self.path), {})
        for content in (
            "",
            "[]\n",
            "overrides: []\n",
            "schema: wrong\noverrides: []\n",
        ):
            with self.subTest(content=content):
                self.path.write_text(content)
                with self.assertRaises(ValueError):
                    load_overrides(self.path)
        self.path.write_text(
            "schema: hyz-question-bank-classification-overrides/v1\noverrides: []\n"
        )
        self.assertEqual(load_overrides(self.path), {})

    def test_malformed_or_duplicate_yaml_keys_fail_closed(self):
        for content in (
            "schema: [\n",
            "schema: hyz-question-bank-classification-overrides/v1\n"
            "overrides: []\n"
            "overrides: []\n",
        ):
            with self.subTest(content=content):
                self.path.write_text(content)
                with self.assertRaises(ValueError):
                    load_overrides(self.path)

    def test_matching_ai_suggestion_is_normalized_and_has_stable_digest(self):
        row = proposal()
        result = validate_proposals([row], [audit()])
        self.assertEqual(result[0]["uuid"], QUESTION_ID)
        self.assertEqual(result[0]["category"], "定义判断")
        self.assertRegex(result[0]["proposal_digest"], r"^[0-9a-f]{64}$")
        self.assertEqual(result, validate_proposals([row], [audit()]))
        self.assertNotIn("proposal_digest", row)
        self.assertFalse(self.path.exists())

    def test_explicit_human_approval_persists_without_rewriting_on_retry(self):
        proposals = validate_proposals([proposal()], [audit()])
        self.assertEqual(approve_proposals(proposals, [], self.path), {})
        self.assertFalse(self.path.exists())
        signed = approval(proposals[0])
        records = approve_proposals(proposals, [signed], self.path)
        self.assertEqual(records, load_overrides(self.path))
        self.assertEqual(records[QUESTION_ID]["status"], "approved")
        self.assertEqual(
            records[QUESTION_ID]["reviewed_source"], proposal()["reviewed_source"]
        )
        self.assertEqual(
            records[QUESTION_ID]["approval_rationale"], signed["rationale"]
        )
        before = self.path.read_bytes()
        modified = self.path.stat().st_mtime_ns
        self.assertEqual(approve_proposals(proposals, [signed], self.path), records)
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(self.path.stat().st_mtime_ns, modified)

    def test_audit_identity_conflicts_block_suggestions(self):
        with self.assertRaisesRegex(ValueError, "UUID"):
            validate_proposals(
                [proposal(paper_uuid=QUESTION_ID)],
                [audit(paper_uuid=QUESTION_ID)],
            )
        for row in (
            audit(conflict_reasons=["duplicate_question_uuid"]),
            audit(uuid=PAPER_ID),
            audit(paper_uuid=None),
        ):
            with self.subTest(row=row):
                with self.assertRaises(ValueError):
                    validate_proposals([proposal()], [row])

    def test_question_uuid_cannot_reuse_another_papers_uuid(self):
        third = "019a3d8e-1b32-7a60-9c83-f34217a6e212"
        other_proposal = proposal(uuid=PAPER_ID, paper_uuid=third)
        other_audit = audit(question_uuid=PAPER_ID, paper_uuid=third)
        with self.assertRaisesRegex(ValueError, "UUID"):
            validate_proposals([proposal(), other_proposal], [audit(), other_audit])
        original = validate_proposals([proposal()], [audit()])[0]
        approve_proposals([original], [approval(original)], self.path)
        before = self.path.read_bytes()
        later = validate_proposals([other_proposal], [other_audit])[0]
        with self.assertRaisesRegex(ValueError, "UUID"):
            approve_proposals([later], [approval(later)], self.path)
        self.assertEqual(self.path.read_bytes(), before)

    def test_stale_missing_duplicate_or_illegal_suggestions_are_rejected(self):
        for changes in (
            {"uuid": None},
            {"uuid": "d3bedcf9-4dbb-46e7-84f5-b38722b875a3"},
            {"paper_uuid": "019a3d8e-1b32-7a60-9c83-f34217a6e211"},
            {"question_hash": "c" * 64},
            {"source_hash": "c" * 64},
            {"classification_rule_version": "hyz-classification/v1"},
            {"module": "伪造模块"},
            {"module": "常识判断"},
            {"category": "政治常识"},
            {"reviewed_source": ""},
            {"review_status": "unreviewed"},
            {"rationale": None},
            {"status": "approved"},
            {"stem": "AI cannot edit a question"},
            {"proposal_digest": "0" * 64},
        ):
            with self.subTest(changes=changes):
                with self.assertRaises(ValueError):
                    validate_proposals([proposal(**changes)], [audit()])
        with self.assertRaises(ValueError):
            validate_proposals([proposal(), proposal()], [audit()])
        with self.assertRaises(ValueError):
            validate_proposals([proposal()], [audit(), audit()])
        with self.assertRaises(ValueError):
            validate_proposals([proposal()], [])
        with self.assertRaises(ValueError):
            validate_proposals([proposal()], [audit(question_uuid=None)])
        self.assertFalse(self.path.exists())

    def test_approval_requires_exact_human_review_and_does_not_touch_paper(self):
        suggestion = validate_proposals([proposal()], [audit()])[0]
        paper = self.path.with_name("original.md")
        paper.write_text("# 原始正文\n")
        for changes in (
            {"proposal_digest": "0" * 64},
            {"reviewer": ""},
            {"rationale": ""},
            {"status": "suggested"},
            {"uuid": PAPER_ID},
            {"answer": "A"},
        ):
            with self.subTest(changes=changes):
                with self.assertRaises(ValueError):
                    approve_proposals(
                        [suggestion], [approval(suggestion, **changes)], self.path
                    )
                self.assertFalse(self.path.exists())
        with self.assertRaises(ValueError):
            approve_proposals([suggestion], [approval(suggestion)] * 2, self.path)
        with self.assertRaises(ValueError):
            approve_proposals([proposal()], [approval(suggestion)], self.path)
        with self.assertRaises(ValueError):
            approve_proposals([suggestion], [approval(suggestion)], paper)
        approve_proposals([suggestion], [approval(suggestion)], self.path)
        self.assertEqual(paper.read_text(), "# 原始正文\n")

    def test_batch_approval_is_atomic_and_existing_approval_never_silently_changes(
        self,
    ):
        second = "019a3d8e-1b32-7a60-9c83-f34217a6e211"
        suggestions = validate_proposals(
            [proposal(), proposal(uuid=second)],
            [audit(), audit(question_uuid=second)],
        )
        with self.assertRaises(ValueError):
            approve_proposals(
                suggestions,
                [
                    approval(suggestions[0]),
                    approval(suggestions[1], status="suggested"),
                ],
                self.path,
            )
        self.assertFalse(self.path.exists())
        approve_proposals(suggestions[:1], [approval(suggestions[0])], self.path)
        before = self.path.read_bytes()
        changed = validate_proposals([proposal(category="逻辑判断")], [audit()])
        with self.assertRaisesRegex(ValueError, "conflicting"):
            approve_proposals(changed, [approval(changed[0])], self.path)
        self.assertEqual(self.path.read_bytes(), before)

    def test_stored_approvals_reject_tampering_and_duplicate_uuid(self):
        suggestion = validate_proposals([proposal()], [audit()])[0]
        approve_proposals([suggestion], [approval(suggestion)], self.path)
        stored = yaml.safe_load(self.path.read_text())
        for altered in (
            lambda rows: rows.append(dict(rows[0])),
            lambda rows: rows[0].update(status="suggested"),
            lambda rows: rows[0].update(classification_rule_version="v0"),
            lambda rows: rows[0].update(question_hash="c" * 64),
            lambda rows: rows[0].update(reviewer=""),
            lambda rows: rows[0].update(options="A. tampered"),
        ):
            with self.subTest(altered=altered):
                rows = [dict(stored["overrides"][0])]
                altered(rows)
                self.path.write_text(yaml.safe_dump({**stored, "overrides": rows}))
                with self.assertRaises(ValueError):
                    load_overrides(self.path)


if __name__ == "__main__":
    unittest.main()
