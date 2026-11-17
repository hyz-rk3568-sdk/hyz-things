#!/usr/bin/env python3
"""OCR a handful of pages and print reconstructed lines for review."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

import pymupdf  # noqa: E402

from wpsbank.ocr import DEFAULT_URL, OcrPool  # noqa: E402
from wpsbank import ocr_layout  # noqa: E402


class ProbeNoise:
    def __init__(self, top: float = 85, bottom: float = 785, texts=(), patterns=()):
        self.top = top
        self.bottom = bottom
        self.texts = tuple(texts)
        self.patterns = tuple(re.compile(pattern) for pattern in patterns)

    def is_noise(self, value: str, bbox) -> bool:
        value = value.strip()
        if not value:
            return True
        if bbox[1] < self.top or bbox[1] > self.bottom:
            return True
        if any(marker in value for marker in self.texts):
            return True
        return any(pattern.fullmatch(value) for pattern in self.patterns)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pdf", required=True)
    parser.add_argument("--pages", required=True, help="1-based page list, e.g. 10,61,131")
    parser.add_argument("--dpi", type=int, default=220)
    parser.add_argument("--cache", default="ocr-cache")
    parser.add_argument("--url", action="append", default=[])
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    pdf = Path(args.pdf)
    pages = [int(value) for value in args.pages.split(",") if value.strip()]
    pool = OcrPool(args.url or [DEFAULT_URL], args.cache, dpi=args.dpi)
    results = pool.recognize(pdf, pages, progress=lambda message: print(message, file=sys.stderr))

    document = pymupdf.open(str(pdf))
    output = {}
    for page_no in pages:
        page = document[page_no - 1]
        png = page.get_pixmap(dpi=args.dpi).tobytes("png")
        noise = ProbeNoise(
            texts=("超格学员专用", "花生十三", "四海公考", "SIHAIGONGKAO"),
            patterns=(r"第\s*\d+\s*页", r"[-—–]\s*\d+\s*[-—–]"),
        )
        blanks = ocr_layout.detect_blanks(page, png, results[page_no], noise.top)
        tokens = ocr_layout.tokens(page_no, results[page_no], noise, blanks)
        output[page_no] = tokens
        print(f"\n===== page {page_no}: {len(results[page_no].blocks)} blocks, {len(blanks)} blanks")
        for token in tokens:
            print(token["text"])
    if args.json_out:
        Path(args.json_out).write_text(
            json.dumps(output, ensure_ascii=False, indent=1), encoding="utf-8"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
