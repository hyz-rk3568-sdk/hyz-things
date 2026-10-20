import { expect, test } from "@playwright/test";

test("study opens an immersive four-choice calculation drill with all three types", async ({ page }) => {
  await page.goto("/#/study");
  await page.getByTestId("study-open-calculation").click();
  await expect(page).toHaveURL(/#\/study\/calculation/);
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
  await page.getByTestId("calc-gap-small").click();
  await page.getByTestId("calculation-start").click();
  await expect(page.getByTestId("calculation-expression")).toContainText(/%/);
  await page.getByTestId("calculation-back").click();
  await expect(page).toHaveURL(/#\/study$/);
});
