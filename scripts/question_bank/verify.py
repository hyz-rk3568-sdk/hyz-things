"""Validate the generated format independently from the upstream parser."""

import json
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit
from collections import Counter

from PIL import Image
import yaml

from assets import image_references
from bank import InvalidPaper, frontmatter
from identity import UNIT, markers, unit_records


def validate_document(text):
    metadata, body = frontmatter(text)
    if metadata.get("schema") != "hyz-paper/v1":
        raise InvalidPaper("unsupported schema")
    if any(k in metadata for k in ("paper_id", "source", "source_url")):
        raise InvalidPaper("forbidden paper metadata")
    unit_records(text)
    units = list(UNIT.finditer(body))
    names = [m[1] for m in units]
    if len(names) != len(set(names)):
        raise InvalidPaper("duplicate question or material")
    materials = {m[3] for m in units if m[3]}
    questions = []
    for i, match in enumerate(units):
        if not match[2]:
            continue
        chunk = body[
            match.end() : units[i + 1].start() if i + 1 < len(units) else len(body)
        ]
        data = re.search(r"```yaml\n(.*?)\n```", chunk, re.S)
        if not data:
            raise InvalidPaper("missing question metadata")
        data = yaml.safe_load(data[1])
        if not isinstance(data, dict) or not isinstance(data.get("materials"), list):
            raise InvalidPaper("invalid material references")
        if not set(data["materials"]) <= materials:
            raise InvalidPaper("dangling material reference")
        for key in ("topics", "issues"):
            if not isinstance(data.get(key), list) or not all(
                isinstance(v, str) for v in data[key]
            ):
                raise InvalidPaper("invalid " + key)
        for section in ("Stem", "Options", "Answer"):
            if len(re.findall(r"^### " + section + "$", chunk, re.M)) != 1:
                raise InvalidPaper("missing or duplicate question section " + section)
        questions.append({"number": int(match[2]), **data})
    if not questions:
        raise InvalidPaper("no question units")
    return metadata, questions


def verify_bank(output):
    output = Path(output).resolve()
    manifest = json.loads((output / "manifest.json").read_text())
    errors, ids, seen_images = [], set(), set()
    counts = Counter()
    for entry in manifest["papers"].values():
        path = output / entry["target"]
        try:
            if not path.resolve().is_relative_to(output) or path.is_symlink():
                raise InvalidPaper("unsafe paper path")
            text = path.read_text()
            _, questions = validate_document(text)
            counts["papers"] += 1
            counts["questions"] += len(questions)
            current = {v for k, v in markers(text)}
            if ids & current:
                raise InvalidPaper("UUID reused across files")
            if not set(entry.get("known_ids", [])) <= current:
                raise InvalidPaper("previously recorded UUID removed")
            ids.update(current)
            for q in questions:
                counts.update(q["issues"])
            for reference in image_references(text):
                parsed = urlsplit(reference)
                image = (path.parent / unquote(parsed.path)).resolve()
                if parsed.scheme or parsed.netloc or not image.is_relative_to(output):
                    raise InvalidPaper("unsafe image path")
                if image in seen_images:
                    continue
                with Image.open(image) as picture:
                    picture.verify()
                seen_images.add(image)
        except (OSError, ValueError, yaml.YAMLError) as error:
            errors.append({"path": entry["target"], "reason": str(error)})
    return {
        "counts": dict(counts),
        "unique_images": len(seen_images),
        "uuid_count": len(ids),
        "errors": errors,
    }
