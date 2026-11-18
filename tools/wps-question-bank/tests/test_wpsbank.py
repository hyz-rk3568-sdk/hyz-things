#!/usr/bin/env python3
"""Host-only tests for the WPS PDF question-bank importer."""

from __future__ import annotations

import dataclasses
import json
from pathlib import Path
import re
import sys
import tempfile
import unittest

TOOL = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOL))

from wpsbank import emit  # noqa: E402
from wpsbank import explain  # noqa: E402
from wpsbank import profile as profile_module  # noqa: E402
from wpsbank import text  # noqa: E402

try:
    from wpsbank import extract  # noqa: E402
except ModuleNotFoundError:  # pragma: no cover - PyMuPDF is optional for unit tests
    extract = None

try:
    from io import BytesIO

    from PIL import Image, ImageDraw

    from wpsbank import ocr_layout  # noqa: E402
    from wpsbank.ocr import OcrPage  # noqa: E402
except ModuleNotFoundError:  # pragma: no cover - OCR extras are optional
    ocr_layout = None

PROFILE_PATH = TOOL / "profiles" / "sihai-2026-haihaishua-ziliao.json"
HUASHENG_PATH = TOOL / "profiles" / "huasheng-2026-luojitiankong-600.json"
CHAOGE_PATH = TOOL / "profiles" / "chaoge-2026-yanyu-jinjie-600.json"


class ProfileTests(unittest.TestCase):
    def test_huasheng_logical_cloze_classification_and_blanks(self):
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        self.assertEqual(loaded.module, "言语理解与表达")
        self.assertEqual(loaded.category, "逻辑填空")
        root = TOOL.parents[1] / "question_bank/collections/huasheng-2026-luojitiankong-600"
        files = sorted(root.glob("*.md"))
        self.assertEqual(len(files), 30)
        for path in files:
            text = path.read_text(encoding="utf-8")
            self.assertNotIn("module: 逻辑填空", text, str(path))
            self.assertEqual(text.count("module: 言语理解与表达"), 20, str(path))
            self.assertEqual(text.count("category: 逻辑填空"), 20, str(path))
            self.assertNotRegex(text, r"(?<!\\)_{2,}", str(path))

    def test_shipped_profile_describes_the_sihai_book(self):
        loaded = profile_module.Profile.load(PROFILE_PATH)
        self.assertEqual(loaded.collection_id, "sihai-2026-haihaishua-ziliao")
        self.assertEqual(len(loaded.papers), 24)
        self.assertEqual(sum(1 for paper in loaded.papers if paper.kind == "考点刷题"), 4)
        self.assertEqual(loaded.option_labels, "ABCD")
        self.assertEqual(loaded.questions_per_material, 5)

    def test_profile_reads_v2_schema_and_answer_sections(self):
        data = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
        data["schema"] = "hyz-paper/v2"
        data["answer_pages"] = [
            {
                "kind": "套题训练",
                "num": 1,
                "pdf": "解析.pdf",
                "page": 3,
                "section": "第一组",
            }
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "v2.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            loaded = profile_module.Profile.load(path)
        self.assertEqual(loaded.schema, "hyz-paper/v2")
        self.assertEqual(loaded.subject, "xingce")
        self.assertEqual(loaded.answer_pages[0].section, "第一组")

    def test_profile_rejects_unknown_schema(self):
        data = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
        data["schema"] = "hyz-paper/v3"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "broken.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            with self.assertRaises(ValueError):
                profile_module.Profile.load(path)

    def test_profile_rejects_duplicate_option_labels(self):
        data = json.loads(PROFILE_PATH.read_text(encoding="utf-8"))
        data["option_labels"] = "ABAB"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "broken.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            with self.assertRaises(ValueError):
                profile_module.Profile.load(path)

    def test_profile_reads_the_ocr_han_space_squeeze_flag(self):
        data = json.loads(CHAOGE_PATH.read_text(encoding="utf-8"))
        data["ocr"]["squeeze_han_spaces"] = True
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            loaded = profile_module.Profile.load(path)
        self.assertTrue(loaded.ocr_squeeze_han_spaces)

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


class HuashengProfileTests(unittest.TestCase):
    def test_ask_line_tolerates_scan_typos(self):
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        for line in (
            "依次填入画横线部分最恰当的一项是：",
            "依次填入画横线部分最恰当的一顶是：",
            "依次填入画横线部分最恰当的项是：",
        ):
            self.assertTrue(loaded.ask_re.match(line), line)

    def test_missing_question_number_is_declared_for_its_page(self):
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        self.assertEqual(loaded.question_starts.get(95), 14)

    def test_scanned_stem_recovers_the_period_after_its_last_blank(self):
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        paper = {
            "kind": "练习题",
            "num": 1,
            "materials": [],
            "questions": [
                {
                    "number": 1,
                    "stem": [
                        {"kind": "text", "text": "成为成长的美好 ______", "x": 78}
                    ],
                    "options": [
                        {"label": label, "blocks": [{"kind": "text", "text": "甲"}]}
                        for label in "ABCD"
                    ],
                }
            ],
        }
        markdown = emit.paper_markdown(paper, {("练习题", 1): {1: "A"}}, loaded)
        self.assertIn("成为成长的美好 " + r"\_" * 6 + "。\n", markdown)

    def test_scanned_stem_ending_in_text_gets_a_period(self):
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        paper = {
            "kind": "练习题",
            "num": 1,
            "materials": [],
            "questions": [
                {
                    "number": 1,
                    "stem": [
                        {
                            "kind": "text",
                            "text": "从而使得传统的价值似是而非，甚至",
                            "x": 78,
                        }
                    ],
                    "options": [
                        {"label": label, "blocks": [{"kind": "text", "text": "甲"}]}
                        for label in "ABCD"
                    ],
                }
            ],
        }
        markdown = emit.paper_markdown(paper, {("练习题", 1): {1: "A"}}, loaded)
        self.assertIn("似是而非，甚至。\n", markdown)

    def test_scanned_blank_before_punctuation_loses_the_gap(self):
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        paper = {
            "kind": "练习题",
            "num": 1,
            "materials": [],
            "questions": [
                {
                    "number": 1,
                    "stem": [
                        {"kind": "text", "text": "始终______ ，以礼相待", "x": 78}
                    ],
                    "options": [
                        {"label": label, "blocks": [{"kind": "text", "text": "甲"}]}
                        for label in "ABCD"
                    ],
                }
            ],
        }
        markdown = emit.paper_markdown(paper, {("练习题", 1): {1: "A"}}, loaded)
        self.assertIn("始终" + r"\_" * 6 + "，以礼相待。\n", markdown)

    def test_text_layer_stem_keeps_its_exact_tail(self):
        loaded = profile_module.Profile.load(PROFILE_PATH)
        paper = {
            "kind": "套题训练",
            "num": 1,
            "materials": [],
            "questions": [
                {
                    "number": 1,
                    "stem": [{"kind": "text", "text": "甲 ______", "x": 78}],
                    "options": [
                        {"label": label, "blocks": [{"kind": "text", "text": "甲"}]}
                        for label in "ABCD"
                    ],
                }
            ],
        }
        markdown = emit.paper_markdown(paper, {("套题训练", 1): {1: "A"}}, loaded)
        self.assertIn("\n甲 ______\n", markdown)


class ChaogeProfileTests(unittest.TestCase):
    def test_ask_lines_of_every_section_are_recognized(self):
        loaded = profile_module.Profile.load(CHAOGE_PATH)
        for line in (
            "填入画横线部分最恰当的一项是：",
            "依次填入画横线部分最恰当的一项是：",
            "以下最合适做这段文字标题的一项是：",
            "最适合做这段文字标题的是：",
            "填入文中横线处最恰当的一项是：",
            "将以上6个句子重新排列，语序正确的一项是：",
            "文段中“拳头”“指头”分别指：",
            "文段中“刻板印象”指的是：",
            "这段文字接下来最可能讲的是：",
            "根据上文，接下来最可能介绍：",
            "这段文字意在说明：",
            "这段文字意在阐明：",
        ):
            self.assertTrue(loaded.ask_re.match(line), line)
        self.assertFalse(
            loaded.ask_re.match("文明的进步，总是和城市的繁荣紧密相连。")
        )

    def test_papers_cover_every_question_page_once(self):
        loaded = profile_module.Profile.load(CHAOGE_PATH)
        self.assertEqual(len(loaded.papers), 17)
        self.assertEqual(loaded.papers[0].start, 5)
        self.assertEqual(loaded.papers[-1].end, 208)
        for current, following in zip(loaded.papers, loaded.papers[1:]):
            self.assertEqual(current.end + 1, following.start)
        self.assertEqual(
            sum(loaded.expected[paper.kind] for paper in loaded.papers), 600
        )

    def test_every_answer_section_is_declared(self):
        loaded = profile_module.Profile.load(CHAOGE_PATH)
        self.assertEqual(len(loaded.answer_pages), 17)
        for entry in loaded.answer_pages:
            self.assertTrue(entry.section, entry)
        keys = {(entry.pdf, entry.page, entry.section) for entry in loaded.answer_pages}
        self.assertEqual(len(keys), 17)


class QuestionStartTests(unittest.TestCase):
    @staticmethod
    def token(value, page, y):
        return {
            "kind": "text",
            "text": value,
            "x": 78,
            "y": y,
            "sy": y + 10,
            "bbox": [78, y, 560, y + 20],
            "page": page,
        }

    def pages(self):
        return {
            1: [
                self.token("1. 第一题 ______", 1, 100),
                self.token("依次填入画横线部分最恰当的一项是：", 1, 140),
                self.token("A. 甲 乙", 1, 180),
                self.token("B. 丙 丁", 1, 220),
                self.token("C. 戊 己", 1, 260),
                self.token("D. 庚 辛", 1, 300),
            ],
            2: [
                self.token("第二题开头 ______ 结尾", 2, 100),
                self.token("依次填入画横线部分最恰当的一项是：", 2, 140),
                self.token("A. 甲 乙", 2, 180),
                self.token("B. 丙 丁", 2, 220),
                self.token("C. 戊 己", 2, 260),
                self.token("D. 庚 辛", 2, 300),
            ],
        }

    def test_missing_number_starts_the_question_listed_for_the_page(self):
        loaded = dataclasses.replace(
            profile_module.Profile.load(HUASHENG_PATH), question_starts={2: 2}
        )
        pages = self.pages()
        paper = extract.build(
            ("练习题", 1, 1, 2), None, loaded, lambda page: pages[page]
        )
        self.assertEqual([q["number"] for q in paper["questions"]], [1, 2])
        self.assertEqual(
            paper["questions"][1]["stem"][0]["text"], "第二题开头 ______ 结尾"
        )

    def test_unnumbered_page_without_a_listing_is_not_a_question(self):
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        pages = self.pages()
        paper = extract.build(
            ("练习题", 1, 1, 2), None, loaded, lambda page: pages[page]
        )
        self.assertEqual([q["number"] for q in paper["questions"]], [1])


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

    def test_ocr_run_line_parses_several_ranges(self):
        answers = emit.parse_answer_runs(
            ["1-5 BDCBD 6-10 ACBCD 11-15 DCDAB 16-20 CCBAA"], self.profile
        )
        self.assertEqual(answers[1], "B")
        self.assertEqual(answers[10], "D")
        self.assertEqual(answers[20], "A")

    def test_ocr_run_line_accepts_full_width_colon_and_tilde(self):
        answers = emit.parse_answer_runs(
            ["1～5：BCDCD 6～10: DCCCD 11～15：DCBAD"], self.profile
        )
        self.assertEqual(answers[5], "D")
        self.assertEqual(answers[10], "D")
        self.assertEqual(answers[15], "D")

    def test_answer_json_file_maps_each_set_to_its_letters(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "answers.json"
            path.write_text(
                json.dumps(
                    {"练习题": {"1": "ADDBACDCABDBBCCBBADD", "2": "DDACB"}},
                    ensure_ascii=False,
                ),
                encoding="utf-8",
            )
            answers = emit.parse_answers(path, self.profile)
        self.assertEqual(answers[("练习题", 1)][1], "A")
        self.assertEqual(answers[("练习题", 1)][20], "D")
        self.assertEqual(answers[("练习题", 2)][5], "B")

    def test_answer_json_file_groups_letters_in_fives(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "answers.json"
            path.write_text(
                json.dumps({"练习题": {"1": "ADDBA CDCAB"}}, ensure_ascii=False),
                encoding="utf-8",
            )
            answers = emit.parse_answers(path, self.profile)
        self.assertEqual(answers[("练习题", 1)][6], "C")

    def test_answer_json_file_rejects_letters_outside_the_labels(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "answers.json"
            path.write_text(
                json.dumps({"练习题": {"1": "ADDBA CDCAB EBBCC"}}, ensure_ascii=False),
                encoding="utf-8",
            )
            with self.assertRaises(ValueError):
                emit.parse_answers(path, self.profile)

    def test_ocr_split_range_and_letters(self):
        answers = emit.parse_answer_runs(["1-5", "BDCBD"], self.profile)
        self.assertEqual(answers[5], "D")

    def test_ocr_answer_key_split_into_blocks(self):
        answers = emit.parse_answer_runs(
            [
                "1~5 BCBBA",
                "/ 6~10",
                "ACACB / 11~15 DABBD",
                "/  16~20",
                "DCCBD",
            ],
            self.profile,
        )
        self.assertEqual(len(answers), 20)
        self.assertEqual(
            "".join(answers[number] for number in range(1, 21)),
            "BCBBAACACBDABBDDCCBD",
        )

    def test_ocr_answer_key_range_at_end_of_line(self):
        answers = emit.parse_answer_runs(
            ["1~5 DAAAA / 6~10 ACABD / 11~15 BCBDB / 16~20", "CBCBA"],
            self.profile,
        )
        self.assertEqual(len(answers), 20)
        self.assertEqual(answers[16], "C")
        self.assertEqual(answers[20], "A")

    def test_ocr_range_after_separator_and_letters_on_next_line(self):
        answers = emit.parse_answer_runs(
            ["1~5 DBDCD / 6~10 CACDB / 11~15 CBCAD", "/  16~20", "CCBAA"],
            self.profile,
        )
        self.assertEqual(answers[16], "C")
        self.assertEqual(answers[20], "A")


class AnswerSectionTests(unittest.TestCase):
    def test_section_stops_at_the_next_heading(self):
        lines = [
            "参考答案",
            "逻辑填空",
            "第一组",
            "1～5：BDCBD 6～10：DCCCD",
            "第二组",
            "1～5：CAABC",
        ]
        self.assertEqual(
            emit.section_lines(lines, "第一组", {"第一组", "第二组"}),
            ["1～5：BDCBD 6～10：DCCCD"],
        )

    def test_section_heading_ignores_width_and_spacing(self):
        lines = ["词句理解题＆细节判断题", "1～5：DADCB"]
        self.assertEqual(
            emit.section_lines(
                lines, "词句理解题&细节判断题", {"词句理解题&细节判断题"}
            ),
            ["1～5：DADCB"],
        )

    def test_missing_section_yields_no_lines(self):
        self.assertEqual(
            emit.section_lines(
                ["第一组", "1～5：BDCBD"], "第二组", {"第一组", "第二组"}
            ),
            [],
        )


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

    def test_logical_cloze_emits_category_and_literal_blanks(self):
        profile = dataclasses.replace(self.profile, module="言语理解与表达", category="逻辑填空")
        paper = self.paper()
        paper["questions"][0]["stem"][0]["text"] = "甲 ______。乙 ______。"
        markdown = emit.paper_markdown(paper, {("套题训练", 1): {1: "A"}}, profile)
        self.assertIn("module: 言语理解与表达\ncategory: 逻辑填空\n", markdown)
        self.assertNotRegex(markdown, r"(?<!\\)_{2,}")

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

    def test_v2_schema_declares_subject_and_objective_fields(self):
        loaded = dataclasses.replace(
            profile_module.Profile.load(PROFILE_PATH), schema="hyz-paper/v2"
        )
        markdown = emit.paper_markdown(
            self.paper(), {("套题训练", 1): {1: "A"}}, loaded
        )
        self.assertTrue(
            markdown.startswith("---\nschema: hyz-paper/v2\nsubject: xingce\n")
        )
        self.assertIn(
            "module: 资料分析\nanswer_type: objective\ngrading_mode: exact\n",
            markdown,
        )

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

    @staticmethod
    def sorting_paper(stem: str, options: dict[str, str]) -> dict:
        return {
            "kind": "语句排序题",
            "num": 1,
            "materials": [],
            "questions": [
                {
                    "number": 1,
                    "stem": [{"kind": "text", "text": stem, "x": 78}],
                    "options": [
                        {"label": label, "blocks": [{"kind": "text", "text": text}]}
                        for label, text in options.items()
                    ],
                }
            ],
        }

    def test_sorting_stem_recovers_a_marker_read_as_a_digit(self):
        loaded = profile_module.Profile.load(CHAOGE_PATH)
        paper = self.sorting_paper(
            "①甲。②乙。③丙。④丁。5戊。⑥己。",
            {
                "A": "⑤⑥②①④③",
                "B": "⑥②⑤④①③",
                "C": "⑥④①③⑤②",
                "D": "⑤④①③⑥②",
            },
        )
        markdown = emit.paper_markdown(
            paper, {("语句排序题", 1): {1: "A"}}, loaded
        )
        self.assertIn("④丁。⑤戊。⑥己。", markdown)

    def test_sorting_stem_recovers_a_bracketed_marker(self):
        loaded = profile_module.Profile.load(CHAOGE_PATH)
        paper = self.sorting_paper(
            "(1甲。②乙。③丙。④丁。⑤戊。⑥己。",
            {
                "A": "①⑥②⑤④③",
                "B": "⑥②⑤④①③",
                "C": "⑥④①③⑤②",
                "D": "①④③⑥⑤②",
            },
        )
        markdown = emit.paper_markdown(
            paper, {("语句排序题", 1): {1: "A"}}, loaded
        )
        self.assertIn("①甲。②乙。", markdown)
        self.assertNotIn("(1", markdown)

    def test_sorting_stem_drops_a_stray_bracket_before_a_marker(self):
        loaded = profile_module.Profile.load(CHAOGE_PATH)
        paper = self.sorting_paper(
            "(①甲。②乙。③丙。④丁。⑤戊。⑥己。",
            {
                "A": "⑤⑥②①④③",
                "B": "⑥②⑤④①③",
                "C": "⑥④①③⑤②",
                "D": "⑤④①③⑥②",
            },
        )
        markdown = emit.paper_markdown(
            paper, {("语句排序题", 1): {1: "A"}}, loaded
        )
        self.assertIn("### Stem\n\n①甲。②乙。", markdown)

    def test_marker_repair_keeps_quantity_digits(self):
        loaded = profile_module.Profile.load(CHAOGE_PATH)
        paper = self.sorting_paper(
            "①甲。5分钟。②乙。③丙。④丁。5戊。⑥己。",
            {
                "A": "⑤⑥②①④③",
                "B": "⑥②⑤④①③",
                "C": "⑥④①③⑤②",
                "D": "⑤④①③⑥②",
            },
        )
        markdown = emit.paper_markdown(
            paper, {("语句排序题", 1): {1: "A"}}, loaded
        )
        self.assertIn("5分钟。", markdown)
        self.assertIn("④丁。⑤戊。⑥己。", markdown)

    def test_glued_option_words_are_split_by_blank_count(self):
        paper = self.paper()
        question = paper["questions"][0]
        question["stem"] = [
            {"kind": "text", "text": "甲 ______ 乙 ______ 丙 ______", "x": 78}
        ]
        texts = {
            "A": "琳琅满目一脉相承 独一无二",
            "B": "汗牛充栋 口耳相传 无与伦比",
            "C": "浩如烟海 倾囊相授 包罗万象",
            "D": "卷帙浩繁 言传身教 价值连城",
        }
        question["options"] = [
            {"label": label, "blocks": [{"kind": "text", "text": text}]}
            for label, text in texts.items()
        ]
        paper["questions"].append(
            {
                "number": 2,
                "stem": [{"kind": "text", "text": "甲 ______", "x": 78}],
                "options": [
                    {"label": label, "blocks": [{"kind": "text", "text": text}]}
                    for label, text in {
                        "A": "琳琅满目",
                        "B": "一脉相承",
                        "C": "独一无二",
                        "D": "汗牛充栋",
                    }.items()
                ],
            }
        )
        self.assertEqual(emit.normalize_option_spacing([paper]), 1)
        self.assertEqual(
            question["options"][0]["blocks"][0]["text"],
            "琳琅满目 一脉相承 独一无二",
        )

    def test_glued_option_words_follow_sibling_lengths(self):
        paper = self.paper()
        question = paper["questions"][0]
        question["stem"] = [
            {"kind": "text", "text": "甲 ______ 乙 ______ 丙 ______", "x": 78}
        ]
        question["options"] = [
            {"label": label, "blocks": [{"kind": "text", "text": text}]}
            for label, text in {
                "A": "琳琅满目 一脉相承 独一无二",
                "B": "汗牛充栋 口耳相传 无与伦比",
                "C": "浩如烟海 倾囊相授 包罗万象",
                "D": "卷帙浩繁言传身教价值连城",
            }.items()
        ]
        self.assertEqual(emit.normalize_option_spacing([paper]), 1)
        self.assertEqual(
            question["options"][3]["blocks"][0]["text"],
            "卷帙浩繁 言传身教 价值连城",
        )

    def test_merged_blank_run_counts_as_one_blank(self):
        paper = self.paper()
        question = paper["questions"][0]
        question["stem"] = [
            {"kind": "text", "text": "甲 ______ 乙 ____________ ，丙", "x": 78}
        ]
        question["options"] = [
            {"label": label, "blocks": [{"kind": "text", "text": text}]}
            for label, text in {
                "A": "琳琅满目 一脉相承",
                "B": "汗牛充栋 口耳相传",
                "C": "浩如烟海 倾囊相授",
                "D": "卷帙浩繁言传身教",
            }.items()
        ]
        self.assertEqual(emit.normalize_option_spacing([paper]), 1)
        self.assertEqual(
            question["options"][3]["blocks"][0]["text"],
            "卷帙浩繁 言传身教",
        )

    def test_glued_option_words_split_on_uniform_sibling_length(self):
        paper = self.paper()
        question = paper["questions"][0]
        question["stem"] = [
            {"kind": "text", "text": "甲 ______ 乙 ______ 丙 ______", "x": 78}
        ]
        question["options"] = [
            {"label": label, "blocks": [{"kind": "text", "text": text}]}
            for label, text in {
                "A": "筚门圭窦 岸芷汀兰一裙一裳",
                "B": "兰艾同焚 粉墙黛瓦一丝一缕",
                "C": "钟鸣鼎食 曲径通幽一衣一带",
                "D": "桑枢瓮牖 小桥流水 一鞋一袜",
            }.items()
        ]
        self.assertEqual(emit.normalize_option_spacing([paper]), 3)
        self.assertEqual(
            question["options"][0]["blocks"][0]["text"],
            "筚门圭窦 岸芷汀兰 一裙一裳",
        )

    def test_adjacent_blanks_keep_their_separator(self):
        profile = profile_module.Profile.load(
            TOOL / "profiles" / "huasheng-2026-luojitiankong-600.json"
        )
        blocks = [
            {"kind": "text", "text": "缅怀党的 ______", "x": 78},
            {"kind": "text", "text": "______ 党的红色印记", "x": 78},
        ]
        rendered = emit.render_paragraphs(blocks, profile)
        self.assertEqual(rendered, "缅怀党的 ______ ______ 党的红色印记")

    def test_option_text_blocks_merge_before_splitting(self):
        paper = self.paper()
        question = paper["questions"][0]
        question["stem"] = [
            {"kind": "text", "text": "甲 ______ 乙 ______ 丙 ______", "x": 78}
        ]
        texts = {
            "A": [("魂牵梦绕 涵养 夹杂",)],
            "B": [("心驰神往 滋养 错杂",)],
            "C": [("流连忘返 浸润 掺杂",)],
            "D": [("铭心刻骨",), ("滋润 混杂",)],
        }
        question["options"] = [
            {
                "label": label,
                "blocks": [
                    {"kind": "text", "text": text, "ocr": True}
                    for (text,) in blocks
                ],
            }
            for label, blocks in texts.items()
        ]
        self.assertEqual(emit.normalize_option_spacing([paper]), 1)
        self.assertEqual(
            emit._option_text(question["options"][3]),
            "铭心刻骨 滋润 混杂",
        )
        self.assertEqual(
            emit.render_option(question["options"][3]),
            "- D. 铭心刻骨 滋润 混杂",
        )

    def test_shape_vote_uses_options_split_in_the_same_pass(self):
        paper = self.paper()
        question = paper["questions"][0]
        question["stem"] = [
            {"kind": "text", "text": "甲 ______ 乙 ______ 丙 ______", "x": 78}
        ]
        question["options"] = [
            {"label": label, "blocks": [{"kind": "text", "text": text}]}
            for label, text in {
                "A": "如数家珍一马当先 捕获",
                "B": "了如指掌一气呵成 挽留",
                "C": "心领神会一鼓作气 发挥",
                "D": "心中有数一以贯之 保存",
            }.items()
        ]
        paper["questions"].append(
            {
                "number": 2,
                "stem": [{"kind": "text", "text": "甲 ______", "x": 78}],
                "options": [
                    {"label": label, "blocks": [{"kind": "text", "text": text}]}
                    for label, text in {
                        "A": "如数家珍",
                        "B": "一马当先",
                        "C": "捕获",
                        "D": "如数家珍",
                    }.items()
                ],
            }
        )
        self.assertEqual(emit.normalize_option_spacing([paper]), 4)
        self.assertEqual(
            [emit._option_text(option) for option in question["options"]],
            [
                "如数家珍 一马当先 捕获",
                "了如指掌 一气呵成 挽留",
                "心领神会 一鼓作气 发挥",
                "心中有数 一以贯之 保存",
            ],
        )

    def test_glued_option_words_need_agreeing_siblings(self):
        paper = self.paper()
        question = paper["questions"][0]
        question["stem"] = [
            {"kind": "text", "text": "甲 ______ 乙 ______", "x": 78}
        ]
        question["options"] = [
            {"label": label, "blocks": [{"kind": "text", "text": text}]}
            for label, text in {
                "A": "琳琅满目 一脉相承",
                "B": "汗牛充 口耳相传",
                "C": "浩如烟海 倾囊相授",
                "D": "卷帙浩繁言传身教",
            }.items()
        ]
        # Siblings disagree on the word lengths, so keep the OCR text as-is.
        self.assertEqual(emit.normalize_option_spacing([paper]), 0)
        self.assertEqual(
            question["options"][3]["blocks"][0]["text"], "卷帙浩繁言传身教"
        )

    def test_option_lines_split_by_ocr_join_without_a_gap_when_squeezing(self):
        paper = self.paper()
        option = paper["questions"][0]["options"][0]
        option["blocks"] = [
            {"kind": "text", "text": "大白鼠在“恐怖箱”中呆立不动的", "ocr": True},
            {"kind": "text", "text": "时间和之前一样", "ocr": True},
        ]
        emit.normalize_option_spacing([paper], squeeze_han_spaces=True)
        self.assertEqual(
            option["blocks"][0]["text"], "大白鼠在“恐怖箱”中呆立不动的时间和之前一样"
        )

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


class FakeOcrProfile:
    top_band = 85
    ocr_squeeze_han_spaces = False

    def correct(self, value):
        return value

    def is_noise(self, value, bbox):
        return not value.strip()


class SqueezingOcrProfile(FakeOcrProfile):
    ocr_squeeze_han_spaces = True


@unittest.skipIf(ocr_layout is None, "PIL or PyMuPDF is not installed")
class OrderTests(unittest.TestCase):
    @unittest.skipIf(extract is None, "PyMuPDF is not installed")
    def test_two_option_columns_read_left_to_right(self):
        def option(text, x, sy):
            return {
                "kind": "text",
                "text": text,
                "page": 1,
                "x": x,
                "sy": sy,
                "bbox": [x, sy - 5, x + 40, sy + 5],
                "ocr": True,
            }

        tokens = [
            option("B. 相得益彰", 313.9, 126.8),
            option("A. 张弛有度", 79.5, 127.3),
            option("D. 劳逸结合", 314.2, 145.5),
            option("C. 一举两得", 79.5, 145.8),
        ]
        ordered = extract.order_tokens(tokens, profile_module.Profile.load(PROFILE_PATH))
        self.assertEqual(
            [token["text"][0] for token in ordered], ["A", "B", "C", "D"]
        )

    @unittest.skipIf(extract is None, "PyMuPDF is not installed")
    def test_skewed_line_stays_together(self):
        def line(text, x, top, bottom):
            return {
                "kind": "text",
                "text": text,
                "page": 1,
                "x": x,
                "sy": (top + bottom) / 2,
                "bbox": [x, top, x + 300, bottom],
                "ocr": True,
            }

        # The scan skews a line by seven points; the blocks still overlap.
        tokens = [
            line("人的注意力范围；而积极情绪则相反", 285, 677, 727),
            line("唤起会", 152, 701, 732),
            line("造力，灵活且包容", 155, 723, 769),
        ]
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        ordered = extract.order_tokens(tokens, loaded)
        self.assertEqual(
            [token["text"] for token in ordered],
            ["唤起会", "人的注意力范围；而积极情绪则相反", "造力，灵活且包容"],
        )

    @unittest.skipIf(extract is None, "PyMuPDF is not installed")
    def test_option_lines_follow_their_labels(self):
        def option(text, x, sy):
            return {
                "kind": "text",
                "text": text,
                "page": 1,
                "x": x,
                "sy": sy,
                "bbox": [x, sy - 5, x + 40, sy + 5],
                "ocr": True,
            }

        # The right column sits 6.5 points above the left one on this page.
        tokens = [
            option("B. 历久弥新 增长 融合", 629.0, 122.6),
            option("A. 久久为功 提升 创新", 142.0, 129.1),
            option("D. 厚积薄发 普及 应用", 630.0, 141.6),
            option("C. 薪火相传 更迭 介入", 143.0, 147.8),
        ]
        loaded = profile_module.Profile.load(HUASHENG_PATH)
        ordered = extract.order_tokens(tokens, loaded)
        self.assertEqual(
            [token["text"][0] for token in ordered], ["A", "B", "C", "D"]
        )


class ExplainTests(unittest.TestCase):
    class FakeProfile:
        ask_re = re.compile(r"依次填入画横线部分最恰当的一项是[：:]?")

    def setUp(self):
        self._sy = 0

    def line(self, value, x=175.0, sy=None, page=1):
        if sy is None:
            self._sy += 10
            sy = self._sy
        return {"text": value, "x": x, "sy": sy, "page": page}

    def test_header_does_not_match_numbered_examples(self):
        lines = [
            self.line("14.(2019年浙江省考 55%)"),
            self.line("1.穿越古今，和平发展", x=190.0),
            self.line("16.（2022年山西省事业单位 42%）"),
        ]
        blocks = explain.split_blocks(lines)
        self.assertEqual([block["number"] for block in blocks], [14, 16])
        self.assertEqual(len(blocks[0]["lines"]), 1)

    def test_parse_drops_repeated_stem_and_options(self):
        lines = [
            self.line("14.(2019年浙江省考 55%)"),
            self.line("近年来，各类互联网平台纷纷推出会员产品。", x=190.0),
            self.line("依次填入画横线部分最恰当的一项是："),
            self.line("A. 层出不穷 同舟共济", x=195.0),
            self.line("B. 接踵而至 团结一致", x=363.0),
            self.line("C. 任重道远 顽强拼搏", x=195.0),
            self.line("D. 比比皆是 众志成城", x=363.0),
            self.line("第一步：第一空描述平台套路很多，", x=197.0),
            self.line("应体现数量多、接连不断之意。"),
            self.line("综上，这道题的正确答案为A选项。", x=197.0),
        ]
        parsed = explain.parse(lines, self.FakeProfile())
        self.assertEqual(list(parsed), [14])
        self.assertEqual(parsed[14]["answer"], "A")
        self.assertTrue(parsed[14]["text"].startswith("第一步：第一空描述平台套路很多，应体现"))
        self.assertNotIn("层出不穷", parsed[14]["text"])

    def test_parse_strips_answer_and_explanation_labels(self):
        lines = [
            self.line("1. (2025年全国事业单位联考 54%)"),
            self.line("依次填入画横线部分最恰当的一项是："),
            self.line("A. 甲", x=195.0),
            self.line("B. 乙", x=363.0),
            self.line("【参考答案】D"),
            self.line("【实战解析】本题考查语境搭配。", x=197.0),
            self.line("第一空需要搭配。", x=175.0),
            self.line("第一步：分析第一空。", x=197.0),
        ]
        parsed = explain.parse(lines, self.FakeProfile())
        self.assertEqual(parsed[1]["answer"], "D")
        self.assertEqual(
            parsed[1]["text"],
            "本题考查语境搭配。第一空需要搭配。\n\n第一步：分析第一空。",
        )

    def test_parse_strips_reference_answer_with_a_trailing_blank(self):
        lines = [
            self.line("1. (2025年全国事业单位联考 54%)"),
            self.line("依次填入画横线部分最恰当的一项是："),
            self.line("A. 甲", x=195.0),
            self.line("B. 乙", x=363.0),
            self.line("【参考答案】B ______"),
            self.line("本题考查语境搭配。", x=197.0),
        ]
        parsed = explain.parse(lines, self.FakeProfile())
        self.assertEqual(parsed[1]["answer"], "B")
        self.assertNotIn("参考答案", parsed[1]["text"])

    def test_parse_keeps_explanation_continuing_on_the_next_page(self):
        lines = [
            self.line("14.(2019年浙江省考 55%)"),
            self.line("依次填入画横线部分最恰当的一项是："),
            self.line("A. 甲", x=195.0),
            self.line("B. 乙", x=363.0),
            self.line("第一步：第一空", x=197.0),
            self.line("继续上一页的说明。", page=2),
            self.line("15.(2019年浙江省考 55%)", page=2),
            self.line("依次填入画横线部分最恰当的一项是：", page=2),
            self.line("A. 甲", x=195.0, page=2),
            self.line("B. 乙", x=363.0, page=2),
            self.line("第二步：第二空", x=197.0, page=2),
        ]
        parsed = explain.parse(lines, self.FakeProfile())
        self.assertEqual(parsed[14]["text"], "第一步：第一空继续上一页的说明。")
        self.assertEqual(parsed[15]["text"], "第二步：第二空")


class OcrLayoutTests(unittest.TestCase):
    @staticmethod
    def png(draw_rule=True):
        image = Image.new("L", (500, 200), 255)
        if draw_rule:
            draw = ImageDraw.Draw(image)
            draw.rectangle([150, 100, 250, 102], fill=0)
            draw.rectangle([40, 50, 60, 52], fill=0)
        output = BytesIO()
        image.save(output, format="PNG")
        return output.getvalue()

    def test_image_blanks_find_rule_and_ignore_short_stroke(self):
        boxes = ocr_layout.image_blanks(self.png(), [], 220)
        self.assertEqual(len(boxes), 1)
        self.assertAlmostEqual(boxes[0][0], 150)
        self.assertAlmostEqual(boxes[0][1], 100)

    def test_overlapping_rules_merge_into_one_blank(self):
        image = Image.new("L", (500, 200), 255)
        draw = ImageDraw.Draw(image)
        draw.rectangle([300, 100, 380, 101], fill=0)
        draw.rectangle([100, 100, 180, 101], fill=0)
        draw.rectangle([300, 101, 360, 102], fill=0)
        draw.rectangle([100, 101, 140, 102], fill=0)
        output = BytesIO()
        image.save(output, format="PNG")
        boxes = ocr_layout.image_blanks(output.getvalue(), [], 220)
        self.assertEqual(len(boxes), 2)

    def test_mask_blanks_erases_the_rule(self):
        boxes = ocr_layout.image_blanks(self.png(), [], 220)
        masked = Image.open(BytesIO(ocr_layout.mask_blanks(self.png(), boxes)))
        self.assertEqual(masked.getpixel((200, 101)), (255, 255, 255))
        self.assertEqual(masked.getpixel((200, 80)), (255, 255, 255))

    def test_blank_between_blocks_appends_to_left_token(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [
                {"text": "开始", "box": [100, 100, 300, 130]},
                {"text": "微信", "box": [500, 100, 700, 130]},
            ],
            [[380, 128, 470, 130]],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), page.blanks)
        self.assertEqual([token["text"] for token in tokens], ["开始 ______", "微信"])

    def test_two_blanks_on_one_line_attach_to_both_ends(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [{"text": "地壳", "box": [260, 100, 500, 130]}],
            [[100, 128, 200, 130], [600, 128, 700, 130]],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), page.blanks)
        self.assertEqual([token["text"] for token in tokens], ["______ 地壳 ______"])

    def test_blank_on_its_own_line_appends_to_the_line_above(self):
        page = OcrPage(
            1,
            72,
            800,
            300,
            [
                {"text": "已经与此形成内在", "box": [100, 100, 700, 130]},
                {"text": "依次填入画横线部分最恰当的一项是：", "box": [100, 160, 600, 190]},
            ],
            [[100, 150, 200, 152]],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), page.blanks)
        self.assertEqual(tokens[0]["text"], "已经与此形成内在 ______")

    def test_han_gap_from_a_wrapped_ocr_line_is_squeezed_when_enabled(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [{"text": "大白鼠呆立不动的 时间和之前一样", "box": [100, 100, 700, 130]}],
            [],
        )
        tokens = ocr_layout.tokens(1, page, SqueezingOcrProfile(), [])
        self.assertEqual(tokens[0]["text"], "大白鼠呆立不动的时间和之前一样")

    def test_han_gap_stays_when_the_profile_keeps_option_word_spaces(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [{"text": "琳琅满目 一脉相承", "box": [100, 100, 700, 130]}],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        self.assertEqual(tokens[0]["text"], "琳琅满目 一脉相承")

    def test_rule_inside_block_is_an_underline_not_a_blank(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [{"text": "验证第一空", "box": [100, 100, 600, 130]}],
            [[300, 128, 400, 130]],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), page.blanks)
        self.assertEqual(tokens[0]["text"], "验证第一空")

    def test_line_final_zero_is_read_as_a_full_stop(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [
                {"text": "成长的美好", "box": [100, 100, 600, 130]},
                {"text": "0", "box": [612, 118, 620, 130]},
            ],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        self.assertEqual([token["text"] for token in tokens], ["成长的美好", "。"])

    def test_zero_after_a_full_stop_is_dropped(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [
                {"text": "成长的美好。", "box": [100, 100, 600, 130]},
                {"text": "o", "box": [612, 118, 620, 130]},
            ],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        self.assertEqual([token["text"] for token in tokens], ["成长的美好。"])

    def test_lone_zero_is_dropped(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [{"text": "0", "box": [612, 118, 620, 130]}],
            [],
        )
        self.assertEqual(ocr_layout.tokens(1, page, FakeOcrProfile(), []), [])

    def test_same_row_blocks_read_left_to_right(self):
        page = OcrPage(
            1,
            72,
            800,
            200,
            [
                {"text": "B. 隐蔽", "box": [500, 105, 700, 135]},
                {"text": "A. 无私", "box": [100, 100, 300, 130]},
            ],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        self.assertEqual([token["text"] for token in tokens], ["A. 无私", "B. 隐蔽"])

    def test_badge_ranges_ignore_toc_and_fill_missing_badges(self):
        ranges = ocr_layout.badge_ranges(
            {
                2: {1, 2, 3},
                4: {1},
                5: set(),
                6: {1},
                8: {2},
                13: {3},
            },
            20,
        )
        self.assertEqual(ranges, [(1, 4, 7), (2, 8, 12), (3, 13, 20)])

    def test_sidebar_is_read_after_the_main_column(self):
        page = OcrPage(
            1,
            72,
            800,
            300,
            [
                {"text": "批注", "box": [50, 100, 120, 130]},
                {"text": "补注", "box": [50, 150, 130, 180]},
                {"text": "旁批", "box": [50, 200, 120, 230]},
                {"text": "正文第一行内容足够宽，可以识别为主栏文本。", "box": [200, 100, 750, 130]},
                {"text": "正文第二行内容同样足够宽，继续识别主栏。", "box": [200, 150, 750, 180]},
                {"text": "正文第三行内容仍然足够宽，保持主栏布局。", "box": [200, 200, 750, 230]},
            ],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        self.assertEqual(tokens[0]["column"], "main")
        self.assertEqual(tokens[-1]["column"], "side")

    def test_sidebar_with_inflated_boxes_uses_the_start_gap(self):
        # Scan OCR can stretch a main-column box over the gutter. The sidebar
        # still starts far to the left of every main-column line.
        page = OcrPage(
            1,
            72,
            800,
            600,
            [
                {"text": "①花生批注：", "box": [50, 350, 120, 380]},
                {"text": "补充辨析：", "box": [50, 390, 120, 420]},
                {"text": "A选项的词义。", "box": [50, 430, 120, 460]},
                {"text": "B选项的词义。", "box": [50, 470, 120, 500]},
                {"text": "C选项的词义。", "box": [50, 510, 120, 540]},
                {"text": "D选项的词义。", "box": [50, 550, 120, 580]},
                {"text": "正文第一行内容足够宽，可以识别为主栏文本。", "box": [120, 350, 520, 380]},
                {"text": "正文第二行内容同样足够宽，继续识别主栏。", "box": [120, 390, 520, 420]},
                {"text": "正文第三行内容仍然足够宽，保持主栏布局。", "box": [120, 430, 520, 460]},
                {"text": "正文第四行内容继续补足主栏的识别样本。", "box": [120, 470, 520, 500]},
                {"text": "正文第五行内容继续补足主栏的识别样本。", "box": [120, 510, 520, 540]},
                {"text": "正文第六行内容继续补足主栏的识别样本。", "box": [120, 550, 520, 580]},
            ],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        texts = {token["text"]: token["column"] for token in tokens}
        self.assertEqual(texts["①花生批注："], "side")
        self.assertEqual(texts["D选项的词义。"], "side")
        self.assertEqual(texts["正文第六行内容继续补足主栏的识别样本。"], "main")

    def test_sidebar_confined_to_the_top_is_still_a_sidebar(self):
        # The answer-key table and the body below the notes can leave the
        # gutter sample with main-column lines only.
        page = OcrPage(
            1,
            72,
            800,
            600,
            [
                {"text": "花生批注：", "box": [50, 100, 120, 130]},
                {"text": "补充辨析：", "box": [50, 130, 120, 160]},
                {"text": "一部书的篇幅很长。", "box": [50, 160, 150, 190]},
                {"text": "正文第一行内容足够宽，可以识别为主栏文本。", "box": [172, 100, 750, 130]},
                {"text": "正文第二行内容同样足够宽，继续识别主栏。", "box": [172, 130, 750, 160]},
                {"text": "正文第三行内容仍然足够宽，保持主栏布局。", "box": [172, 160, 750, 190]},
                {"text": "正文第四行内容继续补足主栏的识别样本。", "box": [172, 350, 750, 380]},
                {"text": "正文第五行内容继续补足主栏的识别样本。", "box": [172, 400, 750, 430]},
                {"text": "正文第六行内容继续补足主栏的识别样本。", "box": [172, 450, 750, 480]},
                {"text": "正文第七行内容继续补足主栏的识别样本。", "box": [172, 500, 750, 530]},
                {"text": "正文第八行内容继续补足主栏的识别样本。", "box": [172, 550, 750, 580]},
                {"text": "正文第九行内容继续补足主栏的识别样本。", "box": [172, 580, 750, 600]},
            ],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        texts = {token["text"]: token["column"] for token in tokens}
        self.assertEqual(texts["花生批注："], "side")
        self.assertEqual(texts["一部书的篇幅很长。"], "side")
        self.assertEqual(texts["正文第九行内容继续补足主栏的识别样本。"], "main")

    def test_sidebar_edge_that_reaches_into_the_gutter_stays_side(self):
        # The gutter sample skips the top of the page, so a note there can end
        # one point past the gutter start. It still belongs to the sidebar.
        page = OcrPage(
            1,
            72,
            800,
            600,
            [
                {"text": "批注", "box": [50, 100, 126, 130]},
                {"text": "易混成语补充", "box": [50, 400, 124, 430]},
                {"text": "一部书的篇幅很长。", "box": [50, 450, 124, 480]},
                {"text": "形容书籍很多或", "box": [50, 500, 124, 530]},
                {"text": "正文第一行内容足够宽，可以识别为主栏文本。", "box": [200, 400, 750, 430]},
                {"text": "正文第二行内容同样足够宽，继续识别主栏。", "box": [200, 450, 750, 480]},
                {"text": "正文第三行内容仍然足够宽，保持主栏布局。", "box": [200, 500, 750, 530]},
                {"text": "正文第四行内容继续补足主栏的识别样本。", "box": [200, 550, 750, 580]},
            ],
            [],
        )
        tokens = ocr_layout.tokens(1, page, FakeOcrProfile(), [])
        texts = {token["text"]: token["column"] for token in tokens}
        self.assertEqual(texts["批注"], "side")
        self.assertEqual(texts["易混成语补充"], "side")
        self.assertEqual(texts["正文第一行内容足够宽，可以识别为主栏文本。"], "main")


if __name__ == "__main__":
    unittest.main()
