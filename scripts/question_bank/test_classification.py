import unittest

from classification import normalize_classification


class ClassificationTests(unittest.TestCase):
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

    def test_audited_definition_judgment_topic_fills_definition_category(self):
        result = normalize_classification("判断推理", None, ["故事类"])
        self.assertEqual(result["category"], "定义判断")

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
