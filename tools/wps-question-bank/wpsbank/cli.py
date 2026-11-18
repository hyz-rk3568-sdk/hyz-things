"""Command line entry point for the WPS PDF question-bank importer."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

from . import emit, explain, extract
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
                if profile.answers_required:
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
    parser.add_argument(
        "--ocr",
        action="store_true",
        help="recognize scan pages through a local PaddleOCR service",
    )
    parser.add_argument(
        "--ocr-url",
        action="append",
        default=[],
        help="OCR endpoint, repeat for one worker per endpoint",
    )
    parser.add_argument("--ocr-dpi", type=int, help="override the profile's OCR dpi")
    parser.add_argument(
        "--ocr-cache",
        default=str(Path.home() / ".cache" / "hyz-wpsbank" / "ocr"),
        help="OCR result cache directory",
    )
    parser.add_argument(
        "--ocr-report",
        action="store_true",
        help="print per-page OCR confidence statistics",
    )
    parser.add_argument(
        "--detect-ocr",
        action="store_true",
        help="OCR every page, group pages by the recurring set badge, and exit",
    )
    parser.add_argument(
        "--explanations",
        action="store_true",
        help="OCR explanation pages and merge them into each question",
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
    explanations: dict = {}
    if answers_path and answers_path.is_file():
        answers = emit.parse_answers(answers_path, profile)
    elif answers_path:
        print(f"answer-key PDF not found: {answers_path}", file=sys.stderr)

    token_source = None
    if args.ocr or profile.ocr_enabled:
        from . import ocr_layout
        from .ocr import DEFAULT_URL, OcrPool

        pages = (
            list(range(1, len(document) + 1))
            if args.detect_ocr
            else sorted(
                {
                    page
                    for spec in profile.paper_specs
                    for page in range(spec[2], spec[3] + 1)
                }
            )
        )
        urls = args.ocr_url or [DEFAULT_URL]
        dpi = args.ocr_dpi or profile.ocr_dpi
        pool = OcrPool(urls, Path(args.ocr_cache).expanduser(), dpi=dpi)

        def prepare(page, pix, png):
            blanks = ocr_layout.page_blanks(page, png, dpi, profile.top_band)
            return ocr_layout.mask_blanks(png, blanks), blanks

        results = pool.recognize(
            book,
            pages,
            progress=lambda message: print(message, file=sys.stderr),
            prepare=prepare,
        )
        if profile.answer_pages:
            grouped: dict[Path, list] = {}
            for entry in profile.answer_pages:
                pdf = _resolve(entry.pdf, base)
                if pdf is None or not pdf.is_file():
                    print(f"answer PDF not found: {entry.pdf}", file=sys.stderr)
                    continue
                grouped.setdefault(pdf, []).append(entry)
            for pdf, entries in grouped.items():
                entries = sorted(entries, key=lambda item: item.page)
                answer_doc = pymupdf.open(str(pdf))
                ranges = []
                for index, entry in enumerate(entries):
                    following = (
                        entries[index + 1].page
                        if index + 1 < len(entries)
                        else len(answer_doc) + 1
                    )
                    start = entry.explain_start or entry.page
                    end = entry.explain_end or following - 1
                    start = max(1, min(start, len(answer_doc)))
                    end = max(start, min(end, len(answer_doc)))
                    ranges.append((entry, start, end))

                pages = {entry.page for entry in entries}
                if args.explanations:
                    pages.update(
                        page
                        for _, start, end in ranges
                        for page in range(start, end + 1)
                    )
                answers_by_page = pool.recognize(
                    pdf,
                    sorted(pages),
                    progress=lambda message: print(message, file=sys.stderr),
                    prepare=prepare,
                )
                for entry in entries:
                    lines = [
                        block["text"]
                        for block in answers_by_page[entry.page].blocks
                    ]
                    if entry.section:
                        lines = emit.section_lines(
                            lines,
                            entry.section,
                            {
                                item.section
                                for item in entries
                                if item.section
                            },
                        )
                    answers[(entry.kind, entry.num)] = emit.parse_answer_runs(
                        lines, profile
                    )
                print(
                    f"{pdf.name}: {len(entries)} answer pages, "
                    f"{sum(len(answers[(e.kind, e.num)]) for e in entries)} letters"
                )

                for entry, start, end in ranges:
                    if not args.explanations:
                        break
                    lines = []
                    for page_no in range(start, end + 1):
                        page_tokens = ocr_layout.tokens(
                            page_no,
                            answers_by_page[page_no],
                            profile,
                            answers_by_page[page_no].blanks,
                        )
                        lines += [
                            token
                            for token in page_tokens
                            if token["column"] == "main"
                        ]
                    parsed = explain.parse(lines, profile)
                    explanations[(entry.kind, entry.num)] = parsed
                    known = answers.setdefault((entry.kind, entry.num), {})
                    for number, item in parsed.items():
                        if not item["answer"]:
                            continue
                        if number in known and known[number] != item["answer"]:
                            print(
                                f"{entry.kind}{entry.num} Q{number}: "
                                f"key {known[number]} but explanation "
                                f"{item['answer']}",
                                file=sys.stderr,
                            )
                        # The per-question answer sits next to the explanation
                        # and survives isolated OCR errors better than the
                        # 20-letter key row.
                        known[number] = item["answer"]
                if args.explanations:
                    count = sum(
                        len(explanations[(entry.kind, entry.num)])
                        for entry, _, _ in ranges
                    )
                    page_count = sum(end - start + 1 for _, start, end in ranges)
                    print(
                        f"{pdf.name}: {len(ranges)} explanation ranges, "
                        f"{page_count} pages, {count} explanations"
                    )

        if args.detect_ocr:
            badge = re.compile(profile.ocr_badge)
            found: dict[int, set[int]] = {}
            missing = []
            for page_no in pages:
                numbers = set()
                for block in results[page_no].blocks:
                    for match in badge.finditer(block["text"]):
                        numbers.add(int(match.group(1)))
                if not numbers:
                    missing.append(page_no)
                found[page_no] = numbers
            for number, start, end in ocr_layout.badge_ranges(found, len(document)):
                print(f"{profile.paper_kind}{number}: pages {start}-{end}")
            if missing:
                print(f"no badge: {', '.join(str(page) for page in missing)}")
            return 0

        cache: dict[int, list[dict]] = {}

        def token_source(page_no: int) -> list[dict]:
            if page_no not in cache:
                page = document[page_no - 1]
                png = page.get_pixmap(dpi=dpi).tobytes("png")
                blanks = results[page_no].blanks
                if not blanks:
                    blanks = ocr_layout.page_blanks(
                        page, png, dpi, profile.top_band
                    )
                cache[page_no] = ocr_layout.tokens(
                    page_no, results[page_no], profile, blanks
                )
            return cache[page_no]

        if args.ocr_report:
            for page_no in pages:
                blocks = results[page_no].blocks
                scores = [block["confidence"] for block in blocks]
                mean = sum(scores) / len(scores) if scores else 0.0
                low = sum(1 for score in scores if score < 0.6)
                page_token_source_tokens = token_source(page_no)
                print(
                    f"ocr page {page_no}: {len(blocks)} blocks, "
                    f"{len(page_token_source_tokens)} tokens, mean {mean:.3f}, low {low}"
                )

    papers = [
        extract.build(spec, document, profile, token_source)
        for spec in profile.paper_specs
    ]
    split_options = emit.normalize_option_spacing(
        papers, profile.ocr_squeeze_han_spaces
    )
    if split_options:
        print(f"recovered spaces in {split_options} options")
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
        emit.write_paper(paper, answers, assets_dir, out_dir, profile, explanations)
    print(
        f"wrote {len(papers)} papers to {out_dir / 'collections' / profile.collection_id}; "
        f"images referenced {total_images}, assets on disk {len(list(assets_dir.iterdir()))}"
    )
    return 0
