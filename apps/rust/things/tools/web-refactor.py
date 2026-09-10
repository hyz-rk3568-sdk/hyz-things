#!/usr/bin/env python3
"""Deterministic staged transformer for the web frontend refactor.

This file is temporary. Each stage is idempotent and is executed by the
branch-only refactor driver. Generated Rust is formatted and compiled before
it is committed.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
MAIN = WEB / "main.rs"


def export_top_level_items(text: str) -> str:
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

    module = export_top_level_items("use super::*;\n\n" + logic + "\n" + ui)
    module = expose_struct_fields(module, "ExamCountdownTarget")
    module = expose_struct_fields(module, "CountdownSnapshot")
    hooks = WEB / "hooks"
    hooks.mkdir(exist_ok=True)
    (hooks / "countdown.rs").write_text(module)

    text = text[:start_ui] + text[end_ui:]
    text = text[:start_logic] + text[end_logic:]
    insertion = (
        'mod ui;\n\n#[path = "hooks/countdown.rs"]\nmod countdown;\n\nuse countdown::*;\n'
    )
    if "mod ui;\n" not in text:
        raise RuntimeError("expected web ui module declaration not found")
    return text.replace("mod ui;\n", insertion, 1)


def item_span(text: str, name: str) -> tuple[int, int]:
    match = re.search(
        rf"(?m)^(?P<kind>const|struct|enum|type)\s+{re.escape(name)}\b",
        text,
    )
    if not match:
        raise RuntimeError(f"top-level item not found: {name}")
    start = match.start()
    while start > 0:
        prev_end = start - 1
        prev_start = text.rfind("\n", 0, prev_end) + 1
        prev = text[prev_start:prev_end].strip()
        if prev.startswith("#["):
            start = prev_start
            continue
        break
    kind = match.group("kind")
    if kind in {"const", "type"}:
        end = text.find("\n", match.end())
        return start, len(text) if end < 0 else end + 1
    brace = text.find("{", match.end())
    if brace < 0:
        raise RuntimeError(f"opening brace not found for {name}")
    depth = 0
    for index in range(brace, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                end = index + 1
                if end < len(text) and text[end] == "\n":
                    end += 1
                return start, end
    raise RuntimeError(f"closing brace not found for {name}")


def pop_item(text: str, name: str) -> tuple[str, str]:
    start, end = item_span(text, name)
    return text[:start] + text[end:], text[start:end].strip() + "\n"


def pop_range(text: str, start_marker: str, end_marker: str) -> tuple[str, str]:
    start = text.index(start_marker)
    end = text.index(end_marker, start)
    return text[:start] + text[end:], text[start:end].rstrip() + "\n"


def make_api_public(block: str) -> str:
    block = re.sub(
        r"(?m)^(const|struct|enum|type)\s+",
        lambda m: f"pub(crate) {m.group(1)} ", block, count=1,
    )
    block = re.sub(
        r"(?m)^(?P<prefix>async\s+)?fn\s+",
        lambda m: f"pub(crate) {m.group('prefix') or ''}fn ", block, count=1,
    )
    if re.search(r"(?m)^pub\(crate\) struct\s+", block):
        block = re.sub(
            r"(?m)^(    )([A-Za-z_][A-Za-z0-9_]*\s*:)",
            r"\1pub(crate) \2", block,
        )
    return block


def extract_api(text: str) -> str:
    if "mod api;" in text:
        return text
    api_dir = WEB / "api"
    api_dir.mkdir(exist_ok=True)
    groups: dict[str, list[str]] = {
        "auth.rs": ["AUTH_LOGIN_ENDPOINT", "AUTH_LOGOUT_ENDPOINT", "AUTH_SESSION_ENDPOINT", "AUTH_PASSWORD_ENDPOINT", "AuthSessionDto", "LoginRequest", "PasswordRequest"],
        "apps.rs": ["APPS_ENDPOINT", "InstalledAppDto", "AppsResponseDto"],
        "camera.rs": ["CAMERA_STATUS_ENDPOINT", "CAMERA_VIEWER_TOKEN_ENDPOINT", "CAMERA_SESSION_CREATE_ENDPOINT", "CAMERA_SESSION_CLOSE_ENDPOINT", "CAMERA_PROFILE_UPDATE_ENDPOINT", "CAMERA_ROTATION_UPDATE_ENDPOINT", "CameraViewerTokenDto", "CameraStatusResponseDto", "CameraProfileUpdateRequestDto", "CameraProfileUpdateResponseDto", "CameraRotationUpdateRequestDto", "CameraRotationUpdateResponseDto", "CameraSessionCreateRequestDto", "CameraSessionCreateResponseDto", "CameraSessionCloseRequestDto"],
        "network.rs": ["NETWORK_CONFIG_ENDPOINT", "NETWORK_PENDING_ENDPOINT", "STA_SCAN_ENDPOINT", "STA_APPLY_ENDPOINT", "AP_PREPARE_ENDPOINT", "AP_APPLY_ENDPOINT", "AP_CONFIRM_ENDPOINT", "AP_CANCEL_ENDPOINT", "WifiCountryDto", "NetworkConfigDto", "PendingConfigDto", "NetworkPendingDto", "WifiScanDto", "NetworkConfigResponseDto", "NetworkScanResponseDto", "StaRequest", "NetworkApplyIntent", "ApRequest"],
        "proxy.rs": ["PROXY_LAN_TUN_ENDPOINT", "PROXY_LOCAL_SYSTEM_ENDPOINT", "PROXY_SELECTION_ENDPOINT", "PROXY_DELAYS_ENDPOINT", "SUBSCRIPTION_ENDPOINT", "SUBSCRIPTION_SOURCE_ENDPOINT", "SUBSCRIPTION_REFRESH_ENDPOINT", "DEVICE_POLICIES_ENDPOINT", "DEVICE_POLICIES_UPDATE_ENDPOINT", "SubscriptionStateDto", "DevicePolicyDto", "DevicePolicyEntryDto", "DevicePolicyConfigDto", "LanClientDto", "DevicePolicySnapshotDto", "DevicePolicyUpdateDto", "SubscriptionDto", "SubscriptionResponseDto", "SubscriptionSourceRequest", "ProxyFeatureRequestDto", "DelayRefreshControlResponse"],
        "tailscale.rs": ["TAILSCALE_ENDPOINT", "TAILSCALE_PEERS_ENDPOINT", "TAILSCALE_MODE_ENDPOINT", "TAILSCALE_LOGIN_ENDPOINT", "TAILSCALE_LOGOUT_ENDPOINT", "TailscaleModeRequestDto", "TailscaleResponseDto", "TailscalePeersResponseDto", "TailscaleMutationResponseDto"],
        "status.rs": ["STATUS_ENDPOINT", "PANEL_ENDPOINT", "DISPLAY_ENDPOINT"],
    }
    extracted = {filename: [] for filename in groups}
    for filename, names in groups.items():
        for name in names:
            text, block = pop_item(text, name)
            extracted[filename].append(make_api_public(block))
    text, empty_request = pop_item(text, "EmptyRequest")
    text, dashboard = pop_range(text, "async fn fetch_dashboard", "async fn fetch_json")
    extracted["status.rs"].append(make_api_public(dashboard))
    text, fetch_json = pop_range(text, "async fn fetch_json", "fn dispatch_control")
    text, post_helpers = pop_range(text, "async fn post_json", "async fn fetch_settings_data")
    post_helpers = re.sub(r"(?m)^async fn\s+", "pub(crate) async fn ", post_helpers)
    modules = ["auth", "apps", "camera", "network", "proxy", "tailscale", "status"]
    for module in modules:
        (api_dir / f"{module}.rs").write_text("use super::*;\n\n" + "\n".join(extracted[f"{module}.rs"]))
    mod_lines = ["use super::*;", "", *[f"mod {m};" for m in modules], "", *[f"pub(crate) use {m}::*;" for m in modules], "", make_api_public(empty_request).rstrip(), "", make_api_public(fetch_json).rstrip(), "", post_helpers.rstrip(), ""]
    (api_dir / "mod.rs").write_text("\n".join(mod_lines))
    text = text.replace("mod ui;\n", "mod ui;\nmod api;\n", 1)
    return text.replace("use countdown::*;\n", "use countdown::*;\nuse api::*;\n", 1)


def extract_camera_page(text: str) -> str:
    marker = '#[path = "pages/camera.rs"]\nmod camera_page;'
    if marker in text:
        return text

    core_decl = text.index("enum CameraViewPhase")
    core_start = text.rfind("#[derive", 0, core_decl)
    core_end = text.index("impl WifiCountryDto", core_decl)
    core = text[core_start:core_end].rstrip() + "\n"
    text = text[:core_start] + text[core_end:]

    props_decl = text.index("struct CameraLiveViewProps")
    ui_start = text.rfind("#[derive", 0, props_decl)
    ui_end = text.index("fn render_deployed_apps", props_decl)
    camera_ui = text[ui_start:ui_end].rstrip() + "\n"
    text = text[:ui_start] + text[ui_end:]

    module = export_top_level_items("use super::*;\n\n" + core + "\n" + camera_ui)
    module = expose_struct_fields(module, "CameraLiveViewProps")
    pages = WEB / "pages"
    pages.mkdir(exist_ok=True)
    (pages / "camera.rs").write_text(module)

    insertion = '#[path = "pages/camera.rs"]\nmod camera_page;\n'
    anchor = '#[path = "hooks/countdown.rs"]\nmod countdown;\n'
    if anchor not in text:
        raise RuntimeError("countdown module anchor missing for camera page")
    text = text.replace(anchor, anchor + "\n" + insertion, 1)
    use_anchor = "use api::*;\n"
    if use_anchor not in text:
        raise RuntimeError("api import anchor missing for camera page")
    return text.replace(use_anchor, use_anchor + "use camera_page::*;\n", 1)


def main() -> None:
    text = MAIN.read_text()
    text = extract_countdown(text)
    text = extract_api(text)
    text = extract_camera_page(text)
    MAIN.write_text(text)


if __name__ == "__main__":
    main()
