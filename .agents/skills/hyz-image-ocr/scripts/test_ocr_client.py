#!/usr/bin/env python3
"""Unit tests for the OCR client's structured-block parsing."""

from __future__ import annotations

import importlib.util
import json
import math
import unittest
from pathlib import Path

CLIENT = Path(__file__).with_name("ocr_client.py")
SPEC = importlib.util.spec_from_file_location("ocr_client", CLIENT)
assert SPEC and SPEC.loader
ocr = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ocr)


def response(texts, scores=None, boxes=None, polys=None):
    pruned = {"rec_texts": texts}
    if scores is not None:
        pruned["rec_scores"] = scores
    if boxes is not None:
        pruned["rec_boxes"] = boxes
    if polys is not None:
        pruned["rec_polys"] = polys
    return {"errorCode": 0, "result": {"ocrResults": [{"prunedResult": pruned}]}}


class FakeScalar:
    def __init__(self, value):
        self.value = value

    def item(self):
        return self.value


class FakeArray:
    def __init__(self, value):
        self.value = value

    def tolist(self):
        return self.value


class BlocksTest(unittest.TestCase):
    def test_normal_blocks_keep_fields_and_index(self):
        result = response(
            ["A 12+30=42", "B x=5"],
            [0.97, 0.88],
            [[12, 43, 250, 81], [527, 61, 658, 101]],
            [
                [[12, 43], [250, 43], [250, 81], [12, 81]],
                [[527, 61], [658, 61], [658, 101], [527, 101]],
            ],
        )

        blocks = ocr.build_blocks(result)

        self.assertEqual(
            blocks[0],
            {
                "id": 0,
                "page": 1,
                "text": "A 12+30=42",
                "confidence": 0.97,
                "box": [12, 43, 250, 81],
                "polygon": [[12, 43], [250, 43], [250, 81], [12, 81]],
            },
        )
        self.assertEqual(blocks[1]["id"], 1)
        self.assertEqual(blocks[1]["text"], "B x=5")
        self.assertEqual(ocr.build_lines(blocks), ["A 12+30=42", "B x=5"])

    def test_missing_confidence_box_and_polygon_are_null(self):
        blocks = ocr.build_blocks(response(["only text"]))

        self.assertEqual(len(blocks), 1)
        self.assertIsNone(blocks[0]["confidence"])
        self.assertIsNone(blocks[0]["box"])
        self.assertIsNone(blocks[0]["polygon"])

    def test_duplicate_text_is_kept_in_blocks_and_deduped_in_lines(self):
        blocks = ocr.build_blocks(response(["same", "same"]))

        self.assertEqual([block["text"] for block in blocks], ["same", "same"])
        self.assertEqual(ocr.build_lines(blocks), ["same"])

    def test_scores_and_boxes_stay_with_their_text_index(self):
        blocks = ocr.build_blocks(
            response(
                ["first", "second", "third"],
                [0.3, 0.1, 0.2],
                [[0, 0, 1, 1], [2, 2, 3, 3], [4, 4, 5, 5]],
            )
        )

        self.assertEqual(
            [(block["text"], block["confidence"], block["box"]) for block in blocks],
            [
                ("first", 0.3, [0, 0, 1, 1]),
                ("second", 0.1, [2, 2, 3, 3]),
                ("third", 0.2, [4, 4, 5, 5]),
            ],
        )

    def test_short_score_and_box_lists_fill_missing_values_with_null(self):
        blocks = ocr.build_blocks(
            response(["one", "two", "three"], [0.9], [[0, 0, 1, 1]])
        )

        self.assertEqual(blocks[0]["confidence"], 0.9)
        self.assertIsNone(blocks[1]["confidence"])
        self.assertIsNone(blocks[2]["confidence"])
        self.assertEqual(blocks[0]["box"], [0, 0, 1, 1])
        self.assertIsNone(blocks[1]["box"])

    def test_empty_result_has_no_blocks_or_lines(self):
        result = response([])

        self.assertEqual(ocr.build_blocks(result), [])
        self.assertEqual(ocr.build_output(result)["lines"], [])

    def test_malformed_container_is_empty_without_crashing(self):
        for result in (None, [], {"errorCode": 0}, {"result": {"ocrResults": "bad"}}):
            self.assertEqual(ocr.build_blocks(result), [])
            self.assertEqual(ocr.build_output(result)["blocks"], [])

    def test_columns_keep_source_order_instead_of_y_order(self):
        blocks = ocr.build_blocks(
            response(
                ["left top", "right top", "left bottom", "right bottom"],
                boxes=[
                    [0, 0, 10, 10],
                    [100, 5, 110, 15],
                    [0, 50, 10, 60],
                    [100, 55, 110, 65],
                ],
            )
        )

        self.assertEqual(
            [block["text"] for block in blocks],
            ["left top", "right top", "left bottom", "right bottom"],
        )

    def test_multiple_pages_get_page_numbers_and_global_ids(self):
        result = {
            "errorCode": 0,
            "result": {
                "ocrResults": [
                    {"prunedResult": {"rec_texts": ["page one"]}},
                    {"prunedResult": {"rec_texts": ["page two"]}},
                ]
            },
        }

        blocks = ocr.build_blocks(result)

        self.assertEqual([block["page"] for block in blocks], [1, 2])
        self.assertEqual([block["id"] for block in blocks], [0, 1])

    def test_legacy_response_with_top_level_rec_texts(self):
        blocks = ocr.build_blocks(
            {"errorCode": 0, "result": {"rec_texts": ["legacy"], "rec_scores": [0.5]}}
        )

        self.assertEqual(len(blocks), 1)
        self.assertEqual(blocks[0]["text"], "legacy")
        self.assertEqual(blocks[0]["confidence"], 0.5)

    def test_numpy_like_values_are_json_safe(self):
        result = response(
            ["numpy"],
            [FakeScalar(0.75)],
            [FakeArray([1, 2, 3, 4])],
            [FakeArray([[1, 2], [3, 2], [3, 4], [1, 4]])],
        )

        output = ocr.build_output(result, include_raw=True)

        self.assertEqual(output["blocks"][0]["confidence"], 0.75)
        self.assertEqual(output["blocks"][0]["box"], [1, 2, 3, 4])
        json.dumps(output)

    def test_raw_is_opt_in(self):
        result = response(["text"])

        self.assertNotIn("raw", ocr.build_output(result))
        self.assertIn("raw", ocr.build_output(result, include_raw=True))

    def test_non_finite_values_are_null(self):
        blocks = ocr.build_blocks(
            response(["text"], [math.nan], [[math.inf, 0, 1, 1]])
        )

        self.assertIsNone(blocks[0]["confidence"])
        self.assertIsNone(blocks[0]["box"])


if __name__ == "__main__":
    unittest.main()
