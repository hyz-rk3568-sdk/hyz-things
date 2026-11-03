import { expect, test, type Page } from '@playwright/test';
import { expectNoHorizontalOverflow, loginAsAdmin, resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

async function loginWithIsolatedHistory(page: Page) {
  await loginAsAdmin(page);
  // The harness reset preserves the SQLite practice store between tests.
  const previousIds = await page.evaluate(async () => {
    const response = await fetch('/api/v1/question-bank/practice/history');
    return (await response.json() as { id: string }[]).map(run => run.id);
  });
  await page.route('**/api/v1/question-bank/practice/history', async route => {
    const response = await route.fetch();
    const runs = await response.json() as { id: string }[];
    await route.fulfill({ response, json: runs.filter(run => !previousIds.includes(run.id)) });
  });
}

async function completeFixturePaper(page: Page) {
  await loginWithIsolatedHistory(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  const region = page.getByTestId('question-bank-region').filter({ hasText: '国考' });
  if (!(await region.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) {
    await region.click();
  }
  await page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' }).click();
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-choice').first().click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed', 'true');
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button', { name: '确认交卷', exact: true }).click();
  await expect(page.getByTestId('question-bank-practice-score')).toContainText('错误 1');
  await page.getByRole('button', { name: '← 返回训练', exact: true }).click();
}

test('question bank history shares paper and module records, filters unfinished work, and repeats a completed paper', async ({ page }) => {
  await completeFixturePaper(page);
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({ hasText: '资料分析' }).click();
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByRole('button', { name: '← 暂停并退出', exact: true }).click();
  await page.getByTestId('question-bank-history-open').click();
  const records = page.getByTestId('question-bank-practice-history-item');
  await expect(records).toHaveCount(2);
  await expect(records.filter({ hasText: '模块训练' })).toContainText('已暂停');
  await page.getByTestId('question-bank-history-unfinished').click();
  await expect(records).toHaveCount(1);
  await expect(records).toContainText('模块训练');
  await records.getByRole('button', { name: '继续练习', exact: true }).click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.getByRole('button', { name: '继续练习', exact: true }).click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toBeVisible();
  await page.getByRole('button', { name: '← 暂停并退出', exact: true }).click();
  await page.getByTestId('question-bank-history-unfinished').click();
  await page.getByTestId('question-bank-history-filter-wrong').click();
  await expect(records).toHaveCount(0);
  await expect(page.getByTestId('question-bank-history-empty')).toBeVisible();
  await page.getByTestId('question-bank-history-filter-paper').click();
  await expect(records).toHaveCount(1);
  await records.getByRole('button', { name: '查看原题与解析', exact: true }).click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  await page.getByRole('button', { name: '← 返回训练', exact: true }).click();
  await records.getByRole('button', { name: '再做一遍', exact: true }).click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed', 'false');
  await expect(page.getByTestId('question-bank-practice-score')).toHaveCount(0);
  await page.getByRole('button', { name: '← 暂停并退出', exact: true }).click();
  await page.getByRole('button', { name: '返回模块', exact: true }).click();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await page.getByTestId('question-bank-history-open').click();
  await expect(records).toHaveCount(3);
  await page.getByTestId('question-bank-history-filter-module').click();
  await expect(records).toHaveCount(1);
  await expect(records).toContainText('模块训练');
});

test('question bank repeat keeps the recorded question set instead of resampling', async ({ page }) => {
  await loginWithIsolatedHistory(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({ hasText: '资料分析' }).click();
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button', { name: '确认交卷', exact: true }).click();
  await expect(page.getByTestId('question-bank-practice-score')).toBeVisible();
  const completed = await page.evaluate(async () => {
    const response = await fetch('/api/v1/question-bank/practice/history');
    const runs = await response.json() as {
      id: string; status: string; items: { question_id: string; paper_id: string }[];
    }[];
    const run = runs.find(candidate => candidate.status === 'completed')!;
    return { id: run.id, items: run.items };
  });
  await page.getByRole('button', { name: '← 返回训练', exact: true }).click();
  await page.getByTestId('question-bank-history-open').click();
  await page.getByTestId('question-bank-history-filter-module').click();
  const repeated = page.waitForRequest(request =>
    request.method() === 'POST' && request.url().endsWith(`/practice/sessions/${completed.id}/repeat`));
  await page.getByRole('button', { name: '再做一遍', exact: true }).click();
  await repeated;
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  const copied = await page.evaluate(async () => {
    const response = await fetch('/api/v1/question-bank/practice/history');
    const runs = await response.json() as {
      status: string; items: { question_id: string; paper_id: string; number: number; answer: string[]; grade: string | null }[];
    }[];
    return runs.find(run => run.status === 'active')!.items;
  });
  const shape = (items: { question_id: string; paper_id: string; number: number }[]) =>
    items.map(({ question_id, paper_id, number }) => ({ question_id, paper_id, number }));
  expect(shape(copied)).toEqual(shape(completed.items));
  expect(copied.every(item => item.answer.length === 0 && item.grade === null)).toBe(true);
});

test('question bank history shows a failed repeat and allows retrying the same record', async ({ page }) => {
  await completeFixturePaper(page);
  await page.getByTestId('question-bank-history-open').click();
  const endpoint = '**/api/v1/question-bank/practice/sessions/*/repeat';
  await page.route(endpoint, route => route.fulfill({
    status: 503, json: { error: 'temporary-unavailable' },
  }));
  const repeat = page.getByRole('button', { name: '再做一遍', exact: true });
  await repeat.click();
  await expect(page.getByTestId('question-bank-error')).toBeVisible();
  await expect(repeat).toBeEnabled();
  await page.unroute(endpoint);
  await repeat.click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-error')).toHaveCount(0);
});

test('question bank setup and history keep controls usable on desktop tablet and phone', async ({ page }) => {
  await loginWithIsolatedHistory(page);
  await page.goto('/#/study/training');
  for (const viewport of [{ width: 1440, height: 900 }, { width: 834, height: 1112 }, { width: 390, height: 640 }]) {
    await page.setViewportSize(viewport);
    for (const mode of ['paper', 'module']) {
      await page.getByTestId(`training-mode-question-bank-${mode}`).click();
      const action = page.getByTestId('training-setup-action');
      await expect(action.getByTestId('question-bank-practice-start')).toBeVisible();
      const before = await action.boundingBox();
      await page.getByTestId('training-setup-scroll').evaluate(node => { node.scrollTop = node.scrollHeight; });
      expect(await action.boundingBox()).toEqual(before);
      await expectNoHorizontalOverflow(page);
      await page.getByTestId('question-bank-history-open').click();
      for (const filter of ['all', 'paper', 'module', 'wrong', 'favorite']) {
        const button = page.getByTestId(`question-bank-history-filter-${filter}`);
        await button.click();
        await expect(button).toHaveAttribute('aria-pressed', 'true');
        await expect(page.getByTestId('question-bank-history-empty')).toBeVisible();
      }
      await expectNoHorizontalOverflow(page);
      await page.getByRole('button', { name: mode === 'paper' ? '返回套卷' : '返回模块', exact: true }).click();
      await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
    }
  }
});

test('question bank year disclosure keeps the selected paper when reopened', async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  const paper = page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' });
  await paper.click();
  const year = page.getByTestId('question-bank-year').filter({ hasText: '2008' });
  await year.click();
  await expect(paper).toBeHidden();
  await year.click();
  await expect(paper).toHaveAttribute('aria-pressed', 'true');
});

test('question bank fixed action summarizes the selected paper and module category', async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' }).click();
  await expect(page.getByTestId('training-setup-action')).toContainText('行测真题训练端到端样卷');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-category').filter({ hasText: '比重' }).click();
  await expect(page.getByTestId('training-setup-action')).toContainText('资料分析 · 比重');
});
