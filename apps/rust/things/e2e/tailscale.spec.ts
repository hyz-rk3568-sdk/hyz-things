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

test("shows local system proxy status from ProxyStatus without Tailscale coupling", async ({
  page,
  request,
}) => {
  await loginAsAdmin(page);
  await goToAppPage(page, "代理");
  const state = await readHarnessState(request);
  state.proxy.state = "degraded";
  state.proxy.issue = {
    code: "local_system_proxy_not_confirmed",
    message: "本机系统代理未确认",
  };
  state.proxy.data.lan_tun.desired = true;
  state.proxy.data.lan_tun.effective = "not_confirmed";
  state.proxy.data.local_system_proxy.desired = true;
  state.proxy.data.local_system_proxy.effective = "not_confirmed";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();

  const proxyCapabilities = page.getByLabel("代理能力");
  await expect(
    proxyCapabilities.getByText("已降级 · 未确认", { exact: true }),
  ).toHaveCount(2);
  await expect(
    proxyCapabilities.getByText(/代理路径|已恢复 Direct/),
  ).toHaveCount(0);

  const ready = await readHarnessState(request);
  ready.proxy.state = "available";
  ready.proxy.issue = null;
  ready.proxy.data.mihomo.configured_required = true;
  ready.proxy.data.mihomo.process = "ready";
  ready.proxy.data.mihomo.runtime_config = "ready";
  ready.proxy.data.mihomo.mixed_port = "ready";
  ready.proxy.data.local_system_proxy.desired = true;
  ready.proxy.data.local_system_proxy.effective = "ready";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: ready })).ok(),
  ).toBeTruthy();
  await expect(
    proxyCapabilities.getByText("已启用", { exact: true }),
  ).toBeVisible({ timeout: 7_500 });

  const unknown = await readHarnessState(request);
  unknown.proxy.data.mihomo.configured_required = null;
  unknown.proxy.data.mihomo.process = "unknown";
  unknown.proxy.data.lan_tun.desired = null;
  unknown.proxy.data.local_system_proxy.desired = null;
  unknown.proxy.data.local_system_proxy.effective = "not_confirmed";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: unknown })).ok(),
  ).toBeTruthy();
  await expect(
    page.getByText("Mihomo core：未知", { exact: true }),
  ).toBeVisible({ timeout: 7_500 });
  await expect(
    proxyCapabilities.getByText("未知 · 未确认", { exact: true }),
  ).toHaveCount(2);
  await expect(
    page.getByRole("switch", { name: "LAN 透明代理" }),
  ).toBeDisabled();
  await expect(
    page.getByRole("switch", { name: "本机系统代理" }),
  ).toBeDisabled();
});

test("supports the administrator Tailscale login, approval, disable, and logout flow", async ({
  page,
  request,
}) => {
  const initial = await readHarnessState(request);
  initial.tailscale.data.authenticated = false;
  initial.tailscale.data.backend_state = "stopped";
  initial.tailscale.data.desired_mode = "disabled";
  initial.tailscale.data.effective_mode = "disabled";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: initial })).ok(),
  ).toBeTruthy();

  await page.goto("/");
  await expect(page.getByText("laptop", { exact: true })).toHaveCount(0);
  await expect(page.getByText("100.64.0.8", { exact: false })).toHaveCount(0);
  await goToAppPage(page, "网络");
  await page.getByLabel("密码").fill("admin");
  await page.getByRole("button", { name: "登录", exact: true }).click();
  await page.getByLabel("当前密码").fill("admin");
  await page.getByLabel("新密码", { exact: true }).fill("router-e2e-password");
  await page.getByLabel("确认新密码").fill("router-e2e-password");
  await page.getByRole("button", { name: "修改密码" }).click();

  await goToAppPage(page, "Tailscale");
  const tailscale = page.getByRole("region", {
    name: "Tailscale 远程 LAN 状态",
  });
  await expect(tailscale.getByText(/Tailnet 设备列表暂不可用/)).toBeVisible();
  await tailscale.getByRole("button", { name: "启用远程 LAN 访问" }).click();
  const loginLink = tailscale.getByRole("link", {
    name: "打开一次性 Tailscale 登录链接",
  });
  await expect(loginLink).toHaveAttribute(
    "href",
    "https://login.tailscale.com/a/router-e2e",
  );
  await expect(loginLink).toHaveAttribute("target", "_blank");
  await expect(loginLink).toHaveAttribute("rel", "noopener noreferrer");
  await expect(tailscale.getByText("路由批准", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);

  const authenticated = await readHarnessState(request);
  authenticated.tailscale.data.authenticated = true;
  authenticated.tailscale.data.backend_state = "running";
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: authenticated })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "已完成登录，继续启用" }).click();
  await expect(tailscale.getByText("本机远程 LAN 访问已启用")).toBeVisible();
  await expect(tailscale.getByText("Tailnet 设备 · 3 / 4 在线")).toBeVisible();
  await expect(
    tailscale.getByText("laptop", { exact: true }),
  ).not.toBeVisible();
  await expect(
    tailscale.getByText("hyz-router", { exact: true }),
  ).not.toBeVisible();
  await tailscale.getByText("查看设备列表", { exact: true }).click();
  await expect(
    tailscale.getByText("hyz-router", { exact: true }),
  ).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();
  await expect(
    tailscale.getByText("hyz-iphone", { exact: true }),
  ).toBeVisible();
  await expect(tailscale.getByText("tablet", { exact: true })).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.7 · linux/)).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.8 · linux/)).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.9 · android/)).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.10 · iOS/)).toBeVisible();
  await expect(
    tailscale.getByText(/active · direct 192\.168\.1\.3:41641/),
  ).toHaveCount(2);
  await expect(tailscale.getByText(/active · relay "sfo"/)).toBeVisible();
  await expect(tailscale.getByText(/tx 1\.2 KiB · rx 789 B/)).toBeVisible();
  await expect(tailscale.getByText(/最近看到/)).toBeVisible();
  await expect(tailscale.getByText("在线", { exact: true })).toHaveCount(3);
  await expect(tailscale.getByText("离线", { exact: true })).toHaveCount(1);
  await expect(
    tailscale.getByText(/不表示它正在访问本路由器的 LAN/),
  ).toBeVisible();
  await expect(tailscale.getByText("路由批准", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);
  await expect(tailscale.getByText(/当前 LAN Access/)).toBeVisible();
  await expect(tailscale.getByText("连接", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText("Direct", { exact: true })).toHaveCount(0);
  const enabledButton = tailscale.getByRole("button", {
    name: "远程 LAN 访问已启用",
  });
  await expect(enabledButton).toBeDisabled();
  await expect(enabledButton).toHaveAttribute("aria-pressed", "true");
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: true,
    desired_mode: "lan_subnet_access",
    effective_mode: "lan_subnet_access",
    route_advertised: true,
    local_firewall_ready: true,
  });
  await page.setViewportSize({ width: 360, height: 800 });
  await expect(enabledButton).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();
  await expect(tailscale.getByText("tablet", { exact: true })).toBeVisible();
  await expect(tailscale.getByText("路由批准", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
  const expandedAccessibility = await new AxeBuilder({ page }).analyze();
  expect(expandedAccessibility.violations).toEqual([]);

  await tailscale.getByRole("button", { name: "停用（保留认证）" }).click();
  const disabledButton = tailscale.getByRole("button", {
    name: "Tailscale 已停用",
  });
  await expect(disabledButton).toBeDisabled();
  await expect(disabledButton).toHaveAttribute("aria-pressed", "true");
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: true,
    desired_mode: "disabled",
    effective_mode: "disabled",
  });

  await tailscale.getByRole("button", { name: "注销并移除认证" }).click();
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: false,
    desired_mode: "disabled",
    effective_mode: "disabled",
  });
  await expect(tailscale.getByText("laptop", { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText("100.64.0.8", { exact: false })).toHaveCount(
    0,
  );
});

test("shows Tailnet peer empty, initial-error, stale, and recovery states", async ({
  page,
  request,
}) => {
  const initial = await readHarnessState(request);
  initial.tailscale.data.authenticated = true;
  initial.tailscale.data.backend_state = "running";
  initial.tailscale.data.desired_mode = "router_only";
  initial.tailscale.data.effective_mode = "router_only";
  initial.tailscale_peers_failure = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: initial })).ok(),
  ).toBeTruthy();

  await loginAsAdmin(page);
  await goToAppPage(page, "Tailscale");
  const tailscale = page.getByRole("region", {
    name: "Tailscale 远程 LAN 状态",
  });
  await expect(tailscale.getByText(/Tailnet 设备列表暂不可用/)).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toHaveCount(0);

  let state = await readHarnessState(request);
  state.tailscale_peers_failure = false;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "重新读取设备" }).click();
  await expect(tailscale.getByText("Tailnet 设备 · 3 / 4 在线")).toBeVisible();
  await tailscale.getByText("查看设备列表", { exact: true }).click();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();

  state = await readHarnessState(request);
  state.tailscale_peers_failure = true;
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "重新读取设备" }).click();
  await expect(tailscale.getByText(/数据可能已过期/)).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toBeVisible();

  state = await readHarnessState(request);
  state.tailscale_peers_failure = false;
  state.tailscale_peers = { total: 0, online: 0, peers: [] };
  expect(
    (await request.put(`${harnessOrigin}/state`, { data: state })).ok(),
  ).toBeTruthy();
  await tailscale.getByRole("button", { name: "重新读取设备" }).click();
  await expect(tailscale.getByText("暂无 Tailnet 设备")).toBeVisible();
  await expect(tailscale.getByText("laptop", { exact: true })).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
});
