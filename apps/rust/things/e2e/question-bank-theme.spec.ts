import { expect, test, type Locator, type Page } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

const themeExpect = expect.configure({ soft: true, timeout: 500 });

test.beforeEach(async ({ request }) => { await resetHarness(request); });

async function themeColors(page: Page) {
  return page.evaluate(() => {
    const probe = document.createElement('span');
    document.body.append(probe);
    const colors: Record<string, string> = {};
    for (const token of ['primary', 'primary-content', 'base-100', 'base-200', 'base-content']) {
      probe.style.color = `var(--color-${token})`;
      colors[token] = getComputedStyle(probe).color;
    }
    probe.remove();
    return colors;
  });
}

async function primaryButton(button: Locator, colors: Record<string, string>) {
  await button.page().mouse.move(0, 0);
  await themeExpect(button).toHaveCSS('background-color', colors.primary);
  await themeExpect(button).toHaveCSS('color', colors['primary-content']);
}

async function trainingChrome(page: Page, mode: string, colors: Record<string, string>) {
  const tab = page.getByTestId(`training-mode-${mode}`);
  await expect(tab).toHaveAttribute('aria-pressed', 'true');
  await themeExpect(tab).toHaveCSS('color', colors.primary);
  await themeExpect(tab).toHaveCSS('border-bottom-color', colors.primary);
  await themeExpect(page.getByTestId('study-time-inline')).toHaveCSS('color', colors.primary);
  for (const selector of ['.training-topbar', '.training-mode-tabs']) {
    const background = await page.locator(selector).evaluate(node => {
      let current: Element | null = node;
      while (current) {
        const color = getComputedStyle(current).backgroundColor;
        if (color !== 'rgba(0, 0, 0, 0)') return color;
        current = current.parentElement;
      }
      throw new Error('Training background is missing');
    });
    themeExpect(background).toBe(colors['base-200']);
  }
}

for (const colorScheme of ['dark', 'light'] as const) {
  test(`${colorScheme} question bank inherits the application theme across training, answers, history and favorites`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await loginAsAdmin(page);
    await page.goto('/#/study/training');
    await page.getByTestId('training-mode-calculation').click();
    const colors = await themeColors(page);
    expect(colors.primary).not.toBe(colors['primary-content']);
    expect(colors['base-100']).not.toBe(colors['base-content']);
    await trainingChrome(page, 'calculation', colors);

    await page.getByTestId('training-mode-question-bank-paper').click();
    await trainingChrome(page, 'question-bank-paper', colors);
    for (const [id, text] of [['question-bank-region', '国考'], ['question-bank-year', '2008']]) {
      const summary = page.getByTestId(id).filter({ hasText: text });
      if (!(await summary.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) {
        await summary.click();
      }
    }
    await page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' }).click();
    await primaryButton(page.getByTestId('question-bank-practice-start'), colors);

    await page.getByTestId('training-mode-question-bank-module').click();
    await trainingChrome(page, 'question-bank-module', colors);
    await page.getByTestId('question-bank-cross-module').filter({ hasText: '资料分析' }).click();
    const category = page.getByTestId('question-bank-cross-category').filter({ hasText: '比重' });
    await category.click();
    await expect(category).toHaveAttribute('aria-pressed', 'true');
    await primaryButton(category, colors);
    await primaryButton(page.getByTestId('question-bank-practice-start'), colors);
    await page.getByTestId('question-bank-practice-start').click();
    const choices = page.getByTestId('question-bank-practice-choice');
    await expect(choices.first()).toBeVisible();
    await themeExpect(choices.first()).toHaveCSS('background-color', colors['base-100']);
    await themeExpect(choices.first()).toHaveCSS('color', colors['base-content']);
    await choices.first().click();
    await expect(choices.first()).toHaveAttribute('aria-pressed', 'true');
    await primaryButton(choices.first(), colors);
    await themeExpect(page.getByTestId('study-time-inline')).toHaveCSS('color', colors.primary);
    await themeExpect(page.getByTestId('question-bank-practice-timer')).toHaveCSS('color', colors['base-content']);
    const favorite = page.getByTestId('question-bank-favorite');
    if (await favorite.getAttribute('aria-label') === '收藏题目') await favorite.click();
    await expect(favorite).toHaveAttribute('aria-label', '取消收藏');
    await themeExpect(favorite).toHaveCSS('color', colors['base-content']);
    await page.getByRole('button', { name: /暂停并退出/ }).click();

    await page.getByTestId('question-bank-history-open').click();
    await expect(page.getByTestId('question-bank-practice-history-item').first()).toBeVisible();
    await primaryButton(page.getByTestId('question-bank-history-filter-all'), colors);
    await themeExpect(page.getByTestId('question-bank-practice-history-item').first()).toHaveCSS('background-color', colors['base-100']);
    await themeExpect(page.getByTestId('question-bank-practice-history-item').first()).toHaveCSS('color', colors['base-content']);
    await page.getByRole('button', { name: '返回模块', exact: true }).click();
    await page.getByTestId('question-bank-collection-favorite').click();
    await expect(page.getByTestId('question-bank-collection-item')).toBeVisible();
    await primaryButton(page.getByTestId('question-bank-practice-start'), colors);
    await themeExpect(page.getByTestId('question-bank-collection-item')).toHaveCSS('color', colors['base-content']);
    await page.getByTestId('question-bank-practice-start').click();
    await expect(favorite).toHaveAttribute('aria-label', '取消收藏');
    await favorite.click();
    await expect(favorite).toHaveAttribute('aria-label', '收藏题目');
  });
}
