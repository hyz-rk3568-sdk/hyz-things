import { expect, test } from '@playwright/test';
import { loginAsAdmin, resetHarness, harnessOrigin } from './fixtures';

// 申论只测本模块的 Question Bank 复盘式工作区；不运行全量 E2E。
test.beforeEach(async ({ request }) => {
  await resetHarness(request);
});

const paperId = '019a3d8c-7f23-7b6e-927d-3c951851d3a6';
const questionId = '019a3d8e-1b32-7a60-9c83-f34217a6e250';
const nextQuestionId = '019a3d8e-1b32-7a60-9c83-f34217a6e251';
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
  await page.getByTestId('question-bank-paper-option').click();
  await page.getByTestId('shenlun-start-review').click();
  await expect(page.getByTestId('shenlun-workspace')).toBeVisible();
}

async function scratchpadHasInk(page: import('@playwright/test').Page) {
  return page.getByTestId('calculation-scratchpad-canvas').evaluate(element => {
    const canvas = element as HTMLCanvasElement;
    const pixels = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data;
    for (let index = 3; index < pixels.length; index += 4) {
      if (pixels[index] !== 0) return true;
    }
    return false;
  });
}

async function scratchpadHasStoredInk(page: import('@playwright/test').Page, scope: string) {
  return page.evaluate(scope => new Promise<boolean>(resolve => {
    const request = indexedDB.open('hyz-study-ink');
    request.onerror = () => resolve(false);
    request.onsuccess = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains('drafts')) {
        database.close();
        resolve(false);
        return;
      }
      const transaction = database.transaction('drafts', 'readonly');
      const read = transaction.objectStore('drafts').get(scope);
      read.onerror = () => resolve(false);
      read.onsuccess = () => {
        const history = read.result?.history;
        resolve(typeof history === 'string' && JSON.parse(history).length > 0);
        database.close();
      };
    };
  }), scope);
}

async function drawScratchStroke(page: import('@playwright/test').Page, paneTestId: string) {
  const bounds = await page.getByTestId(paneTestId).boundingBox();
  expect(bounds).not.toBeNull();
  await page.mouse.move(bounds!.x + 24, bounds!.y + 24);
  await page.mouse.down();
  await page.mouse.move(bounds!.x + 68, bounds!.y + 46, { steps: 4 });
  await page.mouse.up();
  await expect.poll(() => scratchpadHasInk(page)).toBe(true);
}

test('申论复盘启用共享草稿区并支持在材料与题目上书写', async ({ page }) => {
  await loginAsAdmin(page);
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    if (pathname.endsWith('/shenlun/questions/' + questionId + '/source-answer'))
      return route.fulfill({ json: { text: '参考答案。', provenance: '题库来源（非官方）', source_hash: 'source-v1' } });
    throw new Error('Unexpected Shenlun request: ' + route.request().method() + ' ' + pathname);
  });
  await openShenlun(page);
  await page.getByTestId('shenlun-mode-source').click();

  const toggle = page.getByTestId('calculation-scratchpad-toggle');
  await expect(toggle).toBeVisible();
  await toggle.click();
  const canvas = page.getByTestId('calculation-scratchpad-canvas');
  await expect(canvas).toBeVisible();
  const undo = page.getByTestId('calculation-scratchpad-undo');

  await drawScratchStroke(page, 'shenlun-material-scroll');
  await drawScratchStroke(page, 'shenlun-source-scroll');
  await expect(undo).toBeEnabled();
  await undo.click();
  await expect.poll(() => scratchpadHasInk(page)).toBe(true);
  await undo.click();
  await expect.poll(() => scratchpadHasInk(page)).toBe(false);
});

test('申论草稿按题保存，切题后恢复对应笔迹', async ({ page }) => {
  await loginAsAdmin(page);
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    if (pathname.endsWith('/source-answer'))
      return route.fulfill({ json: { text: '参考答案。', provenance: '题库来源（非官方）', source_hash: 'source-v1' } });
    throw new Error('Unexpected Shenlun request: ' + route.request().method() + ' ' + pathname);
  });
  await openShenlun(page);
  await page.getByTestId('shenlun-mode-source').click();
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await drawScratchStroke(page, 'shenlun-source-scroll');
  const scope = `shenlun-${paperId}-${questionId}`;
  await expect.poll(() => scratchpadHasStoredInk(page, scope)).toBe(true);
  await page.getByTestId('calculation-scratchpad-toggle').click();

  const selectQuestion = async (index: number) => {
    await page.getByTestId('training-answer-sheet-toggle').click();
    await page.getByTestId('question-bank-practice-number').nth(index).click();
  };
  await selectQuestion(1);
  await expect(page.getByTestId('shenlun-question')).toContainText('请提出优化基层服务');
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect(page.getByTestId('calculation-scratchpad-canvas')).toBeVisible();
  await expect.poll(() => scratchpadHasInk(page)).toBe(false);
  await page.getByTestId('calculation-scratchpad-toggle').click();

  await selectQuestion(0);
  await expect(page.getByTestId('shenlun-question')).toContainText('请概括基层治理');
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect(page.getByTestId('calculation-scratchpad-canvas')).toBeVisible();
  await expect.poll(() => scratchpadHasInk(page)).toBe(true);
});

test('申论题目工具栏复用无文字图标并保留 Flashcard 创建入口', async ({ page }) => {
  await loginAsAdmin(page);
  let existing: any[] = [];
  let createBody: any;
  const target = '真题练习/申论/归纳概括/错题';
  await page.route(`**/api/v1/question-bank/practice/questions/${questionId}/flashcard-preview`, route =>
    route.fulfill({ json: {
      eligible: true, reason: null, front: '请概括基层治理的要求。', correct_answer: ['压实基层责任'],
      recommended_target: target, allowed_targets: [target], requires_type_confirmation: false,
      source_revision: 'shenlun-v1', existing,
    } }));
  await page.route(`**/api/v1/question-bank/practice/questions/${questionId}/flashcard`, async route => {
    createBody = route.request().postDataJSON();
    existing = [{ card_id: 'shenlun-card-1', deck_path: target, source_file: `${target}/cards.md`,
      front: '请概括基层治理的要求。', back: '压实基层责任', mode: 'practice' }];
    return route.fulfill({ status: 201, json: {
      status: 'created_synced', target_deck: target, source_file: `${target}/cards.md`,
      card_id: 'shenlun-card-1', retryable: false,
    } });
  });
  await page.route(`**/api/v1/question-bank/practice/favorites/${questionId}`, route =>
    route.fulfill({ json: { question_id: questionId, favorite: false } }));
  await page.route(`**/api/v1/question-bank/practice/questions/${questionId}/ai**`, async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/runs'))
      return route.fulfill({ json: { items: [], next_after: null, has_more: false } });
    if (pathname.endsWith('/events') || pathname.endsWith('/ai/events'))
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
    if (pathname.endsWith('/ai'))
      return route.fulfill({ json: { question_id: questionId, status: 'idle', error: null, messages: [] } });
    return route.fulfill({ status: 404, json: {} });
  });
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    throw new Error('Unexpected 申论 request: ' + route.request().method() + ' ' + pathname);
  });
  await openShenlun(page);
  await expect(page.getByRole('group', { name: '收藏与 Flashcard 操作' })).toBeVisible();

  const iconButtons = [
    ['shenlun-favorite', '收藏题目'],
    ['shenlun-create-flashcard', '为当前题目创建 Flashcard'],
    ['shenlun-mode-grade', 'AI 批改'],
    ['shenlun-mode-ocr', 'OCR 转写'],
    ['shenlun-mode-source', '来源参考答案'],
    ['shenlun-mode-ai', 'AI 解析与追问'],
    ['shenlun-more', '更多题目操作'],
  ] as const;
  for (const [testId, label] of iconButtons) {
    const button = page.getByTestId(testId);
    await expect(button).toBeVisible();
    await expect(button).toHaveAttribute('aria-label', label);
    await expect(button).toHaveAttribute('title', label);
    await expect(button.locator('svg')).toHaveCount(1);
    await expect(button).not.toContainText(label);
  }

  await page.getByTestId('shenlun-create-flashcard').click();
  const card = page.getByTestId('favorite-flashcard-card');
  await expect(card).toBeVisible();
  await card.getByLabel('确认写入所选目录').check();
  await card.getByRole('button', { name: '创建并同步', exact: true }).click();
  await expect.poll(() => createBody).toEqual({
    creation_mode: 'practice', target_deck: target, back: '',
    source_revision: 'shenlun-v1', type_confirmed: true,
  });
  const manage = page.getByTestId('shenlun-manage-flashcard');
  await expect(manage).toBeVisible();
  await expect(manage).toHaveAttribute('aria-label', '管理当前题目的 1 张 Flashcard');
  await expect(manage.locator('svg')).toHaveCount(1);
  await manage.click();
  await expect(page.getByTestId('favorite-flashcard-editor')).toBeVisible();
});

test('申论题通过真实 Flashcard 预览接口显示并创建卡片', async ({ page }) => {
  await loginAsAdmin(page);
  const previewResponsePromise = page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.pathname === `/api/v1/question-bank/practice/questions/${questionId}/flashcard-preview`;
  });
  await openShenlun(page);
  const previewResponse = await previewResponsePromise;
  expect(previewResponse.status()).toBe(200);
  const preview = await previewResponse.json();
  expect(preview.eligible).toBe(true);
  expect(preview.correct_answer).toEqual([]);
  await expect(page.getByTestId('shenlun-create-flashcard')).toBeVisible();
  await page.getByTestId('shenlun-create-flashcard').click();
  await expect(page.getByTestId('favorite-flashcard-card')).toBeVisible();
  await page.getByTestId('favorite-flashcard-back-input').fill('压实基层责任，完善公共服务。');
  await page.getByLabel('确认写入所选目录').check();
  const createResponsePromise = page.waitForResponse(response => {
    const url = new URL(response.url());
    return response.request().method() === 'POST'
      && url.pathname === `/api/v1/question-bank/practice/questions/${questionId}/flashcard`;
  });
  await page.getByRole('button', { name: '创建并同步', exact: true }).click();
  const createResponse = await createResponsePromise;
  expect(createResponse.status()).toBe(200);
  expect(await createResponse.json()).toMatchObject({ status: 'created_synced' });
  const manage = page.getByTestId('shenlun-manage-flashcard');
  await expect(manage).toHaveAttribute('aria-label', '管理当前题目的 1 张 Flashcard');
  await manage.click();
  const editor = page.getByTestId('favorite-flashcard-editor');
  await expect(editor).toBeVisible();
  await expect(page.getByTestId('favorite-flashcard-editor-back')).toHaveValue('压实基层责任，完善公共服务。');
  await editor.getByTestId('favorite-flashcard-delete').click();
  await editor.getByTestId('favorite-flashcard-confirm-delete').click();
  await expect(editor).toHaveCount(0);
  await expect(page.getByTestId('shenlun-create-flashcard')).toBeVisible();
});

test('申论选题复用 Question Bank 选择器、操作与切换记忆', async ({ page }) => {
  await loginAsAdmin(page);
  let paperListRequests = 0;
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/shenlun/papers')) {
      paperListRequests += 1;
      return route.fulfill({ json: catalog });
    }
    if (pathname.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    throw new Error('Unexpected Shenlun request: ' + route.request().method() + ' ' + pathname);
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  await expect(page.getByTestId('shenlun-catalog')).toBeVisible();
  await expect(page.getByRole('button', { name: /刷新/ })).toHaveCount(0);
  const paper = page.getByTestId('question-bank-paper-option');
  await paper.click();
  await expect(page.getByTestId('shenlun-catalog')).toBeVisible();
  await expect(paper).toHaveAttribute('aria-pressed', 'true');
  await page.getByTestId('training-mode-calculation').click();
  await page.getByTestId('training-mode-shenlun').click();
  await expect(page.getByTestId('question-bank-paper-option')).toHaveAttribute('aria-pressed', 'true');
  expect(paperListRequests).toBe(1);
  const start = page.getByTestId('shenlun-start-review');
  await expect(start).toBeEnabled();
  await start.click();
  await expect(page.getByTestId('shenlun-workspace')).toBeVisible();
});

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
    if (pathname.endsWith('/shenlun/questions/' + questionId + '/source-answer') && method === 'GET')
      return route.fulfill({ json: {
        text: '来源参考答案：压实基层责任。', provenance: '题库来源（非官方）',
        source_hash: 'source-shenlun-v1',
      } });
    if (pathname.endsWith('/shenlun/questions/' + nextQuestionId + '/source-answer') && method === 'GET')
      return route.fulfill({ json: {
        text: '来源参考答案：完善基层服务。', provenance: '题库来源（非官方）',
        source_hash: 'source-shenlun-v1',
      } });
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
  await page.getByTestId('question-bank-paper-option').click();
  await page.getByTestId('shenlun-start-review').click();
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
  await expect(page.getByTestId('shenlun-mode-source')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('shenlun-source-answer')).toContainText('完善基层服务');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('2/2');
});

test('申论选题布局在桌面与手机视口都保留可见的开始操作', async ({ page }) => {
  await loginAsAdmin(page);
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    throw new Error('Unexpected Shenlun request: ' + route.request().method() + ' ' + pathname);
  });
  await page.setViewportSize({ width: 1366, height: 900 });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-shenlun').click();
  const action = page.getByTestId('shenlun-start-review');
  await expect(action).toBeVisible();
  for (const viewport of [{ width: 1366, height: 900 }, { width: 390, height: 844 }]) {
    await page.setViewportSize(viewport);
    const bounds = await action.boundingBox();
    const layout = await page.getByTestId('training-mode-content').evaluate(element => ({
      width: element.clientWidth,
      scrollWidth: element.scrollWidth,
    }));
    expect(bounds).not.toBeNull();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(viewport.width + 1);
    expect(layout.scrollWidth).toBeLessThanOrEqual(layout.width);
  }
});

test('申论切题后保留当前 OCR 与来源答案模式', async ({ page }) => {
  await loginAsAdmin(page);
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
    if (pathname.endsWith('/shenlun/questions/' + questionId + '/source-answer'))
      return route.fulfill({ json: { text: '参考答案', provenance: '题库来源（非官方）', source_hash: 'source-v1' } });
    if (pathname.endsWith('/shenlun/questions/' + nextQuestionId + '/source-answer'))
      return route.fulfill({ json: { text: '参考答案二', provenance: '题库来源（非官方）', source_hash: 'source-v1' } });
    throw new Error('Unexpected Shenlun request: ' + route.request().method() + ' ' + pathname);
  });
  await openShenlun(page);
  await page.getByTestId('shenlun-mode-ocr').click();
  await expect(page.getByTestId('shenlun-mode-ocr')).toHaveAttribute('aria-pressed', 'true');
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('shenlun-question')).toContainText('请提出优化基层服务');
  await expect(page.getByTestId('shenlun-mode-ocr')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('shenlun-ai-panel')).toBeVisible();
  await page.getByTestId('shenlun-mode-source').click();
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-number').nth(0).click();
  await expect(page.getByTestId('shenlun-question')).toContainText('请概括基层治理');
  await expect(page.getByTestId('shenlun-mode-source')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('shenlun-source-answer')).toBeVisible();
});

test('申论解析和批改分别使用独立会话、草稿与删除状态', async ({ page }) => {
  const legacyRequestId = 'legacy-shenlun-shared-request';
  await page.addInitScript((legacyQuestionId) => {
    sessionStorage.setItem(`hyz.ai-review.draft.${legacyQuestionId}.start`, '旧的共享草稿');
    sessionStorage.setItem(`hyz.ai-review.draft.${legacyQuestionId}.request-id`, 'legacy-shenlun-shared-request');
    const urls: string[] = [];
    (window as any).__shenlunReviewStreams = urls;
    class FakeEventSource extends EventTarget {
      onmessage: ((event: MessageEvent) => void) | null = null;
      onerror: ((event: Event) => void) | null = null;
      readyState = 1;
      constructor(public url: string) { super(); urls.push(url); }
      close() { this.readyState = 2; }
    }
    Object.defineProperty(window, 'EventSource', { configurable: true, value: FakeEventSource });
  }, questionId);
  await loginAsAdmin(page);
  type Scope = 'analysis' | 'grading';
  const states: Record<Scope, any> = {
    analysis: { question_id: questionId, status: 'idle', error: null, messages: [] },
    grading: { question_id: questionId, status: 'idle', error: null, messages: [] },
  };
  const observed = {
    reads: [] as Scope[],
    histories: [] as Scope[],
    historyEvents: [] as Scope[],
    requestStatuses: [] as Scope[],
    requestIds: [] as string[],
    starts: [] as Array<{ scope: Scope; instruction: string }>,
    followUps: [] as Array<{ scope: Scope; message: string }>,
    deletes: [] as Scope[],
  };
  const requests = new Map<string, any>();
  let nextMessageId = 1;
  let legacyGradeJobs = 0;
  const makeReceipt = (scope: Scope, requestId: string, intent: string) => {
    const info = {
      request_id: requestId, run_id: `${scope}-${requestId}`, intent, status: 'complete',
      error: null, remote_uncertain: false, new_thread_requested: false,
      user_message_id: null, retry_of: null,
    };
    requests.set(`${scope}:${requestId}`, info);
    return info;
  };

  await page.route('**/api/v1/question-bank/practice/questions/*/ai**', async route => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const method = request.method();
    const scope = (url.searchParams.get('scope') ?? 'analysis') as Scope;
    if (path.endsWith('/runs')) {
      observed.histories.push(scope);
      const assistant = states[scope].messages.find((message: any) => message.role === 'assistant');
      const items = assistant ? [{
        run_id: `${scope}-${assistant.id}`, status: 'complete', assistant_message_id: assistant.id,
      }] : [];
      return route.fulfill({ json: { items, next_after: null, has_more: false } });
    }
    if (path.includes('/runs/') && path.endsWith('/events')) {
      observed.historyEvents.push(scope);
      return route.fulfill({ json: { items: [], next_after: null, has_more: false } });
    }
    if (path.includes('/requests/') && method === 'GET') {
      observed.requestStatuses.push(scope);
      observed.requestIds.push(path.split('/').at(-1) ?? '');
      const requestId = path.split('/').at(-1)!;
      return route.fulfill({ json: requests.get(`${scope}:${requestId}`) ?? {
        request_id: requestId, run_id: `${scope}-${requestId}`, intent: 'initial', status: 'complete',
        error: null, remote_uncertain: false, new_thread_requested: false,
        user_message_id: null, retry_of: null,
      } });
    }
    if (path.endsWith('/messages') && method === 'POST') {
      const body = request.postDataJSON();
      observed.followUps.push({ scope, message: body.message });
      const answer = scope === 'grading' ? '批改追问独立回答' : '解析追问独立回答';
      states[scope] = {
        ...states[scope], status: 'complete',
        messages: [...states[scope].messages,
          { id: nextMessageId++, role: 'user', content: body.message, created_at: nextMessageId },
          { id: nextMessageId++, role: 'assistant', content: answer, created_at: nextMessageId },
        ],
      };
      return route.fulfill({ status: 202, json: { ...states[scope], request: makeReceipt(scope, body.request_id, 'follow_up') } });
    }
    if (path.endsWith('/ai') && method === 'GET') {
      observed.reads.push(scope);
      return route.fulfill({ json: states[scope] });
    }
    if (path.endsWith('/ai') && method === 'POST') {
      const body = request.postDataJSON();
      observed.starts.push({ scope, instruction: body.instruction });
      const answer = scope === 'grading' ? '批改独立会话正文' : '解析独立会话正文';
      states[scope] = {
        question_id: questionId, status: 'complete', error: null,
        messages: [
          { id: nextMessageId++, role: 'user', content: body.instruction, created_at: nextMessageId },
          { id: nextMessageId++, role: 'assistant', content: answer, created_at: nextMessageId },
        ],
      };
      return route.fulfill({ status: 202, json: { ...states[scope], request: makeReceipt(scope, body.request_id, 'initial') } });
    }
    if (path.endsWith('/ai') && method === 'DELETE') {
      observed.deletes.push(scope);
      states[scope] = { question_id: questionId, status: 'idle', error: null, messages: [] };
      return route.fulfill({ json: states[scope] });
    }
    return route.fulfill({ status: 404, json: {} });
  });
  await page.route('**/api/v1/question-bank/shenlun/**', async route => {
    const { pathname } = new URL(route.request().url());
    const method = route.request().method();
    if (pathname.endsWith('/shenlun/papers') && method === 'GET')
      return route.fulfill({ json: catalog });
    if (pathname.endsWith('/shenlun/papers/' + paperId) && method === 'GET')
      return route.fulfill({ json: paperView });
    if (pathname.includes('/jobs') && method === 'POST') {
      legacyGradeJobs++;
      return route.fulfill({ json: {} });
    }
    throw new Error('Unexpected 申论 request: ' + method + ' ' + pathname);
  });

  await openShenlun(page);
  const input = page.getByTestId('question-bank-ai-instruction');
  await expect(input).toHaveAttribute('placeholder', /输入解析要求/);
  await expect(input).toHaveValue('');
  expect(observed.requestIds).not.toContain(legacyRequestId);
  await input.fill('只属于解析的草稿');
  await page.getByTestId('shenlun-mode-grade').click();
  await expect(input).toHaveAttribute('placeholder', /粘贴申论作答/);
  await expect(input).toHaveValue('');
  await input.fill('只属于批改的作答草稿');
  await page.getByTestId('shenlun-mode-ai').click();
  await expect(input).toHaveValue('只属于解析的草稿');

  await page.getByTestId('question-bank-ai-start').click();
  await expect.poll(() => observed.starts).toEqual([
    { scope: 'analysis', instruction: '只属于解析的草稿' },
  ]);
  await expect(page.getByTestId('question-bank-ai-messages')).toContainText('解析独立会话正文');
  const analysisRun = page.getByTestId('question-bank-ai-progress').getByRole('button');
  await expect(analysisRun).toBeVisible();
  await analysisRun.click();
  await expect.poll(() => observed.historyEvents).toContain('analysis');
  await page.getByTestId('shenlun-mode-grade').click();
  await expect(input).toHaveValue('只属于批改的作答草稿');
  await page.getByTestId('question-bank-ai-start').click();
  await expect.poll(() => observed.starts).toEqual([
    { scope: 'analysis', instruction: '只属于解析的草稿' },
    { scope: 'grading', instruction: '只属于批改的作答草稿' },
  ]);
  const messages = page.getByTestId('question-bank-ai-messages');
  await expect(messages).toContainText('批改独立会话正文');
  await expect(messages).not.toContainText('解析独立会话正文');
  const gradingRun = page.getByTestId('question-bank-ai-progress').getByRole('button');
  await expect(gradingRun).toBeVisible();
  await gradingRun.click();
  await expect.poll(() => observed.historyEvents).toContain('grading');

  await page.getByTestId('question-bank-ai-input').fill('只属于批改的追问');
  await page.getByTestId('question-bank-ai-send').click();
  await expect.poll(() => observed.followUps.some(({ scope, message }) =>
    scope === 'grading' && message.includes('只属于批改的追问'))).toBe(true);
  await expect(messages).toContainText('批改追问独立回答');

  await page.getByTestId('shenlun-mode-ai').click();
  const analysisMessages = page.getByTestId('question-bank-ai-messages');
  await expect(analysisMessages).toContainText('解析独立会话正文');
  await expect(analysisMessages).not.toContainText('批改独立会话正文');
  await expect(analysisMessages).not.toContainText('批改追问独立回答');
  await page.getByTestId('shenlun-mode-grade').click();
  await page.getByTestId('shenlun-more').click();
  await expect(page.getByTestId('shenlun-delete-current')).toBeVisible();
  await page.getByTestId('shenlun-delete-current').click();
  await page.getByTestId('question-bank-ai-delete-confirm').click();
  await expect.poll(() => observed.deletes).toEqual(['grading']);
  await page.getByTestId('shenlun-mode-ai').click();
  await expect(page.getByTestId('question-bank-ai-messages')).toContainText('解析独立会话正文');
  expect(observed.reads).toContain('analysis');
  expect(observed.reads).toContain('grading');
  expect(observed.histories).toEqual(expect.arrayContaining(['analysis', 'grading']));
  expect(observed.historyEvents).toEqual(expect.arrayContaining(['analysis', 'grading']));
  const streamScopes = await page.evaluate(() => ((window as any).__shenlunReviewStreams as string[]).map(url => {
    return new URL(url, window.location.origin).searchParams.get('scope') ?? 'analysis';
  }));
  expect(streamScopes).toEqual(expect.arrayContaining(['analysis', 'grading']));
  expect(observed.requestStatuses).toEqual(expect.arrayContaining(['analysis', 'grading']));
  expect(observed.requestIds).not.toContain(legacyRequestId);
  expect(legacyGradeJobs).toBe(0);
  await page.getByTestId('shenlun-more').click();
  await expect(page.getByTestId('shenlun-delete-current')).toBeVisible();
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('shenlun-question')).toContainText('请提出优化基层服务');
  await expect(page.getByTestId('shenlun-more')).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByTestId('shenlun-delete-current')).toHaveCount(0);
});

for (const viewport of [
  { width: 1366, height: 900 },
  { width: 1133, height: 744 },
  { width: 1024, height: 768 },
  { width: 768, height: 1024 },
  { width: 390, height: 844 },
]) {
  test(`申论复盘与专项训练布局对齐并适配 ${viewport.width}×${viewport.height}`, async ({ page }, testInfo) => {
    await page.setViewportSize(viewport);
    await loginAsAdmin(page);
    await page.route('**/api/v1/question-bank/shenlun/**', async route => {
      const { pathname } = new URL(route.request().url());
      if (pathname.endsWith('/shenlun/papers')) return route.fulfill({ json: catalog });
      if (pathname.endsWith('/shenlun/papers/' + paperId)) return route.fulfill({ json: paperView });
      if (pathname.endsWith('/shenlun/questions/' + questionId + '/source-answer')) return route.fulfill({ json: {
        text: '来源参考答案：压实基层责任。', provenance: '题库来源（非官方）', source_hash: 'source-shenlun-v1',
      } });
      throw new Error('Unexpected 申论 request: ' + route.request().method() + ' ' + pathname);
    });
    await page.route(`**/api/v1/question-bank/practice/favorites/${questionId}`, route =>
      route.fulfill({ json: { question_id: questionId, favorite: false } }));
    await page.route(`**/api/v1/question-bank/practice/questions/${questionId}/flashcard-preview`, route =>
      route.fulfill({ json: {
        eligible: true, reason: null, front: '请概括基层治理的要求。', correct_answer: ['压实基层责任'],
        recommended_target: '真题练习/申论/归纳概括/错题',
        allowed_targets: ['真题练习/申论/归纳概括/错题'], requires_type_confirmation: false,
        source_revision: 'shenlun-v1', existing: [],
      } }));
    await page.route('**/api/v1/question-bank/practice/questions/*/ai**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      const path = url.pathname;
      const question_id = path.split('/')[path.split('/').indexOf('questions') + 1];
      if (path.endsWith('/runs')) return route.fulfill({ json: { items: [], next_after: null, has_more: false } });
      if (path.includes('/runs/') && path.endsWith('/events'))
        return route.fulfill({ json: { items: [], next_after: null, has_more: false } });
      if (path.endsWith('/events')) return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
      if (path.includes('/requests/')) return route.fulfill({ json: {
        request_id: path.split('/').at(-1), run_id: 'layout-run', intent: 'initial', status: 'complete',
        error: null, remote_uncertain: false, new_thread_requested: false, user_message_id: null, retry_of: null,
      } });
      if (path.endsWith('/ai')) return route.fulfill({ json: { question_id, status: 'idle', error: null, messages: [] } });
      return route.fulfill({ status: 404, json: {} });
    });

    await openShenlun(page);
    await page.getByTestId('shenlun-mode-source').click();
    await expect(page.getByTestId('shenlun-source-answer')).toContainText('压实基层责任');
    const shenlun = page.getByTestId('shenlun-split');
    const shenlunHandle = shenlun.getByTestId('training-split-handle');
    await expect(shenlunHandle).toHaveAttribute('aria-orientation', viewport.width >= 768 ? 'vertical' : 'horizontal');
    const shenlunLeft = shenlun.getByTestId('shenlun-materials').locator('.question-bank-pane-header');
    const shenlunRight = shenlun.getByTestId('shenlun-question').locator('.question-bank-pane-header');
    const shenlunLeftBox = await shenlunLeft.boundingBox();
    const shenlunRightBox = await shenlunRight.boundingBox();
    expect(shenlunLeftBox).not.toBeNull();
    expect(shenlunRightBox).not.toBeNull();
    expect(Math.abs(shenlunLeftBox!.height - shenlunRightBox!.height)).toBeLessThanOrEqual(1);
    if (viewport.width >= 768) {
      expect(Math.abs((shenlunLeftBox!.y + shenlunLeftBox!.height) - (shenlunRightBox!.y + shenlunRightBox!.height))).toBeLessThanOrEqual(1);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`shenlun-${viewport.width}x${viewport.height}.png`) });

    await page.goto('/#/study/training');
    await page.getByTestId('training-mode-question-bank-paper').click();
    const region = page.getByTestId('question-bank-region').filter({ hasText: '国考' });
    if (!(await region.evaluate(node => (node.closest('details') as HTMLDetailsElement | null)?.open))) await region.click();
    const year = page.getByTestId('question-bank-year').filter({ hasText: '2008' });
    if (!(await year.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) await year.click();
    await page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' }).click();
    await page.getByTestId('question-bank-practice-start').click();
    await page.getByTestId('question-bank-practice-submit').click();
    await page.getByRole('button', { name: '确认交卷' }).click();
    await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
    await page.getByTestId('question-bank-original-tab').click();
    const practice = page.getByTestId('question-bank-practice-split');
    const practiceHandle = practice.getByTestId('training-split-handle');
    await expect(practiceHandle).toHaveAttribute('aria-orientation', viewport.width >= 768 ? 'vertical' : 'horizontal');
    const practiceLeft = practice.getByTestId('question-bank-practice-material-pane').locator('.question-bank-pane-header');
    const practiceRight = practice.getByTestId('question-bank-practice-question-pane').locator('.question-bank-pane-header');
    const practiceLeftBox = await practiceLeft.boundingBox();
    const practiceRightBox = await practiceRight.boundingBox();
    expect(practiceLeftBox).not.toBeNull();
    expect(practiceRightBox).not.toBeNull();
    expect(Math.abs(practiceLeftBox!.height - practiceRightBox!.height)).toBeLessThanOrEqual(1);
    expect(Math.abs(practiceLeftBox!.height - shenlunLeftBox!.height)).toBeLessThanOrEqual(1);
    if (viewport.width >= 768) {
      expect(Math.abs((practiceLeftBox!.y + practiceLeftBox!.height) - (practiceRightBox!.y + practiceRightBox!.height))).toBeLessThanOrEqual(1);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`practice-${viewport.width}x${viewport.height}.png`) });
  });
}

test('OCR 只创建临时 Codex 转写并直接渲染 Markdown，不创建作答或确认', async ({ page, request }) => {
  await loginAsAdmin(page);
  let attachmentId = '';
  let ocrCount = 0;
  let legacyWrites = 0;
  let uploadedScope: string | null = null;
  let uploadObservation: {
    contentType: string | undefined;
    csrf: string | undefined;
  } | null = null;
  let uploadedRequest: import('@playwright/test').Request | null = null;
  const csrf = await page.evaluate(async () => {
    const response = await fetch('/api/v1/panel');
    return ((await response.json()) as { csrf_token: string }).csrf_token;
  });
  const imageBytes = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9W3Z7b0AAAAASUVORK5CYII=', 'base64');
  page.on('request', uploadRequest => {
    const url = new URL(uploadRequest.url());
    if (url.pathname !== `/api/v1/question-bank/practice/questions/${questionId}/ai/attachments`) return;
    uploadedScope = url.searchParams.get('scope');
    const headers = uploadRequest.headers();
    uploadObservation = {
      contentType: headers['content-type'],
      csrf: headers['x-hyz-csrf'],
    };
    uploadedRequest = uploadRequest;
  });
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
      const markdown = [
        '## 第 1 页',
        '手写的一句话，保留原文。',
        ...Array.from({ length: 32 }, (_, index) => `识别结果第 ${index + 1} 段，持续在结果区内部滚动。`),
      ].join('\n\n');
      return route.fulfill({ json: {
        markdown,
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
    name: 'answer.png', mimeType: 'image/png', buffer: imageBytes,
  });
  await expect(page.getByTestId('shenlun-ocr-images').getByRole('img', { name: 'answer.png' })).toBeVisible();
  await expect(page.getByTestId('shenlun-transcribe-start')).toBeEnabled();
  await expect.poll(() => uploadedScope).toBe('ocr');
  expect(uploadObservation?.contentType).toBe('image/png');
  expect(uploadObservation?.csrf).toBe(csrf);
  expect(uploadedRequest).not.toBeNull();
  const uploadResponse = await uploadedRequest!.response();
  expect(uploadResponse).not.toBeNull();
  expect(uploadResponse!.status()).toBe(200);
  attachmentId = ((await uploadResponse!.json()) as { id: string }).id;
  const relayResponse = await request.get(`${harnessOrigin}/ai-uploads`);
  expect(relayResponse.ok()).toBeTruthy();
  expect(await relayResponse.json()).toEqual({ media_type: 'image/png', bytes: Array.from(imageBytes) });
  await page.getByTestId('shenlun-transcribe-start').click();
  await expect(page.getByTestId('shenlun-ocr-markdown')).toContainText('手写的一句话，保留原文');
  await expect(page.getByTestId('shenlun-ocr-markdown').locator('h2')).toContainText('第 1 页');
  await expect(page.getByTestId('shenlun-ocr-tool-activity')).toContainText('工具调用');
  await expect(page.getByTestId('shenlun-ocr-tool')).toContainText('shell');
  await expect(page.getByTestId('shenlun-ocr-output')).toContainText('OCR 图像识别输出');
  await expect(page.getByTestId('shenlun-ocr-images').getByRole('img', { name: 'answer.png' })).toHaveCount(0);
  const results = page.getByTestId('shenlun-ocr-results');
  const panelBox = await page.getByTestId('shenlun-ai-panel').boundingBox();
  const composer = page.getByTestId('shenlun-ocr-composer');
  const composerBox = await composer.boundingBox();
  expect(panelBox).not.toBeNull();
  expect(composerBox).not.toBeNull();
  expect(composerBox!.y + composerBox!.height).toBeLessThanOrEqual(panelBox!.y + panelBox!.height + 1);
  const resultMetrics = await results.evaluate(element => ({
    overflowY: getComputedStyle(element).overflowY,
    clientHeight: element.clientHeight,
    scrollHeight: element.scrollHeight,
  }));
  expect(resultMetrics.overflowY).toMatch(/auto|scroll/);
  expect(resultMetrics.scrollHeight).toBeGreaterThan(resultMetrics.clientHeight);
  await results.evaluate(element => { element.scrollTop = element.scrollHeight; });
  await expect.poll(() => results.evaluate(element => element.scrollTop)).toBeGreaterThan(0);
  expect(await composer.boundingBox()).toMatchObject({ y: composerBox!.y, height: composerBox!.height });
  await expect(page.getByTestId('shenlun-transcription-confirm')).toHaveCount(0);
  await expect(page.getByTestId('shenlun-transcription-confirm-button')).toHaveCount(0);
  expect(ocrCount).toBe(1);
  expect(legacyWrites).toBe(0);
  await page.getByTestId('shenlun-mode-grade').click();
  await expect(page.getByTestId('question-bank-ai-review')).toBeVisible();
});

test('申论解析和批改复用工作区外观并呈现各自提示语', async ({ page }) => {
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
