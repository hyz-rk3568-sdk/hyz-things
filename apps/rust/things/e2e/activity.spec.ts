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

test("merges discovered, configured-offline, and activity-only devices into list and detail", async ({ page }) => {
  await loginAsAdmin(page);
  await page.route("**/api/v1/proxy/device-policies", route =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        config: {
          version: 1,
          generation: 4,
          entries: [
            { mac: "02:00:00:00:00:10", label: "我的 iPhone", policy: "proxy" },
            { mac: "02:00:00:00:00:20", label: "离线平板", policy: "direct" },
          ],
        },
        clients: [
          {
            mac: "02:00:00:00:00:10",
            lease_address: "192.168.8.10",
            hostname: "e2e-phone",
            associated: true,
            policy: "proxy",
          },
        ],
        effective: true,
        activity: {
          observed_at_unix_ms: 2_000,
          records: [
            {
              connection_id: "activity-custom-label",
              mac: "02:00:00:00:00:10",
              device_name: "测试手机",
              source_address: "192.168.8.10",
              target: "api.openai.com",
              destination_port: 443,
              network: "tcp",
              rule: "MATCH · HYZ-PROXY",
              chains: ["新加坡", "HYZ-PROXY"],
              upload_bytes: 1024,
              download_bytes: 2048,
              first_seen_unix_ms: 1_000,
              last_seen_unix_ms: 2_000,
              active: true,
            },
            {
              connection_id: "activity-mac-fallback",
              mac: "02:00:00:00:00:11",
              device_name: "02:00:00:00:00:11",
              source_address: "192.168.8.11",
              target: "203.0.113.11",
              destination_port: 443,
              network: "udp",
              rule: "GEOSITE · cn",
              chains: ["DIRECT"],
              upload_bytes: 12,
              download_bytes: 34,
              first_seen_unix_ms: 1_500,
              last_seen_unix_ms: 1_900,
              active: false,
            },
          ],
        },
      }),
    }),
  );

  await goToAppPage(page, "设备");
  await expect(page.getByRole("heading", { name: "设备", exact: true })).toBeVisible();
  await expect(page.getByRole("listitem", { name: "设备 我的 iPhone" })).toBeVisible();
  await expect(page.getByRole("listitem", { name: "设备 离线平板" })).toContainText("当前未发现");
  await expect(page.getByRole("listitem", { name: "设备 02:00:00:00:00:11" })).toBeVisible();

  await page.getByLabel("筛选设备").fill("iPhone");
  await page.getByRole("button", { name: "应用筛选" }).click();
  await expect(page).toHaveURL(/#\/devices\?q=iPhone$/);
  await expect(page.getByRole("listitem", { name: "设备 我的 iPhone" })).toBeVisible();
  await expect(page.getByRole("listitem", { name: "设备 离线平板" })).toHaveCount(0);

  const opener = page.getByRole("button", { name: "打开设备 我的 iPhone" });
  await opener.click();
  await expect(page).toHaveURL(/#\/devices\/02:00:00:00:00:10\?q=iPhone$/);
  const detail = page.getByRole("dialog", { name: "我的 iPhone" });
  await expect(detail).toBeVisible();
  await expect(detail.getByText("api.openai.com:443")).toBeVisible();
  await expect(detail.getByText(/MATCH · HYZ-PROXY/)).toBeVisible();
  await expect(detail.getByText(/出口 新加坡 → HYZ-PROXY/)).toBeVisible();
  await expect(detail.getByRole("strong").filter({ hasText: "↑ 1.0 KiB · ↓ 2.0 KiB" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(detail).toHaveCount(0);
  await expect(opener).toBeFocused();
  await expect(page).toHaveURL(/#\/devices\?q=iPhone$/);
});

test("device detail dropdown shows the saved policy instead of defaulting to direct", async ({ page }) => {
  await loginAsAdmin(page);
  await page.route("**/api/v1/proxy/device-policies", route =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        config: {
          version: 1,
          generation: 4,
          entries: [{ mac: "02:00:00:00:00:10", label: "我的 iPhone", policy: "proxy" }],
        },
        clients: [
          {
            mac: "02:00:00:00:00:10",
            lease_address: "192.168.8.10",
            hostname: "e2e-phone",
            associated: true,
            policy: "proxy",
          },
        ],
        effective: true,
      }),
    }),
  );
  await goToAppPage(page, "设备");
  await page.getByRole("button", { name: "打开设备 我的 iPhone" }).click();
  const detail = page.getByRole("dialog");
  const policy = detail.getByLabel("02:00:00:00:00:10 代理策略");
  // The dropdown must reflect the saved 按规则分流 (proxy) policy. A regression used the
  // `value` HTML attribute on <select>, which never selects an option, so Chromium pinned the
  // dropdown to the last option (`直连`) regardless of the device's real policy.
  await expect(policy).toHaveValue("proxy");
  await expect(policy.locator("option:checked")).toHaveText("按规则分流");
});

test("edits policy from device detail and preserves a dirty draft across a 409", async ({ page, request }) => {
  await loginAsAdmin(page);
  await goToAppPage(page, "设备");
  await page.getByRole("button", { name: "打开设备 e2e-phone" }).click();
  const detail = page.getByRole("dialog");
  const nameInput = detail.getByLabel("02:00:00:00:00:10 显示名");
  const policy = detail.getByLabel("02:00:00:00:00:10 代理策略");
  await nameInput.fill("我的 iPhone");
  await policy.selectOption("direct");
  await detail.getByRole("button", { name: "保存设备设置" }).click();
  await expect(page.getByRole("status").filter({ hasText: "设备策略已保存" })).toBeVisible();
  expect((await readHarnessState(request)).device_policies).toMatchObject({
    config: {
      generation: 1,
      entries: [{ mac: "02:00:00:00:00:10", label: "我的 iPhone", policy: "direct" }],
    },
  });

  const raced = await readHarnessState(request);
  raced.device_policies.config.generation = 2;
  expect((await request.put(`${harnessOrigin}/state`, { data: raced })).ok()).toBeTruthy();
  await nameInput.fill("冲突时保留的草稿");
  await policy.selectOption("proxy");
  await detail.getByRole("button", { name: "保存设备设置" }).click();
  await expect(page.getByRole("status").filter({ hasText: "HTTP 409" })).toBeVisible();
  await expect(nameInput).toHaveValue("冲突时保留的草稿");
});

test("reports save success separately when device readback fails", async ({ page }) => {
  await loginAsAdmin(page);
  let failReads = false;
  await page.route("**/api/v1/proxy/device-policies", route => {
    if (route.request().method() === "GET" && failReads) {
      return route.fulfill({ status: 503, contentType: "application/json", body: "{}" });
    }
    return route.continue();
  });
  await goToAppPage(page, "设备");
  await page.getByRole("button", { name: "打开设备 e2e-phone" }).click();
  const detail = page.getByRole("dialog");
  await detail.getByLabel("02:00:00:00:00:10 显示名").fill("readback-test");
  failReads = true;
  await detail.getByRole("button", { name: "保存设备设置" }).click();
  await expect(page.getByRole("status").filter({ hasText: "已保存，状态刷新失败" })).toBeVisible();
});

test("keeps only one device read in flight and pauses ordinary polling while hidden", async ({ page }) => {
  await loginAsAdmin(page);
  let requests = 0;
  await page.route("**/api/v1/proxy/device-policies", async route => {
    if (route.request().method() === "GET") {
      requests += 1;
      await new Promise(resolve => setTimeout(resolve, 3_000));
    }
    await route.continue();
  });
  await goToAppPage(page, "设备");
  await page.waitForTimeout(2_500);
  expect(requests).toBe(1);
  await page.waitForTimeout(1_000);
  await page.unroute("**/api/v1/proxy/device-policies");
  await expect(page.getByRole("button", { name: "打开设备 e2e-phone" })).toBeVisible();

  let visibleRequests = 0;
  page.on("request", request => {
    if (request.method() === "GET" && request.url().includes("/api/v1/proxy/device-policies")) {
      visibleRequests += 1;
    }
  });
  await page.evaluate(() => {
    let hidden = true;
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    (window as any).__hyzSetDocumentHidden = (value: boolean) => {
      hidden = value;
      document.dispatchEvent(new Event("visibilitychange"));
    };
  });
  const baseline = visibleRequests;
  await page.waitForTimeout(2_500);
  expect(visibleRequests).toBe(baseline);
  await page.evaluate(() => (window as any).__hyzSetDocumentHidden(false));
  await expect.poll(() => visibleRequests).toBeGreaterThan(baseline);
});

test("a protected-resource 401 clears protected cache but keeps the target route", async ({ page }) => {
  await loginAsAdmin(page);
  await page.route("**/api/v1/proxy/device-policies", route =>
    route.fulfill({ status: 401, contentType: "application/json", body: "{}" }),
  );
  await goToAppPage(page, "设备");
  await expect(page.getByRole("heading", { name: "管理员登录" })).toBeVisible();
  await expect(page).toHaveURL(/#\/devices$/);
  await page.unroute("**/api/v1/proxy/device-policies");
  await page.getByLabel("密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await expect(page.getByRole("heading", { name: "设备", exact: true })).toBeVisible();
  await expect(page).toHaveURL(/#\/devices$/);
});
