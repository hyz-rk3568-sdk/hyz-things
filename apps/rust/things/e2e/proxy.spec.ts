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

test("controls all four proxy combinations with isolated failures on desktop and mobile", async ({
  page,
  request,
}) => {
  const initial = await readHarnessState(request);
  const selector = initial.panel.proxy_groups.data[0];
  selector.name = "HYZ-PROXY";
  initial.panel.proxy_groups.data = [selector];
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: initial })).ok(),
  ).toBeTruthy();

  await loginAsAdmin(page);
  await goToAppPage(page, "代理");
  const lanTun = page.getByRole("switch", { name: "LAN 透明代理" });
  const localSystemProxy = page.getByRole("switch", {
    name: "本机系统代理",
  });
  const currentProxySummary = page
    .getByText("当前代理节点", { exact: true })
    .locator("..");

  const expectCombination = async (
    lanEnabled: boolean,
    localSystemProxyEnabled: boolean,
  ) => {
    await expect
      .poll(async () => {
        const state = await readHarnessState(request);
        return {
          lanDesired: state.proxy.data.lan_tun.desired,
          lanEffective: state.proxy.data.lan_tun.effective,
          localSystemProxyDesired: state.proxy.data.local_system_proxy.desired,
          localSystemProxyEffective:
            state.proxy.data.local_system_proxy.effective,
          core: state.proxy.data.mihomo.process,
        };
      })
      .toEqual({
        lanDesired: lanEnabled,
        lanEffective: lanEnabled ? "ready" : "ordinary_nat",
        localSystemProxyDesired: localSystemProxyEnabled,
        localSystemProxyEffective: localSystemProxyEnabled
          ? "ready"
          : "disabled",
        core: lanEnabled || localSystemProxyEnabled ? "ready" : "absent",
      });
    await expect(lanTun).toBeChecked({ checked: lanEnabled });
    await expect(localSystemProxy).toBeChecked({
      checked: localSystemProxyEnabled,
    });
  };

  await expectCombination(true, false);
  await expect(currentProxySummary).toContainText("东京");
  await expect(page.getByText("东京", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("combobox", { name: "HYZ-PROXY 节点" }),
  ).toBeEnabled();
  await expect(page.getByRole("combobox")).toHaveCount(1);

  await localSystemProxy.click();
  await expectCombination(true, true);
  await lanTun.click();
  await expectCombination(false, true);
  await localSystemProxy.click();
  await expectCombination(false, false);
  await lanTun.click();
  await expectCombination(true, false);

  await page.setViewportSize({ width: 360, height: 800 });
  await localSystemProxy.click();
  await expectCombination(true, true);
  await lanTun.click();
  await expectCombination(false, true);
  await expectNoHorizontalOverflow(page);
  await expect(page.getByText("当前代理节点")).toBeVisible();
  await expect(
    page.getByRole("combobox", { name: "HYZ-PROXY 节点" }),
  ).toBeEnabled();

  let state = await readHarnessState(request);
  state.proxy_failures.lan_tun = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await lanTun.click();
  await expect(
    page.getByRole("status").filter({ hasText: "操作失败" }),
  ).toBeVisible();
  await expectCombination(false, true);
  await expect(localSystemProxy).toBeChecked();

  state = await readHarnessState(request);
  state.proxy_failures.lan_tun = false;
  state.proxy_failures.local_system_proxy = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await localSystemProxy.click();
  await expect(
    page.getByRole("status").filter({ hasText: "操作失败" }),
  ).toHaveCount(2);
  await expectCombination(false, true);
  await expect(lanTun).not.toBeChecked();

  state = await readHarnessState(request);
  state.proxy_failures.local_system_proxy = false;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();

  // Freeze dashboard polling so this verifies the selection response updates the
  // local proxy summary immediately instead of waiting for the next /panel poll.
  await page.route("**/api/v1/panel", (route) => route.abort());
  await page
    .getByRole("combobox", { name: "HYZ-PROXY 节点" })
    .selectOption("新加坡");
  await expect
    .poll(async () => {
      const groups = (await readHarnessState(request)).panel.proxy_groups.data;
      return groups.find((group) => group.name === "HYZ-PROXY")?.selected;
    })
    .toBe("新加坡");
  await expect(currentProxySummary).toContainText("新加坡");
  await expect(page.getByText("新加坡", { exact: true })).toBeVisible();
  await expect(localSystemProxy).toBeEnabled();
  await expectNoHorizontalOverflow(page);

  const csrf = (
    (await (await request.get("/api/v1/panel")).json()) as {
      csrf_token: string;
    }
  ).csrf_token;
  for (const path of [
    "/api/v1/control/proxy/lan-tun",
    "/api/v1/control/proxy/local-system",
  ]) {
    const forbidden = await page.evaluate(
      async ({ path, csrf }) =>
        (
          await fetch(path, {
            method: "POST",
            credentials: "same-origin",
            headers: { "Content-Type": "application/json", "X-HYZ-CSRF": csrf },
            body: JSON.stringify({
              enabled: true,
              proxy_url: "http://127.0.0.1:7890",
              port: 7890,
              environment: { HTTP_PROXY: "forbidden" },
              provider: "forbidden",
              Controller: "forbidden",
              config: "raw",
            }),
          })
        ).status,
      { path, csrf },
    );
    expect(forbidden).toBe(400);
  }
  const oversized = await page.evaluate(
    async (csrf) =>
      (
        await fetch("/api/v1/control/proxy/lan-tun", {
          method: "POST",
          credentials: "same-origin",
          headers: { "Content-Type": "application/json", "X-HYZ-CSRF": csrf },
          body: JSON.stringify({ enabled: true, padding: "x".repeat(5_000) }),
        })
      ).status,
    csrf,
  );
  expect(oversized).toBe(413);
});
