"""Copy only the local image reference closure, deduplicated by content."""

import hashlib
import os
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

from PIL import Image


MARKDOWN_IMAGE = re.compile(r"(!\[[^\]\n]*\]\(<?)([^\s)>]+)(>?[^)\n]*\))")
HTML_IMAGE = re.compile(r'(<img\b[^>]*?\bsrc\s*=\s*[\'"])([^\'"]+)([\'"])', re.I)


def image_references(text):
    return [
        m[2] for pattern in (MARKDOWN_IMAGE, HTML_IMAGE) for m in pattern.finditer(text)
    ]


class AssetStore:
    def __init__(self, root):
        self.root = Path(root).resolve()
        self.files = {}
        self.errors = []
        self.cache = {}
        self.references = 0

    def rewrite(self, text, source_file, destination):
        def replace(match):
            self.references += 1
            reference = match[2]
            parsed = urlsplit(reference)
            path = (source_file.parent / unquote(parsed.path)).resolve()
            key = str(path) if not parsed.scheme and not parsed.netloc else reference
            if key not in self.cache:
                try:
                    if (
                        parsed.scheme
                        or parsed.netloc
                        or not path.is_relative_to(self.root)
                    ):
                        raise ValueError("external image or path outside source root")
                    if path.is_symlink() or not path.is_file():
                        raise ValueError("missing image")
                    data = path.read_bytes()
                    with Image.open(path) as image:
                        image.verify()
                    name = hashlib.sha256(data).hexdigest() + path.suffix.lower()
                    relative = Path("assets") / name
                    self.files[relative.as_posix()] = path
                    self.cache[key] = (relative, None)
                except (OSError, ValueError, Image.DecompressionBombError) as error:
                    name = hashlib.sha256(reference.encode()).hexdigest() + ".missing"
                    self.cache[key] = (Path("assets/missing") / name, str(error))
            relative, error = self.cache[key]
            if error:
                self.errors.append(
                    {
                        "paper": source_file.relative_to(self.root).as_posix(),
                        "reference": reference,
                        "reason": error,
                    }
                )
            rewritten = os.path.relpath(relative, destination.parent).replace(
                os.sep, "/"
            )
            return match[1] + rewritten + match[3]

        for pattern in (MARKDOWN_IMAGE, HTML_IMAGE):
            text = pattern.sub(replace, text)
        return text


def prepare_images(paper, store, source_file, destination):
    bad_materials = set()
    paper.notes = store.rewrite(paper.notes, source_file, destination)
    for material in paper.materials:
        before = len(store.errors)
        material.content = store.rewrite(material.content, source_file, destination)
        if len(store.errors) != before:
            bad_materials.add(material.name)
    for question in paper.questions:
        before = len(store.errors)
        for field in ("stem", "options", "explanation", "notes"):
            setattr(
                question,
                field,
                store.rewrite(getattr(question, field), source_file, destination),
            )
        if len(store.errors) != before or bad_materials.intersection(
            question.metadata["materials"]
        ):
            question.metadata["issues"].append("missing_image")
