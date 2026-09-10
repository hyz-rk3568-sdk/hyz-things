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

    module = "use super::*;\n\n" + logic + "\n" + ui
    module = export_top_level_items(module)
    module = expose_struct_fields(module, "ExamCountdownTarget")
    module = expose_struct_fields(module, "CountdownSnapshot")

    hooks = WEB / "hooks"
    hooks.mkdir(exist_ok=True)
    (hooks / "countdown.rs").write_text(module)

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
    return text.replace("mod ui;\n", insertion, 1)


def item_span(text: str, name: str) -> tuple[int, int]:
    match = re.search(
        rf"(?m)^(?P<kind>const|struct|enum|type)\s+{re.escape(name)}\b",
        text,
    )
    if not match:
        raise RuntimeError(f"top-level item not found: {name}")

    start = match.start()
    # Pull contiguous derive/serde attributes into the extracted item.
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
        if end < 0:
            end = len(text)
        else:
            end += 1
        return start, end

    brace = text.find("{", match.end())
    if brace < 0:
        raise RuntimeError(f"opening brace not found for {name}")
    depth = 0
    for index in range(brace, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index + 1
                if end < len(text) and text[end] == "\n":
                    end += 1
                return start, end
    raise RuntimeError(f"closing brace not found for {name}")


def pop_item(text: str, name: str) -> tuple[str, str]:
    start, end = item_span(text, name)
    block = text[start:end].strip() + "\n"
    text = text[:start] + text[end:]
    return text, block


def pop_range(text: str, start_marker: str, end_marker: str) -> tuple[str, str]:
    start = text.index(start_marker)
    end = text.index(end_marker, start)
    block = text[start:end].rstrip() + "\n"
    return text[:start] + text[end:], block


def make_api_public(block: str) -> str:
    block = re.sub(
        r"(?m)^(const|struct|enum|type)\s+",
        lambda m: f"pub(crate) {m.group(1)} ",
        block,
        count=1,
    )
    block = re.sub(
        r"(?m)^(?P<prefix>async\s+)?fn\s+",
        lambda m: f"pub(crate) {m.group('prefix') or ''}fn ",
        block,
        count=1,
    )
    if re.search(r"(?m)^pub\(crate\) struct\s+", block):
        block = re.sub(
            r"(?m)^(    )([A-Za-z_][A-Za-z0-9_]*\s*:)",
            r"\1pub(crate) \2",
            block,
        )
    return block


def extract_api(text: str) -> str:
    if "mod api;" in text:
        return text

    api_dir = WEB / "api"
    api_dir.mkdir(exist_ok=True)

    groups: dict[str, list[str]] = {
        "auth.rs": [
            "AUTH_LOGIN_ENDPOINT", "AUTH_LOGOUT_ENDPOINT", "AUTH_SESSION_ENDPOINT",
            "AUTH_PASSWORD_ENDPOINT", "AuthSessionDto", "LoginRequest", "PasswordRequest",
        ],
        "apps.rs": ["APPS_ENDPOINT", "InstalledAppDto", "AppsResponseDto"],
        "camera.rs": [
            "CAMERA_STATUS_ENDPOINT", "CAMERA_VIEWER_TOKEN_ENDPOINT",
            "CAMERA_SESSION_CREATE_ENDPOINT", "CAMERA_SESSION_CLOSE_ENDPOINT",
            "CAMERA_PROFILE_UPDATE_ENDPOINT", "CAMERA_ROTATION_UPDATE_ENDPOINT",
            "CameraViewerTokenDto", "CameraStatusResponseDto",
            "CameraProfileUpdateRequestDto", "CameraProfileUpdateResponseDto",
            "CameraRotationUpdateRequestDto", "CameraRotationUpdateResponseDto",
            "CameraSessionCreateRequestDto", "CameraSessionCreateResponseDto",
            "CameraSessionCloseRequestDto",
        ],
        "network.rs": [
            "NETWORK_CONFIG_ENDPOINT", "NETWORK_PENDING_ENDPOINT", "STA_SCAN_ENDPOINT",
            "STA_APPLY_ENDPOINT", "AP_PREPARE_ENDPOINT", "AP_APPLY_ENDPOINT",
            "AP_CONFIRM_ENDPOINT", "AP_CANCEL_ENDPOINT", "WifiCountryDto",
            "NetworkConfigDto", "PendingConfigDto", "NetworkPendingDto", "WifiScanDto",
            "NetworkConfigResponseDto", "NetworkScanResponseDto", "StaRequest",
            "NetworkApplyIntent", "ApRequest",
        ],
        "proxy.rs": [
            "PROXY_LAN_TUN_ENDPOINT", "PROXY_LOCAL_SYSTEM_ENDPOINT",
            "PROXY_SELECTION_ENDPOINT", "PROXY_DELAYS_ENDPOINT", "SUBSCRIPTION_ENDPOINT",
            "SUBSCRIPTION_SOURCE_ENDPOINT", "SUBSCRIPTION_REFRESH_ENDPOINT",
            "DEVICE_POLICIES_ENDPOINT", "DEVICE_POLICIES_UPDATE_ENDPOINT",
            "SubscriptionStateDto", "DevicePolicyDto", "DevicePolicyEntryDto",
            "DevicePolicyConfigDto", "LanClientDto", "DevicePolicySnapshotDto",
            "DevicePolicyUpdateDto", "SubscriptionDto", "SubscriptionResponseDto",
            "SubscriptionSourceRequest", "ProxyFeatureRequestDto", "DelayRefreshControlResponse",
        ],
        "tailscale.rs": [
            "TAILSCALE_ENDPOINT", "TAILSCALE_PEERS_ENDPOINT", "TAILSCALE_MODE_ENDPOINT",
            "TAILSCALE_LOGIN_ENDPOINT", "TAILSCALE_LOGOUT_ENDPOINT", "TailscaleModeRequestDto",
            "TailscaleResponseDto", "TailscalePeersResponseDto", "TailscaleMutationResponseDto",
        ],
        "status.rs": ["STATUS_ENDPOINT", "PANEL_ENDPOINT", "DISPLAY_ENDPOINT"],
    }

    extracted: dict[str, list[str]] = {filename: [] for filename in groups}
    # Extract declarations in their configured order; pop_item is name-based so
    # source ordering is not a dependency.
    for filename, names in groups.items():
        for name in names:
            text, block = pop_item(text, name)
            extracted[filename].append(make_api_public(block))

    text, empty_request = pop_item(text, "EmptyRequest")
    empty_request = make_api_public(empty_request)

    text, dashboard = pop_range(text, "async fn fetch_dashboard", "async fn fetch_json")
    dashboard = make_api_public(dashboard)
    extracted["status.rs"].append(dashboard)

    text, fetch_json = pop_range(text, "async fn fetch_json", "fn dispatch_control")
    fetch_json = make_api_public(fetch_json)

    text, post_helpers = pop_range(text, "async fn post_json", "async fn fetch_settings_data")
    post_helpers = re.sub(r"(?m)^async fn\s+", "pub(crate) async fn ", post_helpers)

    module_names = ["auth", "apps", "camera", "network", "proxy", "tailscale", "status"]
    for module_name in module_names:
        body = "use super::*;\n\n" + "\n".join(extracted[f"{module_name}.rs"])
        (api_dir / f"{module_name}.rs").write_text(body)

    api_mod = ["use super::*;", ""]
    for module_name in module_names:
        api_mod.append(f"mod {module_name};")
    api_mod.append("")
    for module_name in module_names:
        api_mod.append(f"pub(crate) use {module_name}::*;")
    api_mod.extend(["", empty_request.rstrip(), "", fetch_json.rstrip(), "", post_helpers.rstrip(), ""])
    (api_dir / "mod.rs").write_text("\n".join(api_mod))

    declaration = "mod ui;\nmod api;\n"
    if "mod ui;\n" not in text:
        raise RuntimeError("expected web ui module declaration not found for api insertion")
    text = text.replace("mod ui;\n", declaration, 1)
    use_marker = "use countdown::*;\n"
    if use_marker not in text:
        raise RuntimeError("expected countdown import not found for api insertion")
    text = text.replace(use_marker, use_marker + "use api::*;\n", 1)
    return text


def main() -> None:
    text = MAIN.read_text()
    text = extract_countdown(text)
    text = extract_api(text)
    MAIN.write_text(text)


if __name__ == "__main__":
    main()
