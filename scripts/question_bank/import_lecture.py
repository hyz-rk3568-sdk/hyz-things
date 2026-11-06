#!/usr/bin/env python3
"""Convert a transcribed lecture Markdown into a drill collection.

The lecture source is a single Markdown file with `##` topic headings,
`**N.**` question starts and `images/` references. The output uses the same
`hyz-paper/v1` format as exam papers, with `kind: drill` and a collection id.
Images are copied into the shared content-addressed `assets/` directory.
"""

import argparse
import hashlib
import os
import re
import shutil
from collections import OrderedDict

MAJOR_HEADINGS = {"多级数推", "分组数推", "并非分组", "复杂递推", "常规数推", "总结"}


def topic_of(heading):
    if heading.strip() == "数量关系":
        return "数字推理"
    if heading.startswith("数量关系——") or re.match(r"^\d{4}", heading):
        return heading.rsplit("——", 1)[1].strip()
    return None


def safe_name(text):
    return re.sub(r'[\\/:*?"<>|]', "", text).strip() or "专题"


def asset_name(image_dir, reference, assets):
    data = open(os.path.join(image_dir, os.path.basename(reference)), "rb").read()
    digest = hashlib.sha256(data).hexdigest()
    extension = os.path.splitext(reference)[1].lower().lstrip(".")
    name = f"{digest}.{extension}"
    os.makedirs(assets, exist_ok=True)
    destination = os.path.join(assets, name)
    if not os.path.exists(destination):
        shutil.copyfile(os.path.join(image_dir, os.path.basename(reference)), destination)
    return name


def rewrite_images(text, image_dir, assets):
    def replace(match):
        alt, reference = match.group(1), match.group(2)
        name = asset_name(image_dir, reference, assets)
        return f"![{alt}](../../assets/{name})"

    return re.sub(r"!\[([^\]]*)\]\((images/[^)\s]+)\)", replace, text)


def parse_questions(lines):
    papers = OrderedDict()
    paper = None
    major = ""
    category = ""
    question = None

    def flush():
        nonlocal question
        if question is None or paper is None:
            return
        body = [
            line
            for line in question["lines"]
            if not re.match(r"^\s*<!-- p\d+ -->\s*$", line)
        ]
        while body and not body[0].strip():
            body.pop(0)
        while body and not body[-1].strip():
            body.pop()
        first_option = next(
            (index for index, line in enumerate(body) if re.match(r"^[A-F][.．]", line)),
            None,
        )
        trailing = []
        if first_option is None:
            stem, options = body, []
        else:
            stem, options = body[:first_option], body[first_option:]
        option_lines = []
        for line in options:
            line = line.strip()
            if not line:
                continue
            match = re.match(r"^([A-F])[.．]\s*(.*)$", line)
            if match:
                option_lines.append(f"- {match.group(1)}. {match.group(2)}")
            else:
                # Figures and figure captions that follow the options belong
                # before the choices so the reader sees the diagram first.
                trailing.append(line)
        if trailing:
            stem = stem + [""] + trailing
        paper["questions"].append(
            {
                "number": question["number"],
                "stem": "\n".join(stem).strip(),
                "options": "\n".join(option_lines),
                "category": question["category"],
            }
        )
        question = None

    for line in lines:
        if line.startswith("## "):
            flush()
            heading = line[3:].strip()
            topic = topic_of(heading)
            if topic is not None:
                paper = papers.setdefault(topic, {"title": topic, "questions": []})
                major = ""
                category = ""
            elif heading in MAJOR_HEADINGS:
                major = heading
                category = heading
            else:
                category = f"{major} · {heading}" if major else heading
            continue
        match = re.match(r"^\*\*(\d+)\.\*\*\s*(.*)$", line)
        if match:
            flush()
            if paper is None:
                raise SystemExit(f"question {match.group(1)} appears before a topic")
            question = {
                "number": int(match.group(1)),
                "lines": [match.group(2)],
                "category": category,
            }
            continue
        if question is not None:
            question["lines"].append(line)
    flush()
    return papers


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, help="lecture Markdown file")
    parser.add_argument("--image-dir", required=True, help="directory holding images/")
    parser.add_argument("--output", required=True, help="collection output directory")
    parser.add_argument("--assets", required=True, help="shared question_bank/assets directory")
    parser.add_argument("--collection-id", required=True)
    parser.add_argument("--collection-title", required=True)
    parser.add_argument("--year", type=int, default=2026)
    parser.add_argument("--module", default="数量关系")
    args = parser.parse_args()

    lines = open(args.source, encoding="utf-8").read().splitlines()
    papers = parse_questions(lines)
    os.makedirs(args.output, exist_ok=True)
    for name in os.listdir(args.output):
        if name.endswith(".md"):
            os.remove(os.path.join(args.output, name))
    total = 0
    for index, (title, data) in enumerate(papers.items(), 1):
        questions = data["questions"]
        if not questions:
            continue
        total += len(questions)
        output = [
            "---",
            "schema: hyz-paper/v1",
            f'title: "{title}"',
            f"year: {args.year}",
            "region: 专项",
            f"variant: {args.module}",
            "role: primary",
            "kind: drill",
            f"collection_id: {args.collection_id}",
            f'collection_title: "{args.collection_title}"',
            "---",
            f"# {title}",
            "",
        ]
        for question in questions:
            output += [
                f"## Question {question['number']}",
                "```yaml",
                f"module: {args.module}",
                f"category: \"{question['category']}\"",
                "```",
                "### Stem",
                rewrite_images(question["stem"], args.image_dir, args.assets),
                "",
                "### Options",
                rewrite_images(question["options"], args.image_dir, args.assets),
                "",
                "### Answer",
                "",
                "### Explanation",
                "",
            ]
        path = os.path.join(args.output, f"{index:02d}-{safe_name(title)}.md")
        open(path, "w", encoding="utf-8").write("\n".join(output).rstrip() + "\n")
        print(f"{os.path.basename(path)} {len(questions)}")
    print(f"papers={len(papers)} questions={total}")


if __name__ == "__main__":
    main()
