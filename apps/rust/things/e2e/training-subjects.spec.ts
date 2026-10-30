import { expect, test, type Page } from '@playwright/test';
import { resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

async function subjectFixture(page: Page, logicPromptGate?: Promise<void>) {
  const now = Date.now();
  const reads: string[] = [];
  await page.route('**/api/v1/study/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (!/logic-judgment|quantitative-relations/.test(path)) return route.continue();
    expect(route.request().method()).toBe('GET');
    reads.push(path);
    const quantitative = path.includes('quantitative-relations');
    const subject = quantitative ? '数量' : '判断';
    const run = {
      id: quantitative ? 'quant-paper' : 'logic-paper', status: 'active',
      deadline_at_ms: now + (quantitative ? 900_000 : 600_000), paused_remaining_ms: null,
      server_now_ms: now, duration_seconds: quantitative ? 900 : 600,
      questions: Array.from({ length: 10 }, (_, i) => ({ question: `${subject}题 ${i + 1}`, options: ['甲', '乙', '丙', '丁'] })),
      answers: Array(10).fill(null),
    };
    let data: unknown = [];
    if (path.endsWith('/status')) data = { configured: true, eligible_cards: quantitative ? 22 : 11, preparing_job_id: null };
    else if (path.endsWith('/user-prompt')) {
      if (!quantitative && logicPromptGate) await logicPromptGate;
      data = { user_prompt: `${subject}专用提示`, customized: true };
    }
    else if (path.endsWith('/unfinished') || path.includes('/runs/')) data = path.endsWith('/unfinished') ? [run] : run;
    else if (path.endsWith('/history')) data = [{ id: `${subject}-history`, correct: quantitative ? 8 : 3, answered: 10, total: 10, ended_at_ms: now }];
    else if (path.endsWith('/card-performance')) data = [{
      card_id: `${subject}-card`, category: subject, card_path: `${subject}/cards.md`,
      front: `${subject}方法卡`, back: `${subject}解题方法`, answered_count: 10, correct_count: quantitative ? 8 : 3,
    }];
    await route.fulfill({ json: data });
  });
  return reads;
}

test('training subjects reload their own prompts, cards, history and unfinished papers on every switch', async ({ page }) => {
  const reads = await subjectFixture(page);
  await page.goto('/#/study/training');
  for (const subject of ['logic', 'quant', 'logic', 'quant']) {
    const quantitative = subject === 'quant';
    await page.getByTestId(`training-mode-${quantitative ? 'quantitative' : 'logic-judgment'}`).click();
    const setup = page.getByTestId(`${subject}-setup`);
    await expect(setup).toContainText(`当前有效卡片：${quantitative ? 22 : 11} 张`);
    await expect(page.getByTestId(`${subject}-unfinished`)).toBeVisible();
    const prompt = page.getByTestId(`${subject}-user-prompt`);
    await prompt.locator('summary').click();
    await expect(prompt.locator('textarea')).toHaveValue(`${quantitative ? '数量' : '判断'}专用提示`);
    await page.getByTestId(`${subject}-statistics`).locator('summary').click();
    await expect(page.getByTestId(`${subject}-card-item`)).toContainText(`${quantitative ? '数量' : '判断'}方法卡`);
    await page.getByTestId(`${subject}-history-toggle`).click();
    await expect(page.getByTestId(`${subject}-history`)).toContainText(`${quantitative ? 8 : 3}/10题`);
    await page.getByRole('button', { name: '返回训练', exact: true }).click();
  }
  await page.getByTestId('quant-resume').click();
  await expect(page.getByTestId('quant-question')).toHaveText('数量题 1');
  await expect(page.getByTestId('quant-clock')).toHaveText('15:00');
  expect(reads.filter(path => path === '/api/v1/study/logic-judgment/status')).toHaveLength(2);
  expect(reads.filter(path => path === '/api/v1/study/quantitative-relations/status')).toHaveLength(2);
  expect(reads.filter(path => path.includes('/runs/'))).toEqual(['/api/v1/study/quantitative-relations/runs/quant-paper']);
});

test('late replies from the previous training subject cannot overwrite the selected subject', async ({ page }) => {
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await subjectFixture(page, gate);
  await page.goto('/#/study/training');
  const logicPrompt = page.waitForRequest('**/logic-judgment/user-prompt');
  await page.getByTestId('training-mode-logic-judgment').click();
  await logicPrompt;
  await page.getByTestId('training-mode-quantitative').click();
  await expect(page.getByTestId('quant-setup')).toContainText('当前有效卡片：22 张');
  const lateReply = page.waitForResponse('**/logic-judgment/user-prompt');
  release();
  await lateReply;
  await page.getByTestId('quant-user-prompt').locator('summary').click();
  await expect(page.getByTestId('quant-user-prompt').locator('textarea')).toHaveValue('数量专用提示');
  await expect(page.getByTestId('quant-setup')).toContainText('当前有效卡片：22 张');
  await expect(page.getByTestId('logic-setup')).toHaveCount(0);
});
