import { expect, test } from "@playwright/test";
import { goToAppPage, loginAsAdmin, resetHarness } from "./fixtures";

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test("shows and filters bounded LAN activity by stable MAC identity", async ({ page }) => {
  await loginAsAdmin(page);
  await page.route("**/api/v1/proxy/device-policies", async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        config: { version: 1, generation: 0, entries: [] },
        clients: [],
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
    });
  });

  await goToAppPage(page, "活动");
  await expect(page.getByRole("heading", { name: "活动" })).toBeVisible();
  await expect(page.getByText("api.openai.com:443")).toBeVisible();
  await expect(page.getByText("MATCH · HYZ-PROXY")).toBeVisible();
  await expect(page.getByText("出口 · 新加坡 → HYZ-PROXY")).toBeVisible();
  await expect(page.locator('[data-activity-mac="02:00:00:00:00:11"]')).toBeVisible();

  await page.getByRole("button", { name: "筛选设备 测试手机" }).click();
  await expect(page.locator('[data-activity-mac="02:00:00:00:00:10"]')).toBeVisible();
  await expect(page.locator('[data-activity-mac="02:00:00:00:00:11"]')).toHaveCount(0);

  await page.getByRole("button", { name: "全部设备", exact: true }).click();
  await expect(page.locator('[data-activity-mac="02:00:00:00:00:11"]')).toBeVisible();
});
