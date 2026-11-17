"""Extract per-question explanations from an OCR'd 解析 volume."""

from __future__ import annotations

import re

HEADER = re.compile(r"^(\d{1,2})\s*[.．、]\s*[（(][^）)]{0,40}[）)]")
OPTION = re.compile(r"^([A-Z])\s*[.．、]")
ANSWER = re.compile(
    r"(?:正确答案\s*(?:为|是)|【(?:参考)?答案】)\s*([A-Z])"
)
ANSWER_LINE = re.compile(r"^【(?:参考)?答案】\s*[A-Z](?:\s*_{3,})?\s*$")
LABEL = re.compile(r"^【(?:实战解析|解析|分析|考点|正确答案)】\s*")


def split_blocks(lines: list[dict]) -> list[dict]:
    """Split ordered OCR lines into question blocks at numbered headers."""
    blocks: list[dict] = []
    current = None
    for line in lines:
        match = HEADER.match(line["text"].strip())
        if match:
            current = {"number": int(match.group(1)), "lines": []}
            blocks.append(current)
            continue
        if current is not None:
            current["lines"].append(line)
    return blocks


def _last_option(lines: list[dict], start: int) -> int:
    last = -1
    for index in range(start, len(lines)):
        if OPTION.match(lines[index]["text"].strip()):
            last = index
    return last


def explanation_lines(lines: list[dict], profile) -> list[dict]:
    """Drop the repeated stem and options, keeping the analysis text."""
    limit = len(lines)
    ask = next(
        (
            index
            for index, line in enumerate(lines[:limit])
            if profile.ask_re.fullmatch(line["text"].strip())
        ),
        None,
    )
    if ask is not None:
        last = _last_option(lines, ask + 1)
        if last >= 0:
            return lines[last + 1 :]
    first = next(
        (index for index, line in enumerate(lines) if OPTION.match(line["text"].strip())),
        None,
    )
    if first is not None:
        last = _last_option(lines, first)
        if last >= 0:
            return lines[last + 1 :]
    return lines


def _starts_paragraph(text: str) -> bool:
    return text == "例句" or bool(re.match(r"^【[^】]{1,20}】", text))


def paragraphs(lines: list[dict]) -> list[str]:
    """Join wrapped OCR lines; an indented line starts a new paragraph.

    A blank rule splits one visual line into several tokens, so tokens on the
    same baseline are joined before the indent test. The body margin is read
    from the page: explanation pages put the main column at different x
    positions depending on which side carries the sidebar.
    """
    rows: list[dict] = []
    for line in lines:
        text = line["text"].strip()
        if not text:
            continue
        if (
            rows
            and rows[-1]["page"] == line.get("page")
            and abs(rows[-1]["sy"] - line.get("sy", rows[-1]["sy"])) <= 8
        ):
            rows[-1]["text"] += text
            continue
        rows.append(
            {
                "text": text,
                "x": line.get("x", 0.0),
                "sy": line.get("sy", 0.0),
                "page": line.get("page"),
            }
        )
    if not rows:
        return []
    result: list[str] = []
    buffer = ""
    examples = False
    index = 0
    while index < len(rows):
        page = rows[index]["page"]
        end = index + 1
        while end < len(rows) and rows[end]["page"] == page:
            end += 1
        # The main column moves between pages: with a left sidebar it starts
        # much further right than on a page whose sidebar sits on the right.
        margin = min(row["x"] for row in rows[index:end])
        for row in rows[index:end]:
            text = row["text"]
            if text.startswith("例句"):
                examples = True
            if buffer and (
                (examples and _starts_paragraph(text))
                or (not examples and row["x"] >= margin + 6)
            ):
                result.append(buffer)
                buffer = text
            else:
                buffer += text
        index = end
    if buffer:
        result.append(buffer)
    return result


def parse(lines: list[dict], profile) -> dict[int, dict]:
    """Return ``{question number: {"text": ..., "answer": letter|None}}``."""
    explanations: dict[int, dict] = {}
    for block in split_blocks(lines):
        body = explanation_lines(block["lines"], profile)
        answer = None
        kept = []
        for line in body:
            text = line["text"].strip()
            match = ANSWER.search(text)
            if match and answer is None:
                answer = match.group(1)
            if ANSWER_LINE.fullmatch(text):
                continue
            text = LABEL.sub("", text)
            if not text:
                continue
            kept.append(dict(line, text=text))
        text = "\n\n".join(paragraphs(kept)).strip()
        if not text and not answer:
            continue
        explanations[block["number"]] = {"text": text, "answer": answer}
    return explanations
