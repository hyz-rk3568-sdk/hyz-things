"""Turn OCR blocks into importer tokens and restore fill-in-the-blank rules."""

from __future__ import annotations

from io import BytesIO
import re

from PIL import Image, ImageDraw

from .ocr import OcrPage

OPTION_SPLIT = re.compile(r"(?<![A-Za-z])([A-D])[.．、]\s*")


def clean_ocr(value: str) -> str:
    """OCR keeps meaningful spaces between option words, so only squeeze whitespace."""
    value = value.replace("\u00a0", " ").replace("\u3000", " ")
    value = re.sub(r"[ \t]+", " ", value).strip()
    value = re.sub(r"(?<=[A-Za-z0-9])．(?=\d)", ".", value)
    return value


def _merge_boxes(boxes: list[list[float]], y_gap: float = 3) -> list[list[float]]:
    boxes = sorted(boxes, key=lambda box: (box[1], box[0]))
    merged: list[list[float]] = []
    for box in boxes:
        for last in merged:
            # Compare with every earlier box: two rules at the same height can
            # be separated by a rule between them and still be one streak.
            if (
                box[1] <= last[3] + y_gap
                and not (box[2] < last[0] - 2 or box[0] > last[2] + 2)
            ):
                last[0] = min(last[0], box[0])
                last[1] = min(last[1], box[1])
                last[2] = max(last[2], box[2])
                last[3] = max(last[3], box[3])
                break
        else:
            merged.append(list(box))
    return merged


def vector_blanks(page, dpi: int, top_band: float) -> list[list[float]]:
    """Filled thin rectangles in a vector-outline page, in image pixels."""
    scale = dpi / 72
    boxes = []
    for drawing in page.get_drawings():
        if drawing.get("fill") is None:
            continue
        rect = drawing["rect"]
        if rect.height > 2.5 or not 20 <= rect.width <= 120:
            continue
        if rect.y0 < top_band:
            continue
        boxes.append(
            [rect.x0 * scale, rect.y0 * scale, rect.x1 * scale, rect.y1 * scale]
        )
    return _merge_boxes(boxes, y_gap=1)


def _dash_like(
    data: bytes, width: int, height: int, box: list[float], margin: int
) -> bool:
    """A rule at the vertical middle of adjacent glyphs is a dash, not a blank."""
    x0, y0, x1, _ = (int(value) for value in box)
    rows = []
    for left, right in ((x0 - 2 - margin, x0 - 2), (x1 + 2, x1 + 2 + margin)):
        left, right = max(0, left), min(width, right)
        if right <= left:
            continue
        for y in range(max(0, y0 - 35), min(height, y0 + 12)):
            if any(data[y * width + x] < 180 for x in range(left, right)):
                rows.append(y)
    if len(rows) < 4:
        return False
    top, bottom = min(rows), max(rows)
    if bottom - top < 8:
        return False
    position = (y0 - top) / (bottom - top)
    return 0.35 <= position <= 0.8


def image_blanks(png: bytes, blocks: list[dict], dpi: int) -> list[list[float]]:
    """Find printed rules in a scanned page.

    A blank is a thin horizontal rule roughly one to three characters wide.
    Scan pixel values vary, so the dark threshold is deliberately light; text
    strokes stay much shorter than the minimum run length.
    """
    image = Image.open(BytesIO(png)).convert("L")
    width, height = image.size
    if width < 100 or height < 100:
        return []
    scale = dpi / 220
    min_length = max(30, round(65 * scale))
    max_length = max(min_length + 1, round(190 * scale))
    # At 150 dpi a printed rule can still cover three scan rows.
    max_height = max(3, round(4 * scale))
    margin = max(2, round(4 * scale))
    data = image.tobytes()
    threshold = bytes(0 if value < 210 else 1 for value in range(256))
    candidates: list[list[float]] = []
    for y in range(20, height - 20):
        row = data[y * width : (y + 1) * width].translate(threshold)
        start = row.find(b"\x00")
        while start != -1:
            end = start
            while end < width and row[end] == 0:
                end += 1
            length = end - start
            if min_length <= length <= max_length:
                candidates.append([float(start), float(y), float(end), float(y + 1)])
            start = row.find(b"\x00", end)
    if not candidates:
        return []

    # Keep only rules with mostly white pixels immediately above and below.
    merged = _merge_boxes(candidates, y_gap=3)
    kept = []
    for box in merged:
        x0, y0, x1, y1 = (int(value) for value in box)
        if y1 - y0 > max_height or x1 - x0 > max_length:
            continue
        window = range(x0, min(x1, width))
        dark_above = sum(
            1
            for y in range(max(0, y0 - margin), max(0, y0 - 1))
            for x in window
            if data[y * width + x] < 180
        )
        dark_below = sum(
            1
            for y in range(min(height, y1 + 1), min(height, y1 + margin))
            for x in window
            if data[y * width + x] < 180
        )
        area = max(1, (min(x1, width) - x0) * (margin - 1))
        if dark_above / area > 0.2 or dark_below / area > 0.2:
            continue
        if _dash_like(data, width, height, box, margin):
            continue
        kept.append([float(x0), float(y0), float(x1), float(y1)])
    return _merge_boxes(kept, y_gap=3)


def page_blanks(
    page, png: bytes, dpi: int, top_band: float
) -> list[list[float]]:
    """Blank rules on a page, without needing OCR blocks."""
    boxes = vector_blanks(page, dpi, top_band)
    if boxes:
        return boxes
    return image_blanks(png, [], dpi)


def badge_ranges(
    found: dict[int, set[int]], total: int
) -> list[tuple[int, int, int]]:
    """Turn per-page set badges into ``(number, start, end)`` ranges.

    A page that mentions several sets is a table of contents, so it does not
    define a start. Missed badges are tolerated: every set runs until the page
    before the next set starts.
    """
    first: dict[int, int] = {}
    for page, numbers in found.items():
        if len(numbers) != 1:
            continue
        number = next(iter(numbers))
        first[number] = min(first.get(number, page), page)
    ordered = sorted(first)
    ranges = []
    for index, number in enumerate(ordered):
        end = (
            first[ordered[index + 1]] - 1
            if index + 1 < len(ordered)
            else total
        )
        ranges.append((number, first[number], end))
    return ranges


def detect_blanks(page, png: bytes, ocr_page: OcrPage, top_band: float) -> list[list[float]]:
    return page_blanks(page, png, ocr_page.dpi, top_band)


def mask_blanks(png: bytes, blanks: list[list[float]]) -> bytes:
    """Erase blank rules so OCR splits a line instead of merging across it."""
    if not blanks:
        return png
    image = Image.open(BytesIO(png)).convert("RGB")
    draw = ImageDraw.Draw(image)
    for x0, y0, x1, y1 in blanks:
        draw.rectangle([x0 - 2, y0 - 6, x1 + 2, y1 + 5], fill="white")
    output = BytesIO()
    image.save(output, format="PNG")
    return output.getvalue()


def _runs(text: str) -> list[dict]:
    """Split a block that contains several option labels (fallback path)."""
    matches = list(OPTION_SPLIT.finditer(text))
    if len(matches) < 2 or matches[0].start() != 0:
        return [{"text": text}]
    parts = []
    for index, match in enumerate(matches):
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        parts.append({"text": text[match.start() : end].strip()})
    return parts


def _line_overlap(line: dict, baseline: float, margin: float = 3) -> float:
    top, bottom = line["top"] - margin, line["bottom"] + margin
    return min(baseline + 1, bottom) - max(baseline - 1, top)


def _visual_lines(tokens: list[dict]) -> list[dict]:
    """Group tokens into lines by vertical overlap, tolerating skewed scans."""
    lines: list[dict] = []
    for token in sorted(tokens, key=lambda item: item["bbox"][1]):
        top, bottom = token["bbox"][1], token["bbox"][3]
        for line in reversed(lines):
            overlap = min(bottom, line["bottom"]) - max(top, line["top"])
            shorter = min(bottom - top, line["bottom"] - line["top"])
            if shorter > 0 and overlap >= 0.5 * shorter:
                line["tokens"].append(token)
                line["top"] = min(line["top"], top)
                line["bottom"] = max(line["bottom"], bottom)
                break
        else:
            lines.append({"top": top, "bottom": bottom, "tokens": [token]})
    return lines


def _line_above(
    raw: list[dict], baseline: float, center_x: float, profile
) -> dict | None:
    """A blank alone on its line belongs to the end of the paragraph above."""
    if baseline < profile.top_band:
        return None
    best = None
    for token in raw:
        bottom = token["bbox"][3]
        if bottom > baseline or baseline - bottom > 25:
            continue
        if not (token["bbox"][0] - 20 <= center_x <= token["bbox"][2] + 20):
            continue
        if best is None or bottom > best["bbox"][3]:
            best = token
    return best


def tokens(
    page_no: int,
    ocr_page: OcrPage,
    profile,
    blanks: list[list[float]],
) -> list[dict]:
    scale = ocr_page.dpi / 72
    raw = []
    lone_marks = []
    for block in ocr_page.blocks:
        text = clean_ocr(block["text"])
        text = profile.correct(text)
        if not text:
            continue
        x0, y0, x1, y1 = block["box"]
        bbox = [x0 / scale, y0 / scale, x1 / scale, y1 / scale]
        if text in {"O", "o", "0", "_", "__"} and (x1 - x0) < 60:
            # A full stop at the end of a scanned line is often recognised as a
            # lone "o" or "0". Keep it for the check below, other marks drop.
            if text in {"O", "o", "0"}:
                lone_marks.append(block["box"])
            continue
        if profile.is_noise(text, bbox):
            continue
        for part in _runs(text):
            raw.append(
                {
                    "kind": "text",
                    "text": part["text"],
                    "x": bbox[0],
                    "y": bbox[1],
                    "sy": (bbox[1] + bbox[3]) / 2,
                    "bbox": bbox,
                    "page": page_no,
                    "ocr": True,
                }
            )

    def overlaps(box: list[float], top: float, bottom: float) -> bool:
        return min(box[3], bottom) - max(box[1], top) > 0

    for x0, y0, x1, y1 in lone_marks:
        left, right = x0 / scale, x1 / scale
        top, bottom = y0 / scale, y1 / scale
        bbox = [left, top, right, bottom]
        if profile.is_noise("。", bbox):
            continue
        # A lone mark is only a full stop when text precedes it on the line and
        # nothing follows: a bullet or a page number has no such left anchor.
        anchor = max(
            (
                token
                for token in raw
                if token["bbox"][2] <= left + 1
                and overlaps(token["bbox"], top, bottom)
            ),
            key=lambda token: token["bbox"][2],
            default=None,
        )
        if anchor is None or anchor["text"].rstrip().endswith(
            ("。", "！", "？", "…", "”")
        ):
            continue
        if any(
            token["bbox"][0] >= right - 1 and overlaps(token["bbox"], top, bottom)
            for token in raw
        ):
            continue
        raw.append(
            {
                "kind": "text",
                "text": "。",
                "x": left,
                "y": top,
                "sy": (top + bottom) / 2,
                "bbox": bbox,
                "page": page_no,
                "ocr": True,
            }
        )

    min_blank = 65 * (ocr_page.dpi / 220)
    lines = _visual_lines(raw)
    before: dict[int, list[float]] = {}
    after: dict[int, list[float]] = {}
    for blank in _merge_boxes(blanks, y_gap=3):
        x0, y0, x1, y1 = blank
        # Chinese em dashes are about half the width of a fill-in blank. Older
        # caches kept them as blanks, so enforce the width here as well.
        if x1 - x0 < min_blank:
            continue
        # A printed rule sits on the text baseline, so match the token's bottom
        # edge rather than its vertical centre.
        baseline = ((y0 + y1) / 2) / scale
        center_x = ((x0 + x1) / 2) / scale
        target, side = None, "after"
        for line in sorted(lines, key=lambda item: -_line_overlap(item, baseline)):
            if _line_overlap(line, baseline) <= 0:
                break
            # A rule printed over glyphs is an underline, not a blank. Masking
            # before OCR makes real blanks split the line into blocks.
            if any(
                token["bbox"][0] < center_x < token["bbox"][2]
                and token["bbox"][1] < baseline < token["bbox"][3]
                for token in line["tokens"]
            ):
                break
            left = sorted(
                (
                    token
                    for token in line["tokens"]
                    if token["bbox"][2] <= center_x + 2
                ),
                key=lambda token: token["bbox"][2],
                reverse=True,
            )
            if left:
                target, side = left[0], "after"
                break
            right = sorted(
                (
                    token
                    for token in line["tokens"]
                    if token["bbox"][0] >= center_x - 2
                ),
                key=lambda token: token["bbox"][0],
            )
            if right:
                target, side = right[0], "before"
                break
        if target is None:
            target = _line_above(raw, baseline, center_x, profile)
        if target is None:
            continue
        bucket = after if side == "after" else before
        bucket.setdefault(id(target), []).append(center_x)
    for token in raw:
        prefix = before.get(id(token))
        if prefix:
            token["text"] = (
                " ".join("______" for _ in sorted(prefix))
                + " "
                + token["text"].lstrip()
            )
        suffix = after.get(id(token))
        if suffix:
            token["text"] = token["text"].rstrip() + "".join(
                " ______" for _ in sorted(suffix)
            )

    def cluster_lines(items: list[dict]) -> list[dict]:
        """Cluster tokens into visual lines and read each line left to right."""
        lines: list[dict] = []
        for token in sorted(items, key=lambda value: value["sy"]):
            for line in lines:
                if abs(token["sy"] - line["sy"]) <= 8:
                    line["tokens"].append(token)
                    break
            else:
                lines.append({"sy": token["sy"], "tokens": [token]})
        result: list[dict] = []
        for line in sorted(lines, key=lambda value: value["sy"]):
            result.extend(sorted(line["tokens"], key=lambda token: token["x"]))
        return result

    # Some explanation pages put notes in a narrow sidebar. A full-height
    # whitespace gutter with a narrow side and a wide side identifies it.
    # Option columns never form a gutter because prose lines cross between them.
    for token in raw:
        token["column"] = "main"
    if not raw:
        return []
    # Ignore the cover table and answer-key row above the body: they can span
    # the gutter and would hide it even when the explanation is two-column.
    # Judge by the share of horizontal bands a column of whitespace spans, so
    # a note that only covers the top of the page still counts as a sidebar.
    max_x = int(max(token["bbox"][2] for token in raw)) + 1
    page_height = getattr(ocr_page, "height", 0) / scale
    if page_height <= 0:
        page_height = max(token["bbox"][3] for token in raw)
    bands = 8
    band_height = max(page_height, 1) / bands
    covered = [bytearray(max_x + 2) for _ in range(bands)]
    for token in raw:
        x0, y0, x1, y1 = token["bbox"]
        first = max(0, min(bands - 1, int(y0 / band_height)))
        last = max(0, min(bands - 1, int(y1 / band_height)))
        for band in range(first, last + 1):
            row = covered[band]
            for x in range(max(0, int(x0)), min(max_x, int(x1)) + 1):
                row[x] = 1
    needed = bands * 3 // 4

    def free(x: int) -> bool:
        return sum(1 for band in range(bands) if not covered[band][x]) >= needed

    starts = sorted({token["bbox"][0] for token in raw})
    edges = sorted({token["bbox"][2] for token in raw})
    candidates = []
    for edge in edges:
        for start in starts:
            if start - edge >= 10:
                candidates.append((edge, start))
                break
    chosen = None
    for start, end in candidates:
        if not free(int((start + end) / 2)):
            continue
        middle = (start + end) / 2
        left = [token for token in raw if token["bbox"][2] <= middle]
        right = [token for token in raw if token["bbox"][0] >= middle]
        if len(left) < 3 or len(right) < 3:
            continue
        left_wide = max(token["bbox"][2] - token["bbox"][0] for token in left)
        right_wide = max(token["bbox"][2] - token["bbox"][0] for token in right)
        if not (
            (left_wide >= 200 and right_wide < 200)
            or (right_wide >= 200 and left_wide < 200)
        ):
            continue
        # Short line endings can look like a narrow column. The real gutter is
        # the rightmost candidate: beyond it lies the body or the page margin.
        if chosen is None or start > chosen[0]:
            chosen = (start, end, middle, left_wide, right_wide)
    if chosen is None:
        # Scan OCR sometimes stretches a body box across the gutter, so no
        # pixel column stays clear. The sidebar still starts far left of every
        # body line; use that start-x gap as a last resort.
        starts = sorted({token["bbox"][0] for token in raw})
        fallback = None
        for left_start, right_start in zip(starts, starts[1:]):
            gap = right_start - left_start
            if gap < 60:
                continue
            middle = (left_start + right_start) / 2
            left = [token for token in raw if token["bbox"][0] < middle]
            right = [token for token in raw if token["bbox"][0] >= middle]
            if len(left) < 6 or len(right) < 6:
                continue
            left_wide = max(token["bbox"][2] - token["bbox"][0] for token in left)
            right_wide = max(token["bbox"][2] - token["bbox"][0] for token in right)
            if left_wide < 200 and right_wide >= 200:
                side = "left"
            elif right_wide < 200 and left_wide >= 200:
                side = "right"
            else:
                continue
            if fallback is None or gap > fallback[0]:
                fallback = (gap, middle, side)
        if fallback is not None:
            _, middle, side = fallback
            side_tokens = [
                token
                for token in raw
                if (token["bbox"][0] < middle) == (side == "left")
            ]
            main_tokens = [
                token
                for token in raw
                if (token["bbox"][0] < middle) != (side == "left")
            ]
            for token in side_tokens:
                token["column"] = "side"
            return cluster_lines(main_tokens) + cluster_lines(side_tokens)
    if chosen is not None:
        start, end, middle, left_wide, right_wide = chosen
        # Classify by where a token starts: a note at the top of the page may
        # end a little past the gutter and still belong to the sidebar.
        if left_wide >= 200 and right_wide < 200:
            # Main column on the left, narrow notes on the right.
            side_tokens = [
                token for token in raw if token["bbox"][0] > middle
            ]
            main_tokens = [
                token for token in raw if token["bbox"][0] <= middle
            ]
        else:
            # Narrow notes on the left, main column on the right.
            side_tokens = [
                token for token in raw if token["bbox"][0] < middle
            ]
            main_tokens = [
                token for token in raw if token["bbox"][0] >= middle
            ]
        for token in side_tokens:
            token["column"] = "side"
        return cluster_lines(main_tokens) + cluster_lines(side_tokens)
    return cluster_lines(raw)
