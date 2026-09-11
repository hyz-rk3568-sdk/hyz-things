import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import {
  expectNoHorizontalOverflow,
  goToAppPage,
  harnessOrigin,
  loginAsAdmin,
  readHarnessState,
  resetHarness,
} from "./fixtures";
import {
  installCountdownVideoPipMock,
  readCountdownTotal,
  readCustomCountdownTotal,
  realTouchSwipe,
  swipePortal,
} from "./support";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("renders the dashboard shell and public overview", async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "hyz things", level: 1 }),
  ).toBeVisible();
  await expect(page.getByRole("navigation", { name: "主导航" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "网络拓扑" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "核心健康状态" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "代理状态" })).toBeVisible();
  const topology = page.getByRole("region", { name: "网络拓扑" });
  await expect(topology.getByText("Tailscale", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "运行详情" })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});

test("requires administrator authentication for configuration pages", async ({ page }) => {
  await page.goto("/");
  await goToAppPage(page, "网络");
  await expect(page.getByRole("button", { name: "管理员登录" })).toBeVisible();
  await goToAppPage(page, "代理");
  await expect(page.getByRole("heading", { name: "需要管理员登录" })).toBeVisible();
  await goToAppPage(page, "活动");
  await expect(page.getByRole("heading", { name: "需要管理员登录" })).toBeVisible();
  await goToAppPage(page, "Tailscale");
  await expect(page.getByRole("heading", { name: "需要管理员登录" })).toBeVisible();
});

test("keeps local countdown state across portal page switches", async ({ page }) => {
  await page.goto("/");
  const initial = await readCountdownTotal(page);
  await goToAppPage(page, "网络");
  await goToAppPage(page, "总览");
  await expect
    .poll(async () => readCountdownTotal(page), { timeout: 7_500 })
    .toBeLessThanOrEqual(initial);
});

test("keeps local custom countdown state across portal page switches", async ({ page }) => {
  await page.goto("/");
  const initial = await readCustomCountdownTotal(page);
  await goToAppPage(page, "网络");
  await goToAppPage(page, "总览");
  await expect
    .poll(async () => readCustomCountdownTotal(page), { timeout: 7_500 })
    .toBeLessThanOrEqual(initial);
});

test("keeps the countdown canvas alive across page switches", async ({ page }) => {
  await installCountdownVideoPipMock(page);
  await page.goto("/");
  const canvas = page.locator("canvas[data-countdown-canvas]").first();
  await expect(canvas).toBeVisible();
  await goToAppPage(page, "网络");
  await goToAppPage(page, "总览");
  await expect(canvas).toBeVisible();
});

test("opens countdown video picture-in-picture", async ({ page }) => {
  await installCountdownVideoPipMock(page);
  await page.goto("/");
  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="guangdong-exam"]')
    .dblclick();
  await expect
    .poll(async () =>
      page.evaluate(
        () => (window as any).__hyzCountdownVideoPipCalls.length,
      ),
    )
    .toBe(1);
  const pixel = await page.evaluate(() => {
    const canvas = document.querySelector(
      "canvas[data-countdown-canvas]",
    ) as HTMLCanvasElement;
    const context = canvas.getContext("2d") as CanvasRenderingContext2D;
    return Array.from(context.getImageData(10, 10, 1, 1).data);
  });
  expect(pixel[3]).toBe(255);
  await expectNoHorizontalOverflow(page);
});

test("switches portal pages with horizontal touch swipes", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  const pageButton = (
    name:
      | "总览"
      | "网络"
      | "代理"
      | "活动"
      | "Tailscale"
      | "摄像头"
      | "应用"
      | "系统",
  ) => page.getByRole("button", { name, exact: true });

  await expect(pageButton("总览")).toHaveAttribute("aria-pressed", "true");
  for (const name of [
    "网络",
    "代理",
    "活动",
    "Tailscale",
    "摄像头",
    "应用",
    "系统",
  ] as const) {
    await swipePortal(page, 300, 240, 80, 250);
    await expect(pageButton(name)).toHaveAttribute("aria-pressed", "true");
  }

  await swipePortal(page, 300, 240, 80, 250);
  await expect(pageButton("系统")).toHaveAttribute("aria-pressed", "true");

  await swipePortal(page, 80, 240, 300, 250);
  await expect(pageButton("应用")).toHaveAttribute("aria-pressed", "true");

  await swipePortal(page, 200, 240, 170, 360);
  await expect(pageButton("应用")).toHaveAttribute("aria-pressed", "true");

  await goToAppPage(page, "摄像头");
  await expect(page.getByRole("heading", { name: "摄像头直播" })).toBeVisible();
});

test("switches portal pages from browser touch input", async ({ page }) => {
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

test("renders the dual-uplink topology without mobile overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "网络拓扑" })).toBeVisible();
  await expect(
    page.getByText("Ethernet WAN", { exact: true }).first(),
  ).toBeVisible();
  await expect(
    page.getByText("Wi-Fi WAN", { exact: true }).first(),
  ).toBeVisible();
  await expect(page.getByText("主用", { exact: true }).first()).toBeVisible();
  await expectNoHorizontalOverflow(page);
  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
});

test("keeps the last dashboard while a component becomes degraded", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "代理状态" })).toBeVisible();

  const current = await readHarnessState(request);
  current.proxy.state = "degraded";
  current.proxy.issue = {
    code: "e2e_proxy_degraded",
    message: "代理探测暂时不可用",
  };
  const update = await request.put(`${harnessOrigin}/state`, { data: current });
  expect(update.ok()).toBeTruthy();

  await expect(page.getByText("代理探测暂时不可用")).toBeVisible({
    timeout: 7_500,
  });
  const proxyHealth = page
    .getByRole("region", { name: "核心健康状态" })
    .locator("article")
    .filter({ hasText: "Proxy" });
  await expect(proxyHealth.getByText("需检查", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "代理状态" })).toBeVisible();
});

test("fits a narrow portal screen without horizontal overflow", async ({
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

  for (const name of [
    "总览",
    "网络",
    "代理",
    "活动",
    "Tailscale",
    "摄像头",
    "应用",
    "系统",
  ] as const) {
    await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
  }

  const mobileNavigation = page.getByRole("navigation", { name: "主导航" });
  const mobileOverviewPanel = page.locator("#app-overview-panel");
  const [
    mobileNavigationBox,
    mobilePanelBox,
    mobileOverviewTabBox,
    mobileActivityTabBox,
    mobileTailscaleTabBox,
  ] = await Promise.all([
    mobileNavigation.boundingBox(),
    mobileOverviewPanel.boundingBox(),
    page.getByRole("button", { name: "总览", exact: true }).boundingBox(),
    page.getByRole("button", { name: "活动", exact: true }).boundingBox(),
    page.getByRole("button", { name: "Tailscale", exact: true }).boundingBox(),
  ]);
  expect(mobileNavigationBox).not.toBeNull();
  expect(mobilePanelBox).not.toBeNull();
  expect(mobileOverviewTabBox).not.toBeNull();
  expect(mobileActivityTabBox).not.toBeNull();
  expect(mobileTailscaleTabBox).not.toBeNull();
  expect(mobileNavigationBox!.y + mobileNavigationBox!.height).toBeLessThanOrEqual(
    mobilePanelBox!.y,
  );
  for (const tabBox of [
    mobileOverviewTabBox!,
    mobileActivityTabBox!,
    mobileTailscaleTabBox!,
  ]) {
    expect(tabBox.y).toBeGreaterThanOrEqual(mobileNavigationBox!.y - 1);
    expect(tabBox.y + tabBox.height).toBeLessThanOrEqual(
      mobileNavigationBox!.y + mobileNavigationBox!.height + 1,
    );
  }

  await goToAppPage(page, "网络");
  await expect(page.getByRole("button", { name: "管理员登录" })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});
