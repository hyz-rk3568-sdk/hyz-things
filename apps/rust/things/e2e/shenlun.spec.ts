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
    if (path.endsWith('/jobs') && method === 'GET') {
      return route.fulfill({ json: [] });
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
  await expect.poll(() => versionedSaves).toBeGreaterThan(0);
  await expect(page.getByText('草稿已保存')).toBeVisible();
  await page.getByTestId('shenlun-submit').click();
  await expect(page.getByText('已保存正式作答，可开始 AI 批改。')).toBeVisible();
  expect(submissions).toBe(1);
  await page.reload();
  await page.getByTestId('training-mode-shenlun').click();
  await page.getByTestId('shenlun-open-paper').click();
  await expect(page.getByTestId('shenlun-answer-input')).toHaveValue('加强基层治理，需要明确责任分工。');
  await expect(page.getByTestId('shenlun-answer-input')).toBeDisabled();
});


const submitted = {
  id: '11111111-1111-4111-8111-111111111111',
  paper_id: paperId, question_id: questionId,
  source_hash: 'test-source', text: '我认为应明确责任。', source_kind: 'typed',
  status: 'submitted', revision: 1, created_at_ms: 1000, updated_at_ms: 2000,
  submitted_at_ms: 2000, reference_viewed_at_ms: null, after_reference: false,
};
const catalog = [{
  id: paperId, title: '申论真题端到端样卷', year: 2025, region: '国考',
  variant: '地市级', subject: 'shenlun', role: 'primary', kind: 'exam',
  collection_id: null, collection_title: null, questions: 1,
}];
const paperView = {
  ...catalog[0], materials: [{ id: 'material-1', name: '给定资料 1', markdown: '推进基层治理，需要明确责任。' }],
  questions: [{
    id: questionId, number: 1, stem: '请概括基层治理的要求。', options_markdown: '',
    materials: ['material-1'], module: null, category: null, topics: [],
    answer_type: 'subjective', question_type: 'summary', grading_mode: 'ai', max_score: 20,
  }],
};

test('shenlun source answers remain gated until explicit grading; model answers remain on demand', async ({ page }) => {
  await loginAsAdmin(page);
  let sourceReads = 0;
  const generated: string[] = [];
  let jobs: any[] = [];
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const method = route.request().method();
    if (path.endsWith('/shenlun/papers') && method === 'GET')
      return route.fulfill({ json: catalog });
    if (path.endsWith('/shenlun/papers/' + paperId) && method === 'GET')
      return route.fulfill({ json: paperView });
    if (path.endsWith('/shenlun/attempts') && method === 'GET')
      return route.fulfill({ json: [submitted] });
    if (path.endsWith('/' + submitted.id + '/jobs') && method === 'GET')
      return route.fulfill({ json: jobs });
    if (path.endsWith('/' + submitted.id + '/jobs') && method === 'POST') {
      const req = route.request().postDataJSON();
      generated.push(req.kind);
      expect(req.attachments).toEqual([]);
      const grade = {
        provisional: true, score: 10, max_score: 20, rubric_version: 'hyz-shenlun-v1',
        summary: '表达清楚，措施稍显笼统。', unscored_reason: null,
        items: [{
          id: 'p1', criterion: '明确责任', material_id: 'material-1',
          evidence: '明确责任', weight: 20, earned: 10, coverage: 'partial',
          quote: '明确责任', start: 12, end: 24,
          reason: '缺少具体主体', suggestion: '写出执行主体',
        }],
      };
      const result = req.kind === 'grade' ? grade : {
        text: 'AI 独立的参考答案。', edits: [], provenance: 'AI 生成，非官方',
      };
      const job = {
        id: req.request_id, attempt_id: submitted.id, kind: req.kind, status: 'complete',
        error: null, result, source_hash: submitted.source_hash,
        attempt_revision: 1, skill_version: 'hyz-shenlun-v1',
        model: 'mock-codex', created_at_ms: 5000, updated_at_ms: 5001,
      };
      jobs.unshift(job);
      return route.fulfill({ status: 202, json: job });
    }
    if (path.endsWith('/source-answer') && method === 'GET') {
      sourceReads++;
      return route.fulfill({ json: {
        text: '来源参考答案：压实基层责任。', provenance: '题库来源参考答案（非官方保证）',
        source_hash: submitted.source_hash,
      } });
    }
    throw new Error('Unexpected mocked shenlun call: ' + method + ' ' + path);
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  await page.getByTestId('shenlun-open-paper').click();
  await expect(page.getByTestId('shenlun-grade-start')).toBeVisible();
  await expect(page.getByText('来源参考答案（免费）')).toHaveCount(0);
  expect(sourceReads).toBe(0);
  expect(generated).toEqual([]);
  await page.getByTestId('shenlun-grade-start').click();
  await expect(page.getByTestId('shenlun-score')).toContainText('10/20');
  await expect(page.getByText('来源参考答案（免费）')).toBeVisible();
  expect(sourceReads).toBe(0);
  await page.getByText('来源参考答案（免费）').click();
  await expect(page.getByTestId('shenlun-source-answer')).toContainText('压实基层责任');
  expect(sourceReads).toBe(1);
  await page.getByText('AI 独立参考答案').click();
  await expect.poll(() => generated).toContain('independent_answer');
  await expect(page.getByText('按需生成完整范文')).toHaveCount(0);
});

test('shenlun image transcription is ungraded until explicit editable confirmation', async ({ page }) => {
  await loginAsAdmin(page);
  const attachmentId = '22222222-2222-4222-8222-222222222222';
  const attempt = {
    ...submitted, id: '33333333-3333-4333-8333-333333333333',
    text: '', status: 'draft', revision: 0, submitted_at_ms: null,
  };
  let jobs: any[] = [];
  let confirmed = false;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/attachments', route =>
    route.fulfill({ json: { id: attachmentId } }),
  );
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const url = new URL(route.request().url());
    const path = url.pathname;
    const method = route.request().method();
    if (path.endsWith('/shenlun/papers') && method === 'GET')
      return route.fulfill({ json: catalog });
    if (path.endsWith('/shenlun/papers/' + paperId) && method === 'GET')
      return route.fulfill({ json: paperView });
    if (path.endsWith('/shenlun/attempts') && method === 'GET')
      return route.fulfill({ json: [attempt] });
    if (path.endsWith('/' + attempt.id + '/jobs') && method === 'GET')
      return route.fulfill({ json: jobs });
    if (path.endsWith('/' + attempt.id + '/jobs') && method === 'POST') {
      const req = route.request().postDataJSON();
      expect(req.kind).toBe('transcribe');
      expect(req.attachments).toEqual([attachmentId]);
      const job = {
        id: req.request_id, attempt_id: attempt.id, kind: 'transcribe',
        status: 'complete', error: null,
        result: { pages: [{ page: 1, text: '手写的一句话', uncertain: [
          { text: '一句话', reason: '笔画接近，请检查' },
        ] }], confirmed: false },
        source_hash: 'test-source', attempt_revision: 0,
        skill_version: 'hyz-shenlun-v1', model: 'mock',
        created_at_ms: 3000, updated_at_ms: 3000,
      };
      jobs = [job];
      return route.fulfill({ status: 202, json: job });
    }
    if (path.endsWith('/transcription/confirm') && method === 'POST') {
      const input = route.request().postDataJSON();
      expect(input.expected_revision).toBe(0);
      expect(input.text).toBe('手写确认文本，保留我的原话。');
      confirmed = true;
      return route.fulfill({ json: {
        ...attempt, text: input.text, source_kind: 'vision-confirmed', revision: 1,
      } });
    }
    throw new Error('Unexpected mocked shenlun call: ' + method + ' ' + path);
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  await page.getByTestId('shenlun-open-paper').click();
  await expect(page.getByTestId('shenlun-grade-start')).toHaveCount(0);
  await page.getByTestId('shenlun-transcribe').locator('input[type="file"]').setInputFiles({
    name: 'answer.png', mimeType: 'image/png',
    buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9W3Z7b0AAAAASUVORK5CYII=', 'base64'),
  });
  await expect(page.getByTestId('shenlun-transcribe-start')).toBeEnabled();
  await page.getByTestId('shenlun-transcribe-start').click();
  await expect(page.getByTestId('shenlun-transcribed-edit')).toHaveValue('手写的一句话');
  expect(confirmed).toBe(false);
  await page.getByTestId('shenlun-transcribed-edit').fill('手写确认文本，保留我的原话。');
  await page.getByTestId('shenlun-transcribe-confirm').click();
  await expect.poll(() => confirmed).toBe(true);
  await expect(page.getByTestId('shenlun-answer-input')).toHaveValue('手写确认文本，保留我的原话。');
});
