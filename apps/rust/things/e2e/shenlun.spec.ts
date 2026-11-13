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
  // Reviewing a paper must not create a hidden grading attempt.
  expect(attempts.size).toBe(0);
  await expect(page.getByTestId('question-bank-practice-navigation')).toBeVisible();
  await expect(page.getByTestId('shenlun-workspace').locator('[data-testid="shenlun-split"]')).toBeVisible();
  await expect(page.getByTestId('shenlun-more')).toBeVisible();
  await expect(page.getByTestId('shenlun-mode-ai').locator('svg')).toHaveCount(1);
  await expect(page.getByTestId('shenlun-mode-source').locator('svg')).toHaveCount(1);
  // The default AI tab offers delete; verify the empty menu on the source tab.
  await page.getByTestId('shenlun-mode-source').click();
  await page.getByTestId('shenlun-more').click();
  await expect(page.getByText('暂无可用操作')).toBeVisible();
  await page.getByTestId('shenlun-more').click();
  await expect(page.getByTestId('shenlun-question-nav')).toHaveCount(0);
  await expect(page.getByTestId('shenlun-draft-comparison')).toHaveCount(0);
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('shenlun-question')).toContainText('请提出优化基层服务');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('2/2');
});

test('批改复用 QuestionAiReview 的 Markdown、追问和删除，不创建独立评分记录', async ({ page }) => {
  await loginAsAdmin(page);
  let state: any = {
    question_id: questionId, status: 'idle', error: null, messages: [],
  };
  let startInstruction = '', followMessage = '', deletions = 0, legacyGradeJobs = 0;
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    if (pathname.endsWith('/shenlun/questions/' + questionId + '/source-answer')) return route.fulfill({ json: {
      text: '来源参考答案：压实基层责任。', provenance: '题库来源（非官方）',
      source_hash: 'source-shenlun-v1',
    } });
    if (pathname.includes('/jobs') && route.request().method() === 'POST') legacyGradeJobs++;
    throw new Error('Unexpected dedicated grading request: ' + pathname);
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai**', async route => {
    const { pathname } = new URL(route.request().url());
    const method = route.request().method();
    if (pathname.endsWith('/runs')) return route.fulfill({ json: { items: [], next_after: null, has_more: false } });
    if (pathname.endsWith('/ai') && method === 'GET') return route.fulfill({ json: state });
    if (pathname.endsWith('/ai') && method === 'POST') {
      const req = route.request().postDataJSON();
      startInstruction = req.instruction;
      state = {
        question_id: questionId, status: 'complete', error: null,
        messages: [
          { id: 1, role: 'user', content: startInstruction, created_at: 1 },
          { id: 2, role: 'assistant', content: '# 申论批改\n\n## 练习参考分\n\nAI 练习参考分：12/20（非官方）\n\n## 修改建议\n\n应补充执行主体。', created_at: 2 },
        ],
      };
      return route.fulfill({ status: 202, json: { ...state, request: {
        request_id: req.request_id, intent: 'initial', status: 'complete',
        error: null, remote_uncertain: false,
      } } });
    }
    if (pathname.endsWith('/messages') && method === 'POST') {
      const req = route.request().postDataJSON();
      followMessage = req.message;
      state = { ...state, messages: [...state.messages,
        { id: 3, role: 'user', content: followMessage, created_at: 3 },
        { id: 4, role: 'assistant', content: '原因是材料要求明确执行主体，而原答案未展开。', created_at: 4 },
      ] };
      return route.fulfill({ status: 202, json: { ...state, request: {
        request_id: req.request_id, intent: 'follow_up', status: 'complete',
        error: null, remote_uncertain: false,
      } } });
    }
    if (pathname.endsWith('/ai') && method === 'DELETE') {
      deletions++;
      state = { question_id: questionId, status: 'idle', error: null, messages: [] };
      return route.fulfill({ json: state });
    }
    if (pathname.includes('/requests/') && method === 'GET') return route.fulfill({ json: {
      request_id: pathname.split('/').at(-1), intent: 'initial', status: 'complete',
      error: null, remote_uncertain: false,
    } });
    if (pathname.endsWith('/events') || pathname.endsWith('/ai/events'))
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
    return route.fulfill({ status: 404, json: {} });
  });
  await openShenlun(page);
  await page.getByTestId('shenlun-mode-source').click();
  await expect(page.getByTestId('shenlun-source-answer')).toContainText('压实基层责任');
  await page.getByTestId('shenlun-mode-grade').click();
  await expect(page.getByTestId('question-bank-ai-review')).toBeVisible();
  await page.getByTestId('question-bank-ai-instruction').fill('应明确责任分工，建立协同机制。');
  await page.getByTestId('question-bank-ai-start').click();
  await expect.poll(() => startInstruction).toContain('【申论批改】');
  expect(startInstruction).toContain('应明确责任分工');
  await expect(page.getByTestId('question-bank-ai-messages')).toContainText('练习参考分');
  await page.getByTestId('shenlun-mode-source').click();
  await expect(page.getByTestId('shenlun-source-answer')).toContainText('压实基层责任');
  await page.getByTestId('shenlun-mode-grade').click();
  await page.getByTestId('question-bank-ai-input').fill('为什么这道题要明确执行主体？');
  await page.getByTestId('question-bank-ai-send').click();
  await expect.poll(() => followMessage).toContain('申论批改请求或追问');
  await page.getByTestId('shenlun-mode-ai').click();
  await expect(page.getByTestId('question-bank-ai-messages')).toContainText('应补充执行主体');
  await expect(page.getByTestId('shenlun-job-history')).toHaveCount(0);
  await expect(page.getByTestId('shenlun-grade-compose')).toHaveCount(0);
  await page.getByTestId('shenlun-more').click();
  await expect(page.getByTestId('shenlun-new-grade')).toHaveCount(0);
  await expect(page.getByTestId('shenlun-history-open')).toHaveCount(0);
  await page.getByTestId('shenlun-delete-current').click();
  await page.getByTestId('question-bank-ai-delete-confirm').click();
  await expect.poll(() => deletions).toBe(1);
  expect(legacyGradeJobs).toBe(0);
});

test('OCR 只创建临时 Codex 转写并直接渲染 Markdown，不创建作答或确认', async ({ page }) => {
  await loginAsAdmin(page);
  const attachmentId = '22222222-2222-4222-8222-222222222222';
  let ocrCount = 0;
  let legacyWrites = 0;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/attachments', route =>
    route.fulfill({ json: { id: attachmentId } }));
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    const method = route.request().method();
    if (pathname.endsWith('/shenlun/papers') && method === 'GET')
      return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId) && method === 'GET')
      return route.fulfill({ json: paperView });
    if (pathname.endsWith('/shenlun/questions/' + questionId + '/ocr') && method === 'POST') {
      const req = route.request().postDataJSON();
      expect(req.attachments).toEqual([attachmentId]);
      ocrCount++;
      return route.fulfill({ json: {
        markdown: '## 第 1 页\\n\\n手写的一句话，保留原文。',
        events: [
          { kind: 'tool_started', payload: { item_id: 'ocr-tool-1', tool: 'shell',
            details: { command: 'local-image-ocr' } } },
          { kind: 'command_output_delta', payload: { item_id: 'ocr-tool-1', text: 'OCR 图像识别输出' } },
          { kind: 'tool_finished', payload: { item_id: 'ocr-tool-1', tool: 'shell', status: 'completed',
            details: {} } },
        ],
      } });
    }
    if (method !== 'GET') legacyWrites++;
    throw new Error('Unexpected temporary OCR request: ' + method + ' ' + pathname);
  });
  await openShenlun(page);
  await page.getByTestId('shenlun-mode-ocr').click();
  await expect(page.getByTestId('shenlun-ai-panel')).toBeVisible();
  await expect(page.getByTestId('shenlun-ocr-composer')).toBeVisible();
  await expect(page.getByTestId('shenlun-ocr-input')).toHaveJSProperty('readOnly', true);
  await expect(page.getByTestId('shenlun-ocr-input')).toHaveAttribute('maxlength', '0');
  await expect(page.getByTestId('shenlun-transcribe-start')).toBeDisabled();
  await page.getByTestId('shenlun-ocr-images').locator('input[type="file"]').setInputFiles({
    name: 'answer.png', mimeType: 'image/png',
    buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9W3Z7b0AAAAASUVORK5CYII=', 'base64'),
  });
  await expect(page.getByTestId('shenlun-transcribe-start')).toBeEnabled();
  await page.getByTestId('shenlun-transcribe-start').click();
  await expect(page.getByTestId('shenlun-ocr-markdown')).toContainText('手写的一句话，保留原文');
  await expect(page.getByTestId('shenlun-ocr-markdown').locator('h2')).toContainText('第 1 页');
  await expect(page.getByTestId('shenlun-ocr-tool-activity')).toContainText('工具调用');
  await expect(page.getByTestId('shenlun-ocr-tool')).toContainText('shell');
  await expect(page.getByTestId('shenlun-ocr-output')).toContainText('OCR 图像识别输出');
  await expect(page.getByTestId('shenlun-transcription-confirm')).toHaveCount(0);
  await expect(page.getByTestId('shenlun-transcription-confirm-button')).toHaveCount(0);
  expect(ocrCount).toBe(1);
  expect(legacyWrites).toBe(0);
  await page.getByTestId('shenlun-mode-grade').click();
  await expect(page.getByTestId('question-bank-ai-review')).toBeVisible();
});

test('申论解析和批改入口是同一个 AI 对话工作区', async ({ page }) => {
  await loginAsAdmin(page);
  await page.route('**/api/v1/question-bank/practice/questions/*/ai**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith('/ai')) return route.fulfill({ json: {
      question_id: questionId, status: 'idle', error: null, messages: [],
    } });
    if (path.endsWith('/runs')) return route.fulfill({ json: {
      items: [], next_after: null, has_more: false,
    } });
    return route.fulfill({ status: 404, json: {} });
  });
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    if (path.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    throw new Error('Unexpected 申论 request: ' + path);
  });
  await openShenlun(page);
  await expect(page.getByTestId('question-bank-ai-review')).toBeVisible();
  await expect(page.getByTestId('question-bank-ai-requirements-compose')).toBeVisible();
  await page.getByTestId('shenlun-mode-grade').click();
  await expect(page.getByTestId('question-bank-ai-review')).toBeVisible();
  await expect(page.getByTestId('question-bank-ai-instruction')).toHaveAttribute('placeholder', /粘贴申论作答/);
  await page.getByTestId('shenlun-mode-ai').click();
  await expect(page.getByTestId('question-bank-ai-instruction')).toHaveAttribute('placeholder', /输入解析要求/);
});
