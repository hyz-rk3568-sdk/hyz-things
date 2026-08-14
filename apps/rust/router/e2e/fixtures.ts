import { expect, type APIRequestContext, type Page } from '@playwright/test';

export const webOrigin = `http://127.0.0.1:${process.env.ROUTER_E2E_WEB_PORT ?? 3190}`;
export const harnessOrigin = `http://127.0.0.1:${process.env.ROUTER_E2E_CONTROL_PORT ?? 3191}`;

export async function resetHarness(request: APIRequestContext) {
  const response = await request.put(`${harnessOrigin}/reset`);
  expect(response.ok()).toBeTruthy();
  return response.json();
}

export async function readHarnessState(request: APIRequestContext) {
  const response = await request.get(`${harnessOrigin}/state`);
  expect(response.ok()).toBeTruthy();
  return response.json();
}

export async function loginAsAdmin(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: '网络设置', exact: true }).click();
  await page.getByRole('button', { name: '管理员登录' }).click();
  await page.getByLabel('密码').fill('admin');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.getByLabel('当前密码').fill('admin');
  await page.getByLabel('新密码', { exact: true }).fill('router-e2e-password');
  await page.getByLabel('确认新密码').fill('router-e2e-password');
  await page.getByRole('button', { name: '修改密码' }).click();
  await expect(page.getByRole('heading', { name: '代理设置' })).toBeVisible();
}

export async function expectNoHorizontalOverflow(page: Page) {
  const dimensions = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  expect(dimensions.scrollWidth).toBeLessThanOrEqual(dimensions.clientWidth);
}
