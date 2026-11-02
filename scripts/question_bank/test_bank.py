import unittest

from bank import parse_paper, render_paper


def source(body, year=2008):
    return f"""---
考试: 国考
地区: 国考
年份: {year}
科目: 行测
卷型: 统考
---
# 示例卷
{body}
"""


class PaperTests(unittest.TestCase):
    def test_shared_material_stops_at_explicit_module_boundary(self):
        body = """| 题型模块 | 题量 | 题号 |
| 言语理解与表达 | 2 | 1–2 |
| 数量关系 | 1 | 3–3 |
## 材料 1
共享原文与表格。
"""
        for n in (1, 2, 3):
            body += f"""### 第 {n} 题
题{n}
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 {n} 题 · 答案与解析</summary>
**参考答案（粉笔题库参考内容）**
A
**解析**
解释{n}
</details>
"""
        paper = parse_paper(source(body))
        self.assertEqual(paper.materials[0].content, "共享原文与表格。")
        self.assertEqual(
            [q.metadata["materials"] for q in paper.questions], [["M1"], ["M1"], []]
        )
        self.assertEqual(render_paper(paper).count("共享原文与表格。"), 1)

    def test_unknown_material_boundary_is_distinct_from_unset_boundary(self):
        body = """| 题型模块 | 题量 | 题号 |
| 综合分析能力 | 1 | 1–1 |
| 判断推理 | 1 | 2–2 |
## 材料 1
共享原文。
"""
        for number in (1, 2):
            body += f"""### 第 {number} 题
题干。
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 {number} 题 · 答案与解析</summary>
**答案**：A
</details>
"""
        paper = parse_paper(source(body))
        self.assertIsNone(paper.questions[0].metadata["module"])
        self.assertEqual(
            [q.metadata["materials"] for q in paper.questions], [["M1"], []]
        )

    def test_source_placeholder_is_kept_but_not_ready(self):
        paper = parse_paper(
            source("""### 第 4 题
本题暂未收录
<details><summary>第 4 题 · 答案与解析</summary>
**答案状态：unknown（来源未提供）**
</details>
""")
        )
        q = paper.questions[0]
        self.assertIn("暂未收录", q.stem)
        self.assertEqual(q.answer, "")
        self.assertIn("missing_stem", q.metadata["issues"])
        self.assertIn("missing_options", q.metadata["issues"])
        self.assertIn("missing_answer", q.metadata["issues"])

    def test_letter_enumeration_in_stem_is_not_an_option(self):
        paper = parse_paper(
            source("""### 第 9 题
A、B、C、D四个球队比赛。
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 9 题 · 答案与解析</summary>
**答案**：A
</details>
""")
        )
        self.assertEqual(paper.questions[0].stem, "A、B、C、D四个球队比赛。")

    def test_answer_qualification_is_preserved_separately(self):
        paper = parse_paper(
            source("""### 第 47 题
题干
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 47 题 · 答案与解析</summary>
**答案**：A

**答案性质**：独立校算参考答案（非官方）。
校算说明全文。
</details>
""")
        )
        self.assertEqual(paper.questions[0].answer, "A")
        rendered = render_paper(paper)
        self.assertIn("校算说明全文。", rendered)
        self.assertIn("非官方", rendered)

    def test_unverified_source_order_survives_in_machine_readable_metadata(self):
        text = source("""### 第 1 题
题干
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 1 题 · 答案与解析</summary>
**答案**：A
</details>
""").replace("科目: 行测", "科目: 行测\n题号口径: 按练习卡顺序编号，待与原卷核对")
        paper = parse_paper(text)
        self.assertIn("order_unverified", paper.questions[0].metadata["issues"])

    def test_legacy_multiselect_uses_title_category_without_qid(self):
        paper = parse_paper(
            source("""## 第 8 题　<sub>qid 123 · 逻辑判断</sub>
选择正确的项。
- **A**. 甲
- **B**. 乙
- **C**. 丙
- **D**. 丁
<details>
<summary>第 8 题 · 答案与解析</summary>
**答案**：AB
**官方解析**
完整原始解析。
</details>
""")
        )
        q = paper.questions[0]
        self.assertEqual(q.answer, "AB")
        self.assertEqual(q.metadata["category"], "逻辑判断")
        self.assertEqual(q.metadata["module"], "判断推理")
        self.assertIn("- A. 甲", render_paper(paper))
        self.assertNotIn("qid", render_paper(paper))

    def test_collected_question_preserves_answer_explanation_and_topics(self):
        paper = parse_paper(
            source("""### 第 26 题
题干含 **强调** 与 $x$。
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 26 题 · 答案与解析</summary>
**参考答案（粉笔题库参考内容）**
B
**解析**
原始解析，不改写。
**考点**：关联词-转折
**来源**：原卷第26题
</details>
""")
        )
        question = paper.questions[0]
        self.assertEqual(question.number, 26)
        self.assertEqual(question.answer, "B")
        self.assertEqual(question.explanation, "原始解析，不改写。")
        self.assertEqual(question.metadata["topics"], ["关联词-转折"])
        rendered = render_paper(paper)
        self.assertIn("## Question 26", rendered)
        self.assertIn("题干含 **强调** 与 $x$。", rendered)
        self.assertNotIn("粉笔", rendered)
        self.assertNotIn("source_url:", rendered)

    def test_question_type_module_is_normalized_without_changing_topics(self):
        paper = parse_paper(
            source("""| 题型模块 | 题量 | 题号 |
| 图形推理 | 1 | 1–1 |
### 第 1 题
题干
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 1 题 · 答案与解析</summary>
**答案**：A
**考点**：数量规律-线、属性规律-对称性
</details>
""")
        )
        question = paper.questions[0]
        self.assertEqual(question.metadata["module"], "判断推理")
        self.assertEqual(question.metadata["category"], "图形推理")
        self.assertEqual(
            question.metadata["topics"], ["数量规律-线", "属性规律-对称性"]
        )
        self.assertEqual(question.provenance["raw_module"], "图形推理")

    def test_explicit_question_type_overrides_topic_inference(self):
        paper = parse_paper(
            source("""| 题型模块 | 题量 | 题号 |
| 判断推理 | 1 | 1–1 |
## 第 1 题　<sub>逻辑关系-对应关系</sub>
题干
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 1 题 · 答案与解析</summary>
**答案**：A
**考点**：数量规律-线
</details>
""")
        )
        question = paper.questions[0]
        self.assertEqual(question.metadata["module"], "判断推理")
        self.assertEqual(question.metadata["category"], "类比推理")
        self.assertEqual(question.provenance["category_basis"], "explicit_category")

    def test_explicit_module_and_type_conflict_is_left_for_review(self):
        paper = parse_paper(
            source("""| 题型模块 | 题量 | 题号 |
| 言语理解与表达 | 1 | 1–1 |
## 第 1 题　<sub>图形推理</sub>
题干
A. 甲
B. 乙
C. 丙
D. 丁
<details><summary>第 1 题 · 答案与解析</summary>
**答案**：A
**考点**：数量规律-线
</details>
""")
        )
        question = paper.questions[0]
        self.assertEqual(question.metadata["module"], "言语理解与表达")
        self.assertIsNone(question.metadata["category"])
        self.assertIn(
            "explicit_category_module_mismatch",
            question.provenance["classification_conflicts"],
        )


if __name__ == "__main__":
    unittest.main()
