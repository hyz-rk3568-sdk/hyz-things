#!/usr/bin/env python3
"""Externalize Trunk's inline module bootstrap so strict CSP can remain enabled."""

from __future__ import annotations

import hashlib
import base64
import re
import sys
from pathlib import Path

SCRIPT = re.compile(r"<script(?P<attrs>[^>]*)>(?P<body>.*?)</script\s*>", re.IGNORECASE | re.DOTALL)
MODULE_TYPE = re.compile(r"\btype\s*=\s*([\"'])module\1", re.IGNORECASE)
SRC_ATTRIBUTE = re.compile(r"\bsrc\s*=", re.IGNORECASE)


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"error: {message}")


def update_resource_digest(digest, root: Path, exclude: Path | None = None) -> None:
    for path in sorted(root.rglob("*")):
        if path.is_file() and path != exclude:
            name = path.relative_to(root).as_posix().encode("utf-8")
            digest.update(len(name).to_bytes(8, "big"))
            digest.update(name)
            digest.update(hashlib.sha256(path.read_bytes()).digest())


def fingerprint_snippets(dist: Path, html: str) -> str:
    snippets = dist / "snippets"
    if not snippets.is_dir():
        return html
    digest = hashlib.sha256()
    update_resource_digest(digest, snippets)
    directory = "snippets-" + digest.hexdigest()[:16]
    snippets.rename(dist / directory)
    html = html.replace("/snippets/", f"/{directory}/")
    # wasm-bindgen's crate directory stays stable when a JS module changes.
    for path in sorted(dist.glob("*.js")):
        source = path.read_text(encoding="utf-8")
        updated = source.replace("./snippets/", f"./{directory}/")
        if updated == source:
            continue
        version = hashlib.sha256(updated.encode("utf-8")).hexdigest()[:16]
        name = re.sub(r"(?:-[0-9a-f]{16})?\.js$", f"-{version}.js", path.name)
        path.write_text(updated, encoding="utf-8", newline="\n")
        path.rename(dist / name)
        html = html.replace(path.name, name)
        old_integrity = base64.b64encode(hashlib.sha384(source.encode("utf-8")).digest()).decode()
        new_integrity = base64.b64encode(hashlib.sha384(updated.encode("utf-8")).digest()).decode()
        html = html.replace(f"sha384-{old_integrity}", f"sha384-{new_integrity}")
    return html


def distribution_version(dist: Path, html: str, bootstrap: str) -> str:
    digest = hashlib.sha256(html.encode("utf-8"))
    update_resource_digest(digest, dist, exclude=dist / "index.html")
    digest.update(bootstrap.encode("utf-8"))
    return digest.hexdigest()


def main() -> None:
    if len(sys.argv) != 2:
        fail("expected exactly one Trunk dist directory")

    dist = Path(sys.argv[1])
    index = dist / "index.html"
    bootstrap = dist / "router-bootstrap.js"
    if not index.is_file() or bootstrap.exists():
        fail("unexpected Trunk distribution state")

    html = fingerprint_snippets(dist, index.read_text(encoding="utf-8"))
    scripts = list(SCRIPT.finditer(html))
    inline_scripts = [script for script in scripts if not SRC_ATTRIBUTE.search(script.group("attrs"))]
    inline_modules = [script for script in inline_scripts if MODULE_TYPE.search(script.group("attrs"))]
    if len(inline_scripts) != 1 or len(inline_modules) != 1:
        fail("expected exactly one inline Trunk module bootstrap")

    script = inline_modules[0]
    body = script.group("body").strip()
    if not body or "import init" not in body or "await init" not in body:
        fail("Trunk module bootstrap has an unexpected shape")

    bootstrap_version = hashlib.sha256(body.encode("utf-8")).hexdigest()[:16]
    external_tag = (
        '<script type="module" src="/router-bootstrap.js?v='
        + bootstrap_version
        + '"></script>'
    )
    rewritten = html[: script.start()] + external_tag + html[script.end() :]
    remaining = list(SCRIPT.finditer(rewritten))
    if any(not SRC_ATTRIBUTE.search(script.group("attrs")) for script in remaining):
        fail("rewritten HTML still contains an inline script")

    if rewritten.lower().count("</head>") != 1:
        fail("expected exactly one HTML head")
    version = distribution_version(dist, rewritten, body)
    rewritten = re.sub(
        r"</head>",
        f'<meta name="hyz-frontend-version" content="{version}" /></head>',
        rewritten,
        count=1,
        flags=re.IGNORECASE,
    )
    bootstrap.write_text(body + "\n", encoding="utf-8", newline="\n")
    index.write_text(rewritten, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
