#!/usr/bin/env python3
"""OCR selected pages from several PDFs and print reconstructed lines.

This is the review step before a book gets its profile: it shows whether the
text, option labels and fill-in-the-blank rules are usable.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

import pymupdf  # noqa: E402

from wpsbank import ocr_layout  # noqa: E402
from wpsbank.ocr import DEFAULT_URL, OcrPool  # noqa: E402

PUNCTUATION = "，。、；：？！）》】”’"
OPENING = "（《【“‘"
OPTION = re.compile(r"^[A-D][.．、]")


class PreviewNoise:
    def __init__(self, top: float, bottom: float, texts, patterns, fixes):
        self.top = top
        self.bottom = bottom
        self.texts = tuple(texts)
        self.patterns = tuple(re.compile(pattern) for pattern in patterns)
        self.fixes = dict(fixes)

    def correct(self, text: str) -> str:
        for wrong, right in self.fixes.items():
            text = text.replace(wrong, right)
        return text

    def is_noise(self, value: str, bbox) -> bool:
        value = value.strip()
        if not value:
            return True
        if bbox[1] < self.top or bbox[1] > self.bottom:
            return True
        if any(marker in value for marker in self.texts):
            return True
        if re.fullmatch(r"\d{1,3}", value):
            return True
        return any(pattern.fullmatch(value) for pattern in self.patterns)


def join_tokens(tokens: list[dict]) -> str:
    line = ""
    for token in tokens:
        text = token["text"]
        if not line:
            line = text
            continue
        if text[:1] in PUNCTUATION or line[-1:] in OPENING:
            line += text
        elif OPTION.match(line) and OPTION.match(text):
            line += "    " + text
        elif line[-1:].isascii() or text[:1].isascii():
            line += " " + text
        else:
            line += text
    return line


def reconstruct(tokens: list[dict], page: int) -> list[str]:
    lines: list[list[dict]] = []
    for token in tokens:
        if token["page"] != page:
            continue
        if lines and abs(token["sy"] - lines[-1][-1]["sy"]) <= 8:
            lines[-1].append(token)
        else:
            lines.append([token])
    return [join_tokens(line) for line in lines]


def parse_job(value: str) -> tuple[str, Path, list[int]]:
    if "=" not in value or ":" not in value:
        raise argparse.ArgumentTypeError("job must be NAME=PDF:PAGE,PAGE")
    name, rest = value.split("=", 1)
    path, pages = rest.rsplit(":", 1)
    try:
        numbers = [int(part) for part in pages.split(",") if part.strip()]
    except ValueError as error:
        raise argparse.ArgumentTypeError(str(error)) from error
    return name, Path(path), numbers


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--job", action="append", required=True, type=parse_job)
    parser.add_argument("--dpi", type=int, default=220)
    parser.add_argument("--cache", default="ocr-cache")
    parser.add_argument("--url", action="append", default=[])
    parser.add_argument("--top", type=float, default=85)
    parser.add_argument("--bottom", type=float, default=785)
    parser.add_argument("--text", action="append", default=[])
    parser.add_argument("--pattern", action="append", default=[])
    parser.add_argument("--fix", action="append", default=[], metavar="WRONG=RIGHT")
    parser.add_argument("--json-out")
    args = parser.parse_args(argv)

    fixes = []
    for value in args.fix:
        if "=" not in value:
            parser.error(f"--fix expects WRONG=RIGHT: {value}")
        fixes.append(tuple(value.split("=", 1)))

    noise = PreviewNoise(
        args.top,
        args.bottom,
        args.text,
        args.pattern
        or [r"第\s*\d+\s*页", r"[-—–]\s*\d+\s*[-—–]", r"练习题\s*\d+\s*套"],
        fixes,
    )
    urls = args.url or [
        DEFAULT_URL,
        "http://127.0.0.1:8081/ocr",
        "http://127.0.0.1:8082/ocr",
        "http://127.0.0.1:8083/ocr",
    ]
    pool = OcrPool(urls, args.cache, dpi=args.dpi)

    def prepare(page, pix, png):
        blanks = ocr_layout.page_blanks(page, png, args.dpi, noise.top)
        return ocr_layout.mask_blanks(png, blanks), blanks

    output = {}
    for name, pdf, pages in args.job:
        results = pool.recognize(
            pdf,
            pages,
            progress=lambda message: print(message, file=sys.stderr),
            prepare=prepare,
        )
        document = pymupdf.open(str(pdf))
        output[name] = {}
        for page_no in pages:
            page = document[page_no - 1]
            png = page.get_pixmap(dpi=args.dpi).tobytes("png")
            blanks = results[page_no].blanks
            if not blanks:
                blanks = ocr_layout.page_blanks(page, png, args.dpi, noise.top)
            tokens = ocr_layout.tokens(page_no, results[page_no], noise, blanks)
            lines = reconstruct(tokens, page_no)
            output[name][page_no] = lines
            print(
                f"\n===== {name} page {page_no}: "
                f"{len(results[page_no].blocks)} blocks, {len(blanks)} blanks"
            )
            for line in lines:
                print(line)
        document.close()
        # Tokens carry page numbers, so keys are per source.
    if args.json_out:
        Path(args.json_out).write_text(
            json.dumps(output, ensure_ascii=False, indent=1), encoding="utf-8"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
