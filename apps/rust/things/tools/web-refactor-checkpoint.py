#!/usr/bin/env python3
"""Finish the structural checkpoint before shared UI work.

This temporary migration is deterministic and idempotent. It moves the remaining
Overview/System page ownership out of web/main.rs, introduces normal hooks/pages
module roots, repairs Playwright migration defects found by CI, preserves
crate-internal visibility after adding the extra module layer, and verifies that
the resulting structure stays at the intended checkpoint.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
MAIN = WEB / "main.rs"
APP = WEB / "app.rs"
PAGES = WEB / "pages"
HOOKS = WEB / "hooks"
PORTAL = ROOT / "apps/rust/things/e2e/portal.spec.ts"
E2E_DRIVER = ROOT / "apps/rust/things/tools/web-refactor-e2e.py"


def export_top_level(text: str) -> str:
    text = re.sub(
        r"(?m)^(?P<prefix>(?:async\s+)?)fn\s+",
        lambda m: f"pub(crate) {m.group('prefix')}fn ",
        text,
    )
    text = re.sub(
        r"(?m)^(const|struct|enum|type|trait)\s+",
        lambda m: f"pub(crate) {m.group(1)} ",
        text,
    )
    return text


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def promote_nested_module_visibility(path: Path) -> None:
    """`pub(super)` used to mean the web root; after nesting it is too narrow."""
    text = path.read_text()
    promoted = text.replace("pub(super)", "pub(crate)")
    if promoted != text:
        path.write_text(promoted)


def extract_remaining_pages() -> None:
    text = MAIN.read_text()

    overview = PAGES / "overview.rs"
    if not overview.exists():
        start = text.index("fn render_overview")
        end = text.index("fn render_dashboard", start)
        block = text[start:end].rstrip() + "\n"
        overview.write_text(export_top_level("use super::*;\n\n" + block))
        text = text[:start] + text[end:]

    system = PAGES / "system.rs"
    if not system.exists():
        start = text.index("fn render_dashboard")
        end = text.index("#[cfg(test)]", start)
        block = text[start:end].rstrip() + "\n\n"
        wrapper = '''pub(crate) fn render_system(
    state: &UseReducerHandle<AppState>,
    brightness: UseStateHandle<u16>,
) -> Html {
    html! {
        <>
            if let Some(snapshot) = &state.snapshot {
                {render_dashboard(snapshot)}
                {render_issues(snapshot)}
            }
            {render_display_control(state, brightness)}
        </>
    }
}
'''
        system.write_text(export_top_level("use super::*;\n\n" + block) + wrapper)
        text = text[:start] + text[end:]

    old_modules = '''#[path = "hooks/countdown.rs"]
mod countdown;

#[path = "pages/camera.rs"]
mod camera_page;

#[path = "pages/proxy.rs"]
mod proxy_page;

#[path = "pages/tailscale.rs"]
mod tailscale_page;

#[path = "pages/apps.rs"]
mod apps_page;

#[path = "pages/network.rs"]
mod network_page;
'''
    text = replace_once(text, old_modules, "mod hooks;\nmod pages;\n", "module roots")

    old_uses = '''use api::*;
use app::*;
use apps_page::*;
use camera_page::*;
use countdown::*;
use network_page::*;
use proxy_page::*;
use tailscale_page::*;
'''
    text = replace_once(
        text,
        old_uses,
        '''use api::*;
use app::*;
use hooks::*;
use pages::*;
''',
        "module imports",
    )
    MAIN.write_text(text)

    # The old direct-root modules used pub(super) to expose their composition API
    # to web/main.rs. Once nested under pages/ and hooks/, keep the same effective
    # boundary by promoting those declarations to crate-only visibility.
    for path in [
        PAGES / "apps.rs",
        PAGES / "camera.rs",
        PAGES / "network.rs",
        PAGES / "overview.rs",
        PAGES / "proxy.rs",
        PAGES / "system.rs",
        PAGES / "tailscale.rs",
        HOOKS / "countdown.rs",
    ]:
        promote_nested_module_visibility(path)

    hooks_mod = HOOKS / "mod.rs"
    hooks_mod.write_text(
        "use super::*;\n\nmod countdown;\n\n#[cfg(test)]\nmod countdown_tests;\n\npub(crate) use countdown::*;\n"
    )

    pages_mod = PAGES / "mod.rs"
    pages_mod.write_text(
        '''use super::*;

mod apps;
mod camera;
mod network;
mod overview;
mod proxy;
mod system;
mod tailscale;

pub(crate) use apps::*;
pub(crate) use camera::*;
pub(crate) use network::*;
pub(crate) use overview::*;
pub(crate) use proxy::*;
pub(crate) use system::*;
pub(crate) use tailscale::*;
'''
    )

    app = APP.read_text()
    old_system = '''                    AppPage::System => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            if let Some(snapshot) = &state.snapshot {
                                {render_dashboard(snapshot)}
                                {render_issues(snapshot)}
                            }
                            {render_display_control(&state, brightness.clone())}
                        </section>
                    },
'''
    new_system = '''                    AppPage::System => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            {render_system(&state, brightness.clone())}
                        </section>
                    },
'''
    app = replace_once(app, old_system, new_system, "System page composition")
    APP.write_text(app)


def repair_e2e_driver() -> None:
    """Make the temporary navigation migration safe to run more than once."""
    text = E2E_DRIVER.read_text()
    old = '''        if old in segment:
            segment = segment.replace(old, new, 1)
        elif new not in segment:
            raise SystemExit("administrator journey: Proxy migration anchor not found")
'''
    new = '''        if new in segment:
            pass
        elif old in segment:
            segment = segment.replace(old, new, 1)
        else:
            raise SystemExit("administrator journey: Proxy migration anchor not found")
'''
    if old in text:
        text = text.replace(old, new, 1)
    elif new not in text:
        raise SystemExit("E2E migration driver: administrator idempotency anchor not found")
    E2E_DRIVER.write_text(text)


def repair_e2e_contracts() -> None:
    text = PORTAL.read_text()

    old_counter = '''    let total = 0;
    counter.querySelectorAll("strong[data-value]").forEach((element) => {
      total += Number(element.textContent);
    });
    return total;
'''
    new_counter = '''    const values = Array.from(
      counter.querySelectorAll("strong[data-value]"),
      (element) => Number(element.textContent),
    );
    const weights = [86_400, 3_600, 60, 1];
    return values.reduce(
      (total, value, index) => total + value * (weights[index] ?? 0),
      0,
    );
'''
    if old_counter in text:
        text = text.replace(old_counter, new_counter, 1)
    elif new_counter not in text:
        raise SystemExit("PiP countdown reader: expected helper body not found")

    test_start = text.index(
        'test("supports the administrator, STA, AP, and write-only subscription journey"'
    )
    next_test = text.find('\ntest("', test_start + 1)
    if next_test < 0:
        next_test = len(text)
    segment = text[test_start:next_test]
    password_anchor = '  await page.getByRole("button", { name: "修改密码" }).click();\n'
    proxy_anchor = '  await page\n    .getByRole("combobox", { name: "自动选择 节点" })\n'
    password_end = segment.index(password_anchor) + len(password_anchor)
    proxy_start = segment.index(proxy_anchor, password_end)
    canonical_transition = '''  await expect(
    page.getByRole("button", { name: "上游 Wi-Fi (STA)" }),
  ).toBeVisible();
  await goToAppPage(page, "代理");
  await expect(page.getByRole("heading", { name: "代理设置" })).toBeVisible();
'''
    segment = (
        segment[:password_end]
        + canonical_transition
        + segment[proxy_start:]
    )
    text = text[:test_start] + segment + text[next_test:]
    PORTAL.write_text(text)


def validate_structure() -> None:
    main = MAIN.read_text()
    app = APP.read_text()
    hooks_mod = (HOOKS / "mod.rs").read_text()
    portal = PORTAL.read_text()
    e2e_driver = E2E_DRIVER.read_text()

    required_files = [
        PAGES / "mod.rs",
        PAGES / "overview.rs",
        PAGES / "system.rs",
        HOOKS / "mod.rs",
        HOOKS / "countdown.rs",
        HOOKS / "countdown_tests.rs",
    ]
    missing = [str(path.relative_to(ROOT)) for path in required_files if not path.exists()]
    if missing:
        raise SystemExit(f"structural checkpoint missing files: {', '.join(missing)}")

    forbidden_root_ownership = ["fn render_overview", "fn render_dashboard", "fn render_system"]
    leaked = [symbol for symbol in forbidden_root_ownership if symbol in main]
    if leaked:
        raise SystemExit(f"main.rs still owns page implementation: {', '.join(leaked)}")

    for declaration in ["mod hooks;", "mod pages;"]:
        if declaration not in main:
            raise SystemExit(f"main.rs missing module root: {declaration}")
    for call in ["render_overview(&state)", "render_system(&state, brightness.clone())"]:
        if call not in app:
            raise SystemExit(f"app.rs missing page composition call: {call}")
    if "#[cfg(test)]\nmod countdown_tests;" not in hooks_mod:
        raise SystemExit("countdown deterministic tests are not registered")
    if "const weights = [86_400, 3_600, 60, 1];" not in portal:
        raise SystemExit("Playwright countdown helper is not unit-aware")

    test_start = portal.index(
        'test("supports the administrator, STA, AP, and write-only subscription journey"'
    )
    next_test = portal.find('\ntest("', test_start + 1)
    admin_segment = portal[test_start:] if next_test < 0 else portal[test_start:next_test]
    canonical_transition = '''  await expect(
    page.getByRole("button", { name: "上游 Wi-Fi (STA)" }),
  ).toBeVisible();
  await goToAppPage(page, "代理");
  await expect(page.getByRole("heading", { name: "代理设置" })).toBeVisible();
'''
    if admin_segment.count(canonical_transition) != 1:
        raise SystemExit("administrator journey Proxy transition is duplicated or missing")
    if '        if new in segment:\n            pass\n        elif old in segment:' not in e2e_driver:
        raise SystemExit("E2E migration driver is not idempotent")


if __name__ == "__main__":
    repair_e2e_driver()
    repair_e2e_contracts()
    extract_remaining_pages()
    validate_structure()
    print("completed and validated structural checkpoint plus E2E contract repairs")
