#!/usr/bin/env python3
"""Call the local PaddleX OCR serving endpoint for one temporary attachment.

Only files inside <project>/tmp/hyz-ai-uploads/ are accepted. The endpoint is
fixed to the loopback PaddleOCR service; this script never installs, starts or
exposes anything.
"""
from __future__ import annotations

import argparse
import base64
import json
import math
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_URL = "http://127.0.0.1:8080/ocr"
MAX_BYTES = 8 * 1024 * 1024
TIMEOUT = 120


def upload_root() -> Path:
    return Path(__file__).resolve().parents[4] / "tmp" / "hyz-ai-uploads"


def checked_path(value: str) -> Path:
    path = Path(value).resolve()
    try:
        path.relative_to(upload_root().resolve())
    except ValueError:
        raise SystemExit("只允许读取本次上传的 tmp/hyz-ai-uploads/ 图片") from None
    if not path.is_file() or path.stat().st_size > MAX_BYTES:
        raise SystemExit("图片不存在或超过 8 MiB")
    return path


def normalize(value):
    """Convert numpy values and other non-JSON objects without changing shape."""
    if value is None or isinstance(value, (str, bool)):
        return value
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, dict):
        return {str(key): normalize(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [normalize(item) for item in value]
    if hasattr(value, "tolist"):
        return normalize(value.tolist())
    if hasattr(value, "item"):
        return normalize(value.item())
    return str(value)


def number_or_null(value):
    value = normalize(value)
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and math.isfinite(value):
        return value
    return None


def box_or_null(value):
    value = normalize(value)
    if not isinstance(value, list) or len(value) != 4:
        return None
    coordinates = [number_or_null(item) for item in value]
    if any(item is None for item in coordinates):
        return None
    return coordinates


def polygon_or_null(value):
    value = normalize(value)
    if not isinstance(value, list) or len(value) < 3:
        return None
    points = []
    for point in value:
        point = normalize(point)
        if not isinstance(point, list) or len(point) != 2:
            return None
        x = number_or_null(point[0])
        y = number_or_null(point[1])
        if x is None or y is None:
            return None
        points.append([x, y])
    return points


def sequence(value) -> list:
    return list(value) if isinstance(value, (list, tuple)) else []


def page_results(result) -> list[dict]:
    """Return the per-page dictionaries that carry recognition arrays.

    PaddleX serving currently nests them under
    result.ocrResults[].prunedResult. Older or in-process responses may place
    the same arrays directly on the page or top-level result, so both shapes
    are accepted without reordering the arrays.
    """
    if not isinstance(result, dict):
        return []
    root = result.get("result", result)
    if isinstance(root, list):
        root = {"ocrResults": root}
    if not isinstance(root, dict):
        return []

    pages = []
    ocr_results = root.get("ocrResults")
    if isinstance(ocr_results, list):
        for item in ocr_results:
            if not isinstance(item, dict):
                continue
            pruned = item.get("prunedResult")
            if isinstance(pruned, dict):
                pages.append(pruned)
            elif isinstance(item.get("ocrResult"), dict):
                pages.append(item["ocrResult"])
            elif "rec_texts" in item:
                pages.append(item)

    if pages:
        return pages

    pruned = root.get("prunedResult")
    if isinstance(pruned, dict):
        return [pruned]
    if "rec_texts" in root:
        return [root]
    return []


def build_blocks(result) -> list[dict]:
    """Build text blocks in the exact order returned by the OCR service."""
    blocks = []
    for page_index, page in enumerate(page_results(result), start=1):
        texts = page.get("rec_texts")
        if not isinstance(texts, (list, tuple)):
            continue
        scores = sequence(page.get("rec_scores"))
        boxes = sequence(page.get("rec_boxes"))
        polygons = sequence(page.get("rec_polys")) or sequence(page.get("dt_polys"))
        for index, text in enumerate(texts):
            blocks.append(
                {
                    "id": len(blocks),
                    "page": page_index,
                    "text": text.strip() if isinstance(text, str) else None,
                    "confidence": (
                        number_or_null(scores[index])
                        if index < len(scores)
                        else None
                    ),
                    "box": box_or_null(boxes[index]) if index < len(boxes) else None,
                    "polygon": (
                        polygon_or_null(polygons[index])
                        if index < len(polygons)
                        else None
                    ),
                }
            )
    return blocks


def build_lines(blocks: list[dict]) -> list[str]:
    """Keep the original de-duplicated line list for older callers."""
    lines = []
    for block in blocks:
        text = block.get("text")
        if isinstance(text, str):
            text = text.strip()
            if text and text not in lines:
                lines.append(text)
    return lines


def collect_text(value, output: list[str]) -> None:
    """Fallback for a response that carries text outside the block arrays."""
    if isinstance(value, dict):
        for key, item in value.items():
            if key in {"text", "rec_text", "recText", "content"} and isinstance(item, str):
                item = item.strip()
                if item and item not in output:
                    output.append(item)
            elif key == "rec_texts" and isinstance(item, list):
                for line in item:
                    if isinstance(line, str):
                        line = line.strip()
                        if line and line not in output:
                            output.append(line)
            else:
                collect_text(item, output)
    elif isinstance(value, list):
        for item in value:
            collect_text(item, output)


def build_output(result, include_raw: bool = False) -> dict:
    blocks = build_blocks(result)
    lines = build_lines(blocks)
    if not blocks:
        fallback: list[str] = []
        collect_text(result, fallback)
        lines = fallback
    output = {"ok": True, "lines": lines, "blocks": blocks}
    if include_raw:
        output["raw"] = normalize(result)
    return output


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image")
    parser.add_argument("--url", default=os.environ.get("HYZ_OCR_URL", DEFAULT_URL))
    parser.add_argument(
        "--raw",
        action="store_true",
        help="include the full service response for debugging",
    )
    args = parser.parse_args()
    path = checked_path(args.image)
    payload = json.dumps(
        {
            "file": base64.b64encode(path.read_bytes()).decode(),
            "fileType": 1,
            "visualize": False,
        }
    ).encode()
    request = urllib.request.Request(
        args.url,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT) as response:
            result = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, TimeoutError, UnicodeDecodeError, json.JSONDecodeError) as error:
        print(json.dumps({"ok": False, "error": f"OCR 服务不可用：{error}"}, ensure_ascii=False))
        return 2
    if not isinstance(result, dict):
        print(json.dumps({"ok": False, "error": "OCR 服务返回结构无效"}, ensure_ascii=False))
        return 4
    if result.get("errorCode", 0) != 0:
        print(
            json.dumps(
                {
                    "ok": False,
                    "error": f"OCR 服务返回错误：{result.get('errorMsg', result['errorCode'])}",
                },
                ensure_ascii=False,
            )
        )
        return 3
    print(json.dumps(build_output(result, args.raw), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
