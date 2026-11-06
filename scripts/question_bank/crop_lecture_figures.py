#!/usr/bin/env python3
"""Crop duplicated question text out of lecture screenshots.

The source PDF is a compilation of 1920x1080 lecture screenshots, so a figure
image can contain the question stem above the drawing. This pass detects
images with a dense top text block, keeps the lower figure cluster and writes
a content-addressed replacement. Pure diagrams and board notes are left alone.

Requires: opencv-python-headless and numpy.
"""

import argparse
import hashlib
import re
from pathlib import Path

import cv2
import numpy as np


def choose_crop(image, min_text_components):
    height, width = image.shape[:2]
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    _, binary = cv2.threshold(gray, 200, 255, cv2.THRESH_BINARY_INV)
    _, _, stats, _ = cv2.connectedComponentsWithStats(binary, 8)
    small = []
    large = []
    for x, y, w, h, area in stats[1:]:
        if (w > 0.8 * width and h < 30) or (h > 0.8 * height and w < 30):
            continue
        if y > 0.82 * height and h < 0.12 * height:
            continue
        if 5 <= w <= 45 and 5 <= h <= 45:
            small.append((x, y, w, h, area))
        if (h >= 50 and w >= 15) or (w >= 80 and h >= 20):
            large.append((x, y, w, h, area))
    candidates = [item for item in large if item[1] > 0.22 * height]
    if not candidates:
        return None
    figure = max(candidates, key=lambda item: item[4])
    figure_top = int(figure[1])
    if sum(1 for item in small if item[1] < figure_top) <= min_text_components:
        return None
    cluster = [item for item in large if item[1] + item[3] >= figure_top and item[1] < 0.85 * height]
    x0 = max(0, min(item[0] for item in cluster) - 70)
    y0 = max(0, min(item[1] for item in cluster) - 12)
    x1 = min(width, max(item[0] + item[2] for item in cluster) + 70)
    y1 = min(height, max(item[1] + item[3] for item in cluster) + 18)
    if (x1 - x0) * (y1 - y0) >= 0.9 * width * height:
        return None
    return image[y0:y1, x0:x1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--collection", required=True, help="collection Markdown directory")
    parser.add_argument("--assets", required=True, help="shared question_bank/assets directory")
    parser.add_argument("--min-text-components", type=int, default=80)
    args = parser.parse_args()

    collection = Path(args.collection)
    assets = Path(args.assets)
    markdowns = list(collection.glob("*.md"))
    references = sorted(
        {
            ref
            for path in markdowns
            for ref in re.findall(r"assets/([0-9a-f]{64}\.[a-z]+)", path.read_text(encoding="utf-8"))
        }
    )
    mapping = {}
    for reference in references:
        image = cv2.imread(str(assets / reference))
        if image is None:
            raise SystemExit(f"missing image: {reference}")
        crop = choose_crop(image, args.min_text_components)
        if crop is None:
            continue
        ok, encoded = cv2.imencode(".jpg", crop, [cv2.IMWRITE_JPEG_QUALITY, 92])
        if not ok:
            continue
        name = hashlib.sha256(encoded.tobytes()).hexdigest() + ".jpg"
        (assets / name).write_bytes(encoded.tobytes())
        mapping[reference] = name
    for path in markdowns:
        text = path.read_text(encoding="utf-8")
        for old, new in mapping.items():
            text = text.replace(old, new)
        path.write_text(text, encoding="utf-8")
    # Keep only assets still referenced by the bank.
    live = set()
    for base in [collection.parent, Path("question_bank/papers")]:
        for path in base.rglob("*.md"):
            live.update(re.findall(r"assets/([0-9a-f]{64}\.[a-z]+)", path.read_text(encoding="utf-8")))
    for old in mapping:
        path = assets / old
        if old not in live and path.exists():
            path.unlink()
    print(f"cropped={len(mapping)}")


if __name__ == "__main__":
    main()
