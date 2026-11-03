import json
from pathlib import Path
import tempfile
import unittest

from classification_audit import audit_bank, apply_changes, write_reports


PAPER_ID = "019a3d8e-1b32-7a60-9c83-f34217a6e201"
QUESTION_ID = "019a3d8e-1b32-7a60-9c83-f34217a6e210"


def document(question_id=QUESTION_ID, category="类比推理"):
    marker = f"<!-- hyz-question-id: {question_id} -->" if question_id else ""
    return f"""---
schema: hyz-paper/v1
title: 回归卷
---

<!-- hyz-paper-id: {PAPER_ID} -->
# 回归卷

## Question 1

{marker}
```yaml
module: 判断推理
category: {category}
topics:
- 方式目的
materials: []
passage_source: null
issues: []
```

### Stem

某术语是指一种长期形成并受到社会环境影响的群体行为模式，根据上述定义，下列属于该术语的是？

### Options

- A. 甲

- B. 乙

- C. 丙

- D. 丁

### Answer

A

### Explanation

原始解析。
"""


class ClassificationAuditTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.paper = self.root / "papers/省考/某地/2026/行测-统考.md"
        self.paper.parent.mkdir(parents=True)
        self.paper.write_text(document())
        from importer import digest
        from identity import without_ids

        baseline = digest(without_ids(self.paper.read_text()))
        (self.root / "manifest.json").write_text(
            json.dumps(
                {
                    "schema": "hyz-question-bank-manifest/v1",
                    "papers": {
                        "source": {
                            "target": self.paper.relative_to(self.root).as_posix(),
                            "source_sha256": "a" * 64,
                            "generated_sha256": baseline,
                        }
                    },
                }
            )
        )

    def test_read_only_audits_existing_misclassification_with_uuid_and_hashes(self):
        before = self.paper.read_bytes()
        audit = audit_bank(self.root)
        row = audit["rows"][0]
        self.assertEqual(row["question_uuid"], QUESTION_ID)
        self.assertEqual(row["paper_uuid"], PAPER_ID)
        self.assertEqual(row["current_category"], "类比推理")
        self.assertEqual(row["suggested_category"], "定义判断")
        self.assertEqual(row["status"], "conflict")
        self.assertTrue(row["needs_ai_review"] is False)
        self.assertEqual(row["source_hash"], "a" * 64)
        self.assertEqual(len(row["question_hash"]), 64)
        self.assertEqual(self.paper.read_bytes(), before)
        self.assertFalse((self.root / "reports").exists())

    def test_missing_and_duplicate_uuid_are_reported_and_block_apply(self):
        self.paper.write_text(document(question_id=None))
        audit = audit_bank(self.root)
        self.assertIsNone(audit["rows"][0]["question_uuid"])
        self.assertIn("missing_question_uuid", audit["rows"][0]["conflict_reasons"])
        with self.assertRaisesRegex(ValueError, "UUID"):
            apply_changes(self.root, audit)
        self.paper.write_text(document())
        other = self.paper.with_name("行测-重号.md")
        other.write_text(document())
        audit = audit_bank(self.root)
        self.assertEqual(audit["summary"]["duplicate_question_uuids"], 2)
        with self.assertRaisesRegex(ValueError, "UUID"):
            apply_changes(self.root, audit)

    def test_apply_preserves_question_content_identity_and_is_idempotent(self):
        before = self.paper.read_text()
        audit = audit_bank(self.root)
        self.assertEqual(apply_changes(self.root, audit), 1)
        after = self.paper.read_text()
        self.assertEqual(
            after.replace("category: 定义判断", "category: 类比推理"), before
        )
        self.assertEqual(audit_bank(self.root)["rows"][0]["status"], "verified")
        self.assertEqual(apply_changes(self.root, audit_bank(self.root)), 0)
        self.assertEqual(self.paper.read_text(), after)
        write_reports(self.root, audit_bank(self.root))
        queue = self.root / "reports/classification_queue.jsonl"
        self.assertEqual(queue.read_text(), "")

    def test_real_yunnan_and_cross_province_have_independent_ids(self):
        root = Path(__file__).parents[2] / "question_bank"
        if not (root / "papers/省考/云南/2026/行测-统考.md").exists():
            self.skipTest("local bank unavailable")
        # Only selected real papers are copied to keep the regression fast.
        for province in ("云南", "贵州"):
            path = root / f"papers/省考/{province}/2026/行测-统考.md"
            if path.exists():
                target = self.root / path.relative_to(root)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(path.read_bytes())
        rows = audit_bank(self.root)["rows"]
        yunnan = [r for r in rows if "云南" in r["paper_path"]]
        self.assertEqual(
            [
                r["suggested_category"]
                for r in yunnan
                if 68 <= r["question_number"] <= 84
            ],
            ["定义判断"] * 8 + ["类比推理"] * 9,
        )
        self.assertEqual(
            next(r for r in yunnan if r["question_number"] == 84)["evidence"],
            ["analogy_fill_relation_pattern"],
        )
        self.assertEqual(
            len({r["question_uuid"] for r in rows if r["question_uuid"]}),
            sum(bool(r["question_uuid"]) for r in rows),
        )


if __name__ == "__main__":
    unittest.main()
