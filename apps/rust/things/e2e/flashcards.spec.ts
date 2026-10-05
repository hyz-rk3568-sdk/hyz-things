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

  await page.getByRole("button", { name: /高频 \(2\)/ }).click();
  await expect(cards).toHaveCount(1);
  await expect(cards).toContainText("因果倒置是什么？");
  await expect(page).toHaveURL(/#\/study\/cards\?deck=.*tag=/);
});

test("opens an immersive review route, reveals the answer, and records Good", async ({ page }) => {
  await page.goto("/#/study/review?deck=%E5%88%A4%E6%96%AD%E6%8E%A8%E7%90%86%2F%E9%80%BB%E8%BE%91%E5%88%A4%E6%96%AD&tag=%E9%AB%98%E9%A2%91");
  await expect(page.getByTestId("study-review-layout")).toBeVisible();
  await expect(page.locator("nav[aria-label='主导航']")).toHaveCount(0);
  await expect(page.getByTestId("study-review-card")).toBeVisible();

  await page.getByTestId("study-show-answer").click();
  await expect(page.getByTestId("study-review-answer")).toBeVisible();
  await page.getByTestId("study-rate-good").click();
  await expect(page.getByTestId("study-review-complete")).toBeVisible();
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
