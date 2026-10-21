import { expect, test } from "@playwright/test";
import { resetHarness, webOrigin } from "./fixtures";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("shows Flashcards on Study while preserving the existing countdown panels", async ({
  page,
}) => {
  await page.goto("/#/study");
  await expect(page.getByTestId("study-summary")).toBeVisible();
  await expect(page.getByTestId("study-open-cards")).toBeVisible();
  await expect(page.getByTestId("study-start-review")).toBeVisible();
  await expect(page.getByRole("region", { name: "自定义倒计时" })).toBeVisible();
  await expect(page.getByRole("region", { name: "考试冲刺倒计时" })).toBeVisible();
});

test("filters the card library by Deck, Tag, and their combination", async ({ page }) => {
  await page.goto("/#/study/cards");
  const cards = page.getByTestId("study-card-item");
  await expect(cards).toHaveCount(3);

  await page.getByTestId("study-deck-filter").selectOption("判断推理/逻辑判断");
  await expect(cards).toHaveCount(2);

  await page.getByTestId("study-tag-toggle").click();
  await expect(page.getByTestId("study-tag-panel")).toBeVisible();
  await page.getByTestId("study-tag-filter").getByRole("button", { name: /高频 \(2\)/ }).click();
  await expect(cards).toHaveCount(1);
  await expect(cards).toContainText("因果倒置是什么？");
  await expect(page).toHaveURL(/#\/study\/cards\?deck=.*tag=/);
});

test("opens an immersive review route, reveals the answer, and records Good", async ({ page }) => {
  await page.goto("/#/study/review?deck=%E5%88%A4%E6%96%AD%E6%8E%A8%E7%90%86%2F%E9%80%BB%E8%BE%91%E5%88%A4%E6%96%AD&tag=%E9%AB%98%E9%A2%91");
  await expect(page.getByTestId("study-review-layout")).toBeVisible();
  await expect(page.locator("nav[aria-label='主导航']")).toHaveCount(0);
  await expect(page.getByTestId("study-review-card")).toHaveAttribute("data-answer-shown", "false");
  await expect(page.getByTestId("study-review-front")).toBeVisible();

  await page.getByTestId("study-show-answer").click();
  await expect(page.getByTestId("study-review-card")).toHaveAttribute("data-answer-shown", "true");
  await expect(page.getByTestId("study-review-front")).toBeVisible();
  await expect(page.getByTestId("study-review-answer")).toBeVisible();
  await page.getByTestId("study-rate-good").click();
  await expect(page.getByTestId("study-review-complete")).toBeVisible();
});

test("lets review choose its own Deck and Tag scope", async ({ page }) => {
  await page.goto("/#/study/review");
  await expect(page.getByTestId("study-review-scope")).toBeVisible();
  await expect(page.getByTestId("study-review-deck-filter")).toBeVisible();

  await page.getByTestId("study-review-deck-filter").selectOption("资料分析");
  await expect(page).toHaveURL(/#\/study\/review\?deck=/);
  const reviewCard = page.getByTestId("study-review-card");
  await expect(reviewCard).toContainText("资料分析");

  const cardBeforeTagPanel = await reviewCard.boundingBox();
  await page.getByTestId("study-review-tag-toggle").click();
  await expect(page.getByTestId("study-review-tag-toggle")).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByTestId("study-review-tag-panel")).toBeVisible();
  const cardAfterTagPanel = await reviewCard.boundingBox();
  expect(cardBeforeTagPanel).not.toBeNull();
  expect(cardAfterTagPanel).not.toBeNull();
  expect(Math.abs(cardAfterTagPanel!.y - cardBeforeTagPanel!.y)).toBeLessThan(1);

  await page
    .getByTestId("study-review-tag-filter")
    .getByRole("button", { name: /高频 \(2\)/ })
    .click();
  await expect(page).toHaveURL(/#\/study\/review\?deck=.*tag=/);
  await expect(page.getByTestId("study-review-card")).toContainText("基期量公式？");
});
test("protects sync while keeping card reads and invalid anonymous review public", async ({
  request,
}) => {
  const panel = await request.get("/api/v1/panel");
  expect(panel.ok()).toBeTruthy();
  const csrf = ((await panel.json()) as { csrf_token: string }).csrf_token;

  for (const endpoint of [
    "/api/v1/study/cards",
    "/api/v1/study/decks",
    "/api/v1/study/tags",
    "/api/v1/study/review",
    "/api/v1/study/summary",
  ]) {
    const response = await request.get(endpoint);
    expect(response.status(), endpoint).toBe(200);
  }

  const anonymousReview = await request.post("/api/v1/study/review", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: { card_id: "not-a-uuid", rating: "good" },
  });
  expect(anonymousReview.status()).toBe(400);

  const anonymousSync = await request.post("/api/v1/control/study/sync", {
    headers: { Origin: webOrigin, "X-HYZ-CSRF": csrf },
    data: {},
  });
  expect(anonymousSync.status()).toBe(401);
});

test("shows a successful administrator sync message", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "网络", exact: true }).click();
  await page.getByLabel("密码").fill("admin");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await page.getByLabel("当前密码").fill("admin");
  await page.getByLabel("新密码", { exact: true }).fill("router-e2e-password");
  await page.getByLabel("确认新密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "修改密码" }).click();
  await expect(page.getByRole("button", { name: "上游 Wi-Fi (STA)" })).toBeVisible();

  await page.goto("/#/study");
  await page.getByTestId("study-sync").click();
  await expect(page.getByTestId("study-notice")).toContainText("同步完成");
});


test("study opens an immersive four-choice calculation drill with all three types", async ({ page }) => {
  await page.goto("/#/study");
  await page.getByTestId("study-open-calculation").click();
  await expect(page).toHaveURL(/#\/study\/training/);
  await expect(page.locator("nav[aria-label='主导航']")).toHaveCount(0);
  await expect(page.getByTestId("calculation-setup")).toBeVisible();
  await expect(page.getByTestId("calc-kind-div")).toBeChecked();
  await expect(page.getByTestId("calc-kind-mul")).toBeChecked();
  await expect(page.getByTestId("calc-kind-percent")).toBeChecked();

  await page.getByTestId("calculation-start").click();
  await expect(page.getByTestId("calculation-question")).toBeVisible();
  await expect(page.getByTestId("calculation-answer")).toHaveCount(4);
  await expect(page.getByTestId("calculation-question")).not.toContainText("有效数字");
  await page.getByTestId("calculation-answer").first().click();
  await expect(page.getByTestId("calculation-feedback")).toBeVisible();
  await expect(page.getByTestId("calculation-answer").first()).toBeDisabled();
  await page.getByTestId("calculation-next").click();
  await expect(page.getByTestId("calculation-question")).toBeVisible();
  await page.getByTestId("calculation-finish").click();
  await expect(page.getByTestId("calculation-summary")).toContainText("正确率");
});

test("calculation drill supports percent-only and returns to Study", async ({ page }) => {
  await page.goto("/#/study/calculation");
  await page.getByTestId("calc-kind-div").uncheck();
  await page.getByTestId("calc-kind-mul").uncheck();
  await expect(page.getByTestId("calc-gap-wide")).toHaveCount(0);
  await expect(page.getByTestId("calc-gap-small")).toHaveCount(0);
  await expect(page.getByTestId("calc-gap-mixed")).toHaveCount(0);
  await expect(page.getByText("选项间距")).toHaveCount(0);
  await page.getByTestId("calculation-start").click();
  await expect(page.getByTestId("calculation-expression")).toContainText(/%/);
  await page.getByTestId("calculation-back").click();
  await expect(page).toHaveURL(/#\/study$/);
});


test("transparent calculation ink overlays the question, supports history and resets on next question", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  await page.goto("/#/study/calculation");
  await page.getByTestId("calculation-start").click();
  const scratchpad = page.getByTestId("calculation-scratchpad");
  const canvas = page.getByTestId("calculation-scratchpad-canvas");
  const toggle = page.getByTestId("calculation-scratchpad-toggle");
  await expect(canvas).toBeHidden();
  await expect(toggle).toHaveAttribute("aria-label", "开启草稿");
  await toggle.click();
  await expect(canvas).toBeVisible();
  await expect(canvas).toHaveCSS("touch-action", "none");
  await expect(canvas).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await expect(canvas).toHaveAttribute("width", "900");
  await expect(canvas).toHaveAttribute("height", "450");
  await expect(page.getByTestId("calculation-layout")).toHaveCSS("user-select", "none");
  await expect(page.getByTestId("calculation-expression")).toHaveCSS("user-select", "none");
  await expect(page.getByTestId("calculation-answer").first()).toHaveCSS("user-select", "none");
  await expect(scratchpad).toHaveCSS("user-select", "none");
  const contextMenuAllowed = await canvas.evaluate((element) =>
    element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })),
  );
  expect(contextMenuAllowed).toBe(false);

  const problemBox = await page.getByTestId("calculation-problem").boundingBox();
  const scratchBox = await scratchpad.boundingBox();
  const box = await canvas.boundingBox();
  expect(problemBox).not.toBeNull();
  expect(scratchBox).not.toBeNull();
  expect(box).not.toBeNull();
  expect(scratchBox!.x).toBeLessThanOrEqual(problemBox!.x);
  expect(scratchBox!.x + scratchBox!.width).toBeGreaterThanOrEqual(problemBox!.x + problemBox!.width);
  expect(box!.x).toBeCloseTo(scratchBox!.x, 0);
  expect(box!.y).toBeCloseTo(scratchBox!.y, 0);

  const snapshot = () => canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  const blank = await snapshot();
  const draw = async (x1: number, y1: number, x2: number, y2: number) => {
    await page.mouse.move(box!.x + x1, box!.y + y1);
    await page.mouse.down();
    await page.mouse.move(box!.x + x2, box!.y + y2, { steps: 5 });
    await page.mouse.up();
  };
  await draw(30, 75, 140, 115);
  // 下笔后 Canvas 笔画宽度应比旧的 3.8 更细，且撤销重绘不改变笔迹。
  const actualStrokeWidth = await canvas.evaluate((el) =>
    (el as HTMLCanvasElement).getContext("2d")?.lineWidth,
  );
  expect(actualStrokeWidth).toBe(2.6);
  const first = await snapshot();
  expect(first).not.toEqual(blank);
  await page.getByTestId("calculation-scratchpad-undo").click();
  expect(await snapshot()).toEqual(blank);
  await page.getByTestId("calculation-scratchpad-redo").click();
  expect(await snapshot()).toEqual(first);
  await page.getByTestId("calculation-scratchpad-clear").click();
  expect(await snapshot()).toEqual(blank);
  await page.getByTestId("calculation-scratchpad-undo").click();
  expect(await snapshot()).toEqual(first);
  await page.getByTestId("calculation-scratchpad-redo").click();
  expect(await snapshot()).toEqual(blank);

  await page.getByTestId("calculation-scratchpad-close").click();
  await expect(canvas).toBeHidden();
  await page.getByTestId("calculation-answer").first().click();
  await expect(page.getByTestId("calculation-feedback")).toBeVisible();
  await page.getByTestId("calculation-next").click();
  await toggle.click();
  await expect(canvas).toBeVisible();
  expect(await snapshot()).toEqual(blank);
  await draw(25, 95, 95, 125);

  // iPadOS 遗漏前一笔 pointerup 时，下一个 pen pointerdown 仍可接管。
  const pencilStrokes = await canvas.evaluate((element) => {
    const surface = element as HTMLCanvasElement;
    const rect = surface.getBoundingClientRect();
    const emit = (type: string, pointerId: number, x: number, y: number) =>
      surface.dispatchEvent(new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        pointerType: "pen",
        pointerId,
        isPrimary: true,
        button: 0,
        buttons: type === "pointerup" ? 0 : 1,
        clientX: rect.x + x,
        clientY: rect.y + y,
      }));
    emit("pointerdown", 101, 20, 40);
    emit("pointermove", 101, 70, 80);
    const first = surface.toDataURL();
    emit("pointerdown", 102, 120, 40);
    emit("pointermove", 102, 175, 80);
    emit("pointerup", 102, 175, 80);
    const second = surface.toDataURL();
    emit("pointerdown", 103, 200, 40);
    emit("pointermove", 103, 250, 80);
    emit("pointerup", 103, 250, 80);
    return { first, second, third: surface.toDataURL() };
  });
  expect(pencilStrokes.second).not.toEqual(pencilStrokes.first);
  expect(pencilStrokes.third).not.toEqual(pencilStrokes.second);
});


test("scratchpad cancels Safari single-touch gestures before the second Pencil stroke", async ({ page }) => {
  await page.goto("/#/study/calculation");
  await page.getByTestId("calculation-start").click();
  await page.getByTestId("calculation-scratchpad-toggle").click();
  const prevention = await page.getByTestId("calculation-scratchpad-canvas").evaluate((element) => {
    const canvas = element as HTMLCanvasElement;
    const makeTouch = (id: number) => new Touch({
      identifier: id, target: canvas, clientX: 42 + id, clientY: 42,
    });
    const emit = (type: "touchstart" | "touchend", fingers: number, cancelable = true) => {
      const touches = Array.from({ length: fingers }, (_, index) => makeTouch(index));
      const event = new TouchEvent(type, {
        bubbles: true, cancelable, touches, targetTouches: touches,
        changedTouches: [makeTouch(10)],
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };
    return {
      start: emit("touchstart", 1),
      rapidStart: emit("touchstart", 1),
      end: emit("touchend", 0),
      twoFinger: emit("touchstart", 2),
      noncancelable: emit("touchstart", 1, false),
    };
  });
  expect(prevention).toEqual({
    start: true,
    rapidStart: true,
    end: true,
    twoFinger: false,
    noncancelable: false,
  });
});


test("legacy calculation URL redirects to specialized training without losing the speed drill", async ({ page }) => {
  await page.goto("/#/study/calculation");
  await expect(page).toHaveURL(/#\/study\/training$/);
  await expect(page.getByTestId("training-mode-calculation")).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("calculation-setup")).toBeVisible();
});

test("wrong-card practice filters, reveals, self-grades and never writes FSRS", async ({ page }) => {
  const reviewWrites: string[] = [];
  page.on("request", (request) => {
    if (request.method() === "POST" && request.url().endsWith("/api/v1/study/review")) {
      reviewWrites.push(request.url());
    }
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-flashcards").click();
  await expect(page.getByTestId("practice-setup")).toBeVisible();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 2 张");
  await page.getByTestId("practice-deck-filter").selectOption("资料分析");
  await expect(page.getByTestId("practice-empty")).toBeVisible();
  await page.getByTestId("practice-deck-filter").selectOption("判断推理/逻辑判断");
  await expect(page.getByTestId("practice-candidates")).toContainText("共 2 张");
  await expect(page.getByTestId("practice-candidates")).toContainText("候选数量不足");

  await page.getByTestId("practice-start").click();
  await expect(page.getByTestId("practice-question")).toBeVisible();
  await expect(page.getByTestId("practice-front")).toBeVisible();
  await expect(page.getByTestId("practice-back")).toHaveCount(0);
  const canvas = page.getByTestId("calculation-scratchpad-canvas");
  await expect(canvas).toBeHidden();
  await page.getByTestId("calculation-scratchpad-toggle").click();
  await expect(canvas).toBeVisible();
  await expect(canvas).toHaveAttribute("width", "900");
  await expect(canvas).toHaveCSS("touch-action", "none");
  const blank = await canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  const bounds = await canvas.boundingBox();
  if (!bounds) throw new Error("missing canvas bounds");
  await page.mouse.move(bounds.x + 25, bounds.y + 30);
  await page.mouse.down();
  await page.mouse.move(bounds.x + 90, bounds.y + 80, { steps: 6 });
  await page.mouse.up();
  const drawn = await canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  expect(drawn).not.toEqual(blank);

  await page.getByTestId("practice-reveal").click();
  await expect(page.getByTestId("practice-back")).toBeVisible();
  await page.getByTestId("practice-wrong").click();
  await expect(page.getByTestId("practice-question")).toBeVisible();
  await expect(canvas).toBeHidden();
  await page.getByTestId("calculation-scratchpad-toggle").click();
  const reset = await canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  expect(reset).toEqual(blank);
  await page.getByTestId("practice-reveal").click();
  await page.getByTestId("practice-skip").click();
  await expect(page.getByTestId("practice-summary")).toBeVisible();
  await expect(page.getByTestId("practice-summary")).toContainText("做错");
  await expect(page.getByTestId("practice-summary")).toContainText("跳过");
  expect(reviewWrites).toEqual([]);
});
