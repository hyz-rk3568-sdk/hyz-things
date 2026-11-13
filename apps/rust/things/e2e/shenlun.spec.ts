import { expect, test } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

// 申论只测本模块的 Question Bank 复盘式工作区；不运行全量 E2E。
test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

const paperId = '019a3d8c-7f23-7b6e-927d-3c951851d3a2';
const questionId = '019a3d8e-1b32-7a60-9c83-f34217a6e210';
const nextQuestionId = '019a3d8e-1b32-7a60-9c83-f34217a6e211';
const catalog = [{
  id: paperId, title: '申论真题端到端样卷', year: 2025, region: '国考', variant: '地市级',
  subject: 'shenlun', role: 'primary', kind: 'exam',
  collection_id: null, collection_title: null, questions: 2,
}];
const paperView = {
  ...catalog[0],
  materials: [{ id: 'material-1', name: '给定资料 1', markdown: '推进基层治理，需要明确责任。' }],
  questions: [
    {
      id: questionId, number: 1, stem: '请概括基层治理的要求。', options_markdown: '',
      materials: ['material-1'], module: null, category: null, topics: [],
      answer_type: 'subjective', question_type: 'summary', grading_mode: 'ai', max_score: 20,
    },
    {
      id: nextQuestionId, number: 2, stem: '请提出优化基层服务的对策。', options_markdown: '',
      materials: ['material-1'], module: null, category: null, topics: [],
      answer_type: 'subjective', question_type: 'measures', grading_mode: 'ai', max_score: 20,
    },
  ],
};
type Attempt = {
  id: string; paper_id: string; question_id: string; source_hash: string;
  text: string; source_kind: string; status: 'draft' | 'submitted'; revision: number;
  created_at_ms: number; updated_at_ms: number; submitted_at_ms: number | null;
  reference_viewed_at_ms: number | null; after_reference: boolean;
};

function draftAttempt(id: string, qid = questionId): Attempt {
  return {
    id, paper_id: paperId, question_id: qid, source_hash: 'source-shenlun-v1', text: '',
    source_kind: 'typed', status: 'draft', revision: 0, created_at_ms: 1000,
    updated_at_ms: 1000, submitted_at_ms: null,
    reference_viewed_at_ms: null, after_reference: false,
  };
}
async function openShenlun(page: import('@playwright/test').Page) {
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  await expect(page.getByTestId('shenlun-catalog')).toBeVisible();
  await page.getByTestId('shenlun-open-paper').click();
  await expect(page.getByTestId('shenlun-workspace')).toBeVisible();
}

test('申论原卷复用地区年份目录和统一答题卡，不出现独立题号/交卷/二稿', async ({ page }) => {
  const attempts = new Map<string, Attempt[]>();
  await loginAsAdmin(page);
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname, searchParams } = new URL(route.request().url());
    const method = route.request().method();
    if (pathname.endsWith('/shenlun/papers') && method === 'GET')
      return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId) && method === 'GET')
      return route.fulfill({ json: paperView });
    if (pathname.endsWith('/shenlun/attempts') && method === 'GET')
      return route.fulfill({ json: attempts.get(searchParams.get('question_id') ?? '') ?? [] });
    if (pathname.endsWith('/shenlun/attempts') && method === 'POST') {
      const req = route.request().postDataJSON();
      const created = draftAttempt(req.request_id, req.question_id);
      attempts.set(req.question_id, [created]);
      return route.fulfill({ status: 201, json: created });
    }
    if (pathname.endsWith('/jobs') && method === 'GET')
      return route.fulfill({ json: [] });
    throw new Error('Unexpected 申论 request: ' + method + ' ' + pathname);
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  await expect(page.getByTestId('shenlun-catalog')).toContainText('2025 年');
  await page.getByTestId('shenlun-open-paper').click();
  await expect(page.getByTestId('shenlun-materials')).toContainText('推进基层治理');
  await expect(page.getByTestId('shenlun-question')).toContainText('请概括基层治理');
  await expect(page.getByTestId('question-bank-practice-navigation')).toBeVisible();
  await expect(page.getByTestId('shenlun-question-nav')).toHaveCount(0);
  await expect(page.getByTestId('shenlun-draft-comparison')).toHaveCount(0);
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('training-answer-sheet-number').nth(1).click();
  await expect(page.getByTestId('shenlun-question')).toContainText('请提出优化基层服务');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('2/2');
});

test('批改沿用原稿快照和评分服务；三点菜单查看并删除当前 AI 记录，保留作答', async ({ page }) => {
  await loginAsAdmin(page);
  let attempt = draftAttempt('11111111-1111-4111-8111-111111111111');
  const jobs: any[] = [];
  let submissions = 0, deletions = 0, sourceReads = 0;
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    const method = route.request().method();
    if (pathname.endsWith('/shenlun/papers') && method === 'GET') return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId) && method === 'GET') return route.fulfill({ json: paperView });
    if (pathname.endsWith('/shenlun/attempts') && method === 'GET') return route.fulfill({ json: [attempt] });
    if (pathname.endsWith('/' + attempt.id + '/jobs') && method === 'GET')
      return route.fulfill({ json: jobs });
    if (pathname.endsWith('/draft') && method === 'POST') {
      const req = route.request().postDataJSON();
      expect(req.expected_revision).toBe(attempt.revision);
      attempt = { ...attempt, text: req.text, revision: attempt.revision + 1 };
      return route.fulfill({ json: attempt });
    }
    if (pathname.endsWith('/submit') && method === 'POST') {
      expect(attempt.text).toContain('明确责任');
      submissions++;
      attempt = { ...attempt, status: 'submitted', submitted_at_ms: 2000 };
      return route.fulfill({ json: attempt });
    }
    if (pathname.endsWith('/' + attempt.id + '/jobs') && method === 'POST') {
      const req = route.request().postDataJSON();
      expect(req.kind).toBe('grade');
      expect(req.attachments).toEqual([]);
      const job = {
        id: req.request_id, attempt_id: attempt.id, kind: 'grade', status: 'complete',
        error: null, source_hash: attempt.source_hash, attempt_revision: attempt.revision,
        result: { score: 10, max_score: 20, summary: '表达清楚，措施稍显笼统。',
          items: [{ id: 'p1', criterion: '明确责任', evidence: '明确责任', coverage: 'partial',
            weight: 20, earned: 10, quote: '明确责任', reason: '缺少具体主体',
            suggestion: '写出执行主体', start: 0, end: 0 }] },
        skill_version: 'hyz-shenlun-v1', model: 'mock-codex',
        created_at_ms: 5000, updated_at_ms: 5001,
      };
      jobs.unshift(job);
      return route.fulfill({ status: 202, json: job });
    }
    if (pathname.endsWith('/source-answer') && method === 'GET') {
      sourceReads++;
      return route.fulfill({ json: {
        text: '来源参考答案：压实基层责任。', provenance: '题库来源（非官方）',
        source_hash: attempt.source_hash,
      } });
    }
    if (pathname.endsWith('/delete') && method === 'POST') {
      deletions++;
      const id = pathname.split('/').at(-2);
      const i = jobs.findIndex(job => job.id === id);
      if (i !== -1) jobs.splice(i, 1);
      return route.fulfill({ json: { deleted: true } });
    }
    throw new Error('Unexpected 申论 request: ' + method + ' ' + pathname);
  });
  await openShenlun(page);
  const answer = page.getByTestId('shenlun-answer-input');
  await answer.fill('应明确责任分工，建立协同机制。');
  await page.getByTestId('shenlun-grade-start').click();
  await expect(page.getByTestId('shenlun-score')).toContainText('10/20');
  expect(submissions).toBe(1);
  expect(sourceReads).toBe(0);
  await page.getByTestId('shenlun-mode-source').click();
  await expect(page.getByTestId('shenlun-source-answer')).toContainText('压实基层责任');
  expect(sourceReads).toBe(1);
  await page.getByTestId('shenlun-mode-grade').click();
  await page.getByTestId('shenlun-more').click();
  await page.getByTestId('shenlun-history-open').click();
  await expect(page.getByTestId('shenlun-job-history')).toHaveCount(1);
  await page.getByTestId('shenlun-more').click();
  await page.getByTestId('shenlun-delete-current').click();
  await page.getByTestId('shenlun-delete-confirm').click();
  await expect.poll(() => deletions).toBe(1);
  await expect(page.getByTestId('shenlun-job-history')).toHaveCount(0);
  expect(attempt.text).toContain('明确责任分工');
});

test('OCR 仅图片上传并要求校对确认，不会直接触发批改', async ({ page }) => {
  await loginAsAdmin(page);
  const attachmentId = '22222222-2222-4222-8222-222222222222';
  let attempt = draftAttempt('33333333-3333-4333-8333-333333333333');
  let jobs: any[] = [];
  let confirmed = false;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/attachments', route =>
    route.fulfill({ json: { id: attachmentId } }));
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    const method = route.request().method();
    if (pathname.endsWith('/shenlun/papers') && method === 'GET')
      return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId) && method === 'GET')
      return route.fulfill({ json: paperView });
    if (pathname.endsWith('/shenlun/attempts') && method === 'GET')
      return route.fulfill({ json: [attempt] });
    if (pathname.endsWith('/' + attempt.id + '/jobs') && method === 'GET')
      return route.fulfill({ json: jobs });
    if (pathname.endsWith('/' + attempt.id + '/jobs') && method === 'POST') {
      const req = route.request().postDataJSON();
      expect(req.kind).toBe('transcribe');
      expect(req.attachments).toEqual([attachmentId]);
      const job = {
        id: req.request_id, attempt_id: attempt.id, kind: 'transcribe',
        status: 'complete', error: null,
        result: { pages: [{ page: 1, text: '手写的一句话', uncertain: [
          { text: '一句话', reason: '笔画接近，请检查' },
        ] }] },
        source_hash: attempt.source_hash, attempt_revision: 0,
        skill_version: 'hyz-shenlun-v1', model: 'mock',
        created_at_ms: 3000, updated_at_ms: 3000,
      };
      jobs = [job];
      return route.fulfill({ status: 202, json: job });
    }
    if (pathname.endsWith('/transcription/confirm') && method === 'POST') {
      const input = route.request().postDataJSON();
      expect(input.expected_revision).toBe(0);
      expect(input.text).toBe('手写确认文本，保留我的原话。');
      confirmed = true;
      attempt = { ...attempt, text: input.text, source_kind: 'vision-confirmed', revision: 1 };
      return route.fulfill({ json: attempt });
    }
    throw new Error('Unexpected OCR request: ' + method + ' ' + pathname);
  });
  await openShenlun(page);
  await page.getByTestId('shenlun-mode-ocr').click();
  await expect(page.getByTestId('shenlun-grade-compose')).toHaveCount(0);
  await page.getByTestId('shenlun-ocr-images').locator('input[type="file"]').setInputFiles({
    name: 'answer.png', mimeType: 'image/png',
    buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9W3Z7b0AAAAASUVORK5CYII=', 'base64'),
  });
  await expect(page.getByTestId('shenlun-transcribe-start')).toBeEnabled();
  await page.getByTestId('shenlun-transcribe-start').click();
  await expect(page.getByTestId('shenlun-transcribed-text')).toHaveValue('手写的一句话');
  expect(confirmed).toBe(false);
  await page.getByTestId('shenlun-transcribed-text').fill('手写确认文本，保留我的原话。');
  await page.getByTestId('shenlun-transcription-confirm-button').click();
  await expect.poll(() => confirmed).toBe(true);
  await page.getByTestId('shenlun-mode-grade').click();
  await expect(page.getByTestId('shenlun-answer-input')).toHaveValue('手写确认文本，保留我的原话。');
});

test('AI 解析直接复用 Question Bank 的原生聊天工作区', async ({ page }) => {
  await loginAsAdmin(page);
  const attempt = draftAttempt('44444444-4444-4444-8444-444444444444');
  await page.route('**/api/v1/question-bank/practice/questions/*/ai*', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith('/ai')) return route.fulfill({ json: {
      question_id: questionId, status: 'idle', error: null, messages: [],
    } });
    if (path.endsWith('/runs')) return route.fulfill({ json: {
      items: [], next_after: null, has_more: false,
    } });
    return route.fulfill({ status: 200, json: {} });
  });
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    if (path.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    if (path.endsWith('/shenlun/attempts')) return route.fulfill({ json: [attempt] });
    if (path.endsWith('/jobs')) return route.fulfill({ json: [] });
    throw new Error('Unexpected 申论 request: ' + path);
  });
  await openShenlun(page);
  await page.getByTestId('shenlun-mode-ai').click();
  await expect(page.getByTestId('question-bank-ai-review')).toBeVisible();
  await expect(page.getByTestId('question-bank-ai-requirements-compose')).toBeVisible();
  await expect(page.getByTestId('shenlun-grade-compose')).toHaveCount(0);
});
