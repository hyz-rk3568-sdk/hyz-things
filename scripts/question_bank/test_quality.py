import unittest

from quality import classification_report


class ClassificationReportTests(unittest.TestCase):
    def test_module_relocation_requires_a_change_from_the_existing_value(self):
        before = {
            "papers/a.md": [
                {
                    "number": 1,
                    "module": "言语理解与表达",
                    "category": None,
                    "topics": [],
                }
            ]
        }
        after = {
            "papers/a.md": [
                {
                    "number": 1,
                    "module": "言语理解与表达",
                    "category": "逻辑填空",
                    "topics": [],
                }
            ]
        }
        records = [
            {
                "path": "papers/a.md",
                "number": 1,
                "raw_module": None,
                "raw_category": "逻辑填空",
                "module": "言语理解与表达",
                "category": "逻辑填空",
                "topics": [],
                "category_basis": "explicit_category",
                "evidence": ["逻辑填空"],
                "conflicts": [],
            }
        ]

        summary, details = classification_report(before, after, records)

        self.assertEqual(summary["module_relocation_count"], 0)
        self.assertEqual(details["module_relocations"], [])

    def test_explicit_type_can_resolve_a_missing_module(self):
        before = {
            "papers/a.md": [
                {"number": 1, "module": None, "category": None, "topics": []}
            ]
        }
        after = {
            "papers/a.md": [
                {
                    "number": 1,
                    "module": "判断推理",
                    "category": "图形推理",
                    "topics": [],
                }
            ]
        }
        records = [
            {
                "path": "papers/a.md",
                "number": 1,
                "raw_module": None,
                "raw_category": "图形推理",
                "module": "判断推理",
                "category": "图形推理",
                "topics": [],
                "category_basis": "explicit_category",
                "evidence": ["图形推理"],
                "conflicts": [],
            }
        ]

        summary, details = classification_report(before, after, records)

        self.assertEqual(summary["module_relocation_count"], 1)
        self.assertEqual(len(details["module_relocations"]), 1)


if __name__ == "__main__":
    unittest.main()
