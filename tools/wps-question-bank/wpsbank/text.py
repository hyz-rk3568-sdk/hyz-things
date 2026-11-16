"""Pure text normalization shared by extraction and tests."""

from __future__ import annotations

import re

CJK = re.compile(r"[\u3400-\u9fff\u3000-\u303f\uff00-\uffef]")
NUMERIC = re.compile(r"[\u2212\-]?[\d.,]+%?")


def clean(value: str) -> str:
    """Drop WPS auto-spacing around CJK, then normalize number punctuation.

    WPS 文字 emits visual gaps as real spaces between CJK and Latin text. The
    gaps carry no meaning in Markdown, so they are removed. A space adjacent
    to ``%`` is noise for the same reason.
    """
    text = value.replace("\u00a0", " ").replace("\u3000", " ")
    out = []
    for index, char in enumerate(text):
        if char == " ":
            previous = text[index - 1] if index else ""
            following = text[index + 1] if index + 1 < len(text) else ""
            if (previous and (CJK.match(previous) or previous in "%％")) or (
                following and (CJK.match(following) or following in "%％")
            ):
                continue
        out.append(char)
    text = re.sub(r"[ \t]+", " ", "".join(out)).strip()
    text = re.sub(r"(?<=[A-Za-z0-9])．(?=\d)", ".", text)
    text = re.sub(r"(?:(?<=^)|(?<=[ （，；：、]))(-)(?=\d)", "\u2212", text)
    return text


def clean_cell(value: object) -> str:
    return clean(" ".join(str(value).split()))


def inside(bbox, outer) -> bool:
    center_x = (bbox[0] + bbox[2]) / 2
    center_y = (bbox[1] + bbox[3]) / 2
    return (
        outer[0] - 1 <= center_x <= outer[2] + 1
        and outer[1] - 1 <= center_y <= outer[3] + 1
    )


def numeric_cell(value: str) -> bool:
    return bool(NUMERIC.fullmatch(clean_cell(value)))


def row_has_numeric(row) -> bool:
    for value in row:
        if value is None:
            continue
        for line in str(value).split("\n"):
            if numeric_cell(line):
                return True
    return False
