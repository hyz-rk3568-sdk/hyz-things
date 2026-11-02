import tempfile
import unittest
from pathlib import Path

from importer import import_bank
from test_bank import source


SAMPLE = source(
    """### 第 1 题
题干。
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 1 题 · 答案与解析</summary>
**答案**：A
**解析**
原始解析。
</details>
""",
    year=2001,
)


class ImportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.upstream = self.root / "source"
        self.output = self.root / "bank"
        self.path = Path("国考/2001/行测-统考.md")
        (self.upstream / self.path).parent.mkdir(parents=True)
        (self.upstream / self.path).write_text(SAMPLE)

    def test_images_in_stem_and_explanation_are_local_and_verifiable(self):
        from PIL import Image

        image = self.upstream / self.path.parent / "a.png"
        Image.new("RGB", (2, 2), "red").save(image)
        raw = SAMPLE.replace("题干。", "题干。![图](a.png)").replace(
            "原始解析。", '<img src="a.png" width="2" />'
        )
        (self.upstream / self.path).write_text(raw)
        report = import_bank(self.upstream, self.output)
        paper = self.output / "papers" / self.path
        self.assertNotIn("(a.png)", paper.read_text())
        self.assertNotIn('src="a.png"', paper.read_text())
        self.assertEqual(len(list((self.output / "assets").glob("*"))), 1)
        self.assertEqual(report["image_errors"], [])

    def test_uuid_survives_answer_revision_without_reassignment(self):
        import_bank(self.upstream, self.output)
        paper = self.output / "papers" / self.path
        uuid = "019a3d8e-1b32-7a60-9c83-f34217a6e210"
        paper.write_text(
            paper.read_text().replace(
                "## Question 1\n",
                f"## Question 1\n\n<!-- hyz-question-id: {uuid} -->\n",
            )
        )
        (self.upstream / self.path).write_text(
            SAMPLE.replace("原始解析。", "修订解析。")
        )
        report = import_bank(self.upstream, self.output, update=True)
        self.assertEqual(report["conflicts"], [])
        self.assertIn(uuid, paper.read_text())
        self.assertIn("修订解析。", paper.read_text())

    def test_source_rename_reuses_target_and_uuid(self):
        import_bank(self.upstream, self.output)
        paper = self.output / "papers" / self.path
        uuid = "019a3d8e-1b32-7a60-9c83-f34217a6e210"
        paper.write_text(
            paper.read_text().replace(
                "## Question 1\n",
                f"## Question 1\n\n<!-- hyz-question-id: {uuid} -->\n",
            )
        )
        renamed = self.upstream / self.path.parent / "行测-新文件名.md"
        (self.upstream / self.path).rename(renamed)
        report = import_bank(self.upstream, self.output, update=True)
        self.assertEqual(report["conflicts"], [])
        self.assertEqual(len(list((self.output / "papers").rglob("*.md"))), 1)
        self.assertIn(uuid, paper.read_text())

    def test_missing_image_blocks_question_and_is_reported(self):
        (self.upstream / self.path).write_text(
            SAMPLE.replace("题干。", "题干。![](missing.png)")
        )
        report = import_bank(self.upstream, self.output)
        self.assertEqual(len(report["image_errors"]), 1)
        text = (self.output / "papers" / self.path).read_text()
        self.assertIn("missing_image", text)

    def test_quality_report_counts_positions_and_classification_coverage(self):
        report = import_bank(self.upstream, self.output)
        self.assertEqual(report["source_content_counts"]["question_positions"], 1)
        self.assertEqual(report["source_content_counts"]["structurally_answerable"], 1)
        self.assertTrue((self.output / "reports/quality.md").is_file())
        self.assertTrue((self.output / "reports/provenance.jsonl").is_file())

    def test_missing_previously_identified_file_is_not_regenerated(self):
        import_bank(self.upstream, self.output)
        paper = self.output / "papers" / self.path
        uuid = "019a3d8e-1b32-7a60-9c83-f34217a6e210"
        paper.write_text(
            paper.read_text().replace(
                "## Question 1\n",
                f"## Question 1\n\n<!-- hyz-question-id: {uuid} -->\n",
            )
        )
        import_bank(self.upstream, self.output, update=True)
        paper.unlink()
        report = import_bank(self.upstream, self.output, update=True)
        self.assertTrue(report["conflicts"])
        self.assertFalse(paper.exists())

    def test_duplicate_uuid_across_papers_is_rejected_before_writing(self):
        import_bank(self.upstream, self.output)
        paper = self.output / "papers" / self.path
        uuid = "019a3d8e-1b32-7a60-9c83-f34217a6e210"
        paper.write_text(
            paper.read_text().replace(
                "## Question 1\n",
                f"## Question 1\n\n<!-- hyz-question-id: {uuid} -->\n",
            )
        )
        duplicate = paper.with_name("行测-人工副本.md")
        duplicate.write_text(paper.read_text())
        with self.assertRaisesRegex(ValueError, "duplicate UUID"):
            import_bank(self.upstream, self.output, update=True)

    def test_cli_dry_run_does_not_create_destination(self):
        import subprocess
        import sys

        result = subprocess.run(
            [
                sys.executable,
                str(Path(__file__).with_name("import_bank.py")),
                "--source",
                str(self.upstream),
                "--output",
                str(self.output),
                "--dry-run",
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.output.exists())

    def test_image_content_update_is_not_hidden_by_unchanged_markdown(self):
        from PIL import Image

        image = self.upstream / self.path.parent / "a.png"
        Image.new("RGB", (2, 2), "red").save(image)
        (self.upstream / self.path).write_text(
            SAMPLE.replace("题干。", "题干。![](a.png)")
        )
        import_bank(self.upstream, self.output)
        paper = self.output / "papers" / self.path
        before = paper.read_text()
        Image.new("RGB", (2, 2), "blue").save(image)
        import_bank(self.upstream, self.output, update=True)
        self.assertNotEqual(paper.read_text(), before)

    def test_explicit_supplementary_copy_is_not_a_ready_primary_paper(self):
        (self.upstream / self.path).write_text(
            SAMPLE.replace("科目: 行测", "科目: 行测\n从属佐证版: true")
        )
        report = import_bank(self.upstream, self.output)
        text = (self.output / "papers" / self.path).read_text()
        self.assertIn("role: evidence", text)
        self.assertEqual(report["source_content_counts"].get("ready", 0), 0)

    def test_provenance_retains_original_labels_and_source_revision(self):
        import json

        (self.upstream / self.path).write_text(
            SAMPLE.replace("原始解析。", "原始解析。\n**考点**：甲、乙")
        )
        import_bank(self.upstream, self.output)
        row = json.loads((self.output / "reports/provenance.jsonl").read_text())
        self.assertIn("source_commit", row)
        self.assertEqual(row["questions"][0]["raw_topics"], ["甲、乙"])
        self.assertIn("module_basis", row["questions"][0])

    def test_update_backs_up_previous_paper_before_replacement(self):
        import_bank(self.upstream, self.output)
        paper = self.output / "papers" / self.path
        old = paper.read_text()
        (self.upstream / self.path).write_text(
            SAMPLE.replace("原始解析。", "修订解析。")
        )
        import_bank(self.upstream, self.output, update=True)
        backups = list((self.output / "reports/backups").rglob("*.md"))
        self.assertTrue(any(p.read_text() == old for p in backups))

    def test_import_all_years_and_preserve_manual_edits_on_update(self):
        report = import_bank(self.upstream, self.output)
        self.assertEqual(report["converted_papers"], 1)
        paper = self.output / "papers" / self.path
        manual = paper.read_text().replace("题干。", "人工修改题干。")
        paper.write_text(manual)
        report = import_bank(self.upstream, self.output, update=True)
        self.assertEqual(paper.read_text(), manual)
        self.assertEqual(report["conflicts"], [])
        (self.upstream / self.path).write_text(
            SAMPLE.replace("原始解析。", "上游修改解析。")
        )
        report = import_bank(self.upstream, self.output, update=True)
        self.assertTrue(report["conflicts"])
        self.assertEqual(paper.read_text(), manual)


if __name__ == "__main__":
    unittest.main()
