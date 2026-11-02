"""Reproducible local import; existing content is never silently overwritten."""

import hashlib
from collections import Counter
import json
from pathlib import Path
import re
import subprocess

from identity import UNIT, without_ids, preserve_ids, markers, unit_records
from quality import (
    read_evidence,
    qualify,
    paper_statistics,
    inventory_text,
    classification_report,
    summary_text,
)
from verify import validate_document
from assets import AssetStore, prepare_images
from bank import InvalidPaper, frontmatter, parse_paper, render_paper


def digest(value):
    if isinstance(value, str):
        value = value.encode()
    return hashlib.sha256(value).hexdigest()


def json_text(value):
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def write_if_changed(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    data = content.encode() if isinstance(content, str) else content
    if path.exists() and path.read_bytes() == data:
        return
    temporary = path.with_name(path.name + ".import-tmp")
    with temporary.open("xb") as stream:
        stream.write(data)
    temporary.replace(path)


def classification_mask(text):
    """Mask only module/category in each question metadata block."""
    text = without_ids(text)
    headings = list(UNIT.finditer(text))
    pieces = []
    cursor = 0
    for index, heading in enumerate(headings):
        if not heading[2]:
            continue
        end = headings[index + 1].start() if index + 1 < len(headings) else len(text)
        stem = text.find("### Stem", heading.end(), end)
        if stem < 0:
            return text
        block = text[heading.end() : stem]
        metadata = re.search(r"```yaml\n(.*?)\n```", block, re.S)
        if not metadata:
            return text
        fields = set(re.findall(r"(?m)^(module|category):", metadata[1]))
        if fields != {"module", "category"}:
            return text
        masked = re.sub(
            r"(?m)^(module|category):.*$", r"\1: __classification__", metadata[0]
        )
        start = heading.end() + metadata.start()
        finish = heading.end() + metadata.end()
        pieces.extend((text[cursor:start], masked))
        cursor = finish
    pieces.append(text[cursor:])
    return "".join(pieces)


def preserve_classification_ids(local, generated, previously_seen):
    """Carry UUIDs by unchanged unit keys for a verified metadata-only update."""
    if classification_mask(local) != classification_mask(generated):
        return None
    old_records, new_records = unit_records(local), unit_records(generated)
    old_units = {key: value for key, value in old_records.items() if key != "paper"}
    new_units = {key: value for key, value in new_records.items() if key != "paper"}
    if old_units.keys() != new_units.keys():
        return None
    for key, old in old_units.items():
        new = new_units[key]
        if old["kind"] != new["kind"] or old["signature"] != new["signature"]:
            return None
    old_ids = {row["id"] for row in old_records.values() if row["id"]}
    if not set(previously_seen or []) <= old_ids:
        raise InvalidPaper("previously recorded UUID removed")
    insertions = {
        key: f"<!-- hyz-{row['kind']}-id: {row['id']} -->"
        for key, row in old_units.items()
        if row["id"]
    }
    generated = UNIT.sub(
        lambda match: match[0] + "\n\n" + insertions[match[1]]
        if match[1] in insertions
        else match[0],
        generated,
    )
    paper_id = old_records.get("paper", {}).get("id")
    if paper_id:
        _, body = frontmatter(generated)
        offset = len(generated) - len(body)
        generated = (
            generated[:offset]
            + "\n<!-- hyz-paper-id: "
            + paper_id
            + " -->\n"
            + generated[offset:]
        )
    if {value for kind, value in markers(generated)} != old_ids:
        raise InvalidPaper("classification-only update changed UUID set")
    return generated


def preflight_existing(output):
    seen = set()
    documents = {}
    for path in (output / "papers").rglob("*.md"):
        if path.is_symlink() or not path.resolve().is_relative_to(output):
            raise InvalidPaper("unsafe destination path")
        text = path.read_text()
        _, questions = validate_document(text)
        documents[path.relative_to(output).as_posix()] = questions
        current = {v for k, v in markers(text)}
        if current & seen:
            raise InvalidPaper("duplicate UUID across papers")
        seen.update(current)
    return documents


def import_bank(source, output, *, update=False, dry_run=False):
    source, output = Path(source).resolve(), Path(output).resolve()
    before_documents = preflight_existing(output)
    importer_hash = digest(
        b"".join(
            p.read_bytes()
            for p in sorted(Path(__file__).parent.glob("*.py"))
            if not p.name.startswith("test_")
        )
        + Path(__file__).with_name("classification.yaml").read_bytes()
    )
    try:
        commit = subprocess.check_output(
            ["git", "-C", str(source), "rev-parse", "HEAD"],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
    except subprocess.CalledProcessError:
        commit = None
    catalog, gaps = read_evidence(source)
    counts = Counter()
    provenance, issues, decisions = [], [], []
    classification_records = []
    snapshots = {}
    manifest_path = output / "manifest.json"
    previous = (
        json.loads(manifest_path.read_text())
        if manifest_path.exists()
        else {"papers": {}}
    )
    if previous["papers"] and not update:
        raise InvalidPaper("existing bank: use --update")
    report = {"converted_papers": 0, "conflicts": [], "parse_failures": []}
    assets = AssetStore(source)
    report["image_errors"] = assets.errors
    records = dict(previous["papers"])
    pending = {}
    expected = {}
    files = sorted([*source.glob("国考/*/行测*.md"), *source.glob("省考/*/*/行测*.md")])
    if not files:
        raise InvalidPaper("no candidate papers")
    source_paths = {p.relative_to(source).as_posix() for p in files}
    for path in files:
        relative = path.relative_to(source).as_posix()
        raw = path.read_text()
        try:
            paper = parse_paper(raw)
        except (InvalidPaper, ValueError, KeyError) as error:
            report["parse_failures"].append({"path": relative, "reason": str(error)})
            continue
        source_hash = digest(raw)
        source_record = catalog.get(relative, {})
        qualify(paper, source_record, gaps.get(relative, []), source_hash)
        decisions.append(
            {
                "path": relative,
                "year": paper.metadata["year"],
                "decision": "include",
                "reason": "行测候选；全部年份",
                "source_sha256": source_hash,
            }
        )
        old = records.get(relative)
        if old is None:
            matches = [
                key
                for key, record in records.items()
                if key not in source_paths and record["source_sha256"] == source_hash
            ]
            if len(matches) == 1:
                old = records.pop(matches[0])
                records[relative] = old
            elif len(matches) > 1:
                report["conflicts"].append(
                    {"path": relative, "reason": "ambiguous source rename"}
                )
                continue
        target_relative = old["target"] if old else "papers/" + relative
        prepare_images(paper, assets, path, Path(target_relative))
        rendered = render_paper(paper)
        try:
            validate_document(rendered)
        except (InvalidPaper, ValueError) as error:
            report["parse_failures"].append({"path": relative, "reason": str(error)})
            continue
        classification_records.extend(
            {
                "path": target_relative,
                "source_path": relative,
                "number": q.number,
                "raw_module": q.provenance.get("raw_module"),
                "raw_category": q.provenance.get("raw_category"),
                "module": q.metadata.get("module"),
                "category": q.metadata.get("category"),
                "topics": q.metadata.get("topics", []),
                "module_basis": q.provenance.get("module_basis"),
                "category_basis": q.provenance.get("category_basis"),
                "evidence": q.provenance.get("classification_evidence", []),
                "conflicts": q.provenance.get("classification_conflicts", []),
            }
            for q in paper.questions
        )
        statistics, missing_numbers = paper_statistics(paper)
        counts.update(statistics)
        evidence = {
            "path": relative,
            "source_sha256": source_hash,
            "source_commit": commit,
            "frontmatter": paper.provenance["frontmatter"],
            "catalog": source_record,
            "missing_numbers": missing_numbers,
            "counts": statistics,
            "questions": [
                {"number": q.number, **q.provenance} for q in paper.questions
            ],
        }
        provenance.append(evidence)
        snapshots[source_hash] = evidence
        issues.extend(
            {"path": relative, "number": q.number, "issues": q.metadata["issues"]}
            for q in paper.questions
            if q.metadata["issues"]
        )
        target = output / target_relative
        if old and not target.exists():
            report["conflicts"].append(
                {
                    "path": relative,
                    "reason": "tracked destination missing; refusing identity loss",
                }
            )
            continue
        if target.exists():
            local = target.read_text()
            try:
                current_ids = [v for k, v in markers(local)]
                if not set(old.get("known_ids", []) if old else []) <= set(current_ids):
                    raise InvalidPaper("previously recorded UUID removed")
            except InvalidPaper as error:
                report["conflicts"].append({"path": relative, "reason": str(error)})
                continue
            if (
                old
                and old["source_sha256"] == source_hash
                and old.get("importer_sha256") == importer_hash
                and digest(without_ids(rendered)) == old["generated_sha256"]
            ):
                records[relative] = {**old, "known_ids": current_ids}
                report["converted_papers"] += 1
                continue
            if not old or digest(without_ids(local)) != old["generated_sha256"]:
                report["conflicts"].append(
                    {
                        "path": relative,
                        "reason": "upstream and local changes or unmanaged destination",
                    }
                )
                continue
        if target.exists():
            try:
                metadata_only = preserve_classification_ids(
                    local, rendered, old.get("known_ids", [])
                )
                rendered = metadata_only or preserve_ids(
                    local, rendered, old.get("known_ids", [])
                )
            except InvalidPaper as error:
                report["conflicts"].append({"path": relative, "reason": str(error)})
                continue
        pending[target] = rendered
        expected[target] = digest(local) if target.exists() else None
        records[relative] = {
            "source_sha256": source_hash,
            "generated_sha256": digest(without_ids(rendered)),
            "known_ids": [v for k, v in markers(rendered)],
            "target": target_relative,
            "importer_sha256": importer_hash,
            "source_commit": commit,
            "counts": statistics,
        }
        report["converted_papers"] += 1
    decisions.extend(
        {
            "path": key,
            "year": row.get("year"),
            "decision": "exclude",
            "reason": "非行测",
        }
        for key, row in catalog.items()
        if row.get("subject") != "行测"
    )
    report["source_commit"] = commit
    report["importer_sha256"] = importer_hash
    report["source_content_counts"] = dict(counts)
    report["image_references"] = assets.references
    report["unique_images"] = len(assets.files)
    report["archived_sources"] = sorted(set(records) - source_paths)
    after_documents = dict(before_documents)
    for path, text in pending.items():
        _, questions = validate_document(text)
        after_documents[path.relative_to(output).as_posix()] = questions
    classification_summary, classification_details = classification_report(
        before_documents, after_documents, classification_records
    )
    metadata_only_updates = []
    unchanged_outputs = []
    nonclassification_changes = []
    for path, text in pending.items():
        if not path.exists():
            continue
        local = path.read_text()
        if classification_mask(local) == classification_mask(text):
            if without_ids(local) == without_ids(text):
                unchanged_outputs.append(path.relative_to(output).as_posix())
            else:
                metadata_only_updates.append(path.relative_to(output).as_posix())
        else:
            nonclassification_changes.append(path.relative_to(output).as_posix())
    classification_summary.update(
        metadata_only_update_papers=len(metadata_only_updates),
        unchanged_output_papers=len(unchanged_outputs),
        nonclassification_change_papers=nonclassification_changes,
        new_paper_count=sum(not path.exists() for path in pending),
    )
    classification_details["summary"] = classification_summary
    report["classification"] = classification_summary
    report["asset_changes"] = []
    for relative, source_asset in sorted(assets.files.items()):
        output_asset = output / relative
        if not output_asset.is_file() or digest(output_asset.read_bytes()) != digest(
            source_asset.read_bytes()
        ):
            report["asset_changes"].append(relative)
    manifest = {
        "schema": "hyz-question-bank-manifest/v1",
        "source_commit": commit,
        "papers": records,
    }
    if not dry_run:
        for relative, asset in assets.files.items():
            write_if_changed(output / relative, asset.read_bytes())
        for path, text in pending.items():
            current = path.read_bytes() if path.exists() else None
            if (digest(current) if current is not None else None) != expected[path]:
                raise InvalidPaper(
                    "destination changed during import; retry without overwriting edits"
                )
            if current is not None and current != text.encode():
                write_if_changed(
                    output / f"reports/backups/{digest(current)}.md", current
                )
            write_if_changed(path, text)
        if manifest_path.exists() and manifest_path.read_text() != json_text(manifest):
            old_manifest = manifest_path.read_bytes()
            write_if_changed(
                output / f"reports/backups/manifest/{digest(old_manifest)}.json",
                old_manifest,
            )
        write_if_changed(manifest_path, json_text(manifest))
        write_if_changed(output / "reports/import.json", json_text(report))
        write_if_changed(output / "reports/quality.md", summary_text(report))
        write_if_changed(
            output / "reports/classification.json", json_text(classification_details)
        )
        write_if_changed(
            output / "reports/inventory.md", inventory_text(decisions, commit)
        )
        write_if_changed(
            output / "reports/inventory.json",
            json_text(
                {
                    "source_commit": commit,
                    "default_year_filter": None,
                    "decisions": decisions,
                }
            ),
        )
        for name, rows in [("provenance", provenance), ("issues", issues)]:
            write_if_changed(
                output / f"reports/{name}.jsonl",
                "".join(
                    json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n"
                    for row in rows
                ),
            )
        for key, value in snapshots.items():
            write_if_changed(output / f"reports/sources/{key}.json", json_text(value))
    return report
