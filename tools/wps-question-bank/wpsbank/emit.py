"""Render extracted papers as hyz question-bank Markdown."""

from __future__ import annotations

import hashlib
from pathlib import Path

from .profile import Profile


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


def parse_answers(path: Path | str, profile: Profile) -> dict:
    import pymupdf

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


def paper_markdown(paper: dict, answers: dict, profile: Profile) -> str:
    kind, num = paper["kind"], paper["num"]
    title = f"{kind}{num}"
    lines = [
        "---",
        "schema: hyz-paper/v1",
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
        if used_materials:
            lines.append("materials:")
            lines += [f"- {name}" for name in used_materials]
        lines += ["```", "", "### Stem", ""]
        stem = patch.get("stem") or render_paragraphs(question["stem"], profile)
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
    return "\n".join(lines).rstrip() + "\n"


def paper_path(out_dir: Path, profile: Profile, title: str) -> Path:
    return out_dir / "collections" / profile.collection_id / f"{title}.md"


def write_paper(
    paper: dict, answers: dict, assets_dir: Path, out_dir: Path, profile: Profile
) -> Path:
    path = paper_path(out_dir, profile, f"{paper['kind']}{paper['num']}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(paper_markdown(paper, answers, profile), encoding="utf-8")
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
