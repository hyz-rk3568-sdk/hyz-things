import { expect, test, type Page } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

const paperId = '019a3d8c-7f23-7b6e-927d-3c951851d3a2';
const ids = [
  '019a3d8e-1b32-7a60-9c83-f34217a6e210',
  '019a3d8e-1b32-7a60-9c83-f34217a6e211',
  '019a3d8e-1b32-7a60-9c83-f34217a6e212',
];
const materialId = '019a3d8d-4a21-739a-8b15-7c54e259b610';
const sessionId = '019a3d8f-1b32-7a60-9c83-f34217a6e210';

type Item = { question_id: string; paper_id: string; source_hash: string; number: number;
  answer: string[]; grade: string|null; gradable: boolean; };

function sessionFixture() {
  const items: Item[] = ids.map((id, i) => ({
    question_id: id, paper_id: paperId, source_hash: 'test-hash',
    number: 26 + i, answer: [], grade: null, gradable: true,
  }));
  return {
    id: sessionId, mode: 'paper_module', paper_id: paperId, module: '资料分析',
    status: 'active', created_at: 1234, updated_at: 1234, elapsed_seconds: 0,
    paused: false, revision: 0, items, score: null as any,
  };
}

async function bankFixture(page: Page) {
  const session = sessionFixture();
  const beforeSubmission: string[] = [];
  let submitted = false;
  let favorite = false;
  const questions = ids.map((id, i) => ({
    item: session.items[i], paper_title: '2022 年示例卷',
    question: {
      id, number: 26 + i, stem: `第 ${26+i} 题题干`,
      options_markdown: '- A. 甲\\n- B. 乙\\n- C. 丙\\n- D. 丁',
      materials: i === 2 ? [] : [materialId], module: '资料分析',
      category: null, topics: ['现期比重', '数据比较'], issues: [],
    },
    materials: i === 2 ? [] : [{ id: materialId, name: 'M1',
      markdown: '共享资料\\n\\n' + '这是用于独立滚动的长材料。\\n\\n'.repeat(80) }],
    content_changed: false,
  }));
  await page.route('**/api/v1/question-bank/**', async route => {
    const path = new URL(route.request().url()).pathname;
    const method = route.request().method();
    let result: any = {};
    if (path === '/api/v1/question-bank/papers') {
      result = [{ id: paperId, title: '2022 年示例卷', year: 2022,
        region: '示例地区', variant: 'A', role: 'primary', questions: 3 }];
    } else if (path.endsWith('/modules')) {
      result = { modules: [{ name: '资料分析', count: 3, ungradable: 0 }], unclassified: 0 };
    } else if (path.endsWith('/history')) {
      result = submitted ? [session] : [];
    } else if (path.endsWith('/practice/sessions') && method === 'POST') {
      const request = route.request().postDataJSON();
      expect(request.paper_id).toBe(paperId);
      expect(['paper', 'paper_module']).toContain(request.mode);
      if (request.mode === 'paper_module') expect(request.module).toBe('资料分析');
      session.mode = request.mode;
      session.module = request.module ?? null;
      result = session;
    } else if (path === `/api/v1/question-bank/practice/sessions/${sessionId}` && method === 'GET') {
      result = { session, questions };
      if (!submitted) beforeSubmission.push(JSON.stringify(result));
    } else if (path.endsWith('/review')) {
      expect(submitted).toBe(true);
      result = { session, questions: questions.map((q, i) => ({
        question: q, correct_answer: [i === 2 ? 'A' : 'B'], explanation: `第 ${i+1} 题原始解析。`,
      })) };
    } else if (path.includes('/answers/') && method === 'POST') {
      const id = path.split('/').at(-1)!;
      const item = session.items.find(item => item.question_id === id)!;
      item.answer = route.request().postDataJSON().answer;
      session.revision += 1;
      result = session;
    } else if (path.endsWith('/pause') && method === 'POST') {
      const request = route.request().postDataJSON();
      session.paused = request.paused;
      session.elapsed_seconds = request.elapsed_seconds;
      session.revision += 1;
      result = session;
    } else if (path.endsWith('/submit') && method === 'POST') {
      submitted = true;
      session.status = 'completed';
      session.paused = true;
      session.items[0].grade = 'correct';
      session.items[1].grade = 'wrong';
      session.items[2].grade = 'unanswered';
      session.score = { total: 3, correct: 1, wrong: 1, unanswered: 1, ungradable: 0 };
      result = session;
    } else if (path.includes('/favorites/')) {
      if (method === 'POST') favorite = route.request().postDataJSON().favorite;
      result = { question_id: path.split('/').at(-1), favorite };
    } else if (path.endsWith('/sync/status')) {
      result = { task_id: null, stage: 'idle', running: false, summary: null, error_code: null };
    } else {
      throw new Error(`unexpected bank request ${method} ${path}`);
    }
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(result) });
  });
  return { session, beforeSubmission };
}

test('original-paper module keeps all questions in order, shared material and scoring behind submit', async ({ page }) => {
  const fixture = await bankFixture(page);
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank').click();
  await page.getByRole('button', { name: '原卷模块' }).click();
  await page.getByTestId('question-bank-select-paper-item').click();
  await page.getByTestId('question-bank-module-list').getByRole('button', { name: '资料分析 · 3题' }).click();
  await page.getByRole('button', { name: '开始练习' }).click();
  const pane = page.getByTestId('question-bank-question-pane');
  await expect(pane).toContainText('第 26 题题干');
  await expect(page.getByTestId('question-bank-material-pane')).toBeVisible();
  expect(fixture.session.items.map(item => item.number)).toEqual([26, 27, 28]);
  expect(fixture.beforeSubmission.join('')).not.toContain('correct_answer');
  expect(fixture.beforeSubmission.join('')).not.toContain('explanation');

  const scroll = page.getByTestId('question-bank-material-scroll');
  await scroll.evaluate(element => { element.scrollTop = 180; });
  await page.getByTestId('question-bank-option-B').click();
  await expect(page.getByTestId('question-bank-option-B')).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: '下一题' }).click();
  await expect(pane).toContainText('第 27 题题干');
  expect(await scroll.evaluate(element => element.scrollTop)).toBeGreaterThan(0);
  await page.getByTestId('question-bank-option-C').click();

  await page.getByRole('button', { name: '暂停', exact: true }).click();
  await expect(page.getByRole('button', { name: '恢复' })).toBeVisible();
  await expect(page.getByTestId('question-bank-option-A')).toBeDisabled();
  await page.getByRole('button', { name: '恢复' }).click();
  await page.getByRole('button', { name: '下一题' }).click();
  await expect(page.getByTestId('question-bank-single-pane')).toBeVisible();
  await expect(pane).toContainText('第 28 题题干');

  page.once('dialog', dialog => dialog.accept());
  await page.getByRole('button', { name: '交卷' }).click();
  await expect(page.getByTestId('question-bank-score')).toContainText('正确 1');
  await expect(page.getByTestId('question-bank-score')).toContainText('未答 1');
  await expect(page.getByTestId('question-bank-explanation')).toContainText('第 3 题原始解析');
  await page.getByRole('button', { name: '☆ 收藏' }).click();
  await expect(page.getByRole('button', { name: '★ 已收藏' })).toBeVisible();
});

test('single original paper starts without a fixed question count or random cross-paper mode', async ({ page }) => {
  await bankFixture(page);
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank').click();
  await expect(page.getByTestId('question-bank-practice-modes')).not.toContainText('随机');
  await page.getByTestId('question-bank-select-paper-item').click();
  await page.getByRole('button', { name: '开始练习' }).click();
  await expect(page.getByTestId('question-bank-navigation').getByRole('button')).toHaveCount(3);
  await expect(page.getByTestId('question-bank-question-pane')).toContainText('第 26 题题干');
});
