#!/usr/bin/env python3
"""Externalize Trunk's inline module bootstrap so strict CSP can remain enabled."""

from __future__ import annotations

import hashlib
import re
import sys
from pathlib import Path

SCRIPT = re.compile(r"<script(?P<attrs>[^>]*)>(?P<body>.*?)</script\s*>", re.IGNORECASE | re.DOTALL)
MODULE_TYPE = re.compile(r"\btype\s*=\s*([\"'])module\1", re.IGNORECASE)
SRC_ATTRIBUTE = re.compile(r"\bsrc\s*=", re.IGNORECASE)


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"error: {message}")


def main() -> None:
    if len(sys.argv) != 2:
        fail("expected exactly one Trunk dist directory")

    dist = Path(sys.argv[1])
    index = dist / "index.html"
    bootstrap = dist / "router-bootstrap.js"
    if not index.is_file() or bootstrap.exists():
        fail("unexpected Trunk distribution state")

    html = index.read_text(encoding="utf-8")
    scripts = list(SCRIPT.finditer(html))
    inline_modules = [
        script
        for script in scripts
        if MODULE_TYPE.search(script.group("attrs"))
        and not SRC_ATTRIBUTE.search(script.group("attrs"))
    ]
    if len(scripts) != 1 or len(inline_modules) != 1:
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
    if len(remaining) != 1 or not SRC_ATTRIBUTE.search(remaining[0].group("attrs")):
        fail("rewritten HTML still contains an inline script")

    bootstrap.write_text(body + "\n", encoding="utf-8", newline="\n")
    index.write_text(rewritten, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
