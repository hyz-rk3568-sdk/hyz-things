import { expect, test } from "@playwright/test";
import { resetHarness, webOrigin } from "./fixtures";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("keeps the administrator cookie for thirty days", async ({ request }) => {
  const panel = await request.get("/api/v1/panel");
  expect(panel.ok()).toBeTruthy();
  const csrf = ((await panel.json()) as { csrf_token: string }).csrf_token;

  const login = await request.post("/api/v1/auth/login", {
    headers: {
      Origin: webOrigin,
      "X-HYZ-CSRF": csrf,
      "Content-Type": "application/json",
    },
    data: { password: "admin" },
  });
  expect(login.status()).toBe(200);

  const cookie = login.headers()["set-cookie"] ?? "";
  expect(cookie).toContain("HttpOnly");
  expect(cookie).toContain("SameSite=Strict");
  expect(cookie).toContain("Path=/");
  expect(cookie).toContain("Max-Age=2592000");
});
