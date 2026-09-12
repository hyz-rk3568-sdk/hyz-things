import { expect, test } from '@playwright/test';
import { goToAppPage, resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

test('shows the administrator login form directly on every protected page', async ({ page }) => {
  await page.goto('/');

  for (const name of ['网络', '代理', '活动', 'Tailscale'] as const) {
    await goToAppPage(page, name);
    await expect(page.getByRole('heading', { name: '管理员登录' })).toBeVisible();
    await expect(page.getByLabel('用户名')).toHaveValue('admin');
    await expect(page.getByLabel('密码')).toBeVisible();
    await expect(page.getByText('请先在“网络”页面完成管理员登录')).toHaveCount(0);
  }
});

test('keeps the requested protected page selected through bootstrap password change', async ({ page }) => {
  await page.goto('/');
  await goToAppPage(page, '代理');

  await page.getByLabel('密码').fill('admin');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await expect(
    page.getByRole('alert').filter({ hasText: '必须先修改默认密码' }),
  ).toBeVisible();

  await page.getByLabel('当前密码').fill('admin');
  await page.getByLabel('新密码', { exact: true }).fill('router-e2e-password');
  await page.getByLabel('确认新密码').fill('router-e2e-password');
  await page.getByRole('button', { name: '修改密码' }).click();

  await expect(
    page.getByRole('button', { name: '代理', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('heading', { name: '代理设置' })).toBeVisible();
});
