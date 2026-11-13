import { expect, test } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

// The dedicated 申论 workspace is tested against the subjective wire contract,
// not the legacy objective A-Z practice API.
test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

const paperId = '019a3d8c-7f23-7b6e-927d-3c951851d3a2';
const questionId = '019a3d8e-1b32-7a60-9c83-f34217a6e210';

test('shenlun training opens materials, saves an independent draft and restores a submitted answer', async ({ page }) => {
  let attempt: Record<string, unknown> | null = null;
  let versionedSaves = 0;
  let submissions = 0;
  await loginAsAdmin(page);
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const method = route.request().method();
    if (path.endsWith('/shenlun/papers') && method === 'GET') {
      return route.fulfill({ json: [{
        id: paperId, title: '申论真题端到端样卷', year: 2025, region: '国考', variant: '地市级',
        subject: 'shenlun', role: 'primary', kind: 'exam',
        collection_id: null, collection_title: null, questions: 1,
      }] });
    }
    if (path.endsWith('/shenlun/papers/' + paperId) && method === 'GET') {
      return route.fulfill({ json: {
        id: paperId, title: '申论真题端到端样卷', year: 2025, region: '国考', variant: '地市级',
        subject: 'shenlun', role: 'primary', kind: 'exam', collection_id: null, collection_title: null,
        materials: [{ id: 'material-1', name: '给定资料 1', markdown: '推进基层治理，需要明确责任。' }],
        questions: [{
          id: questionId, number: 1, stem: '请概括基层治理的要求。', options_markdown: '',
          materials: ['material-1'], module: null, category: null, topics: [],
          answer_type: 'subjective', question_type: 'summary', grading_mode: 'ai', max_score: 20,
        }],
      } });
    }
    if (path.endsWith('/shenlun/attempts') && method === 'GET') {
      expect(url.searchParams.get('paper_id')).toBe(paperId);
      expect(url.searchParams.get('question_id')).toBe(questionId);
      return route.fulfill({ json: attempt ? [attempt] : [] });
    }
    if (path.endsWith('/shenlun/attempts') && method === 'POST') {
      const input = route.request().postDataJSON();
      expect(input.paper_id).toBe(paperId);
      expect(input.question_id).toBe(questionId);
      expect(input.request_id).toMatch(/^[a-f0-9-]{36}$/);
      attempt = {
        id: input.request_id, paper_id: paperId, question_id: questionId,
        source_hash: 'test-source', text: '', source_kind: 'typed',
        status: 'draft', revision: 0, created_at_ms: 1000, updated_at_ms: 1000, submitted_at_ms: null,
      };
      return route.fulfill({ status: 201, json: attempt });
    }
    if (path.endsWith('/draft') && method === 'POST') {
      const input = route.request().postDataJSON();
      expect(attempt).not.toBeNull();
      expect(input.expected_revision).toBe(attempt!.revision);
      attempt = { ...attempt!, revision: (attempt!.revision as number) + 1, text: input.text };
      versionedSaves++;
      return route.fulfill({ json: attempt });
    }
    if (path.endsWith('/submit') && method === 'POST') {
      expect(attempt).not.toBeNull();
      expect((attempt!.text as string).trim()).not.toBe('');
      attempt = { ...attempt!, status: 'submitted', submitted_at_ms: 3000 };
      submissions++;
      return route.fulfill({ json: attempt });
    }
    throw new Error('Unexpected 申论 API call: ' + method + ' ' + path);
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  await expect(page.getByTestId('shenlun-catalog')).toBeVisible();
  await page.getByTestId('shenlun-open-paper').click();
  await expect(page.getByTestId('shenlun-materials')).toContainText('推进基层治理');
  await expect(page.getByTestId('shenlun-question')).toContainText('请概括基层治理的要求');
  await page.getByTestId('shenlun-start').click();
  const answer = page.getByTestId('shenlun-answer-input');
  await answer.fill('加强基层治理，需要明确责任分工。');
  await expect(page.getByText('草稿已保存')).toBeVisible();
  expect(versionedSaves).toBeGreaterThan(0);
  await page.getByTestId('shenlun-submit').click();
  await expect(page.getByText('已保存正式作答。AI 评分功能尚未接入。')).toBeVisible();
  expect(submissions).toBe(1);
  await page.reload();
  await page.getByTestId('training-mode-shenlun').click();
  await page.getByTestId('shenlun-open-paper').click();
  await expect(page.getByTestId('shenlun-answer-input')).toHaveValue('加强基层治理，需要明确责任分工。');
  await expect(page.getByTestId('shenlun-answer-input')).toBeDisabled();
});
