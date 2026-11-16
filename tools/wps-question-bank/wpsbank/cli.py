"""Command line entry point for the WPS PDF question-bank importer."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

from . import emit, extract
from .profile import Profile


def _resolve(value: str | None, base: Path) -> Path | None:
    if not value:
        return None
    path = Path(value).expanduser()
    return path if path.is_absolute() else (base / path).resolve()


def validate(papers, answers: dict, profile: Profile) -> list[str]:
    problems = []
    for paper in papers:
        kind, num = paper["kind"], paper["num"]
        title = f"{kind}{num}"
        questions = paper["questions"]
        expected = profile.expected.get(kind)
        if expected is not None and len(questions) != expected:
            problems.append(f"{title}: {len(questions)} questions, expected {expected}")
        numbers = [question["number"] for question in questions]
        if numbers != list(range(1, len(questions) + 1)):
            problems.append(f"{title}: question numbers {numbers}")
        key = answers.get((kind, num), {})
        for question in questions:
            number = question["number"]
            if not question["stem"]:
                problems.append(f"{title} Q{number}: empty stem")
            labels = [option["label"] for option in question["options"]]
            if labels != list(profile.option_labels):
                problems.append(f"{title} Q{number}: options {labels}")
            for option in question["options"]:
                if not option["blocks"]:
                    problems.append(f"{title} Q{number}: empty option {option['label']}")
            if number not in key:
                problems.append(f"{title} Q{number}: missing answer")
            elif key[number] not in profile.option_labels:
                problems.append(f"{title} Q{number}: answer {key[number]!r}")
        if paper["materials"] and len(paper["materials"]) * profile.questions_per_material < len(questions):
            problems.append(f"{title}: too few materials")
    return problems


def _paper_images(paper: dict) -> int:
    count = 0
    for material in paper["materials"]:
        count += sum(1 for block in material["blocks"] if block["kind"] == "image")
    for question in paper["questions"]:
        count += sum(1 for block in question["stem"] if block["kind"] == "image")
        for option in question["options"]:
            count += sum(1 for block in option["blocks"] if block["kind"] == "image")
    return count


def report(papers, answers: dict, profile: Profile) -> None:
    for paper in papers:
        kind, num = paper["kind"], paper["num"]
        key = answers.get((kind, num), {})
        print(
            f"{kind}{num}: {len(paper['questions'])} questions, "
            f"{len(paper['materials'])} materials, {_paper_images(paper)} images, "
            f"{len(key)} answers, aux={len(paper['aux'])}"
        )
    print("answer entries:", sum(len(items) for items in answers.values()))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Convert a WPS 文字 question-book PDF into hyz question-bank Markdown."
    )
    parser.add_argument("--profile", required=True, help="book profile JSON")
    parser.add_argument("--book", help="override the profile's question-book PDF")
    parser.add_argument("--answers", help="override the profile's answer-key PDF")
    parser.add_argument("--out", default="out", help="output root (default: ./out)")
    parser.add_argument(
        "--report-only",
        action="store_true",
        help="extract and validate without writing Markdown",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="write output even when validation reports problems",
    )
    parser.add_argument(
        "--debug-page",
        type=int,
        help="render one 1-based page to PNG and exit",
    )
    parser.add_argument(
        "--debug-out",
        default="wps-debug",
        help="directory for --debug-page (default: ./wps-debug)",
    )
    parser.add_argument(
        "--detect",
        action="store_true",
        help="print page ranges found from paper headings and exit",
    )
    return parser


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    profile_path = Path(args.profile)
    try:
        profile = Profile.load(profile_path)
    except ValueError as error:
        print(f"profile: {error}", file=sys.stderr)
        return 2

    base = profile_path.resolve().parent
    book = _resolve(args.book or profile.book, base)
    answers_path = _resolve(args.answers or profile.answers, base)
    if book is None or not book.is_file():
        print(f"question-book PDF not found: {book}", file=sys.stderr)
        return 2

    import pymupdf

    document = pymupdf.open(str(book))
    if args.detect:
        detected = extract.detect_papers(document, profile)
        configured = {tuple(spec[:2]): tuple(spec[2:]) for spec in profile.paper_specs}
        for kind, num, start, end in detected:
            match = configured.get((kind, num))
            status = "match" if match == (start, end) else f"profile={match}"
            print(f"{kind}{num}: pages {start}-{end} ({status})")
        missing = {key for key in configured if key not in {(k, n) for k, n, _, _ in detected}}
        for kind, num in sorted(missing):
            print(f"{kind}{num}: heading not found (profile={configured[(kind, num)]})")
        return 0
    if args.debug_page:
        if not 1 <= args.debug_page <= len(document):
            print(f"page {args.debug_page} outside 1..{len(document)}", file=sys.stderr)
            return 2
        output = Path(args.debug_out)
        output.mkdir(parents=True, exist_ok=True)
        target = output / f"page-{args.debug_page}.png"
        document[args.debug_page - 1].get_pixmap(dpi=150).save(str(target))
        print(target)
        return 0

    answers = {}
    if answers_path and answers_path.is_file():
        answers = emit.parse_answers(answers_path, profile)
    elif answers_path:
        print(f"answer-key PDF not found: {answers_path}", file=sys.stderr)

    papers = [extract.build(spec, document, profile) for spec in profile.paper_specs]
    report(papers, answers, profile)
    sys.stdout.flush()
    problems = validate(papers, answers, profile)
    if problems:
        print("problems:", file=sys.stderr)
        for problem in problems[:40]:
            print(f"  {problem}", file=sys.stderr)
        if len(problems) > 40:
            print(f"  ... {len(problems) - 40} more", file=sys.stderr)
        if not args.force:
            print(f"{len(problems)} problems; nothing written (use --force to emit)", file=sys.stderr)
            return 1
    if args.report_only:
        return 0

    out_dir = Path(args.out)
    assets_dir = out_dir / "assets"
    assets_dir.mkdir(parents=True, exist_ok=True)
    total_images = 0
    for paper in papers:
        total_images += emit.prepare_assets(paper, assets_dir)
        emit.write_paper(paper, answers, assets_dir, out_dir, profile)
    print(
        f"wrote {len(papers)} papers to {out_dir / 'collections' / profile.collection_id}; "
        f"images referenced {total_images}, assets on disk {len(list(assets_dir.iterdir()))}"
    )
    return 0
