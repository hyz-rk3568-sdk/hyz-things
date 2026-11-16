#!/usr/bin/env python3
"""Host-only tests for the WPS PDF question-bank importer."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile
import unittest

TOOL = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOL))

from wpsbank import emit  # noqa: E402
from wpsbank import profile as profile_module  # noqa: E402
from wpsbank import text  # noqa: E402

try:
    from wpsbank import extract  # noqa: E402
except ModuleNotFoundError:  # pragma: no cover - PyMuPDF is optional for unit tests
    extract = None

PROFILE_PATH = TOOL / "profiles" / "sihai-2026-haihaishua-ziliao.json"


class ProfileTests(unittest.TestCase):
    def test_shipped_profile_describes_the_sihai_book(self):
        loaded = profile_module.Profile.load(PROFILE_PATH)
        self.assertEqual(loaded.collection_id, "sihai-2026-haihaishua-ziliao")
        self.assertEqual(len(loaded.papers), 24)
        self.assertEqual(sum(1 for paper in loaded.papers if paper.kind == "考点刷题"), 4)
        self.assertEqual(loaded.option_labels, "ABCD")
        self.assertEqual(loaded.questions_per_material, 5)

    def test_profile_rejects_duplicate_option_labels(self):
        data = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
        data["option_labels"] = "ABAB"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "broken.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            with self.assertRaises(ValueError):
                profile_module.Profile.load(path)

    def test_profile_rejects_overlapping_paper_ids(self):
        data = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
        data["papers"].append(dict(data["papers"][0]))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "broken.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            with self.assertRaises(ValueError):
                profile_module.Profile.load(path)

    def test_title_pattern_accepts_paper_heading_only(self):
        loaded = profile_module.Profile.load(PROFILE_PATH)
        self.assertTrue(loaded.title_re.fullmatch("套题训练12"))
        self.assertTrue(loaded.title_re.fullmatch("考点刷题1"))
        self.assertFalse(loaded.title_re.fullmatch("套题训练12材料"))

    def test_noise_uses_profile_bands_texts_and_patterns(self):
        loaded = profile_module.Profile.load(PROFILE_PATH)
        self.assertTrue(loaded.is_noise("第 138 页", (100, 100, 200, 120)))
        self.assertTrue(loaded.is_noise("（总计20题，参考时间：25分钟）", (100, 100, 200, 120)))
        self.assertTrue(loaded.is_noise("公众号：上岸船舱，免费分享在职考公资料", (100, 100, 200, 120)))
        self.assertTrue(loaded.is_noise("正文", (100, 20, 200, 60)))
        self.assertTrue(loaded.is_noise("正文", (100, 760, 200, 790)))
        self.assertFalse(loaded.is_noise("2018年1～7月份，社会消费品零售总额", (100, 100, 200, 120)))

    def test_override_is_available_by_paper_and_question(self):
        loaded = profile_module.Profile.load(PROFILE_PATH)
        override = loaded.override("套题训练", 1, 18)
        self.assertIn(r"\frac{1}{12}", override["stem"])
        self.assertEqual(loaded.override("套题训练", 1, 1), {})


@unittest.skipIf(extract is None, "PyMuPDF is not installed")
class DetectTests(unittest.TestCase):
    class Page:
        def __init__(self, value):
            self.value = value

        def get_text(self):
            return self.value

    def test_heading_pages_become_ranges(self):
        loaded = profile_module.Profile.load(PROFILE_PATH)
        pages = [
            self.Page("封面"),
            self.Page("考点刷题1\n题目"),
            self.Page("继续"),
            self.Page("套题训练1\n题目"),
            self.Page("后续"),
        ]
        ranges = extract.detect_papers(pages, loaded)
        self.assertEqual(ranges[0], ("考点刷题", 1, 2, 3))
        self.assertEqual(ranges[-1], ("套题训练", 1, 4, 5))


class TextTests(unittest.TestCase):
    def test_clean_removes_cjk_spacing(self):
        self.assertEqual(text.clean("2021 年 4 ～ 6 月"), "2021年4～6月")

    def test_clean_keeps_percent_attached(self):
        self.assertEqual(text.clean("增长 8.6 %"), "增长8.6%")

    def test_clean_normalizes_pm25_and_leading_minus(self):
        self.assertEqual(text.clean("PM2．5 浓度"), "PM2.5浓度")
        self.assertEqual(text.clean("-5.1%的降幅"), "−5.1%的降幅")


class AnswerTests(unittest.TestCase):
    def setUp(self):
        self.profile = profile_module.Profile.load(PROFILE_PATH)

    def test_combined_run_records_each_letter(self):
        answers = emit.parse_answer_lines(
            ["资料套题1", "1--5 BABBB", "6-10 AABBC", "11--15 AABBC", "16-20 AABBC"],
            self.profile,
        )
        paper = answers[("套题训练", 1)]
        self.assertEqual([paper[n] for n in range(1, 6)], list("BABBB"))
        self.assertEqual(paper[10], "C")
        self.assertEqual(paper[20], "C")

    def test_split_letter_line_fills_pending_range(self):
        answers = emit.parse_answer_lines(
            ["资料套题2", "1～5", "AABBC"],
            self.profile,
        )
        self.assertEqual(answers[("套题训练", 2)][5], "C")

    def test_heading_map_keeps_source_group(self):
        answers = emit.parse_answer_lines(["考点刷题3", "1-5 AAAAA"], self.profile)
        self.assertIn(("考点刷题", 3), answers)


class EmitTests(unittest.TestCase):
    def setUp(self):
        self.profile = profile_module.Profile.load(PROFILE_PATH)

    @staticmethod
    def paper():
        return {
            "kind": "套题训练",
            "num": 1,
            "materials": [{"key": "M1", "blocks": [{"kind": "text", "text": "材料内容", "x": 78}]}],
            "questions": [
                {
                    "number": 1,
                    "stem": [{"kind": "text", "text": "题干", "x": 78}],
                    "options": [
                        {"label": label, "blocks": [{"kind": "text", "text": label}]}
                        for label in "ABCD"
                    ],
                }
            ],
        }

    def test_markdown_has_no_generated_ids(self):
        markdown = emit.paper_markdown(
            self.paper(), {("套题训练", 1): {1: "A"}}, self.profile
        )
        self.assertNotIn("<!-- hyz-", markdown)
        self.assertNotIn("hyz-paper-id", markdown)
        self.assertNotIn("<!--", markdown)

    def test_markdown_uses_material_names_and_module(self):
        markdown = emit.paper_markdown(
            self.paper(), {("套题训练", 1): {1: "A"}}, self.profile
        )
        self.assertIn("materials:\n- M1\n", markdown)
        self.assertIn("module: 资料分析", markdown)
        self.assertIn("collection_id: sihai-2026-haihaishua-ziliao", markdown)
        self.assertTrue(markdown.startswith("---\nschema: hyz-paper/v1\n"))

    def test_options_are_compact(self):
        markdown = emit.paper_markdown(
            self.paper(), {("套题训练", 1): {1: "A"}}, self.profile
        )
        self.assertIn("### Options\n- A. A\n- B. B\n- C. C\n- D. D\n\n### Answer", markdown)

    def test_answer_section_uses_letter(self):
        markdown = emit.paper_markdown(
            self.paper(), {("套题训练", 1): {1: "A"}}, self.profile
        )
        self.assertIn("### Answer\n\nA\n", markdown)

    def test_material_reference_follows_question_block(self):
        paper = self.paper()
        for number in range(1, 7):
            paper["questions"].append(
                {
                    "number": number,
                    "stem": [{"kind": "text", "text": "题干", "x": 78}],
                    "options": [
                        {"label": label, "blocks": [{"kind": "text", "text": label}]}
                        for label in "ABCD"
                    ],
                }
            )
        markdown = emit.paper_markdown(
            paper, {("套题训练", 1): {number: "A" for number in range(1, 7)}}, self.profile
        )
        self.assertIn("## Question 6\n\n```yaml\nmodule: 资料分析\nmaterials:\n- M1\n```", markdown)


if __name__ == "__main__":
    unittest.main()
