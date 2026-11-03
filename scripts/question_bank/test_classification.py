import unittest

from classification import normalize_classification


class ClassificationTests(unittest.TestCase):
    def test_structure_corrects_misleading_topics_across_real_yunnan_questions(self):
        from pathlib import Path
        import re
        import yaml

        path = (
            Path(__file__).parents[2]
            / "question_bank/papers/省考/云南/2026/行测-统考.md"
        )
        if not path.exists():
            self.skipTest("local bank unavailable")
        text = path.read_text()
        for number in range(68, 85):
            with self.subTest(number=number):
                chunk = re.search(
                    rf"(?ms)^## Question {number}\s*$(.*?)(?=^## Question |\Z)", text
                )[1]
                metadata = yaml.safe_load(
                    re.search(r"```yaml\n(.*?)\n```", chunk, re.S)[1]
                )
                stem = re.search(r"### Stem\n(.*?)\n### Options", chunk, re.S)[
                    1
                ].strip()
                options = re.search(r"### Options\n(.*?)\n### Answer", chunk, re.S)[
                    1
                ].strip()
                result = normalize_classification(
                    metadata["module"],
                    None,
                    metadata["topics"],
                    stem=stem,
                    options=options,
                )
                self.assertEqual(
                    result["category"],
                    "定义判断" if number <= 75 else "类比推理",
                )

    def test_combined_image_prompt_and_options_support_graphic_reasoning(self):
        result = normalize_classification(
            "判断推理",
            None,
            [],
            stem="从所给的四个选项中，选择最合适的一个填入问号处，使之呈现一定规律性。\n![](a.png)",
            options="- A. ![](a.png)\n\n- B. ![](b.png)\n\n- C. ![](c.png)\n\n- D. ![](d.png)",
        )
        self.assertEqual(result["category"], "图形推理")
        analogy = normalize_classification(
            "判断推理",
            None,
            [],
            stem="图中词语之间存在关系：![](a.png)",
            options="- A. ![](a.png)\n\n- B. ![](b.png)\n\n- C. ![](c.png)\n\n- D. ![](d.png)",
        )
        self.assertIsNone(analogy["category"])

    def test_if_true_argument_pattern_supports_logic_only_in_judgment_module(self):
        stem = "研究者认为某个现象由实验方案导致，但另一种原因尚未排除。以下哪项如果为真，最能削弱上述结论？"
        options = "- A. 其他研究\n\n- B. 复测结果\n\n- C. 文献记载\n\n- D. 无关数据"
        self.assertEqual(
            normalize_classification("判断推理", None, [], stem=stem, options=options)[
                "category"
            ],
            "逻辑判断",
        )

    def test_growth_of_average_keeps_specific_topic_pending_review(self):
        result = normalize_classification(
            "资料分析",
            None,
            ["一般增长率"],
            stem="与上个月相比，2月份平均工资增长速度最快的车间是：",
            options="- A. 第一车间\n\n- B. 第二车间\n\n- C. 第三车间\n\n- D. 第四车间",
            has_material=True,
        )
        self.assertEqual(result["category"], "增长")
        self.assertIn("material_structure_topic_disagreement", result["conflicts"])

    def test_numeric_values_inside_word_problem_do_not_trigger_number_sequence(self):
        result = normalize_classification(
            "数量关系",
            None,
            [],
            stem="从21，22，23，24，25，26，27，28，29中任意选出三个数，使他们和为奇数，可以有多少种选法？",
            options="- A. 10\n\n- B. 20\n\n- C. 30\n\n- D. 40",
        )
        self.assertIsNone(result["category"])

        result = normalize_classification(
            "判断推理",
            None,
            [],
            stem="小甲：你的方法有用吗？\n\n小乙：我还不知道。以下哪项与小乙的论证方式最相似？",
            options="- A. 小李：我不确定\n\n- B. 小王：请看\n\n- C. 小刘：不会\n\n- D. 小张：这不对",
        )
        self.assertIsNone(result["category"])

    def test_ambiguous_topics_cannot_determine_type(self):
        for topic in ("方式目的", "原因结果", "大前提"):
            with self.subTest(topic=topic):
                self.assertIsNone(
                    normalize_classification("判断推理", None, [topic])["category"]
                )

    def test_conflicting_strong_evidence_quarantines_existing_type(self):
        result = normalize_classification(
            "判断推理",
            "类比推理",
            ["方式目的"],
            stem="语法惊异是指刻意违反语法规则以触发认知冲突与情感共鸣的语言创新机制。根据上述定义，下列不属于语法惊异的是？",
            options="- A. 某人修辞\n\n- B. 某人造句\n\n- C. 某人用词\n\n- D. 某人说话",
            raw_category="类比推理",
        )
        self.assertIsNone(result["category"])
        self.assertIn("strong_evidence_conflict", result["conflicts"])

    def test_module_that_is_a_question_type_is_moved_to_category(self):
        result = normalize_classification("图形推理", None, ["数量规律-线"])
        self.assertEqual(result["module"], "判断推理")
        self.assertEqual(result["category"], "图形推理")

    def test_explicit_category_takes_priority_over_topic_family(self):
        result = normalize_classification(
            "判断推理", None, ["数量规律-线"], raw_category="逻辑判断"
        )
        self.assertEqual(result["category"], "逻辑判断")
        self.assertEqual(result["conflicts"], [])

    def test_common_knowledge_module_is_not_changed_by_political_topics(self):
        result = normalize_classification("常识判断", None, ["新思想总论"])
        self.assertEqual(result["module"], "常识判断")
        self.assertEqual(result["category"], "政治常识")

    def test_political_theory_remains_separate_and_uses_political_category(self):
        result = normalize_classification("政治理论", None, ["新思想总论"])
        self.assertEqual(result["module"], "政治理论")
        self.assertEqual(result["category"], "新思想")

    def test_unique_topics_from_one_family_fill_category(self):
        result = normalize_classification(
            "判断推理", None, ["数量规律-线", "属性规律-对称性"]
        )
        self.assertEqual(result["category"], "图形推理")

    def test_audited_judgment_logic_topics_fill_logic_category(self):
        for topic in ("集合推理", "削弱选非题", "他因削弱", "加强题型"):
            with self.subTest(topic=topic):
                result = normalize_classification("判断推理", None, [topic])
                self.assertEqual(result["category"], "逻辑判断")

    def test_ambiguous_story_topic_remains_unclassified_without_structure(self):
        result = normalize_classification("判断推理", None, ["故事类"])
        self.assertIsNone(result["category"])

    def test_audited_analogy_topic_uses_analogy_category(self):
        result = normalize_classification(
            "判断推理", None, ["拆分思维", "逻辑关系-对应关系"]
        )
        self.assertEqual(result["category"], "类比推理")
        self.assertEqual(result["conflicts"], [])

    def test_audited_graphic_reasoning_topics_use_graphic_category(self):
        result = normalize_classification(
            "判断推理", None, ["数量规律-线", "文字/字母/数字类"]
        )
        self.assertEqual(result["category"], "图形推理")
        self.assertEqual(result["conflicts"], [])

    def test_audited_sentence_topic_fills_fragment_reading(self):
        result = normalize_classification("言语理解与表达", None, ["句子"])
        self.assertEqual(result["category"], "片段阅读")

    def test_audited_quantity_word_problem_topics_fill_math_category(self):
        for topic in (
            "数列问题",
            "构造数列",
            "几何其他题型",
            "比赛问题",
            "同素分堆问题",
            "多集合反向构造",
            "环形排列问题",
            "错位排列",
            "数学运算",
        ):
            with self.subTest(topic=topic):
                result = normalize_classification("数量关系", None, [topic])
                self.assertEqual(result["category"], "数学运算")

    def test_audited_numeric_array_topic_fills_numeric_reasoning(self):
        result = normalize_classification("数量关系", None, ["图形数阵"])
        self.assertEqual(result["category"], "数字推理")

    def test_audited_general_theory_topics_use_module_specific_categories(self):
        cases = (
            ("常识判断", "总论", "政治常识"),
            ("政治理论", "总论", "马克思主义"),
            ("政治理论", "商品经济", "马克思主义"),
        )
        for module, topic, category in cases:
            with self.subTest(module=module, topic=topic):
                result = normalize_classification(module, None, [topic])
                self.assertEqual(result["category"], category)

    def test_topics_that_match_different_families_are_not_guessed(self):
        result = normalize_classification(
            "判断推理", None, ["数量规律-线", "逻辑关系-对应关系"]
        )
        self.assertIsNone(result["category"])
        self.assertIn("ambiguous_topic_categories", result["conflicts"])

    def test_unknown_module_is_not_inferred_from_topics(self):
        result = normalize_classification("思维能力测验", None, ["数量规律-线"])
        self.assertIsNone(result["module"])
        self.assertIsNone(result["category"])
        self.assertIn("unknown_module", result["conflicts"])

    def test_unknown_explicit_type_blocks_topic_guess_and_is_reported(self):
        result = normalize_classification(
            "判断推理", None, ["数量规律-线"], raw_category="待核题型"
        )
        self.assertIsNone(result["category"])
        self.assertIn("unknown_explicit_category", result["conflicts"])

    def test_empty_topics_leave_category_unclassified(self):
        result = normalize_classification("判断推理", None, [])
        self.assertEqual(result["module"], "判断推理")
        self.assertIsNone(result["category"])

    def test_broad_module_label_is_not_kept_as_category(self):
        result = normalize_classification("常识判断", "常识判断", ["行政法"])
        self.assertEqual(result["module"], "常识判断")
        self.assertEqual(result["category"], "法律常识")

    def test_repeated_normalization_is_idempotent_and_does_not_mutate_topics(self):
        topics = ["成语填空", "实词填空"]
        once = normalize_classification("言语理解与表达", None, topics)
        twice = normalize_classification(once["module"], once["category"], topics)
        self.assertEqual(once["module"], twice["module"])
        self.assertEqual(once["category"], twice["category"])
        self.assertEqual(topics, ["成语填空", "实词填空"])


if __name__ == "__main__":
    unittest.main()
