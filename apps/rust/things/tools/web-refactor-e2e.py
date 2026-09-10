#!/usr/bin/env python3
"""Migrate the existing Playwright contracts to the seven-page app shell.

This is a temporary, deterministic refactor driver. It deliberately preserves
existing behavior assertions instead of weakening timeouts or deleting tests.
After migration, the normal PR CI is the source of truth for the full E2E run.
"""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[4]
APP = ROOT / "apps/rust/things/src/web/app.rs"
FIXTURES = ROOT / "apps/rust/things/e2e/fixtures.ts"
PORTAL = ROOT / "apps/rust/things/e2e/portal.spec.ts"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def replace_between(text: str, start: str, end: str, replacement: str, label: str) -> str:
    start_index = text.find(start)
    if start_index < 0:
        raise SystemExit(f"{label}: start marker not found")
    end_index = text.find(end, start_index)
    if end_index < 0:
        raise SystemExit(f"{label}: end marker not found")
    return text[:start_index] + replacement + text[end_index:]


def update_app() -> None:
    text = APP.read_text()
    marker = "// Overview and Network stay mounted so local drafts/timers survive page switches."
    if marker in text:
        return

    old_arms = '''                    AppPage::Overview => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            {render_overview(&state)}
                        </section>
                    },
                    AppPage::Network => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            <Settings state={state.clone()} camera_stop_generation={camera_stop_generation.clone()} />
                        </section>
                    },
'''
    new_arms = '''                    AppPage::Overview | AppPage::Network => Html::default(),
'''
    text = replace_once(text, old_arms, new_arms, "app active Overview/Network arms")

    old_match = '''                {match page {
'''
    mounted_panels = '''                // Overview and Network stay mounted so local drafts/timers survive page switches.
                // The other inactive pages keep empty panel targets in the DOM so every aria-controls
                // relationship remains valid. Camera content itself is still mounted only while active,
                // preserving the existing stop-on-page-leave session lifecycle.
                <section
                    id={AppPage::Overview.panel_id()}
                    class={WORKSPACE_PANEL}
                    aria-labelledby={AppPage::Overview.tab_id()}
                    hidden={page != AppPage::Overview}
                >
                    {render_overview(&state)}
                </section>
                <section
                    id={AppPage::Network.panel_id()}
                    class={WORKSPACE_PANEL}
                    aria-labelledby={AppPage::Network.tab_id()}
                    hidden={page != AppPage::Network}
                >
                    <Settings state={state.clone()} camera_stop_generation={camera_stop_generation.clone()} />
                </section>
                {for AppPage::ALL.into_iter()
                    .filter(|candidate| {
                        *candidate != page
                            && !matches!(candidate, AppPage::Overview | AppPage::Network)
                    })
                    .map(|candidate| html! {
                        <section
                            id={candidate.panel_id()}
                            class={WORKSPACE_PANEL}
                            aria-labelledby={candidate.tab_id()}
                            hidden=true
                        ></section>
                    })}
                {match page {
'''
    text = replace_once(text, old_match, mounted_panels, "app persistent panel insertion")
    APP.write_text(text)


def update_fixtures() -> None:
    text = FIXTURES.read_text()
    if "export async function goToAppPage" not in text:
        anchor = '''export async function readHarnessState(request: APIRequestContext) {
  const response = await request.get(`${harnessOrigin}/state`);
  expect(response.ok()).toBeTruthy();
  return response.json();
}

'''
        helper = anchor + '''export type AppPageName =
  | '总览'
  | '网络'
  | '代理'
  | 'Tailscale'
  | '摄像头'
  | '应用'
  | '系统';

export async function goToAppPage(page: Page, name: AppPageName) {
  const button = page.getByRole('button', { name, exact: true });
  await button.click();
  await expect(button).toHaveAttribute('aria-pressed', 'true');
}

'''
        text = replace_once(text, anchor, helper, "fixtures app navigation helper")

    old_login = '''export async function loginAsAdmin(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: '路由器', exact: true }).click();
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await page.getByRole('button', { name: '管理员登录' }).click();
  await page.getByLabel('密码').fill('admin');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.getByLabel('当前密码').fill('admin');
  await page.getByLabel('新密码', { exact: true }).fill('router-e2e-password');
  await page.getByLabel('确认新密码').fill('router-e2e-password');
  await page.getByRole('button', { name: '修改密码' }).click();
  await expect(page.getByRole('heading', { name: '代理设置' })).toBeVisible();
}
'''
    new_login = '''export async function loginAsAdmin(page: Page) {
  await page.goto('/');
  await goToAppPage(page, '网络');
  await page.getByRole('button', { name: '管理员登录' }).click();
  await page.getByLabel('密码').fill('admin');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.getByLabel('当前密码').fill('admin');
  await page.getByLabel('新密码', { exact: true }).fill('router-e2e-password');
  await page.getByLabel('确认新密码').fill('router-e2e-password');
  await page.getByRole('button', { name: '修改密码' }).click();
  await expect(page.getByRole('button', { name: '上游 Wi-Fi (STA)' })).toBeVisible();
}
'''
    if old_login in text:
        text = text.replace(old_login, new_login, 1)
    elif new_login not in text:
        raise SystemExit("fixtures loginAsAdmin: neither old nor migrated form found")
    FIXTURES.write_text(text)


def update_test_segment(text: str, test_name: str, transform) -> str:
    start = f'test("{test_name}"'
    start_index = text.find(start)
    if start_index < 0:
        raise SystemExit(f"test segment not found: {test_name}")
    next_index = text.find('\ntest("', start_index + len(start))
    if next_index < 0:
        next_index = len(text)
    segment = text[start_index:next_index]
    updated = transform(segment)
    return text[:start_index] + updated + text[next_index:]


def update_portal() -> None:
    text = PORTAL.read_text()

    if "  goToAppPage,\n" not in text:
        text = replace_once(
            text,
            "  expectNoHorizontalOverflow,\n",
            "  expectNoHorizontalOverflow,\n  goToAppPage,\n",
            "portal goToAppPage import",
        )

    first_start = 'test("renders the portal home and applies the anonymous display control"'
    first_end = 'test("renders the upcoming exam countdown in chronological order"'
    first_test = '''test("renders the overview, apps, and anonymous system control", async ({
  page,
  request,
}) => {
  const browserErrors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") browserErrors.push(message.text());
  });
  page.on("pageerror", (error) => browserErrors.push(error.message));

  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "hyz things", level: 1 }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "总览", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("heading", { name: "网络拓扑" })).toBeVisible();
  await expect(
    page.getByText("Ethernet WAN", { exact: true }).first(),
  ).toBeVisible();
  await expect(
    page.getByText("Wi-Fi WAN", { exact: true }).first(),
  ).toBeVisible();
  await expect(page.getByText("主用", { exact: true }).first()).toBeVisible();
  await expect(
    page.getByText("Ethernet 优先", { exact: false }).first(),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "路由 / LAN" })).toBeVisible();
  await expect(
    page.getByText("E2E-Upstream", { exact: false }).first(),
  ).toBeVisible();

  const systemStatus = page.locator("article").filter({ hasText: "系统 / 流量" });
  await expect(systemStatus.getByText("WAN 总接收", { exact: true })).toBeVisible();
  await expect(systemStatus.getByText("34.1 MiB", { exact: true })).toBeVisible();
  await expect(systemStatus.getByText("WAN 总发送", { exact: true })).toBeVisible();
  await expect(systemStatus.getByText("6.6 MiB", { exact: true })).toBeVisible();

  const readonlyProxy = page.getByRole("region", { name: "当前代理与延迟" });
  await expect(readonlyProxy.getByText("当前选择 · 东京")).toBeVisible();
  await expect
    .poll(
      async () =>
        (await readHarnessState(request)).panel.proxy_groups.data[0].options[0]
          .delay_ms,
    )
    .toBe(40);
  await expect(readonlyProxy.getByText("当前 · 40 ms")).toBeVisible();
  await expect(readonlyProxy.getByText("新加坡 · 新加坡")).toBeVisible();
  await expect(readonlyProxy.getByRole("combobox")).toHaveCount(0);
  await expect(readonlyProxy.getByRole("button")).toHaveCount(0);

  const overviewAccessibility = await new AxeBuilder({ page }).analyze();
  expect(overviewAccessibility.violations).toEqual([]);
  await expectNoHorizontalOverflow(page);

  await goToAppPage(page, "应用");
  await expect(page.getByRole("heading", { name: "应用", exact: true })).toBeVisible();
  await expect(page.getByRole("article", { name: "路由器控制面" })).toBeVisible();
  await expect(page.getByRole("article", { name: "摄像头直播" })).toBeVisible();
  const deployedApps = page.getByRole("region", { name: "已部署应用" });
  await expect(deployedApps).toBeVisible();
  await expect(
    deployedApps.getByText("部署时间：", { exact: false }).first(),
  ).toBeVisible();

  // Anonymous users can see the Proxy entry but must not receive its admin controls.
  await goToAppPage(page, "代理");
  await expect(page.getByRole("heading", { name: "需要管理员登录" })).toBeVisible();
  await expect(page.getByRole("switch", { name: "LAN 透明代理" })).toHaveCount(0);
  await expect(page.getByRole("switch", { name: "本机系统代理" })).toHaveCount(0);

  await goToAppPage(page, "系统");
  const slider = page.getByRole("slider", { name: "点亮亮度" });
  await slider.fill("180");
  await page.getByRole("button", { name: "点亮", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "背光已开启" }),
  ).toBeVisible();

  const state = await readHarnessState(request);
  expect(state.panel.display.data).toMatchObject({
    enabled: true,
    brightness: 180,
    actual_brightness: 180,
  });
  expect(state.proxy.data.lan_tun.desired).toBe(true);
  expect(state.proxy.data.local_system_proxy.desired).toBe(false);
  expect(state.panel.proxy_groups.data[0].selected).toBe("东京");

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
  await expectNoHorizontalOverflow(page);
  expect(browserErrors).toEqual([]);
});

'''
    if first_start in text:
        text = replace_between(text, first_start, first_end, first_test, "portal first navigation test")

    swipe_name = "switches portal pages with horizontal touch swipes"
    def migrate_swipe(_segment: str) -> str:
        return '''test("switches portal pages with horizontal touch swipes", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  const pageButton = (name: "总览" | "网络" | "代理" | "Tailscale" | "摄像头" | "应用" | "系统") =>
    page.getByRole("button", { name, exact: true });

  await expect(pageButton("总览")).toHaveAttribute("aria-pressed", "true");
  for (const name of ["网络", "代理", "Tailscale", "摄像头", "应用", "系统"] as const) {
    await swipePortal(page, 300, 240, 80, 250);
    await expect(pageButton(name)).toHaveAttribute("aria-pressed", "true");
  }

  // The last page is a hard boundary.
  await swipePortal(page, 300, 240, 80, 250);
  await expect(pageButton("系统")).toHaveAttribute("aria-pressed", "true");

  await swipePortal(page, 80, 240, 300, 250);
  await expect(pageButton("应用")).toHaveAttribute("aria-pressed", "true");

  // A mostly vertical gesture must not switch pages.
  await swipePortal(page, 200, 240, 170, 360);
  await expect(pageButton("应用")).toHaveAttribute("aria-pressed", "true");

  await goToAppPage(page, "摄像头");
  await expect(page.getByRole("heading", { name: "摄像头直播" })).toBeVisible();
});
'''
    text = update_test_segment(text, swipe_name, migrate_swipe)

    touch_name = "switches portal pages from browser touch input"
    def migrate_touch(_segment: str) -> str:
        return '''test("switches portal pages from browser touch input", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "总览", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");

  await realTouchSwipe(page, "left");
  await expect(
    page.getByRole("button", { name: "网络", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");

  await realTouchSwipe(page, "right");
  await expect(
    page.getByRole("button", { name: "总览", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
});
'''
    text = update_test_segment(text, touch_name, migrate_touch)

    narrow_name = "fits a narrow portal screen without horizontal overflow"
    def migrate_narrow(_segment: str) -> str:
        return '''test("fits a narrow portal screen without horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "hyz things", level: 1 }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "总览", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("heading", { name: "网络拓扑" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "路由 / LAN" })).toBeVisible();
  await expectNoHorizontalOverflow(page);

  for (const name of ["总览", "网络", "代理", "Tailscale", "摄像头", "应用", "系统"] as const) {
    await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
  }

  await goToAppPage(page, "网络");
  await expect(page.getByRole("button", { name: "管理员登录" })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});
'''
    text = update_test_segment(text, narrow_name, migrate_narrow)

    # Collapse the old nested Router -> Network Settings route everywhere it remains.
    nested_route = re.compile(
        r'await page\.getByRole\("button", \{ name: "路由器", exact: true \}\)\.click\(\);\n\s*'
        r'await page\.getByRole\("button", \{ name: "网络设置", exact: true \}\)\.click\(\);'
    )
    text = nested_route.sub('await goToAppPage(page, "网络");', text)

    # Any remaining old Network Settings tab references now refer to the first-level Network page.
    text = text.replace('name: "网络设置", exact: true', 'name: "网络", exact: true')
    text = text.replace('name: "首页", exact: true', 'name: "总览", exact: true')

    # Preserve the network-draft navigation assertion using the new first-level pages.
    text = text.replace(
        'await page.getByRole("button", { name: "总览", exact: true }).click();\n'
        '  await page.getByRole("button", { name: "网络", exact: true }).click();',
        'await goToAppPage(page, "总览");\n  await goToAppPage(page, "网络");',
    )

    # Proxy capability tests now navigate explicitly after the shared login helper.
    for test_name in [
        "controls all four proxy combinations with isolated failures on desktop and mobile",
        "shows local system proxy status from ProxyStatus without Tailscale coupling",
    ]:
        def to_proxy(segment: str) -> str:
            needle = '  await loginAsAdmin(page);\n'
            if '  await goToAppPage(page, "代理");\n' not in segment:
                if needle not in segment:
                    raise SystemExit(f"{test_name}: login helper not found")
                segment = segment.replace(
                    needle,
                    needle + '  await goToAppPage(page, "代理");\n',
                    1,
                )
            return segment
        text = update_test_segment(text, test_name, to_proxy)

    # Tailnet peer test uses the same login helper but belongs on the Tailscale page.
    def to_tailscale_after_helper(segment: str) -> str:
        needle = '  await loginAsAdmin(page);\n'
        if '  await goToAppPage(page, "Tailscale");\n' not in segment:
            if needle not in segment:
                raise SystemExit("tailnet peer test: login helper not found")
            segment = segment.replace(
                needle,
                needle + '  await goToAppPage(page, "Tailscale");\n',
                1,
            )
        return segment
    text = update_test_segment(
        text,
        "shows Tailnet peer empty, initial-error, stale, and recovery states",
        to_tailscale_after_helper,
    )

    # The administrator journey still verifies Proxy node selection, but that control now lives
    # on the Proxy page. Return to Network before continuing STA/AP/device/subscription behavior.
    def migrate_admin_journey(segment: str) -> str:
        old = '''  await expect(page.getByRole("heading", { name: "代理设置" })).toBeVisible();
  await page
    .getByRole("combobox", { name: "自动选择 节点" })
    .selectOption("新加坡");
'''
        new = '''  await expect(
    page.getByRole("button", { name: "上游 Wi-Fi (STA)" }),
  ).toBeVisible();
  await goToAppPage(page, "代理");
  await expect(page.getByRole("heading", { name: "代理设置" })).toBeVisible();
  await page
    .getByRole("combobox", { name: "自动选择 节点" })
    .selectOption("新加坡");
'''
        if new in segment:
            pass
        elif old in segment:
            segment = segment.replace(old, new, 1)
        else:
            raise SystemExit("administrator journey: Proxy migration anchor not found")

        return_marker = '''    .toBe("新加坡");

  await page.getByRole("button", { name: "上游 Wi-Fi (STA)" }).click();
'''
        return_new = '''    .toBe("新加坡");

  await goToAppPage(page, "网络");
  await page.getByRole("button", { name: "上游 Wi-Fi (STA)" }).click();
'''
        if return_marker in segment:
            segment = segment.replace(return_marker, return_new, 1)
        elif return_new not in segment:
            raise SystemExit("administrator journey: Network return anchor not found")
        return segment
    text = update_test_segment(
        text,
        "supports the administrator, STA, AP, and write-only subscription journey",
        migrate_admin_journey,
    )

    # Camera admin controls require logging in through Network, then returning to Camera.
    def migrate_camera(segment: str) -> str:
        segment = segment.replace(
            '// 首页只有应用入口卡片，直播卡片在摄像头视图内。',
            '// 总览不自动启动摄像头；直播能力在摄像头一级页面内。',
        )
        old_expect = '  await expect(page.getByRole("heading", { name: "代理设置" })).toBeVisible();\n\n  await page.getByRole("button", { name: "摄像头", exact: true }).click();'
        new_expect = '  await expect(page.getByRole("button", { name: "上游 Wi-Fi (STA)" })).toBeVisible();\n\n  await goToAppPage(page, "摄像头");'
        if old_expect in segment:
            segment = segment.replace(old_expect, new_expect, 1)
        elif new_expect not in segment:
            raise SystemExit("camera test: admin return anchor not found")
        return segment
    text = update_test_segment(
        text,
        "plays the camera anonymously and controls it as an administrator",
        migrate_camera,
    )

    # Tailscale's full administrator flow logs in through Network, then opens its own page.
    def migrate_tailscale_flow(segment: str) -> str:
        marker = '  const tailscale = page.getByRole("region", {\n    name: "Tailscale 远程 LAN 状态",\n  });\n'
        addition = '  await goToAppPage(page, "Tailscale");\n' + marker
        if addition not in segment:
            if marker not in segment:
                raise SystemExit("Tailscale flow: region anchor not found")
            segment = segment.replace(marker, addition, 1)
        return segment
    text = update_test_segment(
        text,
        "supports the administrator Tailscale login, approval, disable, and logout flow",
        migrate_tailscale_flow,
    )

    # Overview is already the default page; old standalone Router navigation is obsolete.
    text = text.replace(
        '  await page.getByRole("button", { name: "路由器", exact: true }).click();\n'
        '  await expect(page.getByRole("heading", { name: "网络拓扑" })).toBeVisible();',
        '  await expect(page.getByRole("heading", { name: "网络拓扑" })).toBeVisible();',
    )

    stale_patterns = [
        'getByRole("button", { name: "路由器", exact: true })',
        'getByRole("button", { name: "网络设置", exact: true })',
        'getByRole("button", { name: "首页", exact: true })',
    ]
    leftovers = [pattern for pattern in stale_patterns if pattern in text]
    if leftovers:
        lines = []
        for number, line in enumerate(text.splitlines(), 1):
            if any(pattern in line for pattern in leftovers):
                lines.append(f"{number}: {line.strip()}")
        raise SystemExit("stale navigation selectors remain:\n" + "\n".join(lines))

    PORTAL.write_text(text)


update_app()
update_fixtures()
update_portal()
print("migrated app panel lifecycle and Playwright navigation contracts")
