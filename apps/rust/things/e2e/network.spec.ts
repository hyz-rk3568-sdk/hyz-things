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

test("supports the administrator, STA, AP, and write-only subscription journey", async ({
  page,
  request,
}) => {
  await page.goto("/");
  await goToAppPage(page, "网络");
  await expect(
    page.getByRole("button", { name: "网络", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");

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
