import { installDocumentPipWindowFallback } from "./pip-fixtures";
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import {
  expectNoHorizontalOverflow,
  goToAppPage,
  harnessOrigin,
  installAudioSessionMock,
  installCameraWebRtcMock,
  loginAsAdmin,
  readHarnessState,
  resetHarness,
  webOrigin,
} from "./fixtures";
import {
  readCountdownTotal,
  realTouchSwipe,
  swipePortal,
} from "./support";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

async function openStudyPage(page: Page) {
  await page.goto("/");
  await goToAppPage(page, "学习");
  // Legacy PiP cases exercise the full countdown cards inside the new
  // secondary details view. The closest exam stays highlighted above it.
  await page.getByTestId("study-exam-expanded").click();
}

test("renders the upcoming exam countdown in chronological order", async ({
  page,
}) => {
  await openStudyPage(page);

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
  await expect(page.getByTestId("study-exam-hero")).toContainText("下一次国考");
  await expect(page.getByTestId("study-other-exams")).toContainText("广东省考");
  await expect(countdown.getByText("广东省考", { exact: true }).first()).toBeVisible();
  await expect(countdown.getByText("湖南省考", { exact: true }).first()).toBeVisible();
  await expect(
    countdown.getByText("湖南事业编", { exact: true }).first(),
  ).toBeVisible();
  await expect(countdown.locator("[data-exam-id] progress")).toHaveCount(4);
  await expect(countdown.locator("[data-exam-id] progress").first()).toHaveAttribute(
    "aria-label",
    /冲刺进度/,
  );
  await expectNoHorizontalOverflow(page);

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
});

test("legacy morning afternoon evening countdown panel is removed", async ({ page }) => {
  await openStudyPage(page);
  await expect(page.getByRole("region",{name:"自定义倒计时"})).toHaveCount(0);
  await expect(page.getByTestId("study-exam-hero")).toBeVisible();
});

test("keeps exam countdown cards dark and low-contrast", async ({ page }) => {
  await openStudyPage(page);

  const countdown = page.getByRole("region", { name: "考试冲刺倒计时" });
  const cards = countdown.locator("[data-exam-id]");
  await expect(cards.first()).toHaveClass(/bg-base-300\/90/);
  await expect(cards.first().locator('[aria-live="polite"]')).toHaveClass(
    /text-base-content/,
  );
});

test("opens an exam countdown in a Document PiP window with a double click", async ({
  page,
  context,
}) => {
  await installDocumentPipWindowFallback(page);
  await openStudyPage(page);

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

test("keeps the countdown ticking inside the Document PiP window", async ({
  page,
  context,
}) => {
  await installDocumentPipWindowFallback(page);
  await openStudyPage(page);
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

test("closes the Document PiP window with Escape and reopens it", async ({
  page,
  context,
}) => {
  await installDocumentPipWindowFallback(page);
  await openStudyPage(page);
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

test("opens a countdown Document PiP window on mobile", async ({
  page,
  context,
}) => {
  await installDocumentPipWindowFallback(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await openStudyPage(page);

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
  await installAudioSessionMock(page);
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
  await openStudyPage(page);
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
  const preparedVideo = page.locator("video[data-countdown-video]").first();
  await expect(preparedVideo).toHaveJSProperty("muted", true);
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
  const activePipVideo = page.locator("video[data-countdown-pip-active]");
  await expect(activePipVideo).toHaveCount(1);
  await expect(activePipVideo).toHaveJSProperty("muted", true);
  expect(
    await page.evaluate(() => (window as any).__hyzAudioSession.type),
  ).toBe("ambient");
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
  await openStudyPage(page);

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
  await openStudyPage(page);

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
  await openStudyPage(page);

  const countdown = page.getByRole("region", { name: "考试冲刺倒计时" });
  await expect(countdown).toBeVisible();
  await expect(countdown.locator("[data-exam-id]")).toHaveCount(4);
  await expect(
    countdown.getByText("湖南事业编", { exact: true }).first(),
  ).toBeVisible();
  await expectNoHorizontalOverflow(page);

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
});
