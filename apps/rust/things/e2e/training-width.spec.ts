import { expect, test, type Page } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

const widthExpect = expect.configure({ soft: true, timeout: 500 });

async function installTrainingSettings(page: Page) {
  await page.route(/\/api\/v1\/study\/(data-analysis|logic-judgment|quantitative-relations)\/(status|user-prompt|unfinished|history|card-performance)$/, async route => {
    expect(route.request().method()).toBe('GET');
    const path = new URL(route.request().url()).pathname;
    let json: unknown = [];
    if (path.endsWith('/status')) {
      json = {
        configured: true, model: 'width-fixture', eligible_cards: 22, cached_sources: 1,
        verified_groups: 4, available_skills: [], preparing_job_id: null,
      };
    } else if (path.endsWith('/user-prompt')) {
      json = { user_prompt: '基础训练', customized: false };
    }
    await route.fulfill({ json });
  });
}

async function expectSharedWidth(page: Page, mode: string, viewport: { width: number; height: number }) {
  const gutter = viewport.width < 640 ? 16 : 24;
  for (const selector of ['.training-topbar-inner', '.training-modes-inner', '.training-setup-content', '.training-action-inner']) {
    const edges = await page.locator(selector).evaluate(node => {
      const rect = node.getBoundingClientRect();
      const style = getComputedStyle(node);
      return {
        left: rect.left + parseFloat(style.paddingLeft),
        right: innerWidth - rect.right + parseFloat(style.paddingRight),
      };
    });
    const chrome = selector === '.training-topbar-inner' || selector === '.training-modes-inner';
    const contentMax = mode.startsWith('question-bank-') ? 1016 : 936;
    const expected = viewport.width <= 1366 ? gutter
      : chrome ? (viewport.width - 1024) / 2 + gutter : (viewport.width - contentMax) / 2;
    widthExpect(edges.left, `${mode} ${selector} left gutter`).toBeCloseTo(expected, 0);
    widthExpect(edges.right, `${mode} ${selector} right gutter`).toBeCloseTo(expected, 0);
  }
  await widthExpect(page.locator('.training-topbar-inner')).toHaveCSS('height', '56px');
  await widthExpect(page.locator('.training-modes-inner')).toHaveCSS('height', viewport.width < 640 ? '48px' : '52px');
  for (const selector of ['.training-shell', '.training-setup-scroll', '.training-setup-content', '.training-setup-action']) {
    const overflow = await page.locator(selector).evaluate(node => node.scrollWidth - node.clientWidth);
    widthExpect(overflow, `${mode} ${selector} horizontal overflow`).toBeLessThanOrEqual(1);
  }
  const action = page.locator('.training-primary-action button');
  await expect(action).toBeEnabled();
  await expect(action).toBeInViewport({ ratio: 1 });
  await action.click({ trial: true });
}

test('training shell keeps the layout width when iPad reports a stale visual viewport', async ({ page, request }) => {
  await resetHarness(request);
  await page.setViewportSize({ width: 1194, height: 834 });
  await page.addInitScript(() => {
    if (!window.visualViewport) return;
    Object.defineProperty(window.visualViewport, 'width', { configurable: true, get: () => 640 });
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  const shell = await page.getByTestId('training-layout').boundingBox();
  expect(shell!.width).toBeGreaterThan(1150);
  const overflow = await page.getByTestId('training-layout').evaluate(node => node.scrollWidth - node.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});

for (const viewport of [
  { width: 1194, height: 834 },
  { width: 1366, height: 1024 },
  { width: 834, height: 1194 },
  { width: 390, height: 844 },
  { width: 1440, height: 900 },
]) {
  test(`${viewport.width}x${viewport.height} training modes share responsive settings and usable actions`, async ({ page, request }) => {
    await resetHarness(request);
    await page.setViewportSize(viewport);
    await installTrainingSettings(page);
    await loginAsAdmin(page);
    await page.goto('/#/study/training');
    for (const mode of ['calculation', 'question-bank-paper', 'question-bank-module', 'comprehensive', 'logic-judgment', 'quantitative']) {
      const tab = page.getByTestId(`training-mode-${mode}`);
      await tab.click();
      await expect(tab).toHaveAttribute('aria-pressed', 'true');
      await expect(page.locator('.training-setup-content')).toBeVisible();
      if (mode === 'calculation') {
        await page.getByTestId('calculation-count-20').click();
        await expect(page.getByTestId('calculation-count-20')).toHaveAttribute('aria-pressed', 'true');
      } else if (mode === 'question-bank-paper') {
        for (const [id, text] of [['question-bank-region', '国考'], ['question-bank-year', '2008']]) {
          const summary = page.getByTestId(id).filter({ hasText: text });
          if (!(await summary.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) await summary.click();
        }
        const paper = page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' });
        await paper.click();
        await expect(paper).toHaveAttribute('aria-pressed', 'true');
      } else if (mode === 'question-bank-module') {
        await page.getByTestId('question-bank-cross-module').filter({ hasText: '资料分析' }).click();
        const category = page.getByTestId('question-bank-cross-category').filter({ hasText: '比重' });
        await category.click();
        await expect(category).toHaveAttribute('aria-pressed', 'true');
      } else {
        const prefix = mode === 'comprehensive' ? 'analysis' : mode === 'quantitative' ? 'quant' : 'logic';
        const details = page.getByTestId(`${prefix}-user-prompt`);
        await details.locator('summary').click();
        await expect(details.locator('textarea')).toBeVisible();
        await details.locator('textarea').fill('保留基础难度');
        await expect(details.locator('textarea')).toHaveValue('保留基础难度');
        await details.locator('summary').click();
      }
      await expectSharedWidth(page, mode, viewport);
    }
  });
}
