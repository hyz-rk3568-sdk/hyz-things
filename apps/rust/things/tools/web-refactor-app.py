#!/usr/bin/env python3
"""Stage 4: move application state/composition out of web/main.rs."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
MAIN = WEB / "main.rs"
APP = WEB / "app.rs"
NETWORK = WEB / "pages/network.rs"


def attribute_start(text: str, marker: str) -> int:
    pos = text.index(marker)
    start = text.rfind("\n", 0, pos) + 1
    while start > 0:
        prev_end = start - 1
        prev_start = text.rfind("\n", 0, prev_end) + 1
        if text[prev_start:prev_end].strip().startswith("#["):
            start = prev_start
        else:
            break
    return start


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
        raise RuntimeError(f"missing struct {struct_name}")
    body = re.sub(
        r"(?m)^(    )([A-Za-z_][A-Za-z0-9_]*\s*:)",
        r"\1pub(super) \2",
        match.group(2),
    )
    return text[:match.start()] + match.group(1) + body + match.group(3) + text[match.end():]


def pop(text: str, start: int, end: int) -> tuple[str, str]:
    return text[:start] + text[end:], text[start:end].rstrip() + "\n"


def main() -> None:
    text = MAIN.read_text()
    if "mod app;" in text:
        return

    state_start = attribute_start(text, "struct AppState")
    state_end = attribute_start(text, "enum Tone")
    text, state = pop(text, state_start, state_end)

    routes_start = attribute_start(text, "enum AppPage")
    routes_end = text.index("fn render_overview", routes_start)
    text, routes = pop(text, routes_start, routes_end)

    app_start = text.index("#[function_component(App)]")
    dispatch_start = text.index("fn dispatch_control<T>", app_start)
    text, app_component = pop(text, app_start, dispatch_start)

    dispatch_start = text.index("fn dispatch_control<T>")
    helpers_end = text.index("fn subscription_state_label", dispatch_start)
    text, dispatch = pop(text, dispatch_start, helpers_end)

    module = export_top_level(
        "use super::*;\n\n" + state + "\n" + routes + "\n" + app_component + "\n" + dispatch
    )
    module = expose_fields(module, "AppState")
    APP.write_text(module)

    text = text.replace("mod api;\n", "mod api;\nmod app;\n", 1)
    text = text.replace("use api::*;\n", "use api::*;\nuse app::*;\n", 1)
    MAIN.write_text(text)

    # Proxy and Tailscale now have dedicated first-level pages. Keep Network
    # focused on auth/wireless/admin settings instead of repeating those two
    # capability controls inside it.
    network = NETWORK.read_text()
    network = network.replace("                    {render_proxy_control(state)}\n", "")
    network = network.replace("                    {render_tailscale_control(state, &csrf)}\n", "")
    NETWORK.write_text(network)

    # API leaf modules containing only serde data/constants do not need a
    # wildcard parent import; removing these keeps strict clippy warning-free.
    for name in ("apps.rs", "auth.rs", "network.rs"):
        path = WEB / "api" / name
        body = path.read_text()
        if body.startswith("use super::*;\n\n"):
            path.write_text(body[len("use super::*;\n\n"):])


if __name__ == "__main__":
    main()
