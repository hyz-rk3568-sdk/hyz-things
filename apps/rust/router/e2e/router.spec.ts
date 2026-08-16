import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import {
  expectNoHorizontalOverflow,
  harnessOrigin,
  installCameraWebRtcMock,
  loginAsAdmin,
  readHarnessState,
  resetHarness,
  webOrigin,
} from './fixtures';

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test('renders the dashboard and applies the anonymous display control', async ({ page, request }) => {
  const browserErrors: string[] = [];
  page.on('console', message => {
    if (message.type() === 'error') browserErrors.push(message.text());
  });
  page.on('pageerror', error => browserErrors.push(error.message));

  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'HYZ Router', level: 1 })).toBeVisible();
  await expect(page.getByRole('heading', { name: '网络拓扑' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '路由 / LAN' })).toBeVisible();
  await expect(page.getByText('E2E-Upstream', { exact: false }).first()).toBeVisible();
  await expect(page.getByRole('button', { name: '总览', exact: true })).toHaveAttribute('aria-pressed', 'true');

  const overviewAccessibility = await new AxeBuilder({ page }).analyze();
  expect(overviewAccessibility.violations).toEqual([]);

  const slider = page.getByRole('slider', { name: '点亮亮度' });
  await slider.fill('180');
  await page.getByRole('button', { name: '点亮', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '背光已开启' })).toBeVisible();

  await expect(page.getByRole('button', { name: '代理', exact: true })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: '代理状态' })).toBeVisible();
  const readonlyProxy = page.getByRole('region', { name: '当前代理与延迟' });
  await expect(readonlyProxy.getByText('当前选择 · 东京')).toBeVisible();
  await expect
    .poll(async () => (await readHarnessState(request)).panel.proxy_groups.data[0].options[0].delay_ms)
    .toBe(40);
  await expect(readonlyProxy.getByText('当前 · 40 ms')).toBeVisible();
  await expect(readonlyProxy.getByText('新加坡 · 新加坡')).toBeVisible();
  await expect(readonlyProxy.getByRole('combobox')).toHaveCount(0);
  await expect(readonlyProxy.getByRole('button')).toHaveCount(0);

  const state = await readHarnessState(request);
  expect(state.panel.display.data).toMatchObject({
    enabled: true,
    brightness: 180,
    actual_brightness: 180,
  });
  expect(state.proxy.data.lan_tun.desired).toBe(true);
  expect(state.tailscale.data.explicit_proxy_desired).toBe(false);
  expect(state.panel.proxy_groups.data[0].selected).toBe('东京');

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
  await expectNoHorizontalOverflow(page);
  expect(browserErrors).toEqual([]);
});

test('keeps the last dashboard while a component becomes degraded', async ({ page, request }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '代理状态' })).toBeVisible();

  const current = await readHarnessState(request);
  current.proxy.state = 'degraded';
  current.proxy.issue = {
    code: 'e2e_proxy_degraded',
    message: '代理探测暂时不可用',
  };
  const update = await request.put(`${harnessOrigin}/state`, { data: current });
  expect(update.ok()).toBeTruthy();

  await expect(page.getByText('代理探测暂时不可用')).toBeVisible({ timeout: 7_500 });
  await expect(page.getByRole('heading', { name: '代理状态' })).toBeVisible();
});

test('serves the generated bundle through the strict production-shaped HTTP boundary', async ({
  request,
}) => {
  const root = await request.get('/');
  expect(root.ok()).toBeTruthy();
  const csp = root.headers()['content-security-policy'] ?? '';
  expect(csp).toContain("script-src 'self' 'wasm-unsafe-eval'");
  expect(csp).not.toContain("'unsafe-inline'");
  expect(csp).not.toContain("script-src 'self' 'unsafe-eval'");

  const html = await root.text();
  expect(html).not.toMatch(/<style[\s>]/i);
  expect(html).not.toMatch(/<script(?![^>]*\bsrc=)[^>]*>\s*\S/i);
  const bootstrapPath = html.match(/src="(\/router-bootstrap\.js\?v=[0-9a-f]{16})"/)?.[1];
  expect(bootstrapPath).toBeTruthy();

  const bootstrap = await request.get(bootstrapPath!);
  expect(bootstrap.ok()).toBeTruthy();
  expect(bootstrap.headers()['content-type']).toContain('text/javascript');

  const staleAsset = await request.get('/router-web-stale.css');
  expect(staleAsset.status()).toBe(404);
  expect(await staleAsset.text()).not.toContain('<!doctype html>');

  const unknownApi = await request.get('/api/v1/not-a-route');
  expect(unknownApi.status()).toBe(404);
  expect(unknownApi.headers()['content-type']).toContain('application/json');

  const panel = await request.get('/api/v1/panel');
  const csrf = ((await panel.json()) as { csrf_token: string }).csrf_token;
  const anonymousLanTun = await request.post('/api/v1/control/proxy/lan-tun', {
    headers: { Origin: webOrigin, 'X-HYZ-CSRF': csrf },
    data: { enabled: true },
  });
  expect(anonymousLanTun.status()).toBe(401);
  const anonymousTailscaleProxy = await request.post('/api/v1/control/proxy/tailscale', {
    headers: { Origin: webOrigin, 'X-HYZ-CSRF': csrf },
    data: { enabled: true },
  });
  expect(anonymousTailscaleProxy.status()).toBe(401);
  const anonymousTailscalePeers = await request.get('/api/v1/tailscale/peers');
  expect(anonymousTailscalePeers.status()).toBe(401);
  const anonymousCameraStatus = await request.get('/api/v1/camera/status');
  expect(anonymousCameraStatus.status()).toBe(401);
  const anonymousCameraCreate = await request.post('/api/v1/control/camera/session/create', {
    headers: { Origin: webOrigin, 'X-HYZ-CSRF': csrf },
    data: { offer_sdp: 'v=0\r\n' },
  });
  expect(anonymousCameraCreate.status()).toBe(401);
  const removedProxyMode = await request.post('/api/v1/control/proxy/mode', {
    headers: { Origin: webOrigin, 'X-HYZ-CSRF': csrf },
    data: { mode: 'tun' },
  });
  expect(removedProxyMode.status()).toBe(405);
  const anonymousProxySelection = await request.post('/api/v1/control/proxy/selection', {
    headers: { Origin: webOrigin, 'X-HYZ-CSRF': csrf },
    data: { group: '自动选择', proxy: '新加坡' },
  });
  expect(anonymousProxySelection.status()).toBe(401);

  const foreignOrigin = await request.post('/api/v1/control/display', {
    headers: {
      Origin: 'http://evil.example',
      'X-HYZ-CSRF': csrf,
    },
    data: { enabled: false },
  });
  expect(foreignOrigin.status()).toBe(403);

  const missingCsrf = await request.post('/api/v1/control/display', {
    headers: { Origin: webOrigin },
    data: { enabled: false },
  });
  expect(missingCsrf.status()).toBe(403);

  const displayBefore = (await readHarnessState(request)).panel.display.data;
  const oversized = await request.post('/api/v1/control/display', {
    headers: {
      Origin: webOrigin,
      'X-HYZ-CSRF': csrf,
      'Content-Type': 'application/json',
    },
    data: JSON.stringify({
      enabled: true,
      brightness: 1,
      padding: 'x'.repeat(5_000),
    }),
  });
  expect(oversized.status()).toBe(413);
  expect((await readHarnessState(request)).panel.display.data).toEqual(displayBefore);
});

test('plays and cleans up the administrator camera session on desktop and mobile', async ({
  page,
  request,
}) => {
  await installCameraWebRtcMock(page);
  await page.goto('/');
  await expect(page.getByRole('article', { name: '摄像头直播' })).toHaveCount(0);

  await loginAsAdmin(page);
  const camera = page.getByRole('article', { name: '摄像头直播' });
  await expect(camera).toBeVisible();
  await expect(camera.getByText('可用', { exact: true })).toBeVisible();
  await expect(camera.getByText('已停止', { exact: true })).toBeVisible();
  await expect(camera.getByText(/3840 × 2160 · 30 fps · 20\.0 Mbps · h264/)).toBeVisible();
  await expect(camera.getByText('LAN · 0 个会话', { exact: true })).toBeVisible();

  const presetSelect = camera.getByRole('combobox', { name: '画面分辨率与码率' });
  await expect(presetSelect).toBeVisible();
  await expect(presetSelect.locator('option')).toHaveCount(4);

  const harnessSessionId = (createCount: number) =>
    `e2e-camera.${'a'.repeat(46)}${createCount.toString(16).padStart(2, '0')}`;

  // 旋转是服务端媒体管线属性（videoflip）：断言桩端 rotation 与页面指标，而非 CSS class。
  const verifyRotation = async () => {
    const rotateButton = camera.getByRole('button', { name: '旋转画面' });
    await expect(rotateButton).toBeVisible();
    for (const degrees of [270, 180, 90, 0]) {
      await rotateButton.click();
      await expect
        .poll(async () => (await readHarnessState(request)).camera.status.profile.rotation)
        .toBe(`deg_${degrees}`);
      await expect(camera.getByText(`旋转 ${degrees}°`, { exact: false })).toBeVisible();
    }
  };

  await camera.getByRole('button', { name: '播放直播' }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        offer: state.camera.last_offer_sdp,
        pipeline: state.camera.status.pipeline,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: 1,
      offer: 'v=0\r\no=router-e2e-camera 1 1 IN IP4 127.0.0.1\r\n',
      pipeline: 'streaming',
      activeSessions: 1,
      sessions: [harnessSessionId(1)],
    });
  await expect(camera.getByText('直播中', { exact: true })).toBeVisible();
  expect(
    await camera.locator('video').evaluate(video => {
      const stream = (video as HTMLVideoElement).srcObject;
      return stream instanceof MediaStream && stream.getVideoTracks().length === 1;
    }),
  ).toBe(true);
  await verifyRotation();

  // Switching the preset while playing stops the session and reopens with the new profile.
  const beforeSwitch = await readHarnessState(request);
  await presetSelect.selectOption('fhd1080p5m');
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        closeCount: state.camera.close_count,
        width: state.camera.status.profile.width,
        height: state.camera.status.profile.height,
        bitrate: state.camera.status.profile.bitrate_bps,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: beforeSwitch.camera.create_count + 1,
      closeCount: beforeSwitch.camera.close_count + 1,
      width: 1920,
      height: 1080,
      bitrate: 5_000_000,
      activeSessions: 1,
      sessions: [harnessSessionId(beforeSwitch.camera.create_count + 1)],
    });
  await expect(camera.getByText('直播中', { exact: true })).toBeVisible();
  await expect(camera.getByText(/1920 × 1080 · 30 fps · 5\.0 Mbps · h264/)).toBeVisible();

  await camera.getByRole('button', { name: '停止直播' }).click();
  await expect(camera.getByText('未播放', { exact: true })).toBeVisible();
  await expect(camera.getByRole('status').filter({ hasText: '摄像头直播已停止' })).toBeVisible();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        closeCount: state.camera.close_count,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      closeCount: beforeSwitch.camera.close_count + 2,
      activeSessions: 0,
      sessions: [],
    });

  await camera.getByRole('button', { name: '播放直播' }).click();
  await expect(camera.getByText('直播中', { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).__hyzCameraRtcFail());
  await expect(
    camera.getByRole('status').filter({ hasText: '摄像头 WebRTC 连接已中断' }),
  ).toBeVisible();
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);

  await camera.getByRole('button', { name: '播放直播' }).click();
  await expect(camera.getByText('直播中', { exact: true })).toBeVisible();
  await page.setViewportSize({ width: 360, height: 800 });
  await expectNoHorizontalOverflow(page);
  await verifyRotation();
  await page.getByRole('button', { name: '退出登录' }).click();
  await expect(camera).toHaveCount(0);
  await expect
    .poll(async () => (await readHarnessState(request)).camera.sessions)
    .toEqual([]);
});

test('lets two same-account viewers watch the camera concurrently', async ({
  context,
  request,
}) => {
  const first = await context.newPage();
  await installCameraWebRtcMock(first);
  await first.goto('/');
  await loginAsAdmin(first);

  const cameraA = first.getByRole('article', { name: '摄像头直播' });
  await cameraA.getByRole('button', { name: '播放直播' }).click();
  await expect(cameraA.getByText('直播中', { exact: true })).toBeVisible();

  // 同一浏览器上下文（同一管理员 cookie，同一 owner）的第二个页面并发观看。
  const second = await context.newPage();
  await installCameraWebRtcMock(second);
  await second.goto('/');
  const cameraB = second.getByRole('article', { name: '摄像头直播' });
  await cameraB.getByRole('button', { name: '播放直播' }).click();
  await expect(cameraB.getByText('直播中', { exact: true })).toBeVisible();

  const harnessSessionId = (createCount: number) =>
    `e2e-camera.${'a'.repeat(46)}${createCount.toString(16).padStart(2, '0')}`;
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        createCount: state.camera.create_count,
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({
      createCount: 2,
      activeSessions: 2,
      sessions: [harnessSessionId(1), harnessSessionId(2)],
    });
  await expect(first.getByText('直播中', { exact: true })).toBeVisible();
  await expect(second.getByText('直播中', { exact: true })).toBeVisible();

  // 关闭第一个页面：第二个页面的会话不受影响。
  await cameraA.getByRole('button', { name: '停止直播' }).click();
  await expect(cameraA.getByText('未播放', { exact: true })).toBeVisible();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({ activeSessions: 1, sessions: [harnessSessionId(2)] });
  await expect(cameraB.getByText('直播中', { exact: true })).toBeVisible();

  // 关闭第二个页面后全部清理。
  await cameraB.getByRole('button', { name: '停止直播' }).click();
  await expect
    .poll(async () => {
      const state = await readHarnessState(request);
      return {
        activeSessions: state.camera.status.active_sessions,
        sessions: state.camera.sessions,
      };
    })
    .toEqual({ activeSessions: 0, sessions: [] });
});

test('controls all four proxy combinations with isolated failures on desktop and mobile', async ({
  page,
  request,
}) => {
  await loginAsAdmin(page);
  const lanTun = page.getByRole('switch', { name: 'LAN 透明代理' });
  const tailscaleProxy = page.getByRole('switch', { name: 'Tailscale 中继代理' });

  const expectCombination = async (lanEnabled: boolean, tailscaleEnabled: boolean) => {
    await expect
      .poll(async () => {
        const state = await readHarnessState(request);
        return {
          lanDesired: state.proxy.data.lan_tun.desired,
          lanEffective: state.proxy.data.lan_tun.effective,
          tailscaleDesired: state.tailscale.data.explicit_proxy_desired,
          environment: state.tailscale.data.environment,
          core: state.proxy.data.mihomo.process,
        };
      })
      .toEqual({
        lanDesired: lanEnabled,
        lanEffective: lanEnabled ? 'ready' : 'ordinary_nat',
        tailscaleDesired: tailscaleEnabled,
        environment: tailscaleEnabled ? 'mihomo_explicit' : 'direct',
        core: lanEnabled || tailscaleEnabled ? 'ready' : 'absent',
      });
    await expect(lanTun).toBeChecked({ checked: lanEnabled });
    await expect(tailscaleProxy).toBeChecked({ checked: tailscaleEnabled });
  };

  await expectCombination(true, false);
  await tailscaleProxy.click();
  await expectCombination(true, true);
  await lanTun.click();
  await expectCombination(false, true);
  await tailscaleProxy.click();
  await expectCombination(false, false);
  await lanTun.click();
  await expectCombination(true, false);

  await page.setViewportSize({ width: 360, height: 800 });
  await tailscaleProxy.click();
  await expectCombination(true, true);
  await lanTun.click();
  await expectCombination(false, true);
  await expectNoHorizontalOverflow(page);
  await expect(page.getByText('当前代理节点')).toBeVisible();
  await expect(page.getByRole('combobox', { name: '自动选择 节点' })).toBeEnabled();

  let state = await readHarnessState(request);
  state.proxy_failures.lan_tun = true;
  expect((await request.put(`${harnessOrigin}/state`, { data: state })).ok()).toBeTruthy();
  await lanTun.click();
  await expect(page.getByRole('status').filter({ hasText: '操作失败' })).toBeVisible();
  await expectCombination(false, true);
  await expect(tailscaleProxy).toBeChecked();

  state = await readHarnessState(request);
  state.proxy_failures.lan_tun = false;
  state.proxy_failures.tailscale = true;
  expect((await request.put(`${harnessOrigin}/state`, { data: state })).ok()).toBeTruthy();
  await tailscaleProxy.click();
  await expect(page.getByRole('status').filter({ hasText: '操作失败' })).toHaveCount(2);
  await expectCombination(false, true);
  await expect(lanTun).not.toBeChecked();

  state = await readHarnessState(request);
  state.proxy_failures.tailscale = false;
  expect((await request.put(`${harnessOrigin}/state`, { data: state })).ok()).toBeTruthy();
  await page.getByRole('combobox', { name: '自动选择 节点' }).selectOption('新加坡');
  await expect
    .poll(async () => (await readHarnessState(request)).panel.proxy_groups.data[0].selected)
    .toBe('新加坡');
  await expect(tailscaleProxy).toBeEnabled();
  await expectNoHorizontalOverflow(page);

  const csrf = ((await (await request.get('/api/v1/panel')).json()) as { csrf_token: string })
    .csrf_token;
  for (const path of [
    '/api/v1/control/proxy/lan-tun',
    '/api/v1/control/proxy/tailscale',
  ]) {
    const forbidden = await page.evaluate(
      async ({ path, csrf }) =>
        (
          await fetch(path, {
            method: 'POST',
            credentials: 'same-origin',
            headers: { 'Content-Type': 'application/json', 'X-HYZ-CSRF': csrf },
            body: JSON.stringify({
              enabled: true,
              proxy_url: 'http://127.0.0.1:7890',
              port: 7890,
              environment: { HTTP_PROXY: 'forbidden' },
              provider: 'forbidden',
              Controller: 'forbidden',
              config: 'raw',
            }),
          })
        ).status,
      { path, csrf },
    );
    expect(forbidden).toBe(400);
  }
  const oversized = await page.evaluate(
    async csrf =>
      (
        await fetch('/api/v1/control/proxy/lan-tun', {
          method: 'POST',
          credentials: 'same-origin',
          headers: { 'Content-Type': 'application/json', 'X-HYZ-CSRF': csrf },
          body: JSON.stringify({ enabled: true, padding: 'x'.repeat(5_000) }),
        })
      ).status,
    csrf,
  );
  expect(oversized).toBe(413);
});

test('shows layered direct-restored, degraded, and unknown proxy wording', async ({ page, request }) => {
  await loginAsAdmin(page);
  const state = await readHarnessState(request);
  state.proxy.state = 'degraded';
  state.proxy.issue = { code: 'lan_tun_not_confirmed', message: 'LAN TUN 未确认' };
  state.proxy.data.lan_tun.desired = true;
  state.proxy.data.lan_tun.effective = 'not_confirmed';
  state.tailscale.state = 'degraded';
  state.tailscale.issue = { code: 'tailscale_proxy_direct_restored', message: '代理已回退' };
  state.tailscale.data.explicit_proxy_desired = true;
  state.tailscale.data.environment = 'direct';
  state.tailscale.data.proxy_fallback = 'direct_restored';
  expect((await request.put(`${harnessOrigin}/state`, { data: state })).ok()).toBeTruthy();

  const proxyCapabilities = page.getByLabel('代理能力');
  await expect(proxyCapabilities.getByText('已降级 · 未确认', { exact: true })).toBeVisible({
    timeout: 7_500,
  });
  await expect(
    proxyCapabilities.getByText('已降级 · 已恢复 Direct', { exact: true }),
  ).toBeVisible();

  const unavailable = await readHarnessState(request);
  unavailable.tailscale.data.explicit_proxy_desired = true;
  unavailable.tailscale.data.environment = 'mihomo_explicit';
  unavailable.tailscale.data.explicit_proxy_path = 'unavailable';
  unavailable.tailscale.data.proxy_fallback = 'not_confirmed';
  expect((await request.put(`${harnessOrigin}/state`, { data: unavailable })).ok()).toBeTruthy();
  await expect(
    proxyCapabilities.getByText('已降级 · 代理路径不可用', { exact: true }),
  ).toBeVisible({ timeout: 7_500 });

  const unknown = await readHarnessState(request);
  unknown.proxy.data.mihomo.configured_required = null;
  unknown.proxy.data.mihomo.process = 'unknown';
  unknown.proxy.data.lan_tun.desired = null;
  unknown.tailscale.data.explicit_proxy_desired = null;
  unknown.tailscale.data.environment = null;
  unknown.tailscale.data.proxy_fallback = 'not_confirmed';
  expect((await request.put(`${harnessOrigin}/state`, { data: unknown })).ok()).toBeTruthy();
  await expect(page.getByText('Mihomo core：未知', { exact: true })).toBeVisible({ timeout: 7_500 });
  await expect(proxyCapabilities.getByText('未知 · 未确认', { exact: true })).toHaveCount(2);
  await expect(page.getByRole('switch', { name: 'LAN 透明代理' })).toBeDisabled();
  await expect(page.getByRole('switch', { name: 'Tailscale 中继代理' })).toBeDisabled();
});

test('supports the administrator, STA, AP, and write-only subscription journey', async ({
  page,
  request,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await expect(page.getByRole('button', { name: '网络设置', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );

  await page.getByRole('button', { name: '管理员登录' }).click();
  await page.getByLabel('密码').fill('admin');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(page.getByRole('alert').filter({ hasText: '必须先修改默认密码' })).toBeVisible();

  await page.getByLabel('当前密码').fill('admin');
  await page.getByLabel('新密码', { exact: true }).fill('router-e2e-password');
  await page.getByLabel('确认新密码').fill('router-e2e-password');
  await page.getByRole('button', { name: '修改密码' }).click();
  await expect(page.getByRole('button', { name: '上游 Wi-Fi (STA)' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '代理设置' })).toBeVisible();
  await page.getByRole('combobox', { name: '自动选择 节点' }).selectOption('新加坡');
  await expect
    .poll(async () => (await readHarnessState(request)).panel.proxy_groups.data[0].selected)
    .toBe('新加坡');

  await page.getByRole('button', { name: '上游 Wi-Fi (STA)' }).click();
  const staRegion = page.getByRole('region', { name: '上游 Wi-Fi (STA)' });
  await page.getByRole('button', { name: '扫描', exact: true }).click();
  await page.getByRole('button', { name: '选择网络 Guest-Network' }).click();
  await staRegion.getByLabel('密码').fill('guest-password');

  await page.getByRole('button', { name: '总览', exact: true }).click();
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await expect(staRegion.getByLabel('SSID')).toHaveValue('Guest-Network');
  await expect(staRegion.getByLabel('密码')).toHaveValue('guest-password');

  const staApply = page.getByRole('button', { name: '检查并应用 STA' });
  await staApply.click();
  let staConfirmation = page.getByRole('region', { name: '应用上游 Wi-Fi？' });
  await expect(staConfirmation).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  const returnToSta = staConfirmation.getByRole('button', { name: '返回检查' });
  await expect(returnToSta).toBeFocused();
  await returnToSta.click();
  await expect(staApply).toBeFocused();

  await staRegion.getByLabel('密码').fill('guest-password');
  await page.evaluate(() => {
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = function () {
      document.documentElement.dataset.networkConfirmationScrolled = 'true';
      original.call(this);
    };
  });
  await staApply.click();
  staConfirmation = page.getByRole('region', { name: '应用上游 Wi-Fi？' });
  await expect
    .poll(() =>
      staConfirmation.evaluate(element => {
        const bounds = element.getBoundingClientRect();
        return bounds.top >= 0 && bounds.top < window.innerHeight;
      }),
    )
    .toBe(true);
  await expect(page.locator('html')).toHaveAttribute(
    'data-network-confirmation-scrolled',
    'true',
  );
  await staConfirmation.getByRole('button', { name: '确认并开始应用' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'STA 配置已应用' })).toBeVisible();

  let state = await readHarnessState(request);
  expect(state.network.sta_ssid).toBe('Guest-Network');

  const apToggle = page.getByRole('button', { name: '下游 Wi-Fi (AP)' });
  await apToggle.click();
  const apRegion = page.getByRole('region', { name: '下游 Wi-Fi (AP)' });
  await apRegion.getByLabel('SSID').fill('HYZ-New-AP');
  await apRegion.getByLabel('密码').fill('new-ap-password');
  await apRegion.getByLabel('国家 / 地区').selectOption('US');
  await apRegion.getByRole('button', { name: '准备 AP 变更' }).click();
  await expect(apRegion.getByText('HYZ-New-AP', { exact: true })).toBeVisible();
  await apRegion.getByRole('button', { name: '检查风险并应用' }).click();
  const apConfirmation = page.getByRole('region', { name: '应用下游 AP？' });
  await expect(apConfirmation).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await apConfirmation.getByRole('button', { name: '确认并开始应用' }).click();
  await expect(apToggle).toBeFocused();
  await apToggle.click();
  await expect(apRegion.getByText(/等待确认/)).toBeVisible();
  await apRegion.getByRole('button', { name: '确认保留' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'AP 配置已确认' })).toBeVisible();

  state = await readHarnessState(request);
  expect(state.network.ap_ssid).toBe('HYZ-New-AP');
  expect(state.pending_network).toBeNull();

  const devicePolicies = page.getByRole('article', { name: '设备代理' });
  await expect(devicePolicies.getByText('e2e-phone')).toBeVisible();
  const deviceName = devicePolicies.getByLabel('02:00:00:00:00:10 显示名');
  await deviceName.fill('我的 iPhone');
  await devicePolicies.getByRole('button', { name: '保存名称' }).click();
  await expect(page.getByRole('status').filter({ hasText: '设备代理策略已保存' })).toBeVisible();
  expect((await readHarnessState(request)).device_policies).toMatchObject({
    config: {
      generation: 1,
      entries: [{ mac: '02:00:00:00:00:10', label: '我的 iPhone', policy: 'proxy' }],
    },
  });

  const devicePolicy = devicePolicies.getByRole('combobox', { name: '02:00:00:00:00:10 代理策略' });
  await devicePolicy.selectOption('direct');
  await expect(page.getByRole('status').filter({ hasText: '设备代理策略已保存' })).toBeVisible();
  expect((await readHarnessState(request)).device_policies).toMatchObject({
    config: {
      generation: 2,
      entries: [{ mac: '02:00:00:00:00:10', label: '我的 iPhone', policy: 'direct' }],
    },
  });
  const raced = await readHarnessState(request);
  raced.device_policies.config.generation = 3;
  const raceUpdate = await request.put(`${harnessOrigin}/state`, { data: raced });
  expect(raceUpdate.ok()).toBeTruthy();
  await devicePolicy.selectOption('proxy');
  await expect(page.getByRole('status').filter({ hasText: '设备策略更新未完成（HTTP 409）' })).toBeVisible();

  await page.reload();
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  const reloadedPolicies = page.getByRole('article', { name: '设备代理' });
  await expect(reloadedPolicies.getByRole('combobox')).toHaveValue('direct');
  await expect(reloadedPolicies.getByLabel('02:00:00:00:00:10 显示名')).toHaveValue('我的 iPhone');
  await reloadedPolicies.getByRole('combobox').selectOption('proxy');
  await expect(page.getByRole('status').filter({ hasText: '设备代理策略已保存' })).toBeVisible();
  expect((await readHarnessState(request)).device_policies.config.entries).toEqual([
    { mac: '02:00:00:00:00:10', label: '我的 iPhone', policy: 'proxy' },
  ]);
  await reloadedPolicies
    .getByRole('button', { name: '清除 02:00:00:00:00:10 的名称和设备策略' })
    .click();
  await expect(page.getByRole('status').filter({ hasText: '设备代理策略已保存' })).toBeVisible();
  expect((await readHarnessState(request)).device_policies.config.entries).toEqual([]);

  const offline = await readHarnessState(request);
  offline.device_policies.config.generation += 1;
  offline.device_policies.config.entries = [{
    mac: '02:00:00:00:00:20',
    label: 'offline-tablet',
    policy: 'proxy',
  }];
  offline.device_policies.clients.push({
    mac: '02:00:00:00:00:20',
    lease_address: null,
    hostname: null,
    associated: false,
    policy: 'proxy',
  });
  expect((await request.put(`${harnessOrigin}/state`, { data: offline })).ok()).toBeTruthy();
  await page.reload();
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await page.getByRole('button', { name: '清除 02:00:00:00:00:20 的名称和设备策略' }).click();
  await expect(page.getByRole('status').filter({ hasText: '设备代理策略已保存' })).toBeVisible();
  expect((await readHarnessState(request)).device_policies.config.entries).toEqual([]);

  const subscription = page.getByRole('article', { name: '代理订阅' });
  const subscriptionInput = subscription.getByLabel('订阅 URL');
  await subscriptionInput.fill('https://example.com/router-e2e.yaml');
  await subscription.getByRole('button', { name: '保存并立即更新' }).click();
  await expect(subscriptionInput).toHaveValue('');
  await expect(subscription.getByText('已生效')).toBeVisible();
  await expect(page.getByText('https://example.com/router-e2e.yaml')).toHaveCount(0);

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
});

test('supports the administrator Tailscale login, approval, disable, and logout flow', async ({
  page,
  request,
}) => {
  const initial = await readHarnessState(request);
  initial.tailscale.data.authenticated = false;
  initial.tailscale.data.backend_state = 'stopped';
  initial.tailscale.data.desired_mode = 'disabled';
  initial.tailscale.data.effective_mode = 'disabled';
  expect((await request.put(`${harnessOrigin}/state`, { data: initial })).ok()).toBeTruthy();

  await page.goto('/');
  await expect(page.getByText('laptop', { exact: true })).toHaveCount(0);
  await expect(page.getByText('100.64.0.8', { exact: false })).toHaveCount(0);
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await page.getByRole('button', { name: '管理员登录' }).click();
  await page.getByLabel('密码').fill('admin');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.getByLabel('当前密码').fill('admin');
  await page.getByLabel('新密码', { exact: true }).fill('router-e2e-password');
  await page.getByLabel('确认新密码').fill('router-e2e-password');
  await page.getByRole('button', { name: '修改密码' }).click();

  const tailscale = page.getByRole('region', { name: 'Tailscale 远程 LAN 状态' });
  await expect(tailscale.getByText(/Tailnet 设备列表暂不可用/)).toBeVisible();
  await tailscale.getByRole('button', { name: '启用远程 LAN 访问' }).click();
  const loginLink = tailscale.getByRole('link', { name: '打开一次性 Tailscale 登录链接' });
  await expect(loginLink).toHaveAttribute('href', 'https://login.tailscale.com/a/router-e2e');
  await expect(loginLink).toHaveAttribute('target', '_blank');
  await expect(loginLink).toHaveAttribute('rel', 'noopener noreferrer');
  await expect(tailscale.getByText('路由批准', { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);

  const authenticated = await readHarnessState(request);
  authenticated.tailscale.data.authenticated = true;
  authenticated.tailscale.data.backend_state = 'running';
  expect((await request.put(`${harnessOrigin}/state`, { data: authenticated })).ok()).toBeTruthy();
  await tailscale.getByRole('button', { name: '已完成登录，继续启用' }).click();
  await expect(tailscale.getByText('本机远程 LAN 访问已启用')).toBeVisible();
  await expect(tailscale.getByText('Tailnet 设备 · 1 / 2 在线')).toBeVisible();
  await expect(tailscale.getByText('laptop', { exact: true })).not.toBeVisible();
  await tailscale.getByText('查看设备列表', { exact: true }).click();
  await expect(tailscale.getByText('laptop', { exact: true })).toBeVisible();
  await expect(tailscale.getByText('tablet', { exact: true })).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.8 · linux/)).toBeVisible();
  await expect(tailscale.getByText(/100\.64\.0\.9 · android/)).toBeVisible();
  await expect(tailscale.getByText('在线', { exact: true })).toBeVisible();
  await expect(tailscale.getByText('离线', { exact: true })).toBeVisible();
  await expect(tailscale.getByText(/不表示它正在访问本路由器的 LAN/)).toBeVisible();
  await expect(tailscale.getByText('路由批准', { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);
  await expect(tailscale.getByText(/当前 LAN Access/)).toBeVisible();
  await expect(tailscale.getByText('Direct', { exact: true })).toBeVisible();
  const enabledButton = tailscale.getByRole('button', { name: '远程 LAN 访问已启用' });
  await expect(enabledButton).toBeDisabled();
  await expect(enabledButton).toHaveAttribute('aria-pressed', 'true');
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: true,
    desired_mode: 'lan_subnet_access',
    effective_mode: 'lan_subnet_access',
    route_advertised: true,
    local_firewall_ready: true,
  });
  await page.setViewportSize({ width: 360, height: 800 });
  await expect(enabledButton).toBeVisible();
  await expect(tailscale.getByText('laptop', { exact: true })).toBeVisible();
  await expect(tailscale.getByText('tablet', { exact: true })).toBeVisible();
  await expect(tailscale.getByText('路由批准', { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText(/外部确认/)).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
  const expandedAccessibility = await new AxeBuilder({ page }).analyze();
  expect(expandedAccessibility.violations).toEqual([]);

  await tailscale.getByRole('button', { name: '停用（保留认证）' }).click();
  const disabledButton = tailscale.getByRole('button', { name: 'Tailscale 已停用' });
  await expect(disabledButton).toBeDisabled();
  await expect(disabledButton).toHaveAttribute('aria-pressed', 'true');
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: true,
    desired_mode: 'disabled',
    effective_mode: 'disabled',
  });

  await tailscale.getByRole('button', { name: '注销并移除认证' }).click();
  expect((await readHarnessState(request)).tailscale.data).toMatchObject({
    authenticated: false,
    desired_mode: 'disabled',
    effective_mode: 'disabled',
  });
  await expect(tailscale.getByText('laptop', { exact: true })).toHaveCount(0);
  await expect(tailscale.getByText('100.64.0.8', { exact: false })).toHaveCount(0);
});

test('shows Tailnet peer empty, initial-error, stale, and recovery states', async ({
  page,
  request,
}) => {
  const initial = await readHarnessState(request);
  initial.tailscale.data.authenticated = true;
  initial.tailscale.data.backend_state = 'running';
  initial.tailscale.data.desired_mode = 'router_only';
  initial.tailscale.data.effective_mode = 'router_only';
  initial.tailscale_peers_failure = true;
  expect((await request.put(`${harnessOrigin}/state`, { data: initial })).ok()).toBeTruthy();

  await loginAsAdmin(page);
  const tailscale = page.getByRole('region', { name: 'Tailscale 远程 LAN 状态' });
  await expect(tailscale.getByText(/Tailnet 设备列表暂不可用/)).toBeVisible();
  await expect(tailscale.getByText('laptop', { exact: true })).toHaveCount(0);

  let state = await readHarnessState(request);
  state.tailscale_peers_failure = false;
  expect((await request.put(`${harnessOrigin}/state`, { data: state })).ok()).toBeTruthy();
  await tailscale.getByRole('button', { name: '重新读取设备' }).click();
  await expect(tailscale.getByText('Tailnet 设备 · 1 / 2 在线')).toBeVisible();
  await tailscale.getByText('查看设备列表', { exact: true }).click();
  await expect(tailscale.getByText('laptop', { exact: true })).toBeVisible();

  state = await readHarnessState(request);
  state.tailscale_peers_failure = true;
  expect((await request.put(`${harnessOrigin}/state`, { data: state })).ok()).toBeTruthy();
  await tailscale.getByRole('button', { name: '重新读取设备' }).click();
  await expect(tailscale.getByText(/数据可能已过期/)).toBeVisible();
  await expect(tailscale.getByText('laptop', { exact: true })).toBeVisible();

  state = await readHarnessState(request);
  state.tailscale_peers_failure = false;
  state.tailscale_peers = { total: 0, online: 0, peers: [] };
  expect((await request.put(`${harnessOrigin}/state`, { data: state })).ok()).toBeTruthy();
  await tailscale.getByRole('button', { name: '重新读取设备' }).click();
  await expect(tailscale.getByText('暂无其他 Tailnet 设备')).toBeVisible();
  await expect(tailscale.getByText('laptop', { exact: true })).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
});

test('fits a narrow management screen without horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'HYZ Router', level: 1 })).toBeVisible();
  await expect(page.getByRole('heading', { name: '网络拓扑' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '路由 / LAN' })).toBeVisible();
  await expectNoHorizontalOverflow(page);

  await expect(page.getByRole('button', { name: '代理', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await expect(page.getByRole('button', { name: '管理员登录' })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});
