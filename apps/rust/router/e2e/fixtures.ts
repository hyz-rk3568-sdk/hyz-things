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

export async function expectNoHorizontalOverflow(page: Page) {
  const dimensions = await page.evaluate(() => ({
    clientWidth: document.documentElement.clientWidth,
    scrollWidth: document.documentElement.scrollWidth,
  }));
  expect(dimensions.scrollWidth).toBeLessThanOrEqual(dimensions.clientWidth);
}
