#!/usr/bin/env python3
"""Temporary driver for the staged web frontend refactor.

The script is intentionally deterministic and idempotent. During the first
stage it only prints a coarse inventory of top-level Rust declarations so the
module split can be made against the repository's exact source.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
MAIN = ROOT / "apps/rust/things/src/web/main.rs"


def main() -> None:
    text = MAIN.read_text()
    patterns = re.compile(
        r"^(?:#\[[^\n]+\]\s*)?(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?"
        r"(?:fn|struct|enum|type|const|static|trait)\s+([A-Za-z_][A-Za-z0-9_]*)",
        re.MULTILINE,
    )
    print("web/main.rs declaration inventory:")
    for match in patterns.finditer(text):
        line = text.count("\n", 0, match.start()) + 1
        print(f"{line:5d} {match.group(1)}")


if __name__ == "__main__":
    main()
