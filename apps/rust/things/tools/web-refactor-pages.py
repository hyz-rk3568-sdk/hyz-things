#!/usr/bin/env python3
"""Stage 2: move remaining capability-sized UI blocks out of web/main.rs.

This transformer is temporary, deterministic, and idempotent. It preserves
markup and control flow; only module ownership/visibility changes here.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
MAIN = WEB / "main.rs"
PAGES = WEB / "pages"


def export_top_level(text: str) -> str:
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


def expose_fields(text: str, struct_name: str) -> str:
    pattern = re.compile(
        rf"(pub\(super\) struct {re.escape(struct_name)}\s*\{{)(.*?)(^\}})",
        re.MULTILINE | re.DOTALL,
    )
    match = pattern.search(text)
    if not match:
        return text
    body = re.sub(
        r"(?m)^(    )([A-Za-z_][A-Za-z0-9_]*\s*:)",
        r"\1pub(super) \2",
        match.group(2),
    )
    return text[: match.start()] + match.group(1) + body + match.group(3) + text[match.end() :]


def preceding_attribute_start(text: str, marker: str) -> int:
    pos = text.index(marker)
    line_start = text.rfind("\n", 0, pos) + 1
    start = line_start
    while start > 0:
        prev_end = start - 1
        prev_start = text.rfind("\n", 0, prev_end) + 1
        prev = text[prev_start:prev_end].strip()
        if prev.startswith("#["):
            start = prev_start
        else:
            break
    return start


def move_range(text: str, filename: str, start: int, end: int, expose: tuple[str, ...] = ()) -> str:
    block = text[start:end].rstrip() + "\n"
    module = export_top_level("use super::*;\n\n" + block)
    for name in expose:
        module = expose_fields(module, name)
    (PAGES / filename).write_text(module)
    return text[:start] + text[end:]


def add_page_module(text: str, module: str, filename: str) -> str:
    marker = f'#[path = "pages/{filename}"]\nmod {module};'
    if marker not in text:
        anchor = '#[path = "pages/camera.rs"]\nmod camera_page;\n'
        if anchor not in text:
            raise RuntimeError("camera page anchor missing")
        text = text.replace(anchor, anchor + f'\n#[path = "pages/{filename}"]\nmod {module};\n', 1)
    use_marker = f"use {module}::*;\n"
    if use_marker not in text:
        anchor = "use camera_page::*;\n"
        text = text.replace(anchor, anchor + use_marker, 1)
    return text


def extract_network(text: str) -> str:
    if '#[path = "pages/network.rs"]' in text:
        return text
    start = preceding_attribute_start(text, "struct SettingsProps")
    end = text.index("fn subscription_state_label", start)
    text = move_range(text, "network.rs", start, end, ("SettingsProps",))
    return add_page_module(text, "network_page", "network.rs")


def extract_apps(text: str) -> str:
    if '#[path = "pages/apps.rs"]' in text:
        return text
    start = text.index("fn render_deployed_apps")
    end = text.index("fn render_home", start)
    text = move_range(text, "apps.rs", start, end)
    return add_page_module(text, "apps_page", "apps.rs")


def extract_tailscale(text: str) -> str:
    if '#[path = "pages/tailscale.rs"]' in text:
        return text
    start = text.index("fn render_tailscale_peers")
    end = text.index("fn render_proxy_control", start)
    text = move_range(text, "tailscale.rs", start, end)
    return add_page_module(text, "tailscale_page", "tailscale.rs")


def extract_proxy(text: str) -> str:
    if '#[path = "pages/proxy.rs"]' in text:
        return text
    start = text.index("fn render_proxy_control")
    end = text.index("fn render_issues", start)
    text = move_range(text, "proxy.rs", start, end)
    return add_page_module(text, "proxy_page", "proxy.rs")


def main() -> None:
    PAGES.mkdir(exist_ok=True)
    text = MAIN.read_text()
    text = extract_network(text)
    text = extract_apps(text)
    text = extract_tailscale(text)
    text = extract_proxy(text)
    MAIN.write_text(text)


if __name__ == "__main__":
    main()
