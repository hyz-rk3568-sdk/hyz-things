"""Local, loss-conscious Markdown question-bank import. No device dependencies."""

from dataclasses import dataclass, field
import re

import yaml
from classification import material_boundary_module, normalize_classification


class InvalidPaper(ValueError):
    pass


@dataclass
class Question:
    number: int
    stem: str
    options: str
    answer: str
    explanation: str
    notes: str = ""
    metadata: dict = field(default_factory=dict)
    provenance: dict = field(default_factory=dict)


@dataclass
class Material:
    name: str
    content: str
    first: int | None = None
    last: int | None = None


@dataclass
class Paper:
    metadata: dict
    questions: list[Question]
    materials: list[Material] = field(default_factory=list)
    provenance: dict = field(default_factory=dict)
    notes: str = ""


def dump_yaml(value):
    return yaml.safe_dump(value, allow_unicode=True, sort_keys=False).strip()


def frontmatter(text):
    match = re.match(r"\A---\s*\n(.*?)\n---\s*\n", text, re.S)
    if not match:
        raise InvalidPaper("missing frontmatter")
    metadata = yaml.safe_load(match[1])
    if not isinstance(metadata, dict):
        raise InvalidPaper("frontmatter must be a mapping")
    return metadata, text[match.end() :]


QUESTION_HEADING = re.compile(r"^#{2,3} 第\s*(\d+)\s*题([^\n]*)\n", re.M)
OPTION = re.compile(r"^(?:- )?(?:\*\*)?([A-H])(?:\*\*)?[.．][ \t]*", re.M)
LABEL = re.compile(
    r"^\*\*(参考答案[^*]*|答案|官方解析|解析[^*]*|考点|来源)\*\*[：:]?[^\S\n]*", re.M
)


def answer_sections(details):
    details = re.sub(r"<summary>.*?</summary>", "", details, flags=re.S)
    details = details.replace("</details>", "").strip()
    labels = list(LABEL.finditer(details))
    result = [
        (
            m[1],
            details[
                m.end() : labels[i + 1].start() if i + 1 < len(labels) else len(details)
            ].strip(),
        )
        for i, m in enumerate(labels)
    ]
    prefix = details[: labels[0].start()].strip() if labels else details
    if prefix:
        result.append(("__notes__", prefix))
    return result


MISSING = re.compile(r"暂未收录|暂未搜集|题目缺失|题干缺失|原题缺失")


def question_issues(stem, options, answer):
    issues = []
    if not stem or MISSING.search(stem):
        issues.append("missing_stem")
    labels = OPTION.findall(options)
    if not labels or len(set(labels)) != len(labels) or len(labels) < 4:
        issues.append("missing_options")
    if not re.fullmatch(r"[A-H]+", answer):
        issues.append(
            "missing_answer"
            if not answer or "unknown" in answer
            else "answer_unverified"
        )
    elif not set(answer) <= set(labels):
        issues.append("answer_unverified")
    return issues


def parse_question(number, text, heading=""):
    parts = text.split("<details>", 1)
    prompt, details = parts if len(parts) == 2 else (text, "")
    summaries = re.findall(r"<summary>第\s*(\d+)\s*题", details)
    if any(int(n) != number for n in summaries):
        raise InvalidPaper("answer belongs to another question")
    if details.count("</details>") != (1 if details else 0):
        raise InvalidPaper("unbalanced answer block")
    matches = list(OPTION.finditer(prompt))
    stem = prompt[: matches[0].start()].strip() if matches else prompt.strip()
    options = "\n\n".join(
        "- "
        + m[1]
        + ". "
        + prompt[
            m.end() : matches[i + 1].start() if i + 1 < len(matches) else len(prompt)
        ].strip()
        for i, m in enumerate(matches)
    )
    sections = answer_sections(details)
    answer = next(
        (v for k, v in sections if k.startswith("参考答案") or k == "答案"), ""
    )
    notes = [v for k, v in sections if k == "__notes__"]
    if "\n" in answer:
        answer, extra = answer.split("\n", 1)
        notes.append(extra.strip())
    explanation = "\n\n".join(v for k, v in sections if "解析" in k)
    topics = [
        tag.strip()
        for k, v in sections
        if k == "考点"
        for tag in v.split("、")
        if tag.strip()
    ]
    sub = re.search(r"<sub>(.*?)</sub>", heading)
    category = re.sub(r"^qid\s+\S+(?:\s*·\s*)?", "", sub[1]).strip() if sub else None
    category = category or None
    return Question(
        number,
        stem,
        options,
        answer,
        explanation,
        notes="\n\n".join(notes),
        metadata={
            "module": None,
            "category": category,
            "topics": topics,
            "materials": [],
            "passage_source": (
                re.search(r"【文段出处】([^\n]+)", text)[1]
                if "【文段出处】" in text and re.search(r"【文段出处】([^\n]+)", text)
                else None
            ),
            "issues": question_issues(stem, options, answer),
        },
        provenance={
            "answer_labels": [k for k, v in sections if "答案" in k],
            "raw_topics": [v for k, v in sections if k == "考点"],
            "raw_heading": heading,
            "raw_category": category,
            "source": [v for k, v in sections if k == "来源"],
        },
    )


def parse_paper(text):
    raw, body = frontmatter(text)
    title = re.search(r"^# (.+)$", body, re.M)[1]
    metadata = {
        "schema": "hyz-paper/v1",
        "title": title,
        "exam": raw["考试"],
        "year": int(raw["年份"]),
        "region": raw["地区"],
        "variant": raw["卷型"],
        "status": "unverified",
    }
    ranges = [
        (m[1].strip(), int(m[2]), int(m[3]))
        for m in re.finditer(
            r"^\|\s*([^|]+?)\s*\|[^|]*\|\s*(\d+)\s*[–—~～-]\s*(\d+)\s*\|", body, re.M
        )
    ]
    events = list(
        re.finditer(
            r"^#{2,3} (?:第\s*(\d+)\s*题([^\n]*)|材料\s*(\d+)([^\n]*)|([^\n]+))\n",
            body,
            re.M,
        )
    )
    questions, materials = [], []
    active = None
    unset_module = object()
    active_module = unset_module
    for i, event in enumerate(events):
        content = body[
            event.end() : events[i + 1].start() if i + 1 < len(events) else len(body)
        ].strip()
        if event[3]:
            bounds = re.search(r"第\s*(\d+)\s*[–—~～-]\s*(\d+)\s*题", event[4])
            active = Material(
                "M" + event[3],
                content,
                int(bounds[1]) if bounds else None,
                int(bounds[2]) if bounds else None,
            )
            materials.append(active)
            active_module = unset_module
        elif event[1]:
            number = int(event[1])
            q = parse_question(number, content, event[2])
            raw_module = next(
                (name for name, first, last in ranges if first <= number <= last),
                None,
            )
            normalized = normalize_classification(
                raw_module,
                q.metadata["category"],
                q.metadata["topics"],
                raw_category=q.provenance["raw_category"],
                stem=q.stem,
                options=q.options,
                has_material=bool(active),
            )
            q.metadata["module"] = normalized["module"]
            q.metadata["category"] = normalized["category"]
            q.provenance.update(
                raw_module=raw_module,
                module_basis=normalized["module_basis"],
                category_basis=normalized["category_basis"],
                classification_evidence=normalized["category_evidence"],
                classification_conflicts=normalized["conflicts"],
            )
            module = q.metadata["module"]
            boundary_module = material_boundary_module(
                raw_module, q.provenance.get("raw_category")
            )
            if active and active.first is not None:
                if active.first <= number <= active.last:
                    q.metadata["materials"] = [active.name]
                    q.provenance["material_basis"] = "explicit_range"
            elif active:
                if active_module is unset_module:
                    active_module = boundary_module
                if boundary_module != active_module:
                    active = None
                else:
                    q.metadata["materials"] = [active.name]
                    q.provenance["material_basis"] = (
                        "source_schema_and_module_boundary"
                        if module
                        else "source_schema_until_next_material_or_section"
                    )
            explicit = re.search(r"^关联材料[：:]([^\n]+)", q.stem, re.M)
            if explicit:
                q.metadata["materials"] = [
                    "M" + n for n in re.findall(r"材料\s*(\d+)", explicit[1])
                ]
                q.provenance["material_basis"] = "explicit_reference"
                q.stem = q.stem[: explicit.start()] + q.stem[explicit.end() :]
                q.stem = q.stem.strip()
            questions.append(q)
        else:
            active = None
    if not questions:
        raise InvalidPaper("no recognized questions")
    if len({q.number for q in questions}) != len(questions):
        raise InvalidPaper("duplicate question number")
    if len({m.name for m in materials}) != len(materials):
        raise InvalidPaper("duplicate material name")
    for q in questions:
        if not set(q.metadata["materials"]) <= {m.name for m in materials}:
            raise InvalidPaper("dangling material reference")
    preamble = body[: events[0].start()] if events else body
    # Keep source qualifications and instructions, but remove generated navigation.
    preamble = re.sub(
        r"<!-- gwy:overview:start -->.*?<!-- gwy:overview:end -->",
        "",
        preamble,
        flags=re.S,
    )
    preamble = re.sub(r"^# [^\n]*\n", "", preamble, flags=re.M).strip()
    # Instructions between the title and first question are also source content.
    for i, event in enumerate(events):
        if event[5]:
            content = body[
                event.start() : events[i + 1].start()
                if i + 1 < len(events)
                else len(body)
            ].strip()
            if event[5].strip() != "题目":
                preamble += "\n\n" + content
    if "待" in str(raw.get("题号口径", "")) or "未" in str(raw.get("题号口径", "")):
        for q in questions:
            q.metadata["issues"].append("order_unverified")
    return Paper(metadata, questions, materials, {"frontmatter": raw}, preamble.strip())


def section(title, content):
    return f"### {title}\n\n{content}"


def render_paper(paper):
    blocks = [f"---\n{dump_yaml(paper.metadata)}\n---\n\n# {paper.metadata['title']}"]
    if paper.notes:
        blocks.append("## Paper Notes\n\n" + paper.notes)
    for material in paper.materials:
        blocks.append(f"## Material {material.name}\n\n{material.content}")
    for q in paper.questions:
        blocks.append(
            "\n\n".join(
                [
                    f"## Question {q.number}",
                    f"```yaml\n{dump_yaml(q.metadata)}\n```",
                    section("Stem", q.stem),
                    section("Options", q.options),
                    section("Answer", q.answer),
                    *([section("Explanation", q.explanation)] if q.explanation else []),
                    *([section("Notes", q.notes)] if q.notes else []),
                ]
            )
        )
    return "\n\n".join(blocks) + "\n"
