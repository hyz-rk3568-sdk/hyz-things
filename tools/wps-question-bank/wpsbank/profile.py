"""Book profile: the per-PDF constants that the generic pipeline needs."""

from __future__ import annotations

from dataclasses import dataclass, field
import json
from pathlib import Path
import re
from typing import Mapping


@dataclass(frozen=True)
class Paper:
    kind: str
    num: int
    start: int
    end: int


@dataclass(frozen=True)
class AnswerPage:
    kind: str
    num: int
    pdf: str
    page: int
    explain_start: int | None = None
    explain_end: int | None = None
    section: str | None = None


def _compile(pattern: str, field_name: str) -> re.Pattern:
    try:
        return re.compile(pattern)
    except re.error as error:
        raise ValueError(f"invalid {field_name} pattern: {error}") from error


def _require(mapping: Mapping, key: str, kind: type, where: str):
    value = mapping.get(key)
    if value is None:
        raise ValueError(f"missing {key} in {where}")
    if not isinstance(value, kind) or isinstance(value, bool):
        raise ValueError(f"{key} in {where} has the wrong type")
    return value


def _parse_schema(value) -> str:
    if value is None:
        return "hyz-paper/v1"
    if value not in ("hyz-paper/v1", "hyz-paper/v2"):
        raise ValueError(f"unsupported schema: {value!r}")
    return value


def _parse_subject(value) -> str:
    if value is None:
        return "xingce"
    if not isinstance(value, str) or value not in ("xingce", "shenlun"):
        raise ValueError(f"unsupported subject: {value!r}")
    return value


@dataclass(frozen=True)
class Profile:
    path: Path
    collection_id: str
    collection_title: str
    year: int
    region: str
    variant: str
    module: str
    role: str
    paper_kind: str
    option_labels: str
    questions_per_material: int
    body_margin: float
    top_band: float
    bottom_band: float
    noise_texts: tuple[str, ...]
    noise_patterns: tuple[re.Pattern, ...]
    section_re: re.Pattern
    ask_re: re.Pattern
    question_re: re.Pattern
    option_re: re.Pattern
    question_prefixes: tuple[str, ...]
    answer_heading_re: re.Pattern
    answer_heading_map: Mapping[str, str]
    answer_range_re: re.Pattern
    answer_run_re: re.Pattern
    papers: tuple[Paper, ...]
    expected: Mapping[str, int]
    overrides: Mapping[str, Mapping[int, Mapping[int, dict]]]
    question_starts: Mapping[int, int] = field(default_factory=dict)
    book: str | None = None
    answers: str | None = None
    answers_required: bool = True
    answer_pages: tuple[AnswerPage, ...] = ()
    schema: str = "hyz-paper/v1"
    subject: str = "xingce"
    ocr_enabled: bool = False
    ocr_dpi: int = 220
    ocr_badge: str = r"练习题\s*(\d+)\s*套"
    ocr_replacements: tuple[tuple[str, str], ...] = ()
    title_re: re.Pattern = field(init=False, compare=False)

    def __post_init__(self) -> None:
        kinds = sorted({paper.kind for paper in self.papers})
        pattern = _compile(
            r"^(?:" + "|".join(re.escape(kind) for kind in kinds) + r")\s*\d*$",
            "title",
        )
        object.__setattr__(self, "title_re", pattern)

    @property
    def paper_specs(self) -> tuple[tuple[str, int, int, int], ...]:
        return tuple((p.kind, p.num, p.start, p.end) for p in self.papers)

    def is_noise(self, value: str, bbox) -> bool:
        stripped = value.strip()
        if not stripped:
            return True
        if bbox[3] < self.top_band or bbox[1] > self.bottom_band:
            return True
        if any(marker in stripped for marker in self.noise_texts):
            return True
        return any(pattern.fullmatch(stripped) for pattern in self.noise_patterns)

    def correct(self, value: str) -> str:
        for wrong, right in self.ocr_replacements:
            value = value.replace(wrong, right)
        return value

    def override(self, kind: str, num: int, number: int) -> dict:
        by_paper = self.overrides.get(kind, {})
        by_number = by_paper.get(num, {})
        return by_number.get(number, {})

    @classmethod
    def load(cls, path: Path | str) -> "Profile":
        path = Path(path)
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise ValueError(f"cannot read profile {path}: {error}") from error
        if not isinstance(data, dict):
            raise ValueError(f"profile {path} must be a JSON object")

        collection_id = _require(data, "collection_id", str, "profile")
        if not re.fullmatch(r"[a-z0-9_-]{1,64}", collection_id):
            raise ValueError("collection_id must be 1-64 lowercase ASCII characters")
        collection_title = _require(data, "collection_title", str, "profile")
        if not collection_title.strip() or len(collection_title) > 128:
            raise ValueError("collection_title must be 1-128 characters")
        option_labels = _require(data, "option_labels", str, "profile")
        if (
            not 2 <= len(option_labels) <= 12
            or len(set(option_labels)) != len(option_labels)
            or not all("A" <= label <= "Z" for label in option_labels)
        ):
            raise ValueError("option_labels must be 2-12 unique ASCII uppercase letters")
        for label in ("module", "region", "variant", "role", "paper_kind"):
            if not isinstance(data.get(label), str) or not data[label].strip():
                raise ValueError(f"missing {label} in profile")

        patterns = data.get("patterns") or {}
        noise = data.get("noise") or {}
        answer_key = data.get("answer_key") or {}
        ocr = data.get("ocr") or {}
        for label, value in (
            ("patterns", patterns),
            ("noise", noise),
            ("answer_key", answer_key),
            ("ocr", ocr),
        ):
            if not isinstance(value, dict):
                raise ValueError(f"{label} must be an object")
        if not isinstance(data.get("papers"), list):
            raise ValueError("papers must be a list")
        prefixes = patterns.get("question_prefixes") or []
        if not isinstance(prefixes, list) or not all(isinstance(p, str) for p in prefixes):
            raise ValueError("patterns.question_prefixes must be a list of strings")
        if not isinstance(noise.get("texts", []), list) or not all(
            isinstance(value, str) for value in noise.get("texts", [])
        ):
            raise ValueError("noise.texts must be a list of strings")
        if not isinstance(noise.get("patterns", []), list) or not all(
            isinstance(value, str) for value in noise.get("patterns", [])
        ):
            raise ValueError("noise.patterns must be a list of strings")
        if _require(data, "questions_per_material", int, "profile") < 1:
            raise ValueError("questions_per_material must be positive")
        ocr_dpi = ocr.get("dpi", 220)
        if not isinstance(ocr_dpi, int) or not 100 <= ocr_dpi <= 400:
            raise ValueError("ocr.dpi must be an integer between 100 and 400")
        replacements = ocr.get("replacements") or {}
        if not isinstance(replacements, dict) or not all(
            isinstance(k, str) and isinstance(v, str) for k, v in replacements.items()
        ):
            raise ValueError("ocr.replacements must map strings to strings")
        badge = ocr.get("badge", r"练习题\s*(\d+)\s*套")
        if not isinstance(badge, str) or not badge:
            raise ValueError("ocr.badge must be a non-empty regex")
        _compile(badge, "ocr badge")
        heading_map = answer_key.get("heading_map") or {}
        if not isinstance(heading_map, dict) or not all(
            isinstance(k, str) and isinstance(v, str) for k, v in heading_map.items()
        ):
            raise ValueError("answer_key.heading_map must map strings to strings")

        papers = []
        for index, item in enumerate(data.get("papers") or []):
            if not isinstance(item, dict):
                raise ValueError(f"papers[{index}] must be an object")
            kind = _require(item, "kind", str, f"papers[{index}]")
            num = _require(item, "num", int, f"papers[{index}]")
            start = _require(item, "start", int, f"papers[{index}]")
            end = _require(item, "end", int, f"papers[{index}]")
            if start < 1 or end < start:
                raise ValueError(f"papers[{index}] has an invalid page range")
            papers.append(Paper(kind, num, start, end))
        if not papers:
            raise ValueError("profile must list at least one paper")
        if len({(p.kind, p.num) for p in papers}) != len(papers):
            raise ValueError("profile contains duplicate paper kind/number pairs")
        expected = data.get("expected") or {}
        if not isinstance(expected, dict) or not all(
            isinstance(k, str) and isinstance(v, int) and v > 0 for k, v in expected.items()
        ):
            raise ValueError("expected must map paper kinds to positive counts")

        overrides = {}
        for kind, by_paper in (data.get("overrides") or {}).items():
            if not isinstance(by_paper, dict):
                raise ValueError("overrides values must be objects")
            overrides[kind] = {}
            for num, by_number in by_paper.items():
                if not isinstance(by_number, dict):
                    raise ValueError("overrides paper keys must be objects")
                overrides[kind][int(num)] = {}
                for number, patch in by_number.items():
                    if not isinstance(patch, dict):
                        raise ValueError("override patches must be objects")
                    overrides[kind][int(num)][int(number)] = patch

        question_starts = {}
        for page, number in (data.get("question_starts") or {}).items():
            if not str(page).isdigit() or not isinstance(number, int) or number < 1:
                raise ValueError(
                    "question_starts must map page numbers to question numbers"
                )
            question_starts[int(page)] = number

        answer_pages = []
        for index, item in enumerate(data.get("answer_pages") or []):
            if not isinstance(item, dict):
                raise ValueError(f"answer_pages[{index}] must be an object")
            explain_start = item.get("explain_start")
            explain_end = item.get("explain_end")
            section = item.get("section")
            if section is not None and (
                not isinstance(section, str) or not section.strip()
            ):
                raise ValueError(
                    f"section in answer_pages[{index}] must be a non-empty string"
                )
            for name, value in (
                ("explain_start", explain_start),
                ("explain_end", explain_end),
            ):
                if value is not None and (not isinstance(value, int) or value < 1):
                    raise ValueError(
                        f"{name} in answer_pages[{index}] must be a positive integer"
                    )
            answer_pages.append(
                AnswerPage(
                    _require(item, "kind", str, f"answer_pages[{index}]"),
                    _require(item, "num", int, f"answer_pages[{index}]"),
                    _require(item, "pdf", str, f"answer_pages[{index}]"),
                    _require(item, "page", int, f"answer_pages[{index}]"),
                    explain_start,
                    explain_end,
                    section.strip() if section else None,
                )
            )

        answer_heading = _compile(
            _require(answer_key, "heading", str, "answer_key"), "answer heading"
        )
        if answer_heading.groups != 2:
            raise ValueError("answer_key.heading must capture label and number")
        answer_range = _compile(
            _require(answer_key, "range", str, "answer_key"), "answer range"
        )
        if answer_range.groups < 2:
            raise ValueError("answer_key.range must capture start and end")

        return cls(
            path=path,
            collection_id=collection_id,
            collection_title=collection_title,
            year=_require(data, "year", int, "profile"),
            region=data["region"],
            variant=data["variant"],
            module=data["module"],
            role=data["role"],
            paper_kind=data["paper_kind"],
            schema=_parse_schema(data.get("schema")),
            subject=_parse_subject(data.get("subject")),
            option_labels=option_labels,
            questions_per_material=_require(data, "questions_per_material", int, "profile"),
            body_margin=float(data.get("body_margin", 65)),
            top_band=float(noise.get("top_band", 70)),
            bottom_band=float(noise.get("bottom_band", 748)),
            noise_texts=tuple(noise.get("texts") or ()),
            noise_patterns=tuple(
                _compile(pattern, "noise") for pattern in noise.get("patterns") or ()
            ),
            section_re=_compile(_require(patterns, "section", str, "patterns"), "section"),
            ask_re=_compile(_require(patterns, "ask", str, "patterns"), "ask"),
            question_re=_compile(
                _require(patterns, "question", str, "patterns"), "question"
            ),
            option_re=_compile(_require(patterns, "option", str, "patterns"), "option"),
            question_prefixes=tuple(prefixes),
            answer_heading_re=answer_heading,
            answer_heading_map=dict(heading_map),
            answer_range_re=answer_range,
            answer_run_re=_compile(
                _require(answer_key, "run", str, "answer_key"), "answer run"
            ),
            papers=tuple(papers),
            expected=dict(expected),
            overrides=overrides,
            question_starts=question_starts,
            book=data.get("book"),
            answers=data.get("answers"),
            answers_required=bool(data.get("answers_required", True)),
            answer_pages=tuple(answer_pages),
            ocr_enabled=bool(ocr.get("enabled", False)),
            ocr_dpi=ocr_dpi,
            ocr_badge=badge,
            ocr_replacements=tuple(sorted(replacements.items())),
        )
