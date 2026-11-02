"""UUID markers are owned by the device; the importer only validates/preserves."""

import hashlib
import re
import uuid
from collections import Counter

from bank import InvalidPaper, frontmatter


MARKER = re.compile(r"<!-- hyz-(paper|question|material)-id: ([^\n]*?) -->")
UNIT = re.compile(r"^## (Question (\d+)|Material (M\d+))\s*$", re.M)


def without_ids(text):
    return re.sub(r"\n{3,}", "\n\n", MARKER.sub("", text)).strip() + "\n"


def markers(text):
    found = MARKER.findall(text)
    if text.count("<!-- hyz-") != len(found):
        raise InvalidPaper("malformed UUID marker")
    result = []
    for kind, value in found:
        try:
            identifier = uuid.UUID(value)
        except ValueError as error:
            raise InvalidPaper("invalid UUID marker") from error
        if identifier.version != 7 or str(identifier) != value:
            raise InvalidPaper("UUID marker must be canonical UUIDv7")
        result.append((kind, value))
    if len({value for kind, value in result}) != len(result):
        raise InvalidPaper("duplicate UUID")
    return result


def unit_records(text):
    markers(text)
    headings = list(UNIT.finditer(text))
    records = {}
    for i, heading in enumerate(headings):
        chunk = text[
            heading.end() : headings[i + 1].start()
            if i + 1 < len(headings)
            else len(text)
        ]
        kind = "question" if heading[2] else "material"
        ids = markers(chunk)
        if len(ids) > 1 or any(k != kind for k, v in ids):
            raise InvalidPaper("UUID marker attached to wrong unit")
        clean = without_ids(chunk)
        if kind == "question":
            start = clean.find("### Stem\n")
            end = clean.find("### Answer\n")
            if start < 0 or end < start:
                raise InvalidPaper("invalid normalized question sections")
            clean = clean[start:end].strip()
        records[heading[1]] = {
            "signature": hashlib.sha256(clean.encode()).hexdigest(),
            "id": ids[0][1] if ids else None,
            "kind": kind,
        }
    prefix = text[: headings[0].start()] if headings else text
    ids = markers(prefix)
    if len(ids) > 1 or any(k != "paper" for k, v in ids):
        raise InvalidPaper("invalid paper UUID placement")
    if ids:
        records["paper"] = {"signature": "paper", "id": ids[0][1], "kind": "paper"}
    return records


def preserve_ids(local, generated, previously_seen=None):
    old = unit_records(local)
    new = unit_records(generated)
    seen = set(v["id"] for v in old.values() if v["id"])
    if not set(previously_seen or []) <= seen:
        raise InvalidPaper("previously recorded UUID removed")
    signatures = Counter((v["kind"], v["signature"]) for v in new.values())
    old_signatures = Counter((v["kind"], v["signature"]) for v in old.values())
    inserts = {}
    for key, record in old.items():
        if not record["id"]:
            continue
        if key == "paper":
            continue
        signature = (record["kind"], record["signature"])
        if signatures[signature] != 1 or old_signatures[signature] != 1:
            raise InvalidPaper(
                "UUID identity cannot be matched uniquely; manual review required"
            )
        target = next(
            k for k, v in new.items() if (v["kind"], v["signature"]) == signature
        )
        inserts[target] = f"<!-- hyz-{record['kind']}-id: {record['id']} -->"
    generated = UNIT.sub(
        lambda m: m[0] + "\n\n" + inserts[m[1]] if m[1] in inserts else m[0], generated
    )
    if old.get("paper", {}).get("id"):
        _, body = frontmatter(generated)
        offset = len(generated) - len(body)
        generated = (
            generated[:offset]
            + "\n<!-- hyz-paper-id: "
            + old["paper"]["id"]
            + " -->\n"
            + generated[offset:]
        )
    return generated
