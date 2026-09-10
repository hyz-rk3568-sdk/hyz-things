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

async function swipePortal(
  page: Page,
  fromX: number,
  fromY: number,
  toX: number,
  toY: number,
) {
  const surface = page.locator("#portal-swipe-surface");
  const event = {
    bubbles: true,
    button: 0,
    clientX: fromX,
    clientY: fromY,
    isPrimary: true,
    pointerId: 1,
    pointerType: "touch",
  };
  await surface.dispatchEvent("pointerdown", event);
  await surface.dispatchEvent("pointerup", {
    ...event,
    clientX: toX,
    clientY: toY,
  });
}

async function realTouchSwipe(page: Page, direction: "left" | "right") {
  const surface = page.locator("#portal-swipe-surface");
  const box = await surface.boundingBox();
  if (!box) throw new Error("portal swipe surface has no bounding box");
  const client = await page.context().newCDPSession(page);
  await client.send("Emulation.setTouchEmulationEnabled", {
    enabled: true,
    configuration: "mobile",
  });

  const startX = Math.round(
    box.x + box.width * (direction === "left" ? 0.72 : 0.18),
  );
  const endX = Math.round(
    box.x + box.width * (direction === "left" ? 0.18 : 0.72),
  );
  const y = Math.round(box.y + 180);
  const touchPoint = (x: number, y: number) => ({
    x,
    y,
    radiusX: 8,
    radiusY: 8,
    force: 1,
    id: 1,
  });

  await client.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [touchPoint(startX, y)],
    modifiers: 0,
  });
  for (const x of [
    startX + (endX - startX) * 0.35,
    startX + (endX - startX) * 0.7,
    endX,
  ]) {
    await client.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [touchPoint(Math.round(x), y + 4)],
      modifiers: 0,
    });
  }
  await client.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
    modifiers: 0,
  });
}

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

test("renders the upcoming exam countdown in chronological order", async ({
  page,
}) => {
  await page.goto("/");

  const countdown = page.getByRole("region", { name: "考试冲刺倒计时" });
  await expect(countdown).toBeVisible();
  const cards = countdown.locator("[data-exam-id]");
  await expect(cards).toHaveCount(4);
  const cardIds = await cards.evaluateAll((elements) =>
    elements.map((element) => element.getAttribute("data-exam-id")),
  );
  expect(cardIds).toEqual([
    "national-exam",
    "guangdong-exam",
    "hunan-civil-service",
    "hunan-public-institution",
  ]);
  await expect(
    countdown.getByText("下一次国考", { exact: true }),
  ).toBeVisible();
  await expect(countdown.getByText("广东省考", { exact: true })).toBeVisible();
  await expect(countdown.getByText("湖南省考", { exact: true })).toBeVisible();
  await expect(
    countdown.getByText("湖南事业编", { exact: true }),
  ).toBeVisible();
  await expect(countdown.locator("progress")).toHaveCount(4);
  await expect(countdown.locator("progress").first()).toHaveAttribute(
    "aria-label",
    /冲刺进度/,
  );
  await expectNoHorizontalOverflow(page);

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
});

test("renders three compact configurable countdown timers", async ({
  page,
}) => {
  await page.goto("/");

  const timer = page.getByRole("region", { name: "自定义倒计时" });
  await expect(timer).toBeVisible();
  await expect(
    timer.getByRole("heading", {
      name: "自定义倒计时",
      exact: true,
      level: 2,
    }),
  ).toBeVisible();

  const cards = timer.locator('[data-exam-id^="custom-"]');
  await expect(cards).toHaveCount(3);
  await expect(cards.evaluateAll((elements) =>
    elements.map((element) => element.getAttribute("data-exam-id")),
  )).resolves.toEqual([
    "custom-morning-countdown",
    "custom-afternoon-countdown",
    "custom-evening-countdown",
  ]);

  for (const title of ["上午", "下午", "晚上"]) {
    await expect(timer.getByRole("heading", { name: title, level: 3 })).toBeVisible();
    await expect(timer.getByLabel(`${title}小时`)).toHaveAttribute("type", "number");
    await expect(timer.getByLabel(`${title}分钟`)).toHaveAttribute("type", "number");
    await expect(timer.getByLabel(`${title}秒`)).toHaveAttribute("type", "number");
    await expect(
      timer.getByRole("button", { name: `开始${title}倒计时`, exact: true }),
    ).toBeVisible();
    await expect(
      timer.getByRole("button", { name: `进入${title}画中画`, exact: true }),
    ).toBeVisible();
  }

  await expect(timer.getByText("最多 99 小时，输入会自动保存到当前浏览器")).toHaveCount(0);
  await expect(
    timer.getByText("倒计时运行和暂停状态会保存在本机浏览器；刷新页面后会从当前状态继续，画中画需要重新点击进入。"),
  ).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
});

test("keeps the three custom countdowns independent after refresh", async ({
  page,
}) => {
  await page.goto("/");
  const timer = page.getByRole("region", { name: "自定义倒计时" });

  await timer.getByLabel("上午分钟").fill("1");
  await timer.getByLabel("下午秒").fill("10");
  await timer.getByRole("button", { name: "开始上午倒计时", exact: true }).click();
  await expect(
    timer.getByRole("button", { name: "暂停上午倒计时", exact: true }),
  ).toBeVisible();
  await expect(
    timer.getByRole("button", { name: "开始下午倒计时", exact: true }),
  ).toBeVisible();

  await timer.getByRole("button", { name: "暂停上午倒计时", exact: true }).click();
  await expect(
    timer.getByRole("button", { name: "继续上午倒计时", exact: true }),
  ).toBeVisible();
  await expect(
    timer.getByRole("button", { name: "开始下午倒计时", exact: true }),
  ).toBeVisible();

  await page.reload();
  const restoredTimer = page.getByRole("region", { name: "自定义倒计时" });
  await expect(restoredTimer.getByLabel("上午分钟")).toHaveValue("1");
  await expect(
    restoredTimer.getByRole("button", { name: "继续上午倒计时", exact: true }),
  ).toBeVisible();
  await expect(
    restoredTimer.getByRole("button", { name: "开始下午倒计时", exact: true }),
  ).toBeVisible();
  await expectNoHorizontalOverflow(page);
});

test("restarts the selected custom countdown from its configured duration", async ({
  page,
}) => {
  await page.goto("/");
  const timer = page.getByRole("region", { name: "自定义倒计时" });
  const cardId = "custom-evening-countdown";

  await timer.getByLabel("晚上分钟").fill("0");
  await timer.getByLabel("晚上秒").fill("3");
  await timer.getByRole("button", { name: "开始晚上倒计时", exact: true }).click();
  await expect(
    timer.getByRole("button", { name: "暂停晚上倒计时", exact: true }),
  ).toBeVisible();
  await expect
    .poll(() => readCustomCountdownTotal(page, cardId), { timeout: 4_000 })
    .toBeLessThan(3);

  await timer
    .getByRole("button", { name: "重新开始晚上倒计时", exact: true })
    .click();
  await expect
    .poll(() => readCustomCountdownTotal(page, cardId), { timeout: 2_000 })
    .toBeGreaterThanOrEqual(2);
});

test("opens the selected custom countdown in a picture-in-picture window", async ({
  page,
  context,
}) => {
  await page.goto("/");
  const timer = page.getByRole("region", { name: "自定义倒计时" });

  await timer.getByLabel("下午分钟").fill("0");
  await timer.getByLabel("下午秒").fill("10");
  await timer.getByRole("button", { name: "开始下午倒计时", exact: true }).click();

  const pipPagePromise = context.waitForEvent("page");
  await timer.getByRole("button", { name: "进入下午画中画", exact: true }).click();
  const pipPage = await pipPagePromise;
  await expect(
    pipPage.locator('[data-exam-id="custom-afternoon-countdown"]'),
  ).toBeVisible();
  await expect(
    pipPage.getByRole("button", { name: "关闭画中画" }),
  ).toBeVisible();
  await expect(pipPage.locator("[data-pip-completion]")).toHaveText(
    /^预计完成 \d{2}:\d{2}$/,
  );
  await pipPage.getByRole("button", { name: "关闭画中画" }).click();
});

test("shows running, paused, and completed custom countdown completion times in picture-in-picture", async ({
  page,
  context,
}) => {
  await page.goto("/");
  const timer = page.getByRole("region", { name: "自定义倒计时" });

  await timer.getByLabel("下午分钟").fill("0");
  await timer.getByLabel("下午秒").fill("30");
  await timer.getByRole("button", { name: "开始下午倒计时", exact: true }).click();

  let pipPagePromise = context.waitForEvent("page");
  await timer.getByRole("button", { name: "进入下午画中画", exact: true }).click();
  let pipPage = await pipPagePromise;
  await expect(pipPage.locator("[data-pip-completion]")).toHaveText(
    /^预计完成 \d{2}:\d{2}$/,
  );
  await pipPage.getByRole("button", { name: "关闭画中画" }).click();

  await timer.getByRole("button", { name: "暂停下午倒计时", exact: true }).click();
  pipPagePromise = context.waitForEvent("page");
  await timer.getByRole("button", { name: "进入下午画中画", exact: true }).click();
  pipPage = await pipPagePromise;
  await expect(pipPage.locator("[data-pip-completion]")).toHaveText(
    /^继续后预计完成 \d{2}:\d{2}$/,
  );
  await pipPage.getByRole("button", { name: "关闭画中画" }).click();

  await timer.getByLabel("晚上分钟").fill("0");
  await timer.getByLabel("晚上秒").fill("1");
  await timer.getByRole("button", { name: "开始晚上倒计时", exact: true }).click();
  await expect(
    timer.getByRole("article", { name: "晚上倒计时已结束", exact: true }),
  ).toBeVisible({ timeout: 4_000 });

  pipPagePromise = context.waitForEvent("page");
  await timer.getByRole("button", { name: "进入晚上画中画", exact: true }).click();
  pipPage = await pipPagePromise;
  await expect(pipPage.locator("[data-pip-completion]")).toHaveText(
    /^完成于 \d{2}:\d{2}$/,
  );
  await pipPage.getByRole("button", { name: "关闭画中画" }).click();
});

test("persists the completed selected countdown after a page refresh", async ({
  page,
}) => {
  await page.goto("/");
  const timer = page.getByRole("region", { name: "自定义倒计时" });
  const cardId = "custom-evening-countdown";

  await timer.getByLabel("晚上分钟").fill("0");
  await timer.getByLabel("晚上秒").fill("1");
  await timer.getByRole("button", { name: "开始晚上倒计时", exact: true }).click();
  await expect(
    timer.getByRole("article", { name: "晚上倒计时已结束", exact: true }),
  ).toBeVisible({ timeout: 4_000 });

  await page.reload();
  const restoredTimer = page.getByRole("region", { name: "自定义倒计时" });
  await expect(
    restoredTimer.getByRole("article", {
      name: "晚上倒计时已结束",
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    restoredTimer.getByRole("button", {
      name: "重新开始晚上倒计时",
      exact: true,
    }),
  ).toBeVisible();
  await expect(restoredTimer.locator(`[data-exam-id="${cardId}"]`)).toHaveAttribute(
    "data-remaining-seconds",
    "0",
  );
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

test("keeps exam countdown cards dark and low-contrast", async ({ page }) => {
  await page.goto("/");

  const countdown = page.getByRole("region", { name: "考试冲刺倒计时" });
  const cards = countdown.locator("[data-exam-id]");
  await expect(cards.first()).toHaveClass(/bg-base-300\/90/);
  await expect(cards.first().locator('[aria-live="polite"]')).toHaveClass(
    /text-base-content/,
  );
});

async function readCountdownTotal(target: Page, cardId: string): Promise<number> {
  return target.evaluate((id) => {
    const counter = document.querySelector(
      `[data-exam-id="${id}"] [data-countdown-values]`,
    );
    if (!counter) {
      return 0;
    }
    let total = 0;
    counter.querySelectorAll("strong[data-value]").forEach((element) => {
      total += Number(element.textContent);
    });
    return total;
  }, cardId);
}

async function readCustomCountdownTotal(
  target: Page,
  cardId: string,
): Promise<number> {
  return target.evaluate((id) => {
    const card = document.querySelector(`[data-exam-id="${id}"]`);
    return Number(card?.getAttribute("data-remaining-seconds") ?? 0);
  }, cardId);
}

test("opens an exam countdown in a picture-in-picture window with a double click", async ({
  page,
  context,
}) => {
  await page.goto("/");

  const pipPagePromise = context.waitForEvent("page");
  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="national-exam"]')
    .dblclick();
  const pipPage = await pipPagePromise;

  const pipCard = pipPage.locator('[data-exam-id="national-exam"]');
  await expect(pipCard).toBeVisible();
  await expect(
    pipPage.getByRole("button", { name: "关闭画中画" }),
  ).toBeVisible();
  await expect(pipPage.locator("[data-pip-completion]")).toHaveText(
    /^预计完成 \d{4}年\d{2}月\d{2}日 \d{2}:\d{2}$/,
  );

  // 样式表已复制进画中画窗口：卡片计算样式与主页面一致。
  const mainBackground = await page.evaluate(
    () =>
      getComputedStyle(
        document.querySelector('[data-exam-id="national-exam"]'),
      ).backgroundColor,
  );
  await expect
    .poll(() =>
      pipPage.evaluate(
        () =>
          getComputedStyle(
            document.querySelector('[data-exam-id="national-exam"]'),
          ).backgroundColor,
      ),
    )
    .toBe(mainBackground);

  // 数值与主页面一致（画中画刚打开，允许 1 秒的展示偏差）。
  const pipTotal = await readCountdownTotal(pipPage, "national-exam");
  const mainTotal = await readCountdownTotal(page, "national-exam");
  expect(Math.abs(mainTotal - pipTotal)).toBeLessThanOrEqual(1);
  await expectNoHorizontalOverflow(page);
});

test("keeps the countdown ticking inside the picture-in-picture window", async ({
  page,
  context,
}) => {
  await page.goto("/");
  const pipPagePromise = context.waitForEvent("page");
  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="hunan-civil-service"]')
    .dblclick();
  const pipPage = await pipPagePromise;
  await expect(
    pipPage.getByRole("button", { name: "关闭画中画" }),
  ).toBeVisible();

  const before = await readCountdownTotal(pipPage, "hunan-civil-service");
  await pipPage.waitForTimeout(2_300);
  const after = await readCountdownTotal(pipPage, "hunan-civil-service");
  const diff = before - after;
  expect(diff).toBeGreaterThanOrEqual(1);
  expect(diff).toBeLessThanOrEqual(4);
});

test("closes the picture-in-picture window with Escape and reopens it", async ({
  page,
  context,
}) => {
  await page.goto("/");
  const firstPipPromise = context.waitForEvent("page");
  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="national-exam"]')
    .dblclick();
  const pipPage = await firstPipPromise;
  await expect(
    pipPage.getByRole("button", { name: "关闭画中画" }),
  ).toBeVisible();

  // Esc 会让画中画窗口立刻关闭，Playwright 在按键派发中途发现目标页已关闭
  // 会抛 "Target page ... has been closed"，行为本身符合预期，容忍该错误。
  await pipPage.keyboard.press("Escape").catch(() => {});
  await expect.poll(() => context.pages().length).toBe(1);

  const secondPipPromise = context.waitForEvent("page");
  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="national-exam"]')
    .dblclick();
  const secondPip = await secondPipPromise;
  await expect(
    secondPip.getByRole("button", { name: "关闭画中画" }),
  ).toBeVisible();
});

test("opens a countdown picture-in-picture window on mobile", async ({
  page,
  context,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  const pipPagePromise = context.waitForEvent("page");
  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="hunan-public-institution"]')
    .dblclick();
  const pipPage = await pipPagePromise;

  await expect(
    pipPage.locator('[data-exam-id="hunan-public-institution"]'),
  ).toBeVisible();
  await expectNoHorizontalOverflow(page);
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

test("pumps WebCodecs frames into a WebKit-style picture-in-picture stream", async ({
  page,
}) => {
  // 模拟 iPadOS Safari：无 Document PiP，有 VideoTrackGenerator、
  // webkitSetPresentationMode，但没有可靠 captureStream 语义——
  // 前端必须走 WebCodecs 轨道生成器路径并真实泵帧。
  await page.addInitScript(() => {
    Object.defineProperty(window, "documentPictureInPicture", {
      configurable: true,
      value: undefined,
    });
    Object.defineProperty(HTMLVideoElement.prototype, "webkitSetPresentationMode", {
      configurable: true,
      value: function () {},
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
    (window as any).__hyzPipFrameWrites = 0;
    class FakeVideoTrackGenerator {
      track: MediaStreamTrack;
      writable: { getWriter: () => { write: (frame: VideoFrame) => Promise<void> } };
      constructor() {
        const canvas = document.createElement("canvas");
        const stream = (canvas as any).captureStream(10);
        this.track = stream.getVideoTracks()[0];
        this.writable = {
          getWriter: () => ({
            write: (frame: VideoFrame) => {
              (window as any).__hyzPipFrameWrites += 1;
              frame.close();
              return Promise.resolve();
            },
          }),
        };
      }
    }
    Object.defineProperty(window, "VideoTrackGenerator", {
      configurable: true,
      value: FakeVideoTrackGenerator,
    });
  });
  await page.goto("/");
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

  // WebKit 路径不创建 captureStream 复制 canvas。
  await expect(
    page.locator("canvas[data-countdown-capture-canvas]"),
  ).toHaveCount(0);
  // 轨道生成器持续收到真实 VideoFrame。
  await expect
    .poll(() =>
      page.evaluate(() => (window as any).__hyzPipFrameWrites as number),
    )
    .toBeGreaterThan(0);

  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="guangdong-exam"]')
    .dblclick();

  await expect
    .poll(() =>
      page.evaluate(
        () => (window as any).__hyzCountdownVideoPipCalls.length,
      ),
    )
    .toBe(1);
  await expect(page.locator("video[data-countdown-pip-active]")).toHaveCount(1);
  // 源 canvas 首帧已绘制（背景不透明，不是空白/黑屏）。
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

test("shows an activation-failure notice when the picture-in-picture request is rejected", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "documentPictureInPicture", {
      configurable: true,
      value: undefined,
    });
    Object.defineProperty(HTMLVideoElement.prototype, "requestPictureInPicture", {
      configurable: true,
      value: function () {
        throw new DOMException("Not allowed", "NotAllowedError");
      },
    });
  });
  await page.goto("/");

  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="guangdong-exam"]')
    .dblclick();

  await expect(page.locator("[data-exam-countdown-notice]")).toContainText(
    "画中画激活失败，请再试一次",
  );
  // 预创建的隐藏视频常驻 DOM；断言的是没有视频进入激活中的画中画状态。
  await expect(page.locator("video[data-countdown-pip-active]")).toHaveCount(0);
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("shows a notice when no picture-in-picture API is available", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "documentPictureInPicture", {
      configurable: true,
      value: undefined,
    });
    Object.defineProperty(HTMLVideoElement.prototype, "requestPictureInPicture", {
      configurable: true,
      value: undefined,
    });
    Object.defineProperty(HTMLVideoElement.prototype, "webkitSetPresentationMode", {
      configurable: true,
      value: undefined,
    });
  });
  await page.goto("/");

  await page
    .getByRole("region", { name: "考试冲刺倒计时" })
    .locator('[data-exam-id="national-exam"]')
    .dblclick();

  await expect(page.locator("[data-exam-countdown-notice]")).toContainText(
    "当前浏览器不支持画中画",
  );
  // API 全缺时不会预创建隐藏视频，更不会进入激活中的画中画状态。
  await expect(page.locator("video[data-countdown-pip-active]")).toHaveCount(0);
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("keeps the exam countdown usable on mobile", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  const countdown = page.getByRole("region", { name: "考试冲刺倒计时" });
  await expect(countdown).toBeVisible();
  await expect(countdown.locator("[data-exam-id]")).toHaveCount(4);
  await expect(
    countdown.getByText("湖南事业编", { exact: true }),
  ).toBeVisible();
  await expectNoHorizontalOverflow(page);

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
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

test("serves the generated bundle through the strict production-shaped HTTP boundary", async ({
  request,
}) => {
  const root = await request.get("/");
  expect(root.ok()).toBeTruthy();
  const csp = root.headers()["content-security-policy"] ?? "";
  expect(csp).toContain("script-src 'self' 'wasm-unsafe-eval'");
  expect(csp).not.toContain("'unsafe-inline'");
  expect(csp).not.toContain("script-src 'self' 'unsafe-eval'");

  const html = await root.text();
  expect(html).not.toMatch(/<style[\s>]/i);
  expect(html).not.toMatch(/<script(?![^>]*\bsrc=)[^>]*>\s*\S/i);
  const bootstrapPath = html.match(
    /src="(\/router-bootstrap\.js\?v=[0-9a-f]{16})"/,
  )?.[1];
  expect(bootstrapPath).toBeTruthy();

  const bootstrap = await request.get(bootstrapPath!);
  expect(bootstrap.ok()).toBeTruthy();
  expect(bootstrap.headers()["content-type"]).toContain("text/javascript");

  const staleAsset = await request.get("/router-web-stale.css");
  expect(staleAsset.status()).toBe(404);
  expect(await staleAsset.text()).not.toContain("<!doctype html>");

  const unknownApi = await request.get("/api/v1/not-a-route");
  expect(unknownApi.status()).toBe(404);
  expect(unknownApi.headers()["content-type"]).toContain("application/json");

  const panel = await request.get("/api/v1/panel");
  const csrf = ((await panel.json()) as { csrf_token: string }).csrf_token;
  const anonymousLanTun = await request.post("/api/v1/control/proxy/lan-tun", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { enabled: true },
  });
  expect(anonymousLanTun.status()).toBe(401);
  const anonymousTailscaleProxy = await request.post(
    "/api/v1/control/proxy/local-system",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { enabled: true },
    },
  );
  expect(anonymousTailscaleProxy.status()).toBe(401);
  const anonymousTailscalePeers = await request.get("/api/v1/tailscale/peers");
  expect(anonymousTailscalePeers.status()).toBe(401);
  const anonymousCameraStatus = await request.get("/api/v1/camera/status");
  expect(anonymousCameraStatus.status()).toBe(200);
  const anonymousCameraCreate = await request.post(
    "/api/v1/control/camera/session/create",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { offer_sdp: "v=0\r\n" },
    },
  );
  expect(anonymousCameraCreate.status()).toBe(401);
  const apps = await request.get("/api/v1/apps");
  expect(apps.status()).toBe(200);
  const appsBody = (await apps.json()) as {
    apps: Array<{
      name: string;
      sha256: string | null;
      deployed_at_unix_ms: number | null;
    }>;
  };
  expect(appsBody.apps.map((app) => app.name)).toEqual([
    "camera",
    "router",
    "things",
  ]);
  expect(appsBody.apps.every((app) => app.sha256 !== null)).toBeTruthy();
  expect(
    appsBody.apps.every(
      (app) =>
        typeof app.deployed_at_unix_ms === "number" &&
        app.deployed_at_unix_ms > 0,
    ),
  ).toBeTruthy();
  // 匿名 viewer 令牌只允许创建/关闭自己的会话，profile/rotation 仍是管理员专属。
  const viewerToken = await request.post("/api/v1/camera/viewer-token", {
    headers: { Origin: webOrigin, "Content-Type": "application/json" },
    data: {},
  });
  expect(viewerToken.status()).toBe(200);
  const { token: viewerTokenValue } = (await viewerToken.json()) as {
    token: string;
  };
  expect(viewerTokenValue).toMatch(/^[0-9a-f]{64}$/);
  const anonymousViewerCreate = await request.post(
    "/api/v1/control/camera/session/create",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": viewerTokenValue },
      data: { offer_sdp: "v=0\r\n" },
    },
  );
  expect(anonymousViewerCreate.status()).toBe(200);
  const createdSession = (await anonymousViewerCreate.json()) as {
    session_id: string;
  };
  const anonymousViewerClose = await request.post(
    "/api/v1/control/camera/session/close",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": viewerTokenValue },
      data: { session_id: createdSession.session_id },
    },
  );
  expect(anonymousViewerClose.status()).toBe(200);
  const anonymousViewerProfile = await request.post(
    "/api/v1/control/camera/profile",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": viewerTokenValue },
      data: { preset: "fhd1080p5m" },
    },
  );
  expect(anonymousViewerProfile.status()).toBe(403);
  const removedProxyMode = await request.post("/api/v1/control/proxy/mode", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { mode: "tun" },
  });
  expect(removedProxyMode.status()).toBe(405);
  const anonymousProxySelection = await request.post(
    "/api/v1/control/proxy/selection",
    {
      headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
      data: { group: "自动选择", proxy: "新加坡" },
    },
  );
  expect(anonymousProxySelection.status()).toBe(401);

  const foreignOrigin = await request.post("/api/v1/control/display", {
    headers: {
      Origin: "http://evil.example",
      "X-HYZ-CSRF": csrf,
    },
    data: { enabled: false },
  });
  expect(foreignOrigin.status()).toBe(403);

  const missingCsrf = await request.post("/api/v1/control/display", {
    headers: { Origin: webOrigin },
    data: { enabled: false },
  });
  expect(missingCsrf.status()).toBe(403);

  const displayBefore = (await readHarnessState(request)).panel.display.data;
  const oversized = await request.post("/api/v1/control/display", {
    headers: {
      Origin: webOrigin,
      "X-HYZ-CSRF": csrf,
      "Content-Type": "application/json",
    },
    data: JSON.stringify({
      enabled: true,
      brightness: 1,
      padding: "x".repeat(5_000),
    }),
  });
  expect(oversized.status()).toBe(413);
  expect((await readHarnessState(request)).panel.display.data).toEqual(
    displayBefore,
  );
});

test("plays the camera anonymously and controls it as an administrator", async ({
  page,
  request,
}) => {
  await installCameraWebRtcMock(page);
  await page.goto("/");
  // 总览不自动启动摄像头；直播能力在摄像头一级页面内。
  await expect(
    page.getByText("视频不会自动启动", { exact: false }),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "摄像头", exact: true }).click();
  const camera = page.getByRole("article", { name: "摄像头直播" });
  await expect(camera).toBeVisible();
  await expect(camera.getByText("可用", { exact: true })).toBeVisible();
  await expect(camera.getByText("已停止", { exact: true })).toBeVisible();
  await expect(
    camera.getByText(/3840 × 2160 · 30 fps · 20\.0 Mbps · h264/),
  ).toBeVisible();
  await expect(
    camera.getByText("LAN · 0 个会话", { exact: true }),
  ).toBeVisible();

  // 匿名观看：画面设置禁用，没有旋转按钮。
  const presetSelect = camera.getByRole("combobox", {
    name: "画面分辨率与码率",
  });
  await expect(presetSelect).toBeDisabled();

  const harnessSessionId = (createCount: number) =>
    `e2e-camera.${"a".repeat(46)}${createCount.toString(16).padStart(2, "0")}`;

  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        offer: state.camera.last_offer_sdp,
        pipeline: state.camera.status.pipeline,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: 1,
      offer: "v=0\r\no=router-e2e-camera 1 1 IN IP4 127.0.0.1\r\n",
      pipeline: "streaming",
      activeSessions: 1,
      sessions: [harnessSessionId(1)],
    });

  await expect
    .poll(async () =>
      page.evaluate(() => ({
        playbackState: (window as any).__hyzCameraMediaSession.playbackState,
        title: (window as any).__hyzCameraMediaSession.metadata?.title ?? null,
      })),
    )
    .toEqual({ playbackState: "playing", title: "摄像头直播" });

  // 视频尚未有可播放数据时，画中画按钮保留但置灰；首帧就绪后才允许点击。
  await page.evaluate(() => (window as any).__hyzCameraSetVideoReady(false));
  const pipButton = camera.getByRole("button", { name: "画面准备中" });
  await expect(pipButton).toBeVisible();
  await expect(pipButton).toBeDisabled();
  await page.evaluate(() => (window as any).__hyzCameraSetVideoReady(true));
  await expect(
    camera.getByRole("button", { name: "进入画中画" }),
  ).toBeEnabled();
  await camera.getByRole("button", { name: "进入画中画" }).click();
  await expect(
    camera.getByRole("button", { name: "退出画中画" }),
  ).toBeVisible();
  await camera.getByRole("button", { name: "退出画中画" }).click();
  await expect(
    camera.getByRole("button", { name: "进入画中画" }),
  ).toBeVisible();

  const readMediaSession = () =>
    page.evaluate(() => {
      const mediaSession = (window as any).__hyzCameraMediaSession;
      return {
        playbackState: mediaSession.playbackState,
        title: mediaSession.metadata?.title ?? null,
        artist: mediaSession.metadata?.artist ?? null,
        handlers: Object.fromEntries(
          ["play", "pause", "stop"].map((action) => [
            action,
            typeof mediaSession.handlers[action] === "function",
          ]),
        ),
      };
    });

  await expect.poll(readMediaSession).toEqual({
    playbackState: "playing",
    title: "摄像头直播",
    artist: "hyz things",
    handlers: { play: true, pause: true, stop: true },
  });
  await expect(camera.getByRole("button", { name: "暂停直播" })).toBeVisible();
  await camera.getByRole("button", { name: "暂停直播" }).click();
  await expect(camera.getByRole("button", { name: "继续直播" })).toBeVisible();
  await expect(camera.getByText("直播已暂停", { exact: true })).toBeVisible();
  await expect.poll(readMediaSession).toMatchObject({
    playbackState: "paused",
  });
  await camera.getByRole("button", { name: "继续直播" }).click();
  await expect(camera.getByRole("button", { name: "暂停直播" })).toBeVisible();
  await expect(camera.getByText("直播已继续", { exact: true })).toBeVisible();
  await expect.poll(readMediaSession).toMatchObject({
    playbackState: "playing",
  });

  // 锁屏媒体控制的标准播放/暂停操作复用页面直播控制。
  await page.evaluate(() =>
    (window as any).__hyzCameraMediaSession.handlers.pause?.(),
  );
  await expect(camera.getByText("直播已暂停", { exact: true })).toBeVisible();
  await page.evaluate(() =>
    (window as any).__hyzCameraMediaSession.handlers.play?.(),
  );
  await expect(camera.getByText("直播已继续", { exact: true })).toBeVisible();
  await expect.poll(readMediaSession).toMatchObject({
    playbackState: "playing",
  });
  expect(
    await camera.locator("video").evaluate((video) => {
      const stream = (video as HTMLVideoElement).srcObject;
      return (
        stream instanceof MediaStream && stream.getVideoTracks().length === 1
      );
    }),
  ).toBe(true);
  await expect(camera.getByRole("button", { name: "旋转画面" })).toHaveCount(0);

  // 登录后回到摄像头视图：画面设置可用，切换视图时匿名会话已停止。
  await goToAppPage(page, "网络");
  await page.getByRole("button", { name: "管理员登录" }).click();
  await page.getByLabel("密码").fill("admin");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await page.getByLabel("当前密码").fill("admin");
  await page.getByLabel("新密码", { exact: true }).fill("router-e2e-password");
  await page.getByLabel("确认新密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "修改密码" }).click();
  await expect(page.getByRole("button", { name: "上游 Wi-Fi (STA)" })).toBeVisible();

  await goToAppPage(page, "摄像头");
  await expect(camera.getByText("未播放", { exact: true })).toBeVisible();
  await expect(presetSelect).toBeEnabled();
  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();

  // 旋转是服务端媒体管线属性（videoflip）：断言桩端 rotation 与页面指标，而非 CSS class。
  const verifyRotation = async () => {
    const rotateButton = camera.getByRole("button", { name: "旋转画面" });
    await expect(rotateButton).toBeVisible();
    for (const degrees of [270, 180, 90, 0]) {
      await rotateButton.click();
      await expect
        .poll(
          async () =>
            (await readHarnessState(request)).camera.status.profile.rotation,
        )
        .toBe(`deg_${degrees}`);
      await expect(
        camera.getByText(`旋转 ${degrees}°`, { exact: false }),
      ).toBeVisible();
    }
  };
  await verifyRotation();

  // Switching the preset while playing stops the session and reopens with the new profile.
  const beforeSwitch = await readHarnessState(request);
  await presetSelect.selectOption("fhd1080p5m");
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        closeCount: state.camera.close_count,
        width: state.camera.status.profile.width,
        height: state.camera.status.profile.height,
        bitrate: state.camera.status.profile.bitrate_bps,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: beforeSwitch.camera.create_count + 1,
      closeCount: beforeSwitch.camera.close_count + 1,
      width: 1920,
      height: 1080,
      bitrate: 5_000_000,
      activeSessions: 1,
      sessions: [harnessSessionId(beforeSwitch.camera.create_count + 1)],
    });
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();
  await expect(
    camera.getByText(/1920 × 1080 · 30 fps · 5\.0 Mbps · h264/),
  ).toBeVisible();

  await camera.getByRole("button", { name: "停止直播" }).click();
  await expect(camera.getByText("未播放", { exact: true })).toBeVisible();
  await expect(
    camera.getByRole("status").filter({ hasText: "摄像头直播已停止" }),
  ).toBeVisible();
  await expect.poll(readMediaSession).toEqual({
    playbackState: "none",
    title: null,
    artist: null,
    handlers: { play: false, pause: false, stop: false },
  });
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        closeCount: state.camera.close_count,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      closeCount: beforeSwitch.camera.close_count + 2,
      activeSessions: 0,
      sessions: [],
    });

  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).__hyzCameraRtcFail());
  await expect(
    camera.getByRole("status").filter({ hasText: "摄像头 WebRTC 连接已中断" }),
  ).toBeVisible();
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);

  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 360, height: 800 });
  await expectNoHorizontalOverflow(page);
  await verifyRotation();

  // 离开摄像头视图（去网络设置注销）会立即停止会话。
  await goToAppPage(page, "网络");
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);
  await page.getByRole("button", { name: "退出登录" }).click();
  await expect(page.getByRole("button", { name: "管理员登录" })).toBeVisible();
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);
});

test("enables full-duplex intercom with a real microphone track and cleans up", async ({
  page,
  request,
}) => {
  await installCameraWebRtcMock(page);
  await page.goto("/");
  await page.getByRole("button", { name: "摄像头", exact: true }).click();
  const camera = page.getByRole("article", { name: "摄像头直播" });
  await camera.getByRole("button", { name: "播放直播" }).click();
  await expect(camera.getByText("直播中", { exact: true })).toBeVisible();

  // 播放时创建 video + audio 两个 transceiver（audio 为 sendrecv，对讲不重协商）。
  expect(
    await page.evaluate(() => (window as any).__hyzCameraTransceivers),
  ).toEqual(["video", "audio"]);
  // 对讲按钮只在播放且音频能力可用时出现，点击后切换麦克风状态。
  const talkButton = camera.getByRole("button", { name: "开启对讲" });
  await expect(talkButton).toBeVisible();

  // 点击开启对讲：真实 getUserMedia（fake device）提供麦克风轨道并挂载到 sender。
  await talkButton.click();
  await expect(camera.getByText("对讲已开启", { exact: false })).toBeVisible();
  await expect(camera.getByRole("button", { name: "关闭对讲" })).toBeVisible();
  await expect
    .poll(async () =>
      page.evaluate(() => (window as any).__hyzCameraReplaceTrackCalls),
    )
    .toEqual([{ kind: "audio", hasTrack: true }]);

  // 再次点击关闭对讲：摘下并停止麦克风轨道。
  await camera.getByRole("button", { name: "关闭对讲" }).click();
  await expect(camera.getByText("对讲已关闭", { exact: false })).toBeVisible();
  await expect(camera.getByRole("button", { name: "开启对讲" })).toBeVisible();
  await expect
    .poll(async () =>
      page.evaluate(() => (window as any).__hyzCameraReplaceTrackCalls),
    )
    .toEqual([
      { kind: "audio", hasTrack: true },
      { kind: "audio", hasTrack: false },
    ]);

  // 停止直播：会话关闭、管线与 session 归零。
  await camera.getByRole("button", { name: "停止直播" }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        pipeline: state.camera.status.pipeline,
        activeSessions: state.camera.status.active_sessions,
      };
    })
    .toEqual({ pipeline: "stopped", activeSessions: 0 });
});

test("lets two anonymous viewers watch the camera concurrently", async ({
  context,
  request,
}) => {
  const first = await context.newPage();
  await installCameraWebRtcMock(first);
  await first.goto("/");
  await first.getByRole("button", { name: "摄像头", exact: true }).click();

  const cameraA = first.getByRole("article", { name: "摄像头直播" });
  await cameraA.getByRole("button", { name: "播放直播" }).click();
  await expect(cameraA.getByText("直播中", { exact: true })).toBeVisible();

  // 两个匿名观看者各自持有独立 viewer 令牌，可并发观看同一摄像头。
  const second = await context.newPage();
  await installCameraWebRtcMock(second);
  await second.goto("/");
  await second.getByRole("button", { name: "摄像头", exact: true }).click();
  const cameraB = second.getByRole("article", { name: "摄像头直播" });
  await cameraB.getByRole("button", { name: "播放直播" }).click();
  await expect(cameraB.getByText("直播中", { exact: true })).toBeVisible();

  const harnessSessionId = (createCount: number) =>
    `e2e-camera.${"a".repeat(46)}${createCount.toString(16).padStart(2, "0")}`;
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: 2,
      activeSessions: 2,
      sessions: [harnessSessionId(1), harnessSessionId(2)],
    });
  await expect(first.getByText("直播中", { exact: true })).toBeVisible();
  await expect(second.getByText("直播中", { exact: true })).toBeVisible();

  // 关闭第一个页面：第二个页面的会话不受影响。
  await cameraA.getByRole("button", { name: "停止直播" }).click();
  await expect(cameraA.getByText("未播放", { exact: true })).toBeVisible();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({ activeSessions: 1, sessions: [harnessSessionId(2)] });
  await expect(cameraB.getByText("直播中", { exact: true })).toBeVisible();

  // 关闭第二个页面后全部清理。
  await cameraB.getByRole("button", { name: "停止直播" }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({ activeSessions: 0, sessions: [] });
});

test("controls all four proxy combinations with isolated failures on desktop and mobile", async ({
  page,
  request,
}) => {
  await loginAsAdmin(page);
  await goToAppPage(page, "代理");
  const lanTun = page.getByRole("switch", { name: "LAN 透明代理" });
  const localSystemProxy = page.getByRole("switch", {
    name: "本机系统代理",
  });

  const expectCombination = async (
    lanEnabled: boolean,
    localSystemProxyEnabled: boolean,
  ) => {
    await expect
      .poll(async () => {
        const state = await readHarnessState(request);
        return {
          lanDesired: state.proxy.data.lan_tun.desired,
          lanEffective: state.proxy.data.lan_tun.effective,
          localSystemProxyDesired: state.proxy.data.local_system_proxy.desired,
          localSystemProxyEffective:
            state.proxy.data.local_system_proxy.effective,
          core: state.proxy.data.mihomo.process,
        };
      })
      .toEqual({
        lanDesired: lanEnabled,
        lanEffective: lanEnabled ? "ready" : "ordinary_nat",
        localSystemProxyDesired: localSystemProxyEnabled,
        localSystemProxyEffective: localSystemProxyEnabled
          ? "ready"
          : "disabled",
        core: lanEnabled || localSystemProxyEnabled ? "ready" : "absent",
      });
    await expect(lanTun).toBeChecked({ checked: lanEnabled });
    await expect(localSystemProxy).toBeChecked({
      checked: localSystemProxyEnabled,
    });
  };

  await expectCombination(true, false);
  await localSystemProxy.click();
  await expectCombination(true, true);
  await lanTun.click();
  await expectCombination(false, true);
  await localSystemProxy.click();
  await expectCombination(false, false);
  await lanTun.click();
  await expectCombination(true, false);

  await page.setViewportSize({ width: 360, height: 800 });
  await localSystemProxy.click();
  await expectCombination(true, true);
  await lanTun.click();
  await expectCombination(false, true);
  await expectNoHorizontalOverflow(page);
  await expect(page.getByText("当前代理节点")).toBeVisible();
  await expect(
    page.getByRole("combobox", { name: "自动选择 节点" }),
  ).toBeEnabled();

  let state = await readHarnessState(request);
  state.proxy_failures.lan_tun = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await lanTun.click();
  await expect(
    page.getByRole("status").filter({ hasText: "操作失败" }),
  ).toBeVisible();
  await expectCombination(false, true);
  await expect(localSystemProxy).toBeChecked();

  state = await readHarnessState(request);
  state.proxy_failures.lan_tun = false;
  state.proxy_failures.local_system_proxy = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await localSystemProxy.click();
  await expect(
    page.getByRole("status").filter({ hasText: "操作失败" }),
  ).toHaveCount(2);
  await expectCombination(false, true);
  await expect(lanTun).not.toBeChecked();

  state = await readHarnessState(request);
  state.proxy_failures.local_system_proxy = false;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await page
    .getByRole("combobox", { name: "自动选择 节点" })
    .selectOption("新加坡");
  await expect
    .poll(
      async () =>
        (await readHarnessState(request)).panel.proxy_groups.data[0].selected,
    )
    .toBe("新加坡");
  await expect(localSystemProxy).toBeEnabled();
  await expectNoHorizontalOverflow(page);

  const csrf = (
    (await (await request.get("/api/v1/panel")).json()) as {
      csrf_token: string;
    }
  ).csrf_token;
  for (const path of [
    "/api/v1/control/proxy/lan-tun",
    "/api/v1/control/proxy/local-system",
  ]) {
    const forbidden = await page.evaluate(
      async ({ path, csrf }) =>
        (
          await fetch(path, {
            method: "POST",
            credentials: "same-origin",
            headers: { "Content-Type": "application/json", "X-HYZ-CSRF": csrf },
            body: JSON.stringify({
              enabled: true,
              proxy_url: "http://127.0.0.1:7890",
              port: 7890,
              environment: { HTTP_PROXY: "forbidden" },
              provider: "forbidden",
              Controller: "forbidden",
              config: "raw",
            }),
          })
        ).status,
      { path, csrf },
    );
    expect(forbidden).toBe(400);
  }
  const oversized = await page.evaluate(
    async (csrf) =>
      (
        await fetch("/api/v1/control/proxy/lan-tun", {
          method: "POST",
          credentials: "same-origin",
          headers: { "Content-Type": "application/json", "X-HYZ-CSRF": csrf },
          body: JSON.stringify({ enabled: true, padding: "x".repeat(5_000) }),
        })
      ).status,
    csrf,
  );
  expect(oversized).toBe(413);
});

test("shows local system proxy status from ProxyStatus without Tailscale coupling", async ({
  page,
  request,
}) => {
  await loginAsAdmin(page);
  await goToAppPage(page, "代理");
  const state = await readHarnessState(request);
  state.proxy.state = "degraded";
  state.proxy.issue = {
    code: "local_system_proxy_not_confirmed",
    message: "本机系统代理未确认",
  };
  state.proxy.data.lan_tun.desired = true;
  state.proxy.data.lan_tun.effective = "not_confirmed";
  state.proxy.data.local_system_proxy.desired = true;
  state.proxy.data.local_system_proxy.effective = "not_confirmed";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();

  const proxyCapabilities = page.getByLabel("代理能力");
  await expect(
    proxyCapabilities.getByText("已降级 · 未确认", { exact: true }),
  ).toHaveCount(2);
  await expect(
    proxyCapabilities.getByText(/代理路径|已恢复 Direct/),
  ).toHaveCount(0);

  const ready = await readHarnessState(request);
  ready.proxy.state = "available";
  ready.proxy.issue = null;
  ready.proxy.data.mihomo.configured_required = true;
  ready.proxy.data.mihomo.process = "ready";
  ready.proxy.data.mihomo.runtime_config = "ready";
  ready.proxy.data.mihomo.mixed_port = "ready";
  ready.proxy.data.local_system_proxy.desired = true;
  ready.proxy.data.local_system_proxy.effective = "ready";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: ready })).ok(),
  ).toBeTruthy();
  await expect(
    proxyCapabilities.getByText("已启用", { exact: true }),
  ).toBeVisible({ timeout: 7_500 });

  const unknown = await readHarnessState(request);
  unknown.proxy.data.mihomo.configured_required = null;
  unknown.proxy.data.mihomo.process = "unknown";
  unknown.proxy.data.lan_tun.desired = null;
  unknown.proxy.data.local_system_proxy.desired = null;
  unknown.proxy.data.local_system_proxy.effective = "not_confirmed";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: unknown })).ok(),
  ).toBeTruthy();
  await expect(
    page.getByText("Mihomo core：未知", { exact: true }),
  ).toBeVisible({ timeout: 7_500 });
  await expect(
    proxyCapabilities.getByText("未知 · 未确认", { exact: true }),
  ).toHaveCount(2);
  await expect(
    page.getByRole("switch", { name: "LAN 透明代理" }),
  ).toBeDisabled();
  await expect(
    page.getByRole("switch", { name: "本机系统代理" }),
  ).toBeDisabled();
});

test("supports the administrator, STA, AP, and write-only subscription journey", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await goToAppPage(page, "网络");
  await expect(
    page.getByRole("button", { name: "网络", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");

  await page.getByRole("button", { name: "管理员登录" }).click();
  await page.getByLabel("密码").fill("admin");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await expect(
    page.getByRole("alert").filter({ hasText: "必须先修改默认密码" }),
  ).toBeVisible();

  await page.getByLabel("当前密码").fill("admin");
  await page.getByLabel("新密码", { exact: true }).fill("router-e2e-password");
  await page.getByLabel("确认新密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "修改密码" }).click();
  await expect(
    page.getByRole("button", { name: "上游 Wi-Fi (STA)" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "上游 Wi-Fi (STA)" }),
  ).toBeVisible();
  await goToAppPage(page, "代理");
  await expect(page.getByRole("heading", { name: "代理设置" })).toBeVisible();
  await page
    .getByRole("combobox", { name: "自动选择 节点" })
    .selectOption("新加坡");
  await expect
    .poll(
      async () =>
        (await readHarnessState(request)).panel.proxy_groups.data[0].selected,
    )
    .toBe("新加坡");

  await goToAppPage(page, "网络");
  await page.getByRole("button", { name: "上游 Wi-Fi (STA)" }).click();
  const staRegion = page.getByRole("region", { name: "上游 Wi-Fi (STA)" });
  await page.getByRole("button", { name: "扫描", exact: true }).click();
  await page.getByRole("button", { name: "选择网络 Guest-Network" }).click();
  await staRegion.getByLabel("密码").fill("guest-password");

  await goToAppPage(page, "总览");
  await goToAppPage(page, "网络");
  await expect(staRegion.getByLabel("SSID")).toHaveValue("Guest-Network");
  await expect(staRegion.getByLabel("密码")).toHaveValue("guest-password");

  const staApply = page.getByRole("button", { name: "检查并应用 STA" });
  await staApply.click();
  let staConfirmation = page.getByRole("region", { name: "应用上游 Wi-Fi？" });
  await expect(staConfirmation).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const returnToSta = staConfirmation.getByRole("button", { name: "返回检查" });
  await expect(returnToSta).toBeFocused();
  await returnToSta.click();
  await expect(staApply).toBeFocused();

  await staRegion.getByLabel("密码").fill("guest-password");
  await page.evaluate(() => {
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = function () {
      document.documentElement.dataset.networkConfirmationScrolled = "true";
      original.call(this);
    };
  });
  await staApply.click();
  staConfirmation = page.getByRole("region", { name: "应用上游 Wi-Fi？" });
  await expect
    .poll(() =>
      staConfirmation.evaluate((element) => {
        const bounds = element.getBoundingClientRect();
        return bounds.top >= 0 && bounds.top < window.innerHeight;
      }),
    )
    .toBe(true);
  await expect(page.locator("html")).toHaveAttribute(
    "data-network-confirmation-scrolled",
    "true",
  );
  await staConfirmation.getByRole("button", { name: "确认并开始应用" }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "STA 配置已应用" }),
  ).toBeVisible();

  let state = await readHarnessState(request);
  expect(state.network.sta_ssid).toBe("Guest-Network");

  const apToggle = page.getByRole("button", { name: "下游 Wi-Fi (AP)" });
  await apToggle.click();
  const apRegion = page.getByRole("region", { name: "下游 Wi-Fi (AP)" });
  await apRegion.getByLabel("SSID").fill("HYZ-New-AP");
  await apRegion.getByLabel("密码").fill("new-ap-password");
  await expect(apRegion.getByLabel("国家 / 地区")).toHaveValue("中国 (CN)");
  await apRegion.getByRole("button", { name: "准备 AP 变更" }).click();
  await expect(apRegion.getByText("HYZ-New-AP", { exact: true })).toBeVisible();
  await apRegion.getByRole("button", { name: "检查风险并应用" }).click();
  const apConfirmation = page.getByRole("region", { name: "应用下游 AP？" });
  await expect(apConfirmation).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await apConfirmation.getByRole("button", { name: "确认并开始应用" }).click();
  await expect(apToggle).toBeFocused();
  await apToggle.click();
  await expect(apRegion.getByText(/等待确认/)).toBeVisible();
  await apRegion.getByRole("button", { name: "确认保留" }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "AP 配置已确认" }),
  ).toBeVisible();

  state = await readHarnessState(request);
  expect(state.network.ap_ssid).toBe("HYZ-New-AP");
  expect(state.pending_network).toBeNull();

  const devicePolicies = page.getByRole("article", { name: "设备代理" });
  await expect(devicePolicies.getByText("e2e-phone")).toBeVisible();
  const deviceName = devicePolicies.getByLabel("02:00:00:00:00:10 显示名");
  await deviceName.fill("我的 iPhone");
  await devicePolicies.getByRole("button", { name: "保存名称" }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "设备代理策略已保存" }),
  ).toBeVisible();
  expect((await readHarnessState(request)).device_policies).toMatchObject({
    config: {
      generation: 1,
      entries: [
        { mac: "02:00:00:00:00:10", label: "我的 iPhone", policy: "proxy" },
      ],
    },
  });

  const devicePolicy = devicePolicies.getByRole("combobox", {
    name: "02:00:00:00:00:10 代理策略",
  });
  await devicePolicy.selectOption("direct");
  await expect(
    page.getByRole("status").filter({ hasText: "设备代理策略已保存" }),
  ).toBeVisible();
  expect((await readHarnessState(request)).device_policies).toMatchObject({
    config: {
      generation: 2,
      entries: [
        { mac: "02:00:00:00:00:10", label: "我的 iPhone", policy: "direct" },
      ],
    },
  });
  const raced = await readHarnessState(request);
  raced.device_policies.config.generation = 3;
  const raceUpdate = await request.put(`${harnessOrigin}/state`, {
    data: raced,
  });
  expect(raceUpdate.ok()).toBeTruthy();
  await devicePolicy.selectOption("proxy");
  await expect(
    page
      .getByRole("status")
      .filter({ hasText: "设备策略更新未完成（HTTP 409）" }),
  ).toBeVisible();

  await page.reload();
  await goToAppPage(page, "网络");
  const reloadedPolicies = page.getByRole("article", { name: "设备代理" });
  await expect(reloadedPolicies.getByRole("combobox")).toHaveValue("direct");
  await expect(
    reloadedPolicies.getByLabel("02:00:00:00:00:10 显示名"),
  ).toHaveValue("我的 iPhone");
  await reloadedPolicies.getByRole("combobox").selectOption("proxy");
  await expect(
    page.getByRole("status").filter({ hasText: "设备代理策略已保存" }),
  ).toBeVisible();
  expect(
    (await readHarnessState(request)).device_policies.config.entries,
  ).toEqual([
    { mac: "02:00:00:00:00:10", label: "我的 iPhone", policy: "proxy" },
  ]);
  await reloadedPolicies
    .getByRole("button", { name: "清除 02:00:00:00:00:10 的名称和设备策略" })
    .click();
  await expect(
    page.getByRole("status").filter({ hasText: "设备代理策略已保存" }),
  ).toBeVisible();
  expect(
    (await readHarnessState(request)).device_policies.config.entries,
  ).toEqual([]);

  const offline = await readHarnessState(request);
  offline.device_policies.config.generation += 1;
  offline.device_policies.config.entries = [
    {
      mac: "02:00:00:00:00:20",
      label: "offline-tablet",
      policy: "proxy",
    },
  ];
  offline.device_policies.clients.push({
    mac: "02:00:00:00:00:20",
    lease_address: null,
    hostname: null,
    associated: false,
    policy: "proxy",
  });
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: offline })).ok(),
  ).toBeTruthy();
  await page.reload();
  await goToAppPage(page, "网络");
  await page
    .getByRole("button", { name: "清除 02:00:00:00:00:20 的名称和设备策略" })
    .click();
  await expect(
    page.getByRole("status").filter({ hasText: "设备代理策略已保存" }),
  ).toBeVisible();
  expect(
    (await readHarnessState(request)).device_policies.config.entries,
  ).toEqual([]);

  const subscription = page.getByRole("article", { name: "代理订阅" });
  const subscriptionInput = subscription.getByLabel("订阅 URL");
  await subscriptionInput.fill("https://example.com/router-e2e.yaml");
  await subscription.getByRole("button", { name: "保存并立即更新" }).click();
  await expect(subscriptionInput).toHaveValue("");
  await expect(subscription.getByText("已生效")).toBeVisible();
  await expect(
    page.getByText("https://example.com/router-e2e.yaml"),
  ).toHaveCount(0);

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
});

test("supports the administrator Tailscale login, approval, disable, and logout flow", async ({
  page,
  request,
}) => {
  const initial = await readHarnessState(request);
  initial.tailscale.data.authenticated = false;
  initial.tailscale.data.backend_state = "stopped";
  initial.tailscale.data.desired_mode = "disabled";
  initial.tailscale.data.effective_mode = "disabled";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: initial })).ok(),
  ).toBeTruthy();

  await page.goto("/");
  await expect(page.getByText("laptop", { exact: true })).toHaveCount(0);
  await expect(page.getByText("100.64.0.8", { exact: false })).toHaveCount(0);
  await goToAppPage(page, "网络");
  await page.getByRole("button", { name: "管理员登录" }).click();
  await page.getByLabel("密码").fill("admin");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await page.getByLabel("当前密码").fill("admin");
  await page.getByLabel("新密码", { exact: true }).fill("router-e2e-password");
  await page.getByLabel("确认新密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "修改密码" }).click();

  await goToAppPage(page, "Tailscale");
  const tailscale = page.getByRole("region", {
    name: "Tailscale 远程 LAN 状态",
  });
  await expect(tailscale.getByText(/Tailnet 设备列表暂不可用/)).toBeVisible();
  await tailscale.getByRole("button", { name: "启用远程 LAN 访问" }).click();
  const loginLink = tailscale.getByRole("link", {
    name: "打开一次性 Tailscale 登录链接",
  });
  await expect(loginLink).toHaveAttribute(
    "href",
    "https://login.tailscale.com/a/router-e2e",
  );
  await expect(loginLink).toHaveAttribute("target", "_blank");
  await expect(loginLink).toHaveAttribute("rel", "noopener noreferrer");
  await expect(tailscale.getByText("路由批准", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);

  const authenticated = await readHarnessState(request);
  authenticated.tailscale.data.authenticated = true;
  authenticated.tailscale.data.backend_state = "running";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: authenticated })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "已完成登录，继续启用" }).click();
  await expect(tailscale.getByText("本机远程 LAN 访问已启用")).toBeVisible();
  await expect(tailscale.getByText("Tailnet 设备 · 3 / 4 在线")).toBeVisible();
  await expect(
    tailscale.getByText("laptop", { exact: true }),
  ).not.toBeVisible();
  await expect(
    tailscale.getByText("hyz-router", { exact: true }),
  ).not.toBeVisible();
  await tailscale.getByText("查看设备列表", { exact: true }).click();
  await expect(
    tailscale.getByText("hyz-router", { exact: true }),
  ).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();
  await expect(
    tailscale.getByText("hyz-iphone", { exact: true }),
  ).toBeVisible();
  await expect(tailscale.getByText("tablet", { exact: true })).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.7 · linux/)).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.8 · linux/)).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.9 · android/)).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.10 · iOS/)).toBeVisible();
  await expect(
    tailscale.getByText(/active · direct 192\.168\.1\.3:41641/),
  ).toHaveCount(2);
  await expect(tailscale.getByText(/active · relay "sfo"/)).toBeVisible();
  await expect(tailscale.getByText(/tx 1\.2 KiB · rx 789 B/)).toBeVisible();
  await expect(tailscale.getByText(/最近看到/)).toBeVisible();
  await expect(tailscale.getByText("在线", { exact: true })).toHaveCount(3);
  await expect(tailscale.getByText("离线", { exact: true })).toHaveCount(1);
  await expect(
    tailscale.getByText(/不表示它正在访问本路由器的 LAN/),
  ).toBeVisible();
  await expect(tailscale.getByText("路由批准", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);
  await expect(tailscale.getByText(/当前 LAN Access/)).toBeVisible();
  await expect(tailscale.getByText("连接", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText("Direct", { exact: true })).toHaveCount(0);
  const enabledButton = tailscale.getByRole("button", {
    name: "远程 LAN 访问已启用",
  });
  await expect(enabledButton).toBeDisabled();
  await expect(enabledButton).toHaveAttribute("aria-pressed", "true");
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: true,
    desired_mode: "lan_subnet_access",
    effective_mode: "lan_subnet_access",
    route_advertised: true,
    local_firewall_ready: true,
  });
  await page.setViewportSize({ width: 360, height: 800 });
  await expect(enabledButton).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();
  await expect(tailscale.getByText("tablet", { exact: true })).toBeVisible();
  await expect(tailscale.getByText("路由批准", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
  const expandedAccessibility = await new AxeBuilder({ page }).analyze();
  expect(expandedAccessibility.violations).toEqual([]);

  await tailscale.getByRole("button", { name: "停用（保留认证）" }).click();
  const disabledButton = tailscale.getByRole("button", {
    name: "Tailscale 已停用",
  });
  await expect(disabledButton).toBeDisabled();
  await expect(disabledButton).toHaveAttribute("aria-pressed", "true");
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: true,
    desired_mode: "disabled",
    effective_mode: "disabled",
  });

  await tailscale.getByRole("button", { name: "注销并移除认证" }).click();
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: false,
    desired_mode: "disabled",
    effective_mode: "disabled",
  });
  await expect(tailscale.getByText("laptop", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText("100.64.0.8", { exact: false })).toHaveCount(
    0,
  );
});

test("shows Tailnet peer empty, initial-error, stale, and recovery states", async ({
  page,
  request,
}) => {
  const initial = await readHarnessState(request);
  initial.tailscale.data.authenticated = true;
  initial.tailscale.data.backend_state = "running";
  initial.tailscale.data.desired_mode = "router_only";
  initial.tailscale.data.effective_mode = "router_only";
  initial.tailscale_peers_failure = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: initial })).ok(),
  ).toBeTruthy();

  await loginAsAdmin(page);
  await goToAppPage(page, "Tailscale");
  const tailscale = page.getByRole("region", {
    name: "Tailscale 远程 LAN 状态",
  });
  await expect(tailscale.getByText(/Tailnet 设备列表暂不可用/)).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toHaveCount(0);

  let state = await readHarnessState(request);
  state.tailscale_peers_failure = false;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "重新读取设备" }).click();
  await expect(tailscale.getByText("Tailnet 设备 · 3 / 4 在线")).toBeVisible();
  await tailscale.getByText("查看设备列表", { exact: true }).click();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();

  state = await readHarnessState(request);
  state.tailscale_peers_failure = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "重新读取设备" }).click();
  await expect(tailscale.getByText(/数据可能已过期/)).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();

  state = await readHarnessState(request);
  state.tailscale_peers_failure = false;
  state.tailscale_peers = { total: 0, online: 0, peers: [] };
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "重新读取设备" }).click();
  await expect(tailscale.getByText("暂无 Tailnet 设备")).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
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
