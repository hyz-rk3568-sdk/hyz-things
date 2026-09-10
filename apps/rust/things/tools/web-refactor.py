#!/usr/bin/env python3
"""Deterministic staged transformer for the web frontend refactor.

This file is temporary. Each stage is idempotent and is executed by the
branch-only refactor driver. The generated Rust is formatted before commit and
validated again by the normal pull-request CI.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
MAIN = WEB / "main.rs"


def export_top_level_items(text: str) -> str:
    # The extracted files are direct child modules of the crate root. Export
    # their top-level declarations to the parent while keeping internals and
    # struct fields private unless a test explicitly needs them.
    text = re.sub(
        r"(?m)^(?P<prefix>(?:async\s+)?)fn\s+",
        lambda m: f"pub(super) {m.group('prefix')}fn ",
        text,
    )
    text = re.sub(
        r"(?m)^(const|struct|enum|type|trait)\s+",
        lambda m: f"pub(super) {m.group(1)} ",
        text,
    )
    return text


def expose_struct_fields(text: str, struct_name: str) -> str:
    pattern = re.compile(
        rf"(pub\(super\) struct {re.escape(struct_name)}\s*\{{)(.*?)(^\}})",
        re.MULTILINE | re.DOTALL,
    )
    match = pattern.search(text)
    if not match:
        raise RuntimeError(f"struct not found in extracted module: {struct_name}")
    body = re.sub(
        r"(?m)^(    )([A-Za-z_][A-Za-z0-9_]*\s*:)",
        r"\1pub(super) \2",
        match.group(2),
    )
    return text[: match.start()] + match.group(1) + body + match.group(3) + text[match.end() :]


def extract_countdown(text: str) -> str:
    marker = '#[path = "hooks/countdown.rs"]\nmod countdown;'
    if marker in text:
        return text

    start_logic = text.index("const EXAM_COUNTDOWN_TICK_MS")
    end_logic = text.index("fn is_escape_key")
    logic = text[start_logic:end_logic].rstrip() + "\n"

    start_ui = text.index("fn custom_timer_for_duration")
    end_ui = text.index("fn render_home")
    ui = text[start_ui:end_ui].rstrip() + "\n"

    module = "use super::*;\n\n" + logic + "\n" + ui
    module = export_top_level_items(module)
    # Existing root tests inspect these two value types directly.
    module = expose_struct_fields(module, "ExamCountdownTarget")
    module = expose_struct_fields(module, "CountdownSnapshot")

    hooks = WEB / "hooks"
    hooks.mkdir(exist_ok=True)
    (hooks / "countdown.rs").write_text(module)

    # Remove later range first so original offsets remain valid.
    text = text[:start_ui] + text[end_ui:]
    text = text[:start_logic] + text[end_logic:]

    insertion = (
        'mod ui;\n\n'
        '#[path = "hooks/countdown.rs"]\n'
        'mod countdown;\n\n'
        'use countdown::*;\n'
    )
    if "mod ui;\n" not in text:
        raise RuntimeError("expected web ui module declaration not found")
    text = text.replace("mod ui;\n", insertion, 1)
    return text


def main() -> None:
    text = MAIN.read_text()
    text = extract_countdown(text)
    MAIN.write_text(text)


if __name__ == "__main__":
    main()
