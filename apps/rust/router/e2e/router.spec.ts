import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import {
  expectNoHorizontalOverflow,
  harnessOrigin,
  readHarnessState,
  resetHarness,
  webOrigin,
} from './fixtures';

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test('renders the dashboard and applies anonymous typed controls', async ({ page, request }) => {
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

  await page.getByRole('button', { name: '代理', exact: true }).click();
  await expect(page.getByRole('button', { name: '代理', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('heading', { name: '代理路径与节点' })).toBeVisible();
  await page.getByRole('button', { name: '显式代理' }).click();
  await expect(page.getByRole('status').filter({ hasText: '已切换到显式代理' })).toBeVisible();

  await page.getByRole('combobox', { name: '自动选择 节点' }).selectOption('新加坡');
  await expect(page.getByRole('status').filter({ hasText: '代理节点已切换' })).toBeVisible();

  const state = await readHarnessState(request);
  expect(state.panel.display.data).toMatchObject({
    enabled: true,
    brightness: 180,
    actual_brightness: 180,
  });
  expect(state.proxy.data.mode).toBe('explicit');
  expect(state.panel.proxy_groups.data[0].selected).toBe('新加坡');

  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
  await expectNoHorizontalOverflow(page);
  expect(browserErrors).toEqual([]);
});

test('keeps the last dashboard while a component becomes degraded', async ({ page, request }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Mihomo / TUN' })).toBeVisible();

  const current = await readHarnessState(request);
  current.proxy.state = 'degraded';
  current.proxy.issue = {
    code: 'e2e_proxy_degraded',
    message: '代理探测暂时不可用',
  };
  const update = await request.put(`${harnessOrigin}/state`, { data: current });
  expect(update.ok()).toBeTruthy();

  await expect(page.getByText('代理探测暂时不可用')).toBeVisible({ timeout: 7_500 });
  await expect(page.getByRole('heading', { name: 'Mihomo / TUN' })).toBeVisible();
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

test('fits a narrow management screen without horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'HYZ Router', level: 1 })).toBeVisible();
  await expect(page.getByRole('heading', { name: '网络拓扑' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '路由 / LAN' })).toBeVisible();
  await expectNoHorizontalOverflow(page);

  await page.getByRole('button', { name: '代理', exact: true }).click();
  await expect(page.getByRole('combobox', { name: '自动选择 节点' })).toBeVisible();
  await expectNoHorizontalOverflow(page);

  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await expect(page.getByRole('button', { name: '管理员登录' })).toBeVisible();
  await expectNoHorizontalOverflow(page);
});
