import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import {
  goToAppPage,
  harnessOrigin,
  loginAsAdmin,
  readHarnessState,
  resetHarness,
} from "./fixtures";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("supports administrator STA/AP journeys while keeping proxy and device settings off Network", async ({ page, request }) => {
  await page.goto("/");
  await goToAppPage(page, "网络");
  await page.getByLabel("密码").fill("admin");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: "必须先修改默认密码" })).toBeVisible();
  await page.getByLabel("当前密码").fill("admin");
  await page.getByLabel("新密码", { exact: true }).fill("router-e2e-password");
  await page.getByLabel("确认新密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "修改密码" }).click();
  await expect(page.getByRole("button", { name: "上游 Wi-Fi (STA)" })).toBeVisible();

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
  await expect.poll(() => staConfirmation.evaluate(element => {
    const bounds = element.getBoundingClientRect();
    return bounds.top >= 0 && bounds.top < window.innerHeight;
  })).toBe(true);
  await expect(page.locator("html")).toHaveAttribute("data-network-confirmation-scrolled", "true");
  await staConfirmation.getByRole("button", { name: "确认并开始应用" }).click();
  await expect(page.getByRole("status").filter({ hasText: "STA 配置已应用" })).toBeVisible();
  expect((await readHarnessState(request)).network.sta_ssid).toBe("Guest-Network");

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
  await apConfirmation.getByRole("button", { name: "确认并开始应用" }).click();
  await expect(apToggle).toBeFocused();
  await apToggle.click();
  await expect(apRegion.getByText(/等待确认/)).toBeVisible();
  await apRegion.getByRole("button", { name: "确认保留" }).click();
  await expect(page.getByRole("status").filter({ hasText: "AP 配置已确认" })).toBeVisible();

  const state = await readHarnessState(request);
  expect(state.network.ap_ssid).toBe("HYZ-New-AP");
  expect(state.pending_network).toBeNull();
  await expect(page.getByRole("article", { name: "代理订阅" })).toHaveCount(0);
  await expect(page.getByRole("article", { name: "设备代理" })).toHaveCount(0);

  await goToAppPage(page, "代理");
  const subscription = page.getByRole("article", { name: "代理订阅" });
  const subscriptionInput = subscription.getByLabel("订阅 URL");
  await subscriptionInput.fill("https://example.com/router-e2e.yaml");
  await subscription.getByRole("button", { name: "保存并立即更新" }).click();
  await expect(subscriptionInput).toHaveValue("");
  await expect(subscription.getByText("已生效")).toBeVisible();
  await expect(page.getByText("https://example.com/router-e2e.yaml")).toHaveCount(0);

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
});

test("isolates pending-state failures and does not subscribe to unrelated resources on Network", async ({ page }) => {
  await loginAsAdmin(page);
  await goToAppPage(page, "总览");
  const unrelated: string[] = [];
  page.on("request", request => {
    if (/device-policies|proxy\/subscription|tailscale\/peers|proxy\/delays/.test(request.url())) {
      unrelated.push(request.url());
    }
  });
  await page.route("**/api/v1/network/pending", route =>
    route.fulfill({ status: 503, contentType: "application/json", body: "{}" }),
  );
  await goToAppPage(page, "网络");
  await expect(page.getByText(/待确认网络配置读取失败/)).toBeVisible();
  await page.getByRole("button", { name: "下游 Wi-Fi (AP)" }).click();
  await expect(page.getByRole("button", { name: "准备 AP 变更" })).toBeDisabled();
  await page.waitForTimeout(2_500);
  expect(unrelated).toEqual([]);
});
