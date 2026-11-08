import { expect, test } from "@playwright/test";
import { expectUnselectable, loginAsAdmin, resetHarness, webOrigin } from "./fixtures";
import { expectInlineInkControls, inkSnapshot } from "./ink-fixtures";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("shows Flashcards on Study alongside the exam countdown", async ({
  page,
}) => {
  await page.goto("/#/study");
  await expect(page.getByTestId("study-summary")).toBeVisible();
  await expect(page.getByTestId("study-open-cards")).toBeVisible();
  await expect(page.getByTestId("study-start-review")).toHaveCount(0);
  await expect(page.getByRole("region", { name: "自定义倒计时" })).toHaveCount(0);
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

test("legacy review route redirects into specialized training", async ({ page }) => {
  await page.goto("/#/study/review?deck=%E5%88%A4%E6%96%AD%E6%8E%A8%E7%90%86");
  await expect(page).toHaveURL(/#\/study\/training/);
  await expect(page.getByTestId("training-layout")).toBeVisible();
  await expect(page.getByTestId("training-mode-flashcards")).toBeVisible();
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
  await expect(page.getByRole("group",{name:"训练模式"})).toHaveCount(0);
  const drillFrame=await page.getByTestId("calculation-layout").boundingBox();
  expect(drillFrame).not.toBeNull();
  expect(drillFrame!.x).toBe(0);
  expect(drillFrame!.y).toBe(0);
  expect(drillFrame!.height).toBe(page.viewportSize()!.height);
  await expect(page.getByTestId("calculation-question-scroll")).toHaveCSS("overflow-y","auto");
  await expect(page.getByTestId("calculation-controls")).toBeVisible();
  expect(await page.evaluate(()=>window.scrollY)).toBe(0);
  await expect(page.getByTestId("calculation-answer")).toHaveCount(4);
  await expect(page.getByTestId("calculation-question")).not.toContainText("有效数字");
  await page.getByTestId("calculation-answer").first().click();
  await expect(page.getByTestId("calculation-feedback")).toBeVisible();
  await expect(page.getByTestId("calculation-feedback").locator("p").last()).toHaveCSS("font-size", "16px");
  await expect(page.getByTestId("calculation-answer").first()).toBeDisabled();
  await page.getByTestId("calculation-next").click();
  await expect(page.getByTestId("calculation-question")).toBeVisible();
  await page.getByTestId("calculation-finish").click();
  await expect(page.getByTestId("calculation-summary")).toContainText("正确率");
});

test("calculation drill supports percent-only and returns to training choice", async ({ page }) => {
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
  await expect(page).toHaveURL(/#\/study\/training$/);
  await expect(page.getByRole("group",{name:"训练模式"})).toBeVisible();
  await expect(page.getByTestId("calculation-setup")).toBeVisible();
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
  await expect(toggle).toHaveAttribute("aria-label", "关闭草稿");
  await expect(toggle).toHaveText("✎");
  await expect(scratchpad.getByTestId("calculation-scratchpad-toolbar")).toBeVisible();
  await expect(scratchpad.getByTestId("calculation-scratchpad-close")).toHaveCount(0);
  await expect(scratchpad.getByLabel("铅笔已选中")).toHaveCount(0);
  await expectInlineInkControls(page.getByTestId("calculation-problem").locator(":scope > div").first());
  await expect(canvas).toHaveCSS("touch-action", "none");
  await expect(canvas).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  // Responsive full-screen scratchpad scales its backing pixels to the
  // displayed card size (and devicePixelRatio), rather than fixed 900x450.
  const bitmap = await canvas.evaluate(el=>{
    const c=el as HTMLCanvasElement;
    return {width:c.width,height:c.height,displayWidth:c.clientWidth,displayHeight:c.clientHeight};
  });
  expect(bitmap.width).toBeGreaterThanOrEqual(bitmap.displayWidth);
  expect(bitmap.height).toBeGreaterThanOrEqual(bitmap.displayHeight);
  await expectUnselectable(page.getByTestId("calculation-layout"));
  await expectUnselectable(page.getByTestId("calculation-expression"));
  await expectUnselectable(page.getByTestId("calculation-answer").first());
  await expectUnselectable(scratchpad);
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

  const snapshot = () => inkSnapshot(canvas);
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
  expect(actualStrokeWidth).toBeCloseTo(2.6, 4);
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

  await toggle.click();
  await expect(canvas).toBeHidden();
  await expect(toggle).toHaveAttribute("aria-label", "开启草稿");
  await expect(scratchpad.getByTestId("calculation-scratchpad-toolbar")).toHaveCount(0);
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
    const makeTouch = (id: number) => ({
      identifier: id, target: canvas, clientX: 42 + id, clientY: 42,
    });
    const emit = (type: "touchstart" | "touchend", fingers: number, cancelable = true) => {
      const touches = Array.from({ length: fingers }, (_, index) => makeTouch(index));
      // WebKit exposes TouchEvent through createEvent while Touch is not constructible.
      const event = document.createEvent("TouchEvent");
      event.initEvent(type, true, cancelable);
      Object.defineProperties(event, {
        touches: { value: touches }, targetTouches: { value: touches },
        changedTouches: { value: [makeTouch(10)] },
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

test("flashcard review selects card directories as three nested levels instead of tags", async ({ page }) => {
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-flashcards").click();
  await expect(page.getByTestId("training-mode-flashcards")).toHaveText("闪卡回顾");
  await expect(page.getByTestId("practice-tag-filter")).toHaveCount(0);
  await expect(page.getByTestId("practice-deck-tree")).toBeVisible();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 3 张");

  const level1 = page.getByTestId("practice-deck-level1");
  await expect(level1).toHaveCount(2);
  const dataCard = page.getByTestId("practice-deck-group").filter({ hasText: "资料分析" });
  await dataCard.getByTestId("practice-deck-level1").click();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 1 张");
  await expect(dataCard.getByTestId("practice-deck-level2-empty")).toBeVisible();

  await page.getByTestId("practice-deck-all").click();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 3 张");
  await level1.filter({ hasText: "判断推理" }).click();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 2 张");
  const level2 = page.getByTestId("practice-deck-level2");
  await expect(level2).toHaveCount(1);
  await level2.filter({ hasText: "逻辑判断" }).click();
  await expect(level2).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("practice-candidates")).toContainText("共 2 张");
  await expect(page.getByTestId("practice-deck-leaf")).toBeVisible();

  await page.getByTestId("practice-deck-level1").filter({ hasText: "判断推理" }).click();
  await expect(page.getByTestId("practice-deck-level2")).toHaveCount(1);
  await expect(page.getByTestId("practice-deck-level2")).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByTestId("practice-deck-level3")).toHaveCount(0);
  await expect(page.getByTestId("practice-candidates")).toContainText("共 2 张");
});

test("flashcard review loads every card and records FSRS ratings", async ({ page }) => {
  const reviews: Array<{card_id: string; rating: string}> = [];
  page.on("request", (request) => {
    if (request.method() === "POST" && request.url().endsWith("/api/v1/study/review")) {
      reviews.push(request.postDataJSON());
    }
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-flashcards").click();
  await expect(page.getByTestId("practice-setup")).toBeVisible();
  await expect(page.getByTestId("practice-count")).toHaveCount(0);
  await expect(page.getByTestId("practice-candidates")).toContainText("共 3 张");
  await page.getByTestId("practice-deck-level1").filter({ hasText: "资料分析" }).click();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 1 张");
  await page.getByTestId("practice-deck-all").click();
  await page.getByTestId("practice-deck-level1").filter({ hasText: "判断推理" }).click();
  await page.getByTestId("practice-deck-level2").filter({ hasText: "逻辑判断" }).click();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 2 张");
  await expect(page.getByTestId("practice-start")).toContainText("开始复习");

  await page.getByTestId("practice-start").click();
  await expect(page.getByTestId("practice-question")).toBeVisible();
  await expect(page.getByRole("group",{name:"训练模式"})).toHaveCount(0);
  const practiceFrame=await page.getByTestId("practice-layout").boundingBox();
  expect(practiceFrame).not.toBeNull();
  expect(practiceFrame!.x).toBe(0);
  expect(practiceFrame!.y).toBe(0);
  expect(practiceFrame!.height).toBe(page.viewportSize()!.height);
  await expect(page.getByTestId("practice-card-scroll")).toHaveCSS("overflow-y","auto");
  await expect(page.getByTestId("practice-controls")).toBeVisible();
  expect(await page.evaluate(()=>window.scrollY)).toBe(0);
  await expect(page.getByTestId("practice-front")).toBeVisible();
  await expect(page.getByTestId("practice-back")).toHaveCount(0);
  const tabs=page.getByTestId("practice-question-tab");
  await expect(tabs).toHaveCount(2);
  await expect(tabs.nth(0)).toHaveAttribute("data-grade","unanswered");
  await page.getByTestId("practice-next").click();
  await expect(page.getByTestId("practice-question")).toContainText("第 2 / 2 题");
  await expect(tabs.nth(0)).toHaveAttribute("data-grade","unanswered");
  await expect(tabs.nth(1)).toHaveAttribute("data-grade","unanswered");
  await expect(page.getByTestId("practice-back")).toHaveCount(0);
  await page.getByTestId("practice-previous").click();
  await expect(page.getByTestId("practice-question")).toContainText("第 1 / 2 题");
  const canvas = page.getByTestId("calculation-scratchpad-canvas");
  await expect(canvas).toBeHidden();
  await page.getByTestId("calculation-scratchpad-toggle").click();
  await expect(canvas).toBeVisible();
  await expectInlineInkControls(page.getByTestId("practice-card-header"));
  const practiceBitmap=await canvas.evaluate(el=>{
    const c=el as HTMLCanvasElement;
    return {width:c.width,height:c.height,displayWidth:c.clientWidth,displayHeight:c.clientHeight};
  });
  expect(practiceBitmap.width).toBeGreaterThanOrEqual(practiceBitmap.displayWidth);
  expect(practiceBitmap.height).toBeGreaterThanOrEqual(practiceBitmap.displayHeight);
  await expect(canvas).toHaveCSS("touch-action", "none");
  const blank = await canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  const bounds = await page.getByTestId("practice-card-scroll").boundingBox();
  if (!bounds) throw new Error("missing practice content bounds");
  await page.mouse.move(bounds.x + 25, bounds.y + 30);
  await page.mouse.down();
  await page.mouse.move(bounds.x + 90, bounds.y + 80, { steps: 6 });
  await page.mouse.up();
  const drawn = await canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  expect(drawn).not.toEqual(blank);

  await page.getByTestId("practice-reveal").click();
  await expect(page.getByTestId("practice-back")).toBeVisible();
  await expect(page.getByTestId("practice-grading")).toBeVisible();
  await expect(page.getByTestId("practice-controls")).toBeVisible();
  expect(await page.getByTestId("practice-layout").evaluate(el=>el.getBoundingClientRect().top)).toBe(0);
  await page.getByTestId("practice-good").click();
  await expect.poll(() => reviews.map((value) => value.rating)).toEqual(["good"]);
  await expect(tabs.nth(0)).toHaveAttribute("data-grade","good");
  await expect(tabs.nth(1)).toHaveAttribute("data-grade","unanswered");
  await expect(page.getByTestId("practice-question")).toContainText("第 2 / 2 题");

  await page.keyboard.press(" ");
  await expect(page.getByTestId("practice-back")).toBeVisible();
  await page.keyboard.press("1");
  await expect.poll(() => reviews.map((value) => value.rating)).toEqual(["good", "again"]);
  await expect(page.getByTestId("practice-summary")).toBeVisible();
  await expect(page.getByTestId("practice-summary")).toContainText("已复习");
  await expect(page.getByTestId("practice-summary")).toContainText("不会");
  await expect(page.getByTestId("practice-summary")).toContainText("会了");
});

test("flashcard review swipes between cards and keeps each card's draft", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-flashcards").click();
  await page.getByTestId("practice-deck-level1").filter({ hasText: "判断推理" }).click();
  await page.getByTestId("practice-deck-level2").filter({ hasText: "逻辑判断" }).click();
  await page.getByTestId("practice-start").click();
  await expect(page.getByTestId("practice-question")).toContainText("第 1 / 2 题");
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);

  const canvas = page.getByTestId("calculation-scratchpad-canvas");
  await page.getByTestId("calculation-scratchpad-toggle").click();
  await expect(canvas).toBeVisible();
  const blank = await canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  const bounds = await page.getByTestId("practice-card-scroll").boundingBox();
  if (!bounds) throw new Error("missing practice content bounds");
  await page.mouse.move(bounds.x + 25, bounds.y + 30);
  await page.mouse.down();
  await page.mouse.move(bounds.x + 90, bounds.y + 80, { steps: 6 });
  await page.mouse.up();
  const drawn = await canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL());
  expect(drawn).not.toEqual(blank);

  const swipe = async (direction: "next" | "previous") => {
    await page.getByTestId("training-swipe-surface").evaluate((node, direction) => {
      const [start, end] = direction === "next" ? [500, 250] : [250, 500];
      const emit = (type: string, x: number) =>
        node.dispatchEvent(new PointerEvent(type, {
          bubbles: true,
          pointerType: "touch",
          pointerId: 17,
          isPrimary: true,
          clientX: x,
          clientY: 230,
        }));
      emit("pointerdown", start);
      emit("pointermove", end);
      emit("pointerup", end);
    }, direction);
  };
  await swipe("next");
  await expect(page.getByTestId("practice-question")).toContainText("第 2 / 2 题");
  await swipe("previous");
  await expect(page.getByTestId("practice-question")).toContainText("第 1 / 2 题");

  await page.getByTestId("calculation-scratchpad-toggle").click();
  await expect(canvas).toBeVisible();
  await expect.poll(() => canvas.evaluate((element) => (element as HTMLCanvasElement).toDataURL()))
    .toEqual(drawn);
});

test("flashcard navigation never rates unvisited cards", async ({ page }) => {
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-flashcards").click();
  await page.getByTestId("practice-deck-level1").filter({ hasText: "判断推理" }).click();
  await page.getByTestId("practice-deck-level2").filter({ hasText: "逻辑判断" }).click();
  await expect(page.getByTestId("practice-candidates")).toContainText("共 2 张");
  await page.getByTestId("practice-start").click();
  const tabs=page.getByTestId("practice-question-tab");
  await expect(tabs).toHaveCount(2);
  await tabs.nth(1).click();
  await expect(page.getByTestId("practice-question")).toContainText("第 2 / 2 题");
  await expect(tabs.nth(0)).toHaveAttribute("data-grade","unanswered");
  await expect(tabs.nth(1)).toHaveAttribute("data-grade","unanswered");
  await page.getByTestId("practice-reveal").click();
  await page.getByTestId("practice-good").click();
  // One rated, one unanswered: the session stays open on the ungraded card.
  await expect(page.getByTestId("practice-question")).toContainText("第 1 / 2 题");
  await expect(tabs.nth(1)).toHaveAttribute("data-grade","good");
  await expect(tabs.nth(0)).toHaveAttribute("data-grade","unanswered");
  await page.getByTestId("practice-finish").click();
  await expect(page.getByTestId("practice-summary")).toContainText("未评");
  await expect(page.getByTestId("practice-summary")).toContainText("1");
  await expect(page.getByTestId("practice-summary")).toContainText("会了");
});


test("syncs the question bank and loads one whole paper with shared material", async ({ page }) => {
  let synced = false;
  const id = "019a3d8c-7f23-7b6e-927d-3c951851d3a2";
  const material = "019a3d8d-4a21-739a-8b15-7c54e259b610";
  const summary = { id, title: "2001 示例真题卷", year: 2001, region: "国考", variant: "统考", role: "primary", questions: 2 };
  await page.route("**/api/v1/question-bank/papers", route => route.fulfill({ json: synced ? [summary] : [] }));
  const syncSummary = { papers: 1, questions: 2, materials: 1, generated_ids: 4 };
  await page.route("**/api/v1/question-bank/sync/status", route => route.fulfill({ json: {
    task_id: synced ? "test-sync-task" : null,
    stage: synced ? "completed" : "idle",
    running: false,
    summary: synced ? syncSummary : null,
    error_code: null,
  } }));
  await page.route("**/api/v1/control/question-bank/sync", async route => {
    expect(route.request().method()).toBe("POST");
    expect(route.request().postDataJSON()).toEqual({});
    expect(route.request().headers()["x-hyz-csrf"]).toBeTruthy();
    synced = true;
    await route.fulfill({ status: 202, json: {
      task_id: "test-sync-task", stage: "scanning", running: true,
      summary: null, error_code: null,
    } });
  });
  await page.route(`**/api/v1/question-bank/papers/${id}`, route => route.fulfill({ json: {
    ...summary,
    materials: [{ id: material, name: "M1", markdown: "完整共享材料，只显示一次。" }],
    questions: [26, 27].map(number => ({
      id: `019a3d8e-1b32-7a60-9c83-f34217a6e2${number}`,
      number, stem: `原始题干${number}`, options_markdown: "- A. 甲\n- B. 乙\n- C. 丙\n- D. 丁",
      materials: [material], module: "资料分析", category: null, topics: [],
    })),
  } }));
  await loginAsAdmin(page);
  await page.goto("/#/study");
  await page.getByTestId("question-bank-sync").click();
  await expect(page.getByTestId("question-bank-notice")).toContainText("已同步 1 份文件");
  await page.getByTestId("question-bank-open").click();
  await expect(page.getByTestId("question-bank-paper")).toContainText("2001 示例真题卷");
  await expect(page.getByTestId("question-bank-question")).toHaveCount(2);
  await expect(page.getByTestId("question-bank-material")).toHaveCount(1);
  await expect(page.getByTestId("question-bank-paper")).toContainText("原卷第 26 题");
  await expect(page.getByTestId("question-bank-paper")).toContainText("原卷第 27 题");
});
