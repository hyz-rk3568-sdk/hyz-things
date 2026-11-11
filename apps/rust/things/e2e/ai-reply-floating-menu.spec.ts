import { expect, test, type Page } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

type Bridge = {
  version: 1;
  mountFloatingMenu: (reference: HTMLElement, floating: HTMLElement) => () => void;
};
type InstrumentedWindow = Window & {
  hyzThingsUi?: Bridge;
  menuMounts: number;
  menuCleanups: number;
  savedBridge?: Bridge;
};

async function openReview(page: Page) {
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    const parts = new URL(route.request().url()).pathname.split('/');
    await route.fulfill({ json: {
      question_id: parts[parts.indexOf('questions') + 1], status: 'complete', error: null,
      messages: [{ role: 'assistant', content: '测试 AI 回复\n\n' + '正文段落。\n\n'.repeat(30), created_at: 1 }],
    } });
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  const region = page.getByTestId('question-bank-region').filter({ hasText: '国考' });
  if (!(await region.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) await region.click();
  const year = page.getByTestId('question-bank-year').filter({ hasText: '2008' });
  if (!(await year.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) await year.click();
  await page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' }).click();
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button', { name: '确认交卷' }).click();
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('测试 AI 回复');
  await expect.poll(() => page.evaluate(() => Boolean((window as InstrumentedWindow).hyzThingsUi))).toBe(true);
  await page.evaluate(() => {
    const w = window as InstrumentedWindow;
    const bridge = w.hyzThingsUi!;
    const mount = bridge.mountFloatingMenu;
    w.menuMounts = 0;
    w.menuCleanups = 0;
    bridge.mountFloatingMenu = (reference, floating) => {
      w.menuMounts++;
      const cleanup = mount(reference, floating);
      return () => { w.menuCleanups++; cleanup(); };
    };
  });
}

async function expectPositioned(page: Page) {
  const menu = page.getByTestId('ai-reply-menu');
  await expect(menu).toBeVisible();
  await expect.poll(() => menu.evaluate(node => (node as HTMLElement).style.left)).not.toBe('');
  const rect = await menu.boundingBox();
  expect(rect).not.toBeNull();
  const viewport = page.viewportSize()!;
  expect(rect!.x).toBeGreaterThanOrEqual(0);
  expect(rect!.x + rect!.width).toBeLessThanOrEqual(viewport.width + 1);
  expect(rect!.y).toBeGreaterThanOrEqual(0);
  expect(rect!.y + rect!.height).toBeLessThanOrEqual(viewport.height + 1);
}

test.beforeEach(async ({ request }) => { await resetHarness(request); });

for (const viewport of [{ width: 1280, height: 800 }, { width: 390, height: 844 }]) {
  test(`AI reply menu stays positioned and releases its observers at ${viewport.width}px`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await openReview(page);
    const trigger = page.getByRole('button', { name: '回复操作' }).first();
    await trigger.scrollIntoViewIfNeeded();
    await trigger.click();
    await expectPositioned(page);
    await expect.poll(() => page.evaluate(() => (window as InstrumentedWindow).menuMounts)).toBe(1);
    const scrolled = await trigger.evaluate(node => {
      let parent = node.parentElement;
      while (parent) {
        const style = getComputedStyle(parent);
        if (/(auto|scroll)/.test(style.overflowY) && parent.scrollHeight > parent.clientHeight + 10) {
          const before = parent.scrollTop;
          parent.scrollTop += before >= 20 ? -20 : 20;
          if (parent.scrollTop !== before) return true;
        }
        parent = parent.parentElement;
      }
      return false;
    });
    expect(scrolled).toBe(true);
    await expectPositioned(page);
    await page.getByRole('button', { name: '查看源码' }).click();
    await expect(page.getByTestId('question-bank-ai-source')).toContainText('测试 AI 回复');
    await expect(page.getByTestId('ai-reply-menu')).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => (window as InstrumentedWindow).menuCleanups)).toBe(1);
    await trigger.click();
    await expectPositioned(page);
    await page.getByTestId('question-bank-original-tab').click();
    await expect(page.getByTestId('ai-reply-menu')).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => (window as InstrumentedWindow).menuCleanups)).toBe(2);
  });
}

test('closing before the bridge becomes ready releases a late menu mount', async ({ page }) => {
  await openReview(page);
  await page.evaluate(() => {
    const w = window as InstrumentedWindow;
    w.savedBridge = w.hyzThingsUi;
    delete w.hyzThingsUi;
  });
  const trigger = page.getByRole('button', { name: '回复操作' }).first();
  await trigger.scrollIntoViewIfNeeded();
  await trigger.click();
  await expect(page.getByTestId('ai-reply-menu')).toBeVisible();
  await trigger.click();
  await expect(page.getByTestId('ai-reply-menu')).toHaveCount(0);
  await page.evaluate(() => {
    const w = window as InstrumentedWindow;
    w.hyzThingsUi = w.savedBridge;
    window.dispatchEvent(new Event('hyz-things-ui-ready'));
  });
  await expect.poll(() => page.evaluate(() => (window as InstrumentedWindow).menuMounts)).toBe(1);
  await expect.poll(() => page.evaluate(() => (window as InstrumentedWindow).menuCleanups)).toBe(1);
});
