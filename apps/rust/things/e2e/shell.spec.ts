import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import {
  expectNoHorizontalOverflow,
  goToAppPage,
  harnessOrigin,
  installCameraWebRtcMock,
  loginAsAdmin,
  readHarnessState,
  resetHarness,
  webOrigin,
} from "./fixtures";
import {
  readCountdownTotal,
  readCustomCountdownTotal,
  realTouchSwipe,
  swipePortal,
} from "./support";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("renders the overview, apps, and anonymous system control", async ({
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

test("serves a standalone PWA manifest", async ({ request }) => {
  const root = await request.get("/");
  expect(root.ok()).toBeTruthy();
  expect(await root.text()).toContain('rel="manifest"');

  const manifestResponse = await request.get("/manifest.webmanifest");
  expect(manifestResponse.ok()).toBeTruthy();
  expect(manifestResponse.headers()["content-type"]).toContain("manifest+json");
  const manifest = await manifestResponse.json();
  expect(manifest).toMatchObject({
    id: "/",
    start_url: "/",
    scope: "/",
    display: "standalone",
    display_override: ["standalone", "fullscreen"],
  });
  expect(manifest.icons).toEqual(
    expect.arrayContaining([
      expect.objectContaining({
        src: "/pwa-icon-192.svg",
        sizes: "192x192",
      }),
      expect.objectContaining({
        src: "/pwa-icon-512.svg",
        sizes: "512x512",
      }),
    ]),
  );
});

test("falls back to a canvas video stream when documentPictureInPicture is missing", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "documentPictureInPicture", {
      configurable: true,
      value: undefined,
    });
    const calls: string[] = [];
    let inPip = false;
    (window as any).__hyzCountdownVideoInPip = () => inPip;
    Object.defineProperty(HTMLVideoElement.prototype, "requestPictureInPicture", {
      configurable: true,
      value: function () {
        calls.push("request");
        inPip = true;
        return Promise.resolve();
      },
    });
    (window as any).__hyzCountdownVideoPipCalls = calls;
  });
  await page.goto("/");
  // addInitScript 里的 document 是导航前的空文档，属性定义不生效；
  // 导航后再桩掉 pictureInPictureElement：请求成功后才返回倒计时 video，
  // 模拟系统画中画真的激活（避免轮询在请求前就误判已激活）。
  await page.evaluate(() => {
    Object.defineProperty(document, "pictureInPictureElement", {
      configurable: true,
      get() {
        return (window as any).__hyzCountdownVideoInPip()
          ? document.querySelector("video[data-countdown-pip-active]") || null
          : null;
      },
    });
  });

  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="guangdong-exam"]')
    .dblclick();

  await expect(
    page.locator("canvas[data-countdown-canvas]").first(),
  ).toBeAttached();
  await expect(
    page.locator("video[data-countdown-video]").first(),
  ).toBeAttached();
  await expect
    .poll(() =>
      page.evaluate(
        () => (window as any).__hyzCountdownVideoPipCalls.length,
      ),
    )
    .toBe(1);
  // 画布首帧已绘制（背景不透明，不是空白/黑屏）。
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

  for (const name of ["总览", "网络", "代理", "Tailscale", "摄像头", "应用", "系统"] as const) {
    await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
  }

  await goToAppPage(page, "网络");
  await expect(page.getByRole("button", { name: "管理员登录" })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});
