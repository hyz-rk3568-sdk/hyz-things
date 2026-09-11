import { expect, test } from "@playwright/test";
import { goToAppPage, loginAsAdmin, resetHarness } from "./fixtures";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("shows the admin activity dashboard as a top-level page", async ({ page }) => {
  await loginAsAdmin(page);
  await goToAppPage(page, "活动");
  await expect(page.getByRole("heading", { name: "活动" })).toBeVisible();
  await expect(page.getByText("全部设备")).toBeVisible();
});
