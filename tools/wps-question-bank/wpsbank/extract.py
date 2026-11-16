"""Extract papers, materials, questions, tables and images from a WPS PDF."""

from __future__ import annotations

import hashlib
import re

import pymupdf

from .profile import Profile
from .text import clean, clean_cell, inside, numeric_cell, row_has_numeric


def line_objects(page) -> list[dict]:
    """Group spans into lines and merge same-row fragments."""
    raw = []
    for block in page.get_text("dict")["blocks"]:
        for line in block.get("lines", []):
            value = "".join(span["text"] for span in line["spans"])
            if not value.strip():
                continue
            math = [
                (span["text"], list(span["bbox"]), span["size"])
                for span in line["spans"]
                if "Cambria" in span["font"]
            ]
            baseline = [
                span["origin"][1]
                for span in line["spans"]
                if "Cambria" not in span["font"]
            ]
            if not baseline:
                baseline = [span["origin"][1] for span in line["spans"]]
            raw.append(
                {
                    "bbox": list(line["bbox"]),
                    "text": value,
                    "math": math,
                    "size": max(span["size"] for span in line["spans"]),
                    "baseline": max(baseline),
                }
            )
    raw.sort(key=lambda item: (round(item["bbox"][1], 1), item["bbox"][0]))
    merged = []
    for item in raw:
        if merged:
            previous = merged[-1]
            gap = item["bbox"][0] - previous["bbox"][2]
            if abs(previous["bbox"][1] - item["bbox"][1]) < 3 and -2 < gap < 20:
                previous["text"] = (
                    previous["text"] + (" " if gap > 4 else "") + item["text"]
                )
                previous["bbox"][2] = max(previous["bbox"][2], item["bbox"][2])
                previous["bbox"][3] = max(previous["bbox"][3], item["bbox"][3])
                previous["baseline"] = max(previous["baseline"], item["baseline"])
                previous["math"].extend(item["math"])
                previous["size"] = max(previous["size"], item["size"])
                continue
        merged.append(item)
    return merged


def grid_edges(trows) -> list[float]:
    """Vertical boundaries of every cell, so a merged label column still counts."""
    edges = []
    for row in trows:
        for cell in row.cells:
            if not cell:
                continue
            for value in (cell[0], cell[2]):
                if not edges or all(abs(value - edge) > 3 for edge in edges):
                    edges.append(value)
    return sorted(edges)


def project_row(row, cells, edges, duplicate: bool) -> list[str]:
    """Map a raw table row onto the drawn column grid.

    Header merges need their text repeated under every covered column so the
    sub-header can be appended; a data row's wide label belongs to one cell.
    """
    columns = max(len(edges) - 1, 1)
    output = [""] * columns
    for index, cell in enumerate(cells):
        if not cell or index >= len(row):
            continue
        value = "" if row[index] is None else str(row[index])
        if not value.strip():
            continue
        covered = [
            k
            for k in range(columns)
            if cell[0] - 1 <= (edges[k] + edges[k + 1]) / 2 <= cell[2] + 1
        ]
        if not covered:
            k = next(
                (
                    k
                    for k in range(columns)
                    if edges[k] - 1 <= cell[0] <= edges[k + 1] + 1
                ),
                columns - 1,
            )
            covered = [k]
        for k in covered if duplicate else covered[:1]:
            if not output[k]:
                output[k] = value
    return output


def is_artifact_row(row) -> bool:
    return (
        bool(row)
        and re.fullmatch(r"\d{1,2}", row[0] or "")
        and len(row) > 1
        and row[1] in {"．", "."}
    )


def tidy_table(rows) -> list[list[str]]:
    rows = [[clean_cell(cell) for cell in row] for row in rows]
    while rows and is_artifact_row(rows[0]):
        rows.pop(0)
    if not rows:
        return []
    width = len(rows[0])
    rows = [row[:width] + [""] * (width - len(row)) for row in rows]
    keep = [index for index in range(width) if any(row[index] for row in rows)]
    rows = [[row[index] for index in keep] for row in rows]
    if not rows or len(rows[0]) < 2:
        return []
    return rows


def count_header_rows(trows, rows) -> int:
    """Header rows are everything above the first numeric data row."""
    for index, row in enumerate(rows):
        cells = [cell for cell in row if cell is not None and str(cell).strip()]
        if cells and row_has_numeric(row):
            return max(index, 1)
        if index > 0:
            boxes = {tuple(round(value, 1) for value in cell) for cell in trows[index].cells if cell}
            if len(boxes) == 1:
                # Full-width merged section label starts the data body.
                return index
    return 1


def explode_band(page, bbox, edges) -> list[list[str]]:
    """Rebuild visual rows from a table band that has no horizontal rules."""
    items = []
    for block in page.get_text("dict")["blocks"]:
        for line in block.get("lines", []):
            value = "".join(span["text"] for span in line["spans"]).strip()
            if not value:
                continue
            x0, y0, x1, y1 = line["bbox"]
            if not (bbox[0] - 1 <= (x0 + x1) / 2 <= bbox[2] + 1):
                continue
            if not (bbox[1] - 1 <= (y0 + y1) / 2 <= bbox[3] + 1):
                continue
            items.append(((y0 + y1) / 2, x0, clean(value)))
    items.sort()
    clusters = []
    for center, x0, value in items:
        if clusters and abs(clusters[-1][0] - center) <= 4:
            clusters[-1][1].append((x0, value))
        else:
            clusters.append([center, [(x0, value)]])
    width = max(len(edges) - 1, 1)
    rows = []
    for _, line_items in clusters:
        cells = [""] * width
        for x0, value in sorted(line_items):
            column = width - 1
            for k in range(width):
                if edges[k] - 1 <= x0 <= edges[k + 1] + 1:
                    column = k
                    break
            cells[column] = (
                f"{cells[column]} {value}".strip() if cells[column] else value
            )
        rows.append(cells)
    return rows


def header_from_cells(rows) -> list[str]:
    """Combine header rows per column, stripping repeated sub-header suffixes."""
    if not rows:
        return [""]
    entries = [
        (index, clean_cell(cell))
        for index, row in enumerate(rows)
        for cell in row
        if cell
    ]
    width = max(len(row) for row in rows)
    header = []
    for column in range(width):
        parts = []
        for index, row in enumerate(rows):
            value = clean_cell(row[column]) if column < len(row) and row[column] else ""
            if value and value not in parts:
                parts.append(value)
        cleaned = []
        for index, part in enumerate(parts):
            for other_index, other in entries:
                if other_index > index and other != part and part.endswith(other):
                    rest = part[: -len(other)].strip()
                    if rest:
                        part = rest
            if part and part not in cleaned:
                cleaned.append(part)
        header.append(" ".join(cleaned))
    return header


def header_polluted(page, trows, edges) -> bool:
    """A merged header cell whose text extraction swallowed other columns."""
    if not trows or not edges:
        return False
    columns = len(edges) - 1
    cells = {tuple(round(value, 1) for value in cell) for cell in trows[0].cells if cell}
    for cell in cells:
        covered = sum(
            1
            for k in range(columns)
            if cell[0] - 1 <= (edges[k] + edges[k + 1]) / 2 <= cell[2] + 1
        )
        if covered < 2:
            continue
        spans = 0
        for block in page.get_text("dict")["blocks"]:
            for line in block.get("lines", []):
                for span in line["spans"]:
                    if not span["text"].strip():
                        continue
                    bbox = span["bbox"]
                    center_x = (bbox[0] + bbox[2]) / 2
                    center_y = (bbox[1] + bbox[3]) / 2
                    if cell[0] <= center_x <= cell[2] and cell[1] <= center_y <= cell[3]:
                        spans += 1
        if spans > 2 * covered:
            return True
    return False


def header_from_spans(page, trows, header_rows: int, edges) -> list[str]:
    """Build the header from positioned spans.

    WPS sometimes draws several columns' header text on one baseline, so cell
    text extraction concatenates unrelated labels. Spans keep their own x.
    """
    if header_rows <= 0 or not edges:
        return [""]
    top = trows[0].bbox[1]
    bottom = trows[header_rows].bbox[1] if header_rows < len(trows) else trows[-1].bbox[3]
    columns = max(len(edges) - 1, 1)
    cells = [[] for _ in range(columns)]
    for block in page.get_text("dict")["blocks"]:
        for line in block.get("lines", []):
            for span in line["spans"]:
                value = span["text"].strip()
                if not value:
                    continue
                x0, y0, x1, y1 = span["bbox"]
                center_y = (y0 + y1) / 2
                if not (top - 1 <= center_y <= bottom + 1):
                    continue
                covered = [
                    k
                    for k in range(columns)
                    if x0 - 1 <= (edges[k] + edges[k + 1]) / 2 <= x1 + 1
                ]
                if not covered:
                    center_x = (x0 + x1) / 2
                    covered = [
                        next(
                            (
                                k
                                for k in range(columns)
                                if edges[k] - 1 <= center_x <= edges[k + 1] + 1
                            ),
                            0,
                        )
                    ]
                for k in covered:
                    cells[k].append((y0, x0, value))
    header = []
    for items in cells:
        items.sort()
        groups = []
        for y, x, value in items:
            if groups and abs(groups[-1][0] - y) <= 4:
                groups[-1][1].append((x, value))
            else:
                groups.append([y, [(x, value)]])
        header.append(
            "".join(
                "".join(text for _, text in sorted(group[1])) for group in groups
            )
        )
    return header


def table_layout(page, table) -> dict | None:
    """Project one detected table onto its drawn grid."""
    rows = table.extract()
    trows = table.rows
    skip = 0
    while skip < len(rows) and is_artifact_row(rows[skip]):
        skip += 1
    rows = rows[skip:]
    trows = trows[skip:]
    if not rows:
        return None
    if not any(row_has_numeric(row) for row in rows):
        header_rows = len(rows)
    else:
        header_rows = count_header_rows(trows, rows)
    width = max(len(row) for row in rows)
    rows = [list(row) + [None] * (width - len(row)) for row in rows]
    edges = grid_edges(trows)
    filled = [
        project_row(
            row, trows[index].cells if index < len(trows) else [], edges, index < header_rows
        )
        for index, row in enumerate(rows)
    ]
    data = []
    for index in range(header_rows, len(filled)):
        if sum(1 for cell in filled[index] if "\n" in str(cell)) > 1:
            data.extend(explode_band(page, trows[index].bbox, edges))
        else:
            data.append(filled[index])
    header = (
        header_from_spans(page, trows, header_rows, edges)
        if header_polluted(page, trows, edges)
        else header_from_cells(filled[:header_rows])
    )
    return {
        "bbox": list(table.bbox),
        "header": header,
        "data": data,
        "raw": filled[:header_rows] + data,
    }


def rows_to_markdown(rows) -> str | None:
    rows = tidy_table(rows)
    if not rows:
        return None
    width = len(rows[0])
    output = ["| " + " | ".join(rows[0]) + " |"]
    align = []
    for column in range(width):
        values = [row[column] for row in rows[1:] if column < len(row) and row[column]]
        align.append(
            bool(values)
            and all(numeric_cell(value) for value in values)
        )
    output.append("| " + " | ".join("---:" if a else "---" for a in align) + " |")
    for row in rows[1:]:
        output.append("| " + " | ".join(cell.replace("|", "\\|") for cell in row) + " |")
    return "\n".join(output)


def table_markdown(page, table) -> str | None:
    layout = table_layout(page, table)
    if not layout:
        return None
    return rows_to_markdown([layout["header"]] + layout["data"])


_repeated_cache: dict[int, set[str]] = {}


def repeated_image_hashes(document, profile: Profile) -> set[str]:
    """Image hashes stamped on many pages: ad strips repeated by the typesetter."""
    key = id(document)
    if key not in _repeated_cache:
        counts: dict[str, int] = {}
        for page in document:
            for image in page.get_images(full=True):
                try:
                    box = page.get_image_bbox(image)
                except Exception:
                    continue
                if box.height >= 30 or box.width < 40:
                    continue
                info = document.extract_image(image[0])
                if not info or not info.get("image"):
                    continue
                digest = hashlib.sha256(info["image"]).hexdigest()
                counts[digest] = counts.get(digest, 0) + 1
        _repeated_cache[key] = {digest for digest, count in counts.items() if count >= 3}
    return _repeated_cache[key]


def page_tokens(page, document, page_no: int, profile: Profile) -> list[dict]:
    lines = [
        line
        for line in line_objects(page)
        if not profile.is_noise(line["text"], line["bbox"])
    ]
    tables = [
        table
        for table in page.find_tables().tables
        if table.row_count >= 2
        and table.col_count >= 2
        and (table.bbox[2] - table.bbox[0]) >= 80
    ]
    table_boxes = [list(table.bbox) for table in tables]

    images = []
    for image in page.get_images(full=True):
        try:
            box = page.get_image_bbox(image)
        except Exception:
            continue
        if box.y1 < profile.top_band or box.width < 40 or box.height < 12:
            continue
        info = document.extract_image(image[0])
        if not info or not info.get("image"):
            continue
        digest = hashlib.sha256(info["image"]).hexdigest()
        if box.height < 20 and digest in repeated_image_hashes(document, profile):
            continue
        images.append(
            {"bbox": list(box), "data": info["image"], "ext": info["ext"]}
        )

    cell_texts = set()
    for table in tables:
        for row in table.extract():
            for cell in row:
                if cell:
                    cell_texts.add(clean_cell(cell))

    # A logical table can be cut into several detected tables by full-width
    # section labels ("按行业类别分"). Merge those back and turn the label into
    # a row instead of leaving a paragraph between table fragments.
    layouts = [
        layout
        for layout in (table_layout(page, table) for table in tables)
        if layout
    ]
    table_tokens = []
    consumed = set()
    index = 0
    while index < len(layouts):
        group = [layouts[index]]
        labels = []
        next_index = index + 1
        while next_index < len(layouts):
            previous, following = group[-1], layouts[next_index]
            same = (
                abs(previous["bbox"][0] - following["bbox"][0]) < 3
                and abs(previous["bbox"][2] - following["bbox"][2]) < 3
            )
            gap = following["bbox"][1] - previous["bbox"][3]
            if not same or gap > 30:
                break
            middle_lines = [
                line
                for line in lines
                if line["bbox"][1] >= previous["bbox"][3] - 1
                and line["bbox"][3] <= following["bbox"][1] + 1
            ]
            middle = "".join(clean(line["text"]) for line in middle_lines)
            if not middle or "表" in middle or len(middle) > 20:
                break
            labels.append((middle, middle_lines))
            group.append(following)
            next_index += 1
        rows = [group[0]["header"]] + group[0]["data"]
        all_rows = list(rows)
        for extra in group[1:]:
            all_rows.extend(extra["raw"])
        width = max(len(row) for row in all_rows)
        for extra, (middle, middle_lines) in zip(group[1:], labels):
            rows.append([middle] + [""] * (width - 1))
            rows.extend(extra["raw"])
            consumed.update(id(line) for line in middle_lines)
        markdown = rows_to_markdown(rows)
        if markdown:
            bbox = [
                group[0]["bbox"][0],
                group[0]["bbox"][1],
                group[-1]["bbox"][2],
                group[-1]["bbox"][3],
            ]
            table_tokens.append(
                {
                    "kind": "table",
                    "y": bbox[1],
                    "sy": bbox[1] + 2,
                    "x": bbox[0],
                    "md": markdown,
                    "bbox": bbox,
                }
            )
        index = next_index

    text_tokens = []
    for line in lines:
        if id(line) in consumed:
            continue
        cleaned = clean(line["text"])
        label = profile.question_re.match(cleaned) or profile.option_re.match(cleaned)
        in_table = any(inside(line["bbox"], box) for box in table_boxes)
        # Row labels such as "5.日用品类" look like question starts and are
        # already present as table cells; a real question line swallowed by a
        # table bbox is not.
        if label and in_table and not re.fullmatch(r"\d{1,2}[．.]", cleaned):
            if cleaned in cell_texts:
                label = None
        if not label and in_table:
            continue
        text_tokens.append(
            {
                "kind": "text",
                "y": line["bbox"][1],
                "x": line["bbox"][0],
                "sy": line["baseline"],
                "text": cleaned,
                "bbox": line["bbox"],
            }
        )
    tokens = text_tokens + table_tokens
    for image in images:
        tokens.append(
            {
                "kind": "image",
                "y": image["bbox"][1],
                "sy": image["bbox"][1] + 2,
                "x": image["bbox"][0],
                "bbox": image["bbox"],
                "data": image["data"],
                "ext": image["ext"],
            }
        )
    tokens = attach_option_images(tokens, profile)
    for token in tokens:
        token["page"] = page_no
    return tokens


def attach_option_images(tokens, profile: Profile) -> list[dict]:
    """Chart-option questions: bind a small chart to the option label it sits beside."""
    options = [
        token
        for token in tokens
        if token["kind"] == "text" and profile.option_re.match(token["text"])
    ]
    used = set()
    for token in tokens:
        if token["kind"] != "image" or (token["bbox"][3] - token["bbox"][1]) > 170:
            continue
        best = None
        for option in options:
            option_box, image_box = option["bbox"], token["bbox"]
            center_y = (option_box[1] + option_box[3]) / 2
            beside = (
                image_box[0] - 5 > option_box[0]
                and image_box[0] - option_box[0] < 60
                and image_box[1] - 8 <= center_y <= image_box[3] + 8
            )
            below = (
                image_box[1] >= option_box[3] - 2
                and image_box[1] - option_box[3] < 10
            )
            if beside or below:
                distance = abs(center_y - (image_box[1] + image_box[3]) / 2)
                if best is None or distance < best[0]:
                    best = (distance, option)
        if best:
            best[1].setdefault("images", []).append(token)
            used.add(id(token))
    return [token for token in tokens if id(token) not in used]


def order_tokens(tokens, profile: Profile) -> list[dict]:
    """Sort by text baseline.

    Word equations report a numerator bbox several lines above its baseline,
    so bbox-top ordering puts ``A．138.1 × 0.013`` before the question it
    belongs to; the baseline keeps the reading order intact.
    """
    for token in tokens:
        if token["kind"] == "text" and profile.question_re.match(token["text"]):
            for other in tokens:
                if (
                    other["kind"] == "table"
                    and other["page"] == token["page"]
                    and inside(token["bbox"], other["bbox"])
                    and token["x"] < other["bbox"][0] + 25
                ):
                    token["sy"] = other["bbox"][1] - 2
                    break
    tokens.sort(key=lambda item: (item["page"], round(item["sy"]), item["x"]))
    return tokens


def detect_papers(document, profile: Profile):
    """Locate paper headings in reading order and derive page ranges.

    The first page that shows ``<kind><num>`` starts a paper; the next heading
    ends it. This is an assist for new books: profiles stay authoritative so
    a stray table-of-contents heading cannot move a range silently.
    """
    kinds = sorted({paper.kind for paper in profile.papers})
    heading = re.compile(
        r"^(?P<kind>" + "|".join(re.escape(kind) for kind in kinds) + r")\s*(?P<num>\d+)$"
    )
    found = {}
    for page_no, page in enumerate(document):
        for line in page.get_text().splitlines():
            match = heading.match(line.strip())
            if match:
                key = (match.group("kind"), int(match.group("num")))
                found.setdefault(key, page_no + 1)
    ordered = sorted(found.items(), key=lambda item: item[1])
    detected = []
    for index, ((kind, num), start) in enumerate(ordered):
        end = ordered[index + 1][1] - 1 if index + 1 < len(ordered) else len(document)
        detected.append((kind, num, start, end))
    return detected


def build(spec, document, profile: Profile) -> dict:
    kind, num, start, end = spec
    tokens = []
    for page_no in range(start - 1, end):
        tokens.extend(page_tokens(document[page_no], document, page_no, profile))
    tokens = order_tokens(tokens, profile)
    questions = []
    materials = []
    material = None
    current = None
    in_options = False
    last_option = None
    aux = []  # detached images kept for review
    for token in tokens:
        if token["kind"] == "text":
            value = token["text"]
            if profile.title_re.fullmatch(value):
                continue
            match = profile.section_re.match(value)
            if match:
                material = {"key": f"M{len(materials) + 1}", "blocks": []}
                materials.append(material)
                current, in_options, last_option = None, False, None
                continue
            if profile.ask_re.match(value):
                continue
            question_match = profile.question_re.match(value)
            if (
                question_match
                and int(question_match.group(1)) == len(questions) + 1
            ):
                current = {
                    "number": int(question_match.group(1)),
                    "stem": [],
                    "options": [],
                }
                questions.append(current)
                in_options, last_option = False, None
                rest = value[question_match.end() :].strip()
                if rest:
                    current["stem"].append(
                        {"kind": "text", "text": rest, "x": token["x"]}
                    )
                continue
            option_match = profile.option_re.match(value)
            if option_match and current is not None:
                label = option_match.group(1)
                expected = (
                    profile.option_labels[len(current["options"])]
                    if len(current["options"]) < len(profile.option_labels)
                    else None
                )
                if (
                    label != expected
                    and expected
                    and label not in [option["label"] for option in current["options"]]
                ):
                    # book typo: option letters out of order (e.g. A B D D)
                    current["option_fix"] = f"{label}->{expected}"
                    label = expected
                if label == expected:
                    option = {
                        "label": label,
                        "blocks": [
                            {
                                "kind": "text",
                                "text": value[option_match.end() :].strip(),
                            }
                        ],
                    }
                    option["blocks"] += [
                        {"kind": "image", "data": image["data"], "ext": image["ext"]}
                        for image in token.get("images", [])
                    ]
                    current["options"].append(option)
                    in_options, last_option = True, option
                    continue
            prefix = next(
                (
                    candidate
                    for candidate in profile.question_prefixes
                    if value.startswith(candidate)
                ),
                None,
            )
            if prefix:
                value = value[len(prefix) :].strip()
                if current is not None:
                    current["stem"].append(
                        {"kind": "text", "text": value, "x": token["x"]}
                    )
                in_options, last_option = False, None
                continue
            # plain body text
            if current is not None:
                if in_options and last_option is not None:
                    last_option["blocks"].append(
                        {"kind": "text", "text": value, "x": token["x"]}
                    )
                else:
                    current["stem"].append(
                        {"kind": "text", "text": value, "x": token["x"]}
                    )
            elif material is not None:
                material["blocks"].append(
                    {"kind": "text", "text": value, "x": token["x"]}
                )
            else:
                aux.append(value)
            continue
        # table / image
        if current is not None:
            if in_options and last_option is not None and token["kind"] == "image":
                last_option["blocks"].append(
                    {"kind": "image", "data": token["data"], "ext": token["ext"]}
                )
            else:
                current["stem"].append(token)
        elif material is not None:
            material["blocks"].append(token)
        else:
            aux.append(token["kind"])
    for question in questions:
        if question["options"] or question.get("materials"):
            continue
        images = [block for block in question["stem"] if block.get("kind") == "image"]
        if len(images) != 1:
            continue
        token = images[0]
        box = token["bbox"]
        width, height = box[2] - box[0], box[3] - box[1]
        if width < 300 or width / max(height, 1) < 2.5:
            continue
        page = document[token["page"]]
        step = width / len(profile.option_labels)
        question["stem"] = [block for block in question["stem"] if block is not token]
        for index, label in enumerate(profile.option_labels):
            clip = pymupdf.Rect(
                box[0] + index * step + 1,
                box[1],
                box[0] + (index + 1) * step - 1,
                box[3],
            )
            png = page.get_pixmap(clip=clip, dpi=220).tobytes("png")
            question["options"].append(
                {
                    "label": label,
                    "blocks": [{"kind": "image", "data": png, "ext": "png"}],
                }
            )
        question["split_note"] = "pie chart options split from one image"
    return {
        "kind": kind,
        "num": num,
        "materials": materials,
        "questions": questions,
        "aux": aux,
    }
