import { expect, test } from '@playwright/test';
import { expectNoHorizontalOverflow, resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

for (const viewport of [{ width: 1180, height: 820 }, { width: 390, height: 844 }]) {
  test(`analysis review expands its method card and handles legacy papers at ${viewport.width}px`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const now = Date.now();
    const materials = Array.from({ length: 4 }, (_, i) => ({ topic: `材料 ${i + 1}`, text: '业务量1200亿元，同比增长20%。\n\n'.repeat(30) }));
    const questions = Array.from({ length: 20 }, (_, i) => ({
      number: i + 1, question: `第 ${i + 1} 题`, options: ['1000', '1100', '1200', '1300'], chosen: 0, correct_index: 0,
      explanation: `第 ${i + 1} 题解析`, skill: '基期量', evidence: '1200亿元，增长20%',
      card_path: '资料/基期量/cards.md', card_id: `card-${i}`,
      ...(i === 0 ? { related_card: { front: '基期量方法 $B=\\frac{A}{1+r}$', back: '**先判断基期**，再计算 $1200/1.2=1000$。' } } : {}),
    }));
    const writes: string[] = [];
    await page.route('**/api/v1/study/data-analysis/**', async route => {
      const path = new URL(route.request().url()).pathname;
      if (route.request().method() !== 'GET') writes.push(path);
      let data: unknown = [];
      if (path.endsWith('/status')) data = { configured: true, model: 'fixture', preparing_job_id: null, answer_summary: { answered_count: 0, correct_count: 0 }, card_stats: [] };
      else if (path.endsWith('/user-prompt')) data = { user_prompt: '', customized: false };
      else if (path.endsWith('/history')) data = [{ id: 'card-review', started_at_ms: now, elapsed_ms: 1000, correct: 20, total: 20, topics: ['材料'] }];
      else if (path.endsWith('/result')) data = { id: 'card-review', correct: 20, total: 20, materials, questions };
      await route.fulfill({ json: data });
    });
    await page.goto('/#/study/training');
    await page.getByTestId('training-mode-comprehensive').click();
    await page.getByRole('button', { name: '查看练习历史', exact: true }).click();
    await page.getByRole('button', { name: '查看原题与解析', exact: true }).click();
    const card = page.getByTestId('analysis-related-card');
    await expect(card).toBeVisible();
    await expect(card).not.toHaveAttribute('open', '');
    await card.locator('summary').click();
    await expect(card).toContainText('基期量方法');
    await expect(card).toContainText('先判断基期');
    await expect(card.locator('.katex').first()).toBeVisible();
    await expect(card.locator('strong')).toHaveText('先判断基期');
    await expectNoHorizontalOverflow(page);
    await card.locator('summary').click();
    await expect(page.getByTestId('analysis-explanation')).toHaveText('第 1 题解析');
    await page.getByRole('button', { name: '下一题', exact: true }).click();
    await card.locator('summary').click();
    await expect(card).toContainText('此答卷未保存关联卡片内容');
    await expect(card).not.toContainText('基期量方法');
    await expect(page.getByTestId('analysis-explanation')).toHaveText('第 2 题解析');
    expect(writes).toEqual([]);
  });
}

for (const [prefix, mode, endpoint] of [
  ['logic', 'logic-judgment', 'logic-judgment'],
  ['quant', 'quantitative', 'quantitative-relations'],
]) {
  test(`${prefix} review uses the shared method-card disclosure`, async ({ page }) => {
    const now = Date.now();
    await page.route(`**/api/v1/study/${endpoint}/**`, async route => {
      const path = new URL(route.request().url()).pathname;
      expect(route.request().method()).toBe('GET');
      let data: unknown = [];
      if (path.endsWith('/status')) data = { configured: true, eligible_cards: 20, preparing_job_id: null };
      else if (path.endsWith('/user-prompt')) data = { user_prompt: '', customized: false };
      else if (path.endsWith('/history')) data = [{ id: 'shared-card', correct: 10, answered: 10, total: 10, ended_at_ms: now }];
      else if (path.endsWith('/result')) data = {
        id: 'shared-card', correct: 10, total: 10,
        questions: Array.from({ length: 10 }, (_, i) => ({
          question: `第 ${i + 1} 题`, options: ['甲', '乙', '丙', '丁'], chosen: 0, correct_index: 0,
          explanation: '解析', card_path: '数量/经济利润/cards.md',
          front: `第 ${i + 1} 张方法卡 $A=B$`, back: '**关联卡片正文**',
        })),
      };
      await route.fulfill({ json: data });
    });
    await page.goto('/#/study/training');
    await page.getByTestId(`training-mode-${mode}`).click();
    await page.getByTestId(`${prefix}-history-toggle`).click();
    await page.getByRole('button', { name: '查看原题与解析', exact: true }).click();
    const card = page.getByTestId(`${prefix}-related-card`);
    await card.locator('summary').click();
    await expect(card).toContainText('第 1 张方法卡');
    await expect(card.locator('.katex').first()).toBeVisible();
    await expect(card.locator('strong')).toHaveText('关联卡片正文');
    await card.locator('summary').click();
    await page.getByRole('button', { name: '下一题', exact: true }).click();
    await expect(card).not.toHaveAttribute('open', '');
    await card.locator('summary').click();
    await expect(card).toContainText('第 2 张方法卡');
  });
}
