import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const oldVersion = 'a'.repeat(64);
const newVersion = 'b'.repeat(64);
const moduleSource = readFileSync(join(__dirname, '../src/web/pwa.js'), 'utf8');

test('shared PWA controls offer an update without replacing the current page', async ({ page }) => {
  await page.route('**/?frontend-version-check=1', route => route.fulfill({
    contentType: 'text/html',
    body: `<meta name="hyz-frontend-version" content="${newVersion}">`,
  }));
  await page.goto('/');
  await expect(page.getByRole('button', { name: '重新加载', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '更新版本', exact: true })).toBeVisible();
  await page.evaluate(() => { (window as any).__pwaPagePreserved = true; });
  await page.evaluate(() => window.dispatchEvent(new Event('pageshow')));
  expect(await page.evaluate(() => (window as any).__pwaPagePreserved)).toBe(true);
  await page.setViewportSize({ width: 360, height: 800 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

for (const action of ['重新加载', '更新版本']) {
  test(`isolated PWA ${action} waits for persistence and blocks refresh on failure`, async ({ page }) => {
    let latest = oldVersion;
    let navigations = 0;
    await page.route('**/pwa-test-module.js', route => route.fulfill({ contentType: 'text/javascript', body: moduleSource }));
    await page.route('**/pwa-test-entry.js', route => route.fulfill({ contentType: 'text/javascript', body: `
      import { mountPwa } from '/pwa-test-module.js';
      window.flushMode = 'pending';
      mountPwa(document.querySelector('section'), () => window.flushMode === 'fail'
        ? Promise.reject(new Error('quota'))
        : new Promise(resolve => { window.finishFlush = resolve; }));
    ` }));
    await page.route('**/?frontend-version-check=1', route => route.fulfill({
      contentType: 'text/html', body: `<meta name="hyz-frontend-version" content="${latest}">`,
    }));
    await page.route('**/pwa-test', route => {
      navigations++;
      return route.fulfill({ contentType: 'text/html', headers: {
        'Content-Security-Policy': "default-src 'self'; script-src 'self'; object-src 'none'",
      }, body: `<!doctype html><meta charset="utf-8"><meta name="hyz-frontend-version" content="${oldVersion}">
        <section aria-label="应用更新"><button data-pwa-reload>重新加载</button>
        <button data-pwa-update hidden>更新版本</button><span role="status" data-pwa-status></span></section>
        <script type="module" src="/pwa-test-entry.js"></script>` });
    });
    await page.goto('/pwa-test');
    await expect(page.getByRole('button', { name: '更新版本', exact: true })).toBeHidden();
    latest = newVersion;
    await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
    await expect(page.getByRole('button', { name: '更新版本', exact: true })).toBeVisible();
    expect(navigations).toBe(1);
    await page.evaluate(() => { (window as any).flushMode = 'fail'; });
    await page.getByRole('button', { name: action, exact: true }).click();
    await expect(page.getByRole('status')).toContainText('草稿保存失败，已停止刷新');
    expect(navigations).toBe(1);
    await page.evaluate(() => { (window as any).flushMode = 'pending'; });
    await page.getByRole('button', { name: action, exact: true }).click();
    await expect(page.getByRole('button', { name: action, exact: true })).toBeDisabled();
    await expect(page.getByRole('status')).toContainText('正在保存草稿');
    expect(navigations).toBe(1);
    await page.evaluate(() => (window as any).finishFlush());
    await expect.poll(() => navigations).toBe(2);
  });
}
