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

test("keeps exam countdown cards dark and low-contrast", async ({ page }) => {
  await page.goto("/");

  const countdown = page.getByRole("region", { name: "考试冲刺倒计时" });
  const cards = countdown.locator("[data-exam-id]");
  await expect(cards.first()).toHaveClass(/bg-base-300\/90/);
  await expect(cards.first().locator('[aria-live="polite"]')).toHaveClass(
    /text-base-content/,
  );
});

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
