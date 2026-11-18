"""Render extracted papers as hyz question-bank Markdown."""

from __future__ import annotations

import hashlib
from pathlib import Path
import re

from .profile import Profile

_BLANK_RUN = re.compile(r"_{4,}")
BLANK_GAP = re.compile(r"(_{4,})\s+([。，、；：！？])")
_CIRCLED = "①②③④⑤⑥⑦⑧⑨"
_MARKER_ONLY = re.compile(rf"^[{_CIRCLED}]+$")
# A statement marker never leads into a unit of measure; a quantity does.
_QUANTITY_TAIL = re.compile(
    r"^(?:年|月|日|天|周|小时|分钟|分|秒|毫秒|倍|个|人|名|次|种|元|万|亿|岁|米|点|成|%)"
)


def _fill_answers(target: dict[int, str], start: int, end: int, letters: str) -> None:
    if len(letters) != end - start + 1:
        raise ValueError(
            f"answer run {start}--{end} has {len(letters)} letters: {letters}"
        )
    for index, letter in enumerate(letters):
        target[start + index] = letter


def parse_answer_lines(lines, profile: Profile) -> dict:
    """Parse loose answer-key lines into ``{(kind, num): {number: letter}}``."""
    answers: dict = {}
    current = None
    pending = None
    for raw in lines:
        value = raw.strip()
        if not value:
            continue
        heading = profile.answer_heading_re.fullmatch(value)
        if heading:
            label, number = heading.groups()
            group = profile.answer_heading_map.get(label, label)
            current = (group, int(number))
            answers[current] = {}
            pending = None
            continue
        span = profile.answer_range_re.fullmatch(value)
        if span and current is not None:
            groups = span.groups()
            start, end = int(groups[0]), int(groups[1])
            letters = groups[2] if len(groups) > 2 else None
            if letters:
                _fill_answers(answers[current], start, end, letters)
                pending = None
            else:
                pending = (start, end)
            continue
        if (
            pending is not None
            and current is not None
            and profile.answer_run_re.fullmatch(value)
        ):
            _fill_answers(answers[current], pending[0], pending[1], value)
            pending = None
    return answers


ANSWER_RUN = re.compile(
    r"(\d{1,2})\s*[～~\-—–]\s*(\d{1,2})\s*[：:]?\s*([A-D][A-D\s]{0,24})"
)
ANSWER_SPAN = re.compile(
    r"^[^\d]{0,4}\s*(\d{1,2})\s*[～~\-—–]\s*(\d{1,2})\s*[：:]?$"
)
LETTERS = re.compile(r"[A-D]{2,12}")


def _normalize_heading(value: str) -> str:
    return value.replace("＆", "&").replace(" ", "").strip()


def section_lines(lines, heading: str, headings) -> list[str]:
    """Answer lines between a section heading and the next section heading.

    One answer page can carry several sections, and every section restarts its
    numbering at 1, so the page must be sliced before the runs are parsed.
    """
    texts = [line.strip() for line in lines]
    normalized = [_normalize_heading(line) for line in texts]
    known = {_normalize_heading(item) for item in headings}
    try:
        start = normalized.index(_normalize_heading(heading))
    except ValueError:
        return []
    stop = len(texts)
    for index in range(start + 1, len(texts)):
        if normalized[index] in known:
            stop = index
            break
    return texts[start + 1 : stop]


def parse_answer_runs(lines, profile: Profile) -> dict[int, str]:
    """Parse OCR answer lines such as ``1-5 BDCBD 6-10 ACBCD``."""
    answers: dict[int, str] = {}
    pending = None
    for raw in lines:
        # OCR cuts the key row wherever a mark interrupts it, so the pieces of
        # one row arrive as separate lines or blocks. Split on the slashes and
        # read the fragments in order.
        for fragment in re.split(r"[/／]", raw):
            value = fragment.strip()
            if not value:
                continue
            for match in ANSWER_RUN.finditer(value):
                start, end = int(match.group(1)), int(match.group(2))
                letters = re.sub(r"\s+", "", match.group(3))
                if len(letters) == end - start + 1:
                    _fill_answers(answers, start, end, letters)
                    pending = None
            span = ANSWER_SPAN.fullmatch(value)
            if span:
                pending = (int(span.group(1)), int(span.group(2)))
                continue
            if pending is not None and LETTERS.fullmatch(value):
                _fill_answers(answers, pending[0], pending[1], value)
                pending = None
    return answers


def parse_answer_json(path: Path | str, profile: Profile) -> dict:
    """Read a hand-checked key: ``{"练习题": {"1": "ADDBA..."}}``.

    Answer sheets that exist only as images are transcribed once into this
    shape, so a re-import does not need another OCR pass over the key.
    """
    import json

    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ValueError(f"cannot read answer key {path}: {error}") from error
    if not isinstance(data, dict):
        raise ValueError("answer key must map paper kinds to set runs")
    answers = {}
    labels = set(profile.option_labels)
    for kind, papers in data.items():
        if not isinstance(papers, dict):
            raise ValueError(f"answer key {kind!r} must map set numbers to runs")
        for num, letters in papers.items():
            if not isinstance(letters, str):
                raise ValueError(f"answer key {kind}{num} must be a string")
            run = re.sub(r"\s+", "", letters)
            if not run or not set(run) <= labels:
                raise ValueError(
                    f"answer key {kind}{num} has letters outside {profile.option_labels}"
                )
            answers[(kind, int(num))] = {
                index: letter for index, letter in enumerate(run, start=1)
            }
    return answers


def parse_answers(path: Path | str, profile: Profile) -> dict:
    import pymupdf

    if str(path).lower().endswith(".json"):
        return parse_answer_json(path, profile)
    document = pymupdf.open(str(path))
    lines = []
    for page in document:
        for block in page.get_text("dict")["blocks"]:
            for line in block.get("lines", []):
                value = "".join(span["text"] for span in line["spans"]).strip()
                if value:
                    lines.append(value)
    return parse_answer_lines(lines, profile)


def render_paragraphs(blocks, profile: Profile) -> str:
    """Join wrapped lines; an indented line starts a new paragraph."""
    paragraphs = []
    buffer = ""
    for block in blocks:
        if block["kind"] == "text":
            value = block["text"]
            if not value:
                continue
            if not buffer:
                buffer = value
            elif block.get("x", 78) >= profile.body_margin:
                paragraphs.append(buffer)
                buffer = value
            else:
                # Two blanks can meet at a line break; keep them apart so the
                # reader still sees two blanks.
                if buffer.endswith("_") and value.startswith("_"):
                    buffer += " "
                buffer += value
        else:
            if buffer:
                paragraphs.append(buffer)
                buffer = ""
            if block["kind"] == "table":
                paragraphs.append(block["md"])
            else:
                paragraphs.append(f"![]({block['asset']})")
    if buffer:
        paragraphs.append(buffer)
    return "\n\n".join(paragraphs)


def render_option(option: dict) -> str:
    parts = []
    for block in option["blocks"]:
        if block["kind"] == "text":
            if block["text"]:
                parts.append(block["text"])
        else:
            parts.append(f"![]({block['asset']})")
    return f"- {option['label']}. {''.join(parts)}"


def write_asset(block: dict, assets_dir: Path) -> str:
    data = block["data"]
    digest = hashlib.sha256(data).hexdigest()
    extension = "png" if block["ext"] == "png" else "jpg"
    name = f"{digest}.{extension}"
    path = assets_dir / name
    if not path.exists():
        path.write_bytes(data)
    block["asset"] = f"../../assets/{name}"
    return name


def _option_text(option: dict) -> str:
    return "".join(
        block["text"] for block in option["blocks"] if block["kind"] == "text"
    ).strip()


def repair_statement_markers(stem: str, options: list[dict]) -> str:
    """Restore statement markers that scan OCR read as plain digits.

    Sorting questions number their statements ①…⑥, and their options are
    permutations of those markers. That makes the expected marker set known:
    a marker missing from the stem while a matching ASCII digit sits in its
    place was misread, for example ⑤ as ``5`` or ① as ``(1``.
    """
    texts = [re.sub(r"\s+", "", _option_text(option)) for option in options]
    if not texts or not all(_MARKER_ONLY.fullmatch(text) for text in texts):
        return stem
    expected = sorted({char for text in texts for char in text})
    present = {char for char in stem if char in _CIRCLED}
    for char in expected:
        if char in present:
            continue
        digit = str(_CIRCLED.index(char) + 1)
        bracketed = re.compile(rf"[（(]\s*{digit}(?=[\u4e00-\u9fff])")
        match = bracketed.search(stem)
        if match:
            stem = stem[: match.start()] + char + stem[match.end() :]
            present.add(char)
            continue
        loose = re.compile(rf"(?<![\d.]){digit}(?![\d.])(?=[\u4e00-\u9fff])")
        for candidate in loose.finditer(stem):
            if _QUANTITY_TAIL.match(stem[candidate.end() :]):
                continue
            stem = stem[: candidate.start()] + char + stem[candidate.end() :]
            present.add(char)
            break
    # The same misread can leave a bracket in front of a marker.
    return re.sub(rf"[（(](?=[{_CIRCLED}])", "", stem)


def _merge_option_blocks(option: dict, squeeze_han_spaces: bool = False) -> bool:
    """Join option lines that OCR split, inserting the missing word gap."""
    blocks = [block for block in option["blocks"] if block["kind"] == "text"]
    if len(blocks) < 2:
        return False
    merged = ""
    inserted = False
    for block in blocks:
        text = block["text"]
        # OCR splits at a word gap, so the space is real. A text-layer line
        # break can fall inside a word, so the pieces stay glued. A prose book
        # only wraps long options, so its profile asks for no gap at all.
        if (
            block.get("ocr")
            and merged
            and not squeeze_han_spaces
            and not merged.endswith(" ")
            and not text.startswith(" ")
        ):
            merged += " "
            inserted = True
        merged += text
    blocks[0]["text"] = merged
    for block in blocks[1:]:
        block["text"] = ""
    return inserted


def normalize_option_spacing(
    papers: list[dict], squeeze_han_spaces: bool = False
) -> int:
    """Split option words that OCR glued together.

    The stem's blank count equals the number of words in each option. A
    lexicon built from all options lets us recover the missing boundaries
    without splitting single-word options or unknown four-character idioms.
    """
    words: set[str] = set()
    changed = 0
    for paper in papers:
        for question in paper["questions"]:
            for option in question["options"]:
                if _merge_option_blocks(option, squeeze_han_spaces):
                    changed += 1
                for word in _option_text(option).split():
                    if len(word) >= 2:
                        words.add(word)
    for paper in papers:
        for question in paper["questions"]:
            # Narrow adjacent blanks merge into one long run of underscores,
            # so every run is a single blank.
            blanks = len(
                _BLANK_RUN.findall(
                    "".join(
                        block.get("text", "")
                        for block in question["stem"]
                        if block["kind"] == "text"
                    )
                )
            )
            if blanks < 2:
                continue
            pending = []
            for option in question["options"]:
                parts = _option_text(option).split()
                if len(parts) >= blanks:
                    continue
                merged = "".join(parts)
                split = _segment(merged, blanks, words)
                if split:
                    _apply_split(option, split)
                    changed += 1
                else:
                    pending.append((option, merged))
            if not pending:
                continue
            # Options split by the lexicon now show the word-length pattern
            # that the still-glued options of this question follow.
            shapes: dict[tuple[int, ...], int] = {}
            for option in question["options"]:
                words_in_option = _option_text(option).split()
                if len(words_in_option) != blanks:
                    continue
                shape = tuple(len(word) for word in words_in_option)
                shapes[shape] = shapes.get(shape, 0) + 1
            for option, merged in pending:
                split = _by_shape(merged, shapes)
                if not split:
                    continue
                _apply_split(option, split)
                changed += 1
    return changed


def _apply_split(option: dict, split: list[str]) -> None:
    for block in option["blocks"]:
        if block["kind"] == "text":
            block["text"] = " ".join(split)
            break


def _by_shape(text: str, shapes: dict[tuple[int, ...], int]) -> list[str] | None:
    """Split with the word lengths that the sibling options agree on.

    A word that appears in only one option never reaches the lexicon.  The
    other options of the same question still show the pattern, for example
    three four-character words, which pins the missing boundaries.
    """
    if len(shapes) != 1:
        return None
    shape = next(iter(shapes))
    # The sibling options of a question are parallel, so a single agreeing
    # shape pins the boundaries once its total length matches.
    if sum(shape) != len(text) or min(shape) < 2:
        return None
    parts = []
    start = 0
    for width in shape:
        parts.append(text[start : start + width])
        start += width
    return parts


def _segment(text: str, count: int, words: set[str]) -> list[str] | None:
    length = len(text)

    def solve(position: int, left: int) -> list[str] | None:
        if left == 0:
            return [] if position == length else None
        for end in range(length, position, -1):
            piece = text[position:end]
            if len(piece) >= 2 and piece in words:
                rest = solve(end, left - 1)
                if rest is not None:
                    return [piece] + rest
        return None

    return solve(0, count)


def paper_markdown(
    paper: dict,
    answers: dict,
    profile: Profile,
    explanations: dict | None = None,
) -> str:
    kind, num = paper["kind"], paper["num"]
    title = f"{kind}{num}"
    lines = [
        "---",
        f"schema: {profile.schema}",
    ]
    if profile.schema == "hyz-paper/v2":
        lines.append(f"subject: {profile.subject}")
    lines += [
        f'title: "{title}"',
        f"year: {profile.year}",
        f"region: {profile.region}",
        f"variant: {profile.variant}",
        f"role: {profile.role}",
        f"kind: {profile.paper_kind}",
        f"collection_id: {profile.collection_id}",
        f'collection_title: "{profile.collection_title}"',
        "---",
        "",
        f"# {title}",
        "",
    ]
    for material in paper["materials"]:
        lines += [
            f"## Material {material['key']}",
            "",
            render_paragraphs(material["blocks"], profile),
            "",
        ]

    for question in paper["questions"]:
        number = question["number"]
        patch = profile.override(kind, num, number)
        used_materials = []
        if paper["materials"]:
            index = min(
                (number - 1) // profile.questions_per_material,
                len(paper["materials"]) - 1,
            )
            used_materials = [paper["materials"][index]["key"]]
        lines += [
            f"## Question {number}",
            "",
            "```yaml",
            f"module: {profile.module}",
        ]
        if profile.category:
            lines.append(f"category: {profile.category}")
        if profile.schema == "hyz-paper/v2":
            lines += ["answer_type: objective", "grading_mode: exact"]
        if used_materials:
            lines.append("materials:")
            lines += [f"- {name}" for name in used_materials]
        lines += ["```", "", "### Stem", ""]
        stem = patch.get("stem") or render_paragraphs(question["stem"], profile)
        if not patch.get("stem"):
            stem = repair_statement_markers(stem, question["options"])
        if profile.ocr_enabled and stem:
            # A blank attached to a punctuation block keeps the spacing that
            # separates words, which the printed page does not have.
            stem = BLANK_GAP.sub(r"\1\2", stem)
            if stem[-1] not in "。！？…”：；，、":
                # A printed full stop that lands on its own line is easy for
                # the scan OCR to miss; the sentence still needs its period.
                stem += "。"
        if profile.category == "逻辑填空":
            stem = re.sub(r"(?<!\\)_{2,}", lambda match: "\\_" * len(match.group()), stem)
        lines += [stem, "", "### Options"]
        for option in question["options"]:
            replacement = patch.get("options", {}).get(option["label"])
            lines.append(
                f"- {option['label']}. {replacement}"
                if replacement
                else render_option(option)
            )
        answer = answers.get((kind, num), {}).get(number)
        lines += ["", "### Answer", "", answer or "", ""]
        explanation = (explanations or {}).get((kind, num), {}).get(number)
        if explanation:
            lines += ["### Explanation", "", explanation["text"], ""]
    return "\n".join(lines).rstrip() + "\n"


def paper_path(out_dir: Path, profile: Profile, title: str) -> Path:
    return out_dir / "collections" / profile.collection_id / f"{title}.md"


def write_paper(
    paper: dict,
    answers: dict,
    assets_dir: Path,
    out_dir: Path,
    profile: Profile,
    explanations: dict | None = None,
) -> Path:
    path = paper_path(out_dir, profile, f"{paper['kind']}{paper['num']}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        paper_markdown(paper, answers, profile, explanations), encoding="utf-8"
    )
    return path


def prepare_assets(paper: dict, assets_dir: Path) -> int:
    """Write every image block and attach its content-addressed reference."""
    count = 0
    for material in paper["materials"]:
        for block in material["blocks"]:
            if block["kind"] == "image":
                write_asset(block, assets_dir)
                count += 1
    for question in paper["questions"]:
        for block in question["stem"]:
            if block["kind"] == "image":
                write_asset(block, assets_dir)
                count += 1
        for option in question["options"]:
            for block in option["blocks"]:
                if block["kind"] == "image":
                    write_asset(block, assets_dir)
                    count += 1
    return count
