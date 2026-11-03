import json
from pathlib import Path
import tempfile
import unittest

from classification_ai import import_suggestions, approve_suggestions, lookup_question
from classification_audit import audit_bank
from test_classification_audit import document


class AiReviewWorkflowTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.paper = self.root / "papers/省考/某地/2026/行测-统考.md"
        self.paper.parent.mkdir(parents=True)
        self.paper.write_text(
            document(category="null").replace(
                "某术语是指一种长期形成并受到社会环境影响的群体行为模式，根据上述定义，下列属于该术语的是？",
                "某术语与某选项之间的关系尚不明确，请选择最合适的一项。",
            )
        )
        self.row = audit_bank(self.root)["rows"][0]

    def suggestion(self, **changes):
        return {
            "question_uuid": self.row["question_uuid"],
            "paper_uuid": self.row["paper_uuid"],
            "question_hash": self.row["question_hash"],
            "source_hash": self.row["source_hash"],
            "classification_rule_version": self.row["classification_rule_version"],
            "module": "判断推理",
            "category": "定义判断",
            "can_determine": True,
            "rationale": "题干先界定概念，然后询问四个选项何者属于该定义。",
            "unable_reason": None,
            **changes,
        }

    def test_ai_suggestion_requires_separate_human_approval_and_stable_version(self):
        pending = import_suggestions(self.root, [self.suggestion()])
        self.assertEqual(len(pending), 1)
        self.assertFalse((self.root / "classification_overrides.yaml").exists())
        self.assertIsNone(audit_bank(self.root)["rows"][0]["current_category"])
        approval = [
            {
                "uuid": pending[0]["uuid"],
                "proposal_digest": pending[0]["proposal_digest"],
                "reviewer": "human-reviewer",
                "rationale": "确认提问结构与题目原文一致。",
                "status": "approved",
            }
        ]
        approved = approve_suggestions(self.root, approval)
        self.assertIn(pending[0]["uuid"], approved)
        self.assertEqual(
            audit_bank(self.root)["rows"][0]["suggested_category"], "定义判断"
        )
        self.assertIsNone(audit_bank(self.root)["rows"][0]["current_category"])
        self.assertEqual(approve_suggestions(self.root, approval), approved)

    def test_stale_and_duplicate_ai_outputs_fail_without_writing(self):
        with self.assertRaisesRegex(ValueError, "stale"):
            import_suggestions(self.root, [self.suggestion(question_hash="0" * 64)])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            item = self.suggestion()
            import_suggestions(self.root, [item, item])
        with self.assertRaisesRegex(ValueError, "nonstandard"):
            import_suggestions(
                self.root, [self.suggestion(category="类比推理", module="数量关系")]
            )
        self.assertFalse((self.root / "classification_overrides.yaml").exists())
        self.assertFalse(
            (self.root / "reports/classification_proposals.jsonl").exists()
        )

    def test_indeterminate_question_stays_in_human_review_queue(self):
        self.assertEqual(
            import_suggestions(
                self.root,
                [
                    self.suggestion(
                        can_determine=False,
                        module=None,
                        category=None,
                        unable_reason="图片缺失，无法核实题型",
                        rationale="无法确定。",
                    )
                ],
            ),
            [],
        )
        unresolved = self.root / "reports/classification_unresolved.jsonl"
        self.assertEqual(len(unresolved.read_text().splitlines()), 1)
        self.assertEqual(
            json.loads(unresolved.read_text())["question_uuid"],
            self.row["question_uuid"],
        )
        self.assertFalse((self.root / "classification_overrides.yaml").exists())
        self.assertEqual((self.paper.read_text().count("category: null")), 1)

    def test_lookup_by_uuid_is_unique_and_does_not_expose_answer(self):
        item = lookup_question(self.root, self.row["question_uuid"])
        self.assertIn("某术语", item["stem"])
        self.assertNotIn("answer", item)
        self.assertNotIn("explanation", item)
        self.paper.write_text(
            self.paper.read_text().replace(
                "<!-- hyz-question-id:", "<!-- hyz-question-id: x"
            )
        )
        with self.assertRaisesRegex(ValueError, "UUID"):
            lookup_question(self.root, self.row["question_uuid"])


if __name__ == "__main__":
    unittest.main()
