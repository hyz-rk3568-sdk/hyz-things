import { expect, test, type Page } from '@playwright/test';
import { expectNoHorizontalOverflow, loginAsAdmin, resetHarness } from './fixtures';

const root = '/api/v1/question-bank';
const rows = [
  { question_id: 'supported', module: '数量关系', category: '数学运算' },
  { question_id: 'unsupported', module: '常识判断', category: '法律' },
].map((row, index) => ({ ...row, paper_id: 'paper', paper_title: '收藏测试卷', year: 2026, region: '国考', number: index + 1 }));
const favoriteState = new Map<string, boolean>();
const material = '## 完整材料\n\n材料中的数量为 **1200**。\n\n求结果 $A=B$';
const preview = (eligible = true) => ({
  eligible: true, reason: null,
  front: `${material}\n\n- A. 100\n- B. 200`, correct_answer: ['B'], recommended_target: eligible ? '真题练习/数量关系/数学运算/错题' : '真题练习/常识判断/法律/错题',
  allowed_targets: eligible ? ['真题练习/数量关系/数学运算/错题', '用户整理/数量关系/数学运算/错题', '用户整理/数量关系/工程效率/错题'] : ['真题练习/常识判断/法律/错题', '用户整理/常识判断/法律/错题'],
  requires_type_confirmation: false, source_revision: 'source-v1',
});
const detail = (row: (typeof rows)[number]) => ({
  item: row,
  question: {
    id: row.question_id, number: row.number, stem: '求结果', materials: ['M1'],
    options_markdown: '- A. 100\n- B. 200', module: row.module, category: row.category, topics: ['工程效率'],
  },
  materials: [{ id: 'M1', name: '材料', markdown: material }],
  correct_answer: ['B'], explanation: '先列方程，再代入。',
});

async function install(page: Page, collection: 'favorite' | 'wrong' = 'favorite') {
  favoriteState.clear();
  for (const row of rows) favoriteState.set(row.question_id, true);
  await loginAsAdmin(page);
  await page.route(`**${root}/**`, async route => {
    const url = new URL(route.request().url());
    const path = url.pathname.slice(root.length);
    if (path === '/papers') await route.fulfill({ json: [] });
    else if (path === '/practice/catalog') await route.fulfill({ json: { unclassified: 0, modules: rows.map(row => ({ name: row.module, count: 1, done: 0, uncategorized: 0, categories: [{ name: row.category, count: 1, done: 0 }] })) } });
    else if (path === '/practice/history') await route.fulfill({ json: [] });
    else if (path === '/practice/questions') {
      const mode = url.searchParams.get('mode');
      const list = mode === 'favorite' ? rows.filter(row => favoriteState.get(row.question_id)) : mode === collection ? rows : [];
      await route.fulfill({ json: list });
    }
    else if (/^\/practice\/questions\/[^/]+$/.test(path)) {
      const row = rows.find(value => value.question_id === decodeURIComponent(path.split('/').pop()!));
      await route.fulfill(row ? { json: detail(row) } : { status: 404, json: { error: 'question-not-found' } });
    }
    else if (/^\/practice\/favorites\/[^/]+$/.test(path)) {
      const id = decodeURIComponent(path.split('/').pop()!);
      const favorite = route.request().method() === 'POST'
        ? (route.request().postDataJSON() as { favorite: boolean }).favorite
        : favoriteState.get(id) ?? false;
      favoriteState.set(id, favorite);
      await route.fulfill({ json: { question_id: id, favorite } });
    }
    else if (path.endsWith('/flashcard-preview')) await route.fulfill({ json: preview(!path.includes('/unsupported/')) });
    else await route.continue();
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await page.getByTestId(`question-bank-collection-${collection}`).click();
}

async function openReview(page: Page, number = 1) {
  await page.getByTestId('question-bank-collection-item').filter({ hasText: `第 ${number} 题` }).click();
  const review = page.getByTestId('collection-review');
  await expect(review).toBeVisible();
  return review;
}

async function openCreate(page: Page) {
  const review = await openReview(page);
  await review.getByTestId('collection-review-create-flashcard').click();
  const panel = page.getByTestId('favorite-flashcard-panel');
  await expect(panel).toBeVisible();
  return panel;
}

test.beforeEach(async ({ request }) => { await resetHarness(request); });

test('user-organized destination requires an explicit choice', async ({ page }) => {
  await install(page);
  const card = (await openCreate(page)).getByTestId('favorite-flashcard-card');
  const target = card.getByLabel('目标牌组');
  await expect(target).toHaveValue('真题练习/数量关系/数学运算/错题');
  await card.getByTestId('favorite-flashcard-user-mode').click();
  await expect(target).toHaveValue('');
  await expect(card.getByRole('button', { name: '创建并同步', exact: true })).toBeDisabled();
});

test('changing the chosen directory requires a new confirmation', async ({ page }) => {
  await install(page);
  const card = (await openCreate(page)).getByTestId('favorite-flashcard-card');
  await card.getByTestId('favorite-flashcard-user-mode').click();
  await card.getByLabel('目标牌组').selectOption('用户整理/数量关系/数学运算/错题');
  await card.getByLabel('确认写入所选目录').check();
  await card.getByLabel('目标牌组').selectOption('用户整理/数量关系/工程效率/错题');
  await expect(card.getByLabel('确认写入所选目录')).not.toBeChecked();
  await expect(card.getByRole('button', { name: '创建并同步', exact: true })).toBeDisabled();
});

test('user-organized mode explains when no directories exist', async ({ page }) => {
  await install(page);
  await page.route('**/flashcard-preview', route => route.fulfill({ json: {
    ...preview(), allowed_targets: ['真题练习/数量关系/数学运算/错题'],
  } }));
  const card = (await openCreate(page)).getByTestId('favorite-flashcard-card');
  await card.getByTestId('favorite-flashcard-user-mode').click();
  await expect(card.getByTestId('favorite-flashcard-empty-directories')).toBeVisible();
  await expect(card.getByRole('button', { name: '创建并同步', exact: true })).toBeDisabled();
});

for (const width of [1180, 390]) {
  test(`favorite review creates a flashcard in a centered card and preserves filters at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await install(page);
    await expect(page.getByTestId('collection-review-create-flashcard')).toHaveCount(0);
    await page.getByTestId('question-bank-collection-module').filter({ hasText: '数量关系' }).click();
    await page.getByTestId('question-bank-collection-category').filter({ hasText: '数学运算' }).click();
    const review = await openReview(page);
    await expect(review).toContainText('完整材料');
    await expect(review.locator('strong').filter({ hasText: '1200' })).toBeVisible();
    await expect(review.locator('.katex').first()).toBeVisible();
    await expect(review.getByTestId('collection-review-answer')).toHaveText('B');
    await expect(review.getByRole('region', { name: '答案统计' })).toHaveClass(/rounded-box/);
    await expect(review.getByRole('region', { name: '答案统计' })).toContainText('本题累计');
    await expect(review.getByTestId('collection-review-choice').first()).toHaveCSS('font-size','16px');
    await expect(review.getByTestId('collection-review-explanation')).toHaveCSS('font-size','16px');
    await expect(review.getByTestId('collection-review-explanation')).toContainText('先列方程');
    await review.getByTestId('collection-review-create-flashcard').click();
    const panel = page.getByTestId('favorite-flashcard-panel');
    await expect(panel).toBeVisible();
    await expect(review.getByTestId('collection-review-split')).toHaveCount(0);
    const card = panel.getByTestId('favorite-flashcard-card');
    await expect(card.getByLabel('背面')).toHaveValue('');
    await card.getByTestId('favorite-flashcard-user-mode').click();
    await card.getByLabel('目标牌组').selectOption('用户整理/数量关系/数学运算/错题');
    await expect(card.getByRole('button', { name: '创建并同步', exact: true })).toBeDisabled();
    await card.getByLabel('背面').fill('先列方程，再代入。');
    await card.getByLabel('确认写入所选目录').check();
    const box = await card.boundingBox();
    expect(box!.width).toBeLessThanOrEqual(768);
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    let body: unknown;
    let csrf: string | undefined;
    await page.route(`**${root}/practice/questions/supported/flashcard`, async route => {
      body = route.request().postDataJSON();
      csrf = route.request().headers()['x-hyz-csrf'];
      await route.fulfill({ json: { status: 'created_synced', target_deck: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', card_id: 'new-card', retryable: false } });
    });
    await card.getByRole('button', { name: '创建并同步', exact: true }).click();
    await expect(card).toContainText('已创建');
    expect(body).toEqual({ creation_mode: 'user_organized', target_deck: '用户整理/数量关系/数学运算/错题', back: '先列方程，再代入。', source_revision: 'source-v1', type_confirmed: true });
    expect(csrf).toBeTruthy();
    await panel.getByRole('button', { name: '返回复盘', exact: true }).first().click();
    await expect(review.getByTestId('collection-review-split')).toBeVisible();
    await review.getByTestId('collection-review-back').click();
    await expect(page.getByTestId('question-bank-collection-item')).toHaveCount(1);
    await expect(page.getByTestId('question-bank-collection-category').filter({ hasText: '数学运算' })).toHaveAttribute('aria-pressed', 'true');
    await expectNoHorizontalOverflow(page);
  });
}

test('non-AI knowledge category creates an ordinary review flashcard', async ({ page }) => {
  await install(page);
  const review = await openReview(page, 2);
  await expect(review.getByTestId('collection-review-answer')).toHaveText('B');
  await review.getByTestId('collection-review-create-flashcard').click();
  const card = page.getByTestId('favorite-flashcard-card');
  const submit = card.getByRole('button', { name: '创建并同步', exact: true });
  await expect(submit).toBeDisabled();
  await expect(card.getByLabel('目标牌组')).toHaveValue('真题练习/常识判断/法律/错题');
  await card.getByLabel('目标牌组').selectOption('真题练习/常识判断/法律/错题');
  await expect(card.getByTestId('favorite-flashcard-path')).toHaveText('将追加或创建：flashcards/真题练习/常识判断/法律/错题/cards.md');
  await expect(submit).toBeDisabled();
  await card.getByLabel('确认写入所选目录').check();
  await expect(submit).toBeEnabled();
});

test('practice default and user-organized AI source remain independently selectable', async ({ page }) => {
  await install(page);
  const card = await openCreate(page);
  const submit = card.getByRole('button', {name:'创建并同步',exact:true});
  await expect(card.getByLabel('目标牌组')).toHaveValue('真题练习/数量关系/数学运算/错题');
  await card.getByLabel('目标牌组').selectOption('真题练习/数量关系/数学运算/错题');
  await expect(card.getByTestId('favorite-flashcard-path')).toContainText('flashcards/真题练习/数量关系/数学运算/错题/cards.md');
  await card.getByLabel('确认写入所选目录').check();
  await expect(submit).toBeEnabled();
  await card.getByTestId('favorite-flashcard-user-mode').click();
  await expect(submit).toBeDisabled();
  await expect(card.getByLabel('目标牌组')).toHaveValue('');
  await card.getByLabel('目标牌组').selectOption('用户整理/数量关系/数学运算/错题');
  await card.getByLabel('确认写入所选目录').check();
  await expect(card.getByTestId('favorite-flashcard-path')).toContainText('flashcards/用户整理/数量关系/数学运算/错题/cards.md');
  await expect(submit).toBeEnabled();
  await card.getByTestId('favorite-flashcard-practice-mode').click();
  await expect(card.getByLabel('目标牌组')).toHaveValue('真题练习/数量关系/数学运算/错题');
  await expect(submit).toBeDisabled();
});

test('wrong review opens the same split view with flashcard creation', async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 844 });
  await install(page, 'wrong');
  const review = await openReview(page);
  await expect(review.getByTestId('collection-review-split')).toBeVisible();
  await expect(review.getByTestId('collection-review-material-pane')).toContainText('完整材料');
  await expect(review.getByTestId('collection-review-answer')).toHaveText('B');
  await expect(review.getByTestId('collection-review-scope')).toContainText('错题复盘 · 第 1/2 题');
  await expect(review.getByTestId('collection-review-create-flashcard')).toBeVisible();
  await expect(review.getByTestId('collection-review-flashcard-unavailable')).toHaveCount(0);
  await review.getByTestId('collection-review-next').click();
  await expect(review.getByTestId('collection-review-scope')).toContainText('第 2/2 题');
  await expect(review.getByTestId('collection-review-heading')).toContainText('第 2 题');
  await expectNoHorizontalOverflow(page);
});

test('review cancels a saved favorite and refreshes the collection', async ({ page }) => {
  await install(page);
  const review = await openReview(page);
  const star = review.getByTestId('collection-review-favorite');
  await expect(star).toHaveAttribute('aria-label', '取消收藏');
  let body: unknown;
  await page.route(`**${root}/practice/favorites/supported`, async route => {
    body = route.request().postDataJSON();
    favoriteState.set('supported', false);
    await route.fulfill({ json: { question_id: 'supported', favorite: false } });
  });
  await star.click();
  await expect(star).toHaveAttribute('aria-label', '收藏题目');
  expect(body).toEqual({ favorite: false });
  await review.getByTestId('collection-review-back').click();
  await expect(page.getByTestId('question-bank-collection-item')).toHaveCount(1);
  await expect(page.getByTestId('question-bank-collection-item')).toContainText('第 2 题');
});

test('review reuses the training header with the favorite on the left and the scratchpad pen on the right', async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 844 });
  await install(page);
  const review = await openReview(page);
  const star = review.getByTestId('collection-review-favorite');
  const heading = review.getByTestId('collection-review-heading');
  const toggle = review.getByTestId('calculation-scratchpad-toggle');
  await expect(toggle).toHaveAttribute('aria-label', '开启草稿');
  const starBox = await star.boundingBox();
  const headingBox = await heading.boundingBox();
  const penBox = await toggle.boundingBox();
  expect(starBox!.x).toBeLessThan(headingBox!.x);
  expect(penBox!.x).toBeGreaterThanOrEqual(headingBox!.x + headingBox!.width);
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-label', '关闭草稿');
  await expect(review.getByRole('group', { name: '题目与解析滚动' })).toBeVisible();
  const canvas = review.getByTestId('calculation-scratchpad-canvas');
  await expect(canvas).toBeVisible();
  const box = await canvas.boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x + 80, box!.y + 120);
  await page.mouse.down();
  await page.mouse.move(box!.x + 180, box!.y + 160, { steps: 6 });
  await page.mouse.up();
  await expect(review.getByTestId('calculation-scratchpad-undo')).toBeEnabled();
  await toggle.click();
  await expect(canvas).toBeHidden();
  await expectNoHorizontalOverflow(page);
});

test('review swipe moves to the neighbouring saved question', async ({ page }) => {
  await page.setViewportSize({ width: 1180, height: 844 });
  await install(page, 'wrong');
  const review = await openReview(page);
  const surface = review.getByTestId('training-swipe-surface');
  await surface.evaluate(node => {
    const emit = (type: string, x: number) => node.dispatchEvent(new PointerEvent(type, {
      bubbles: true, pointerType: 'touch', pointerId: 31, isPrimary: true, clientX: x, clientY: 300,
    }));
    emit('pointerdown', 600); emit('pointermove', 540); emit('pointermove', 460);
  });
  await expect(review.getByTestId('collection-review-scope')).toContainText('第 2/2 题');
  await expect(review.getByTestId('collection-review-position')).toHaveText('2 / 2');
  await surface.evaluate(node => {
    node.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerType: 'touch', pointerId: 31, isPrimary: true, clientX: 460, clientY: 300,
    }));
  });
  await expect(page.locator('[data-page-turn="out"]')).toHaveCount(0, { timeout: 2000 });
  await expect(review.getByTestId('collection-review-heading')).toContainText('第 2 题');
});

test('favorite review retries failed flashcard loading and ignores a closed question response', async ({ page }) => {
  await install(page);
  let reads = 0;
  await page.route(`**${root}/practice/questions/supported/flashcard-preview`, async route => {
    reads += 1;
    await route.fulfill(reads === 1 ? { status: 503, json: { error: 'temporary-unavailable' } } : { json: preview() });
  });
  const review = await openReview(page);
  await review.getByTestId('collection-review-flashcard-retry').click();
  await expect(review.getByTestId('collection-review-create-flashcard')).toBeVisible();
  expect(reads).toBe(2);
  await review.getByTestId('collection-review-back').click();
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**${root}/practice/questions/supported/flashcard-preview`, async route => {
    await gate;
    await route.fulfill({ json: { ...preview(), front: '过期响应不得显示' } });
  });
  await openReview(page);
  await review.getByTestId('collection-review-back').click();
  await openReview(page, 2);
  release();
  await expect(review.getByTestId('collection-review-create-flashcard')).toBeVisible();
  await expect(review).not.toContainText('过期响应不得显示');
});

test('favorite flashcard sync recovery retries the identical request and blocks duplicate submits', async ({ page }) => {
  await install(page);
  const modal = await openCreate(page);
  await modal.getByLabel('背面').fill('恢复同步的背面');
  await modal.getByTestId('favorite-flashcard-user-mode').click();
  await modal.getByLabel('目标牌组').selectOption('用户整理/数量关系/数学运算/错题');
  await modal.getByLabel('确认写入所选目录').check();
  const requests: unknown[] = [];
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**${root}/practice/questions/supported/flashcard`, async route => {
    requests.push(route.request().postDataJSON());
    if (requests.length === 1) await gate;
    await route.fulfill({ json: { status: requests.length === 1 ? 'created_sync_pending' : 'already_exists', target_deck: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', card_id: 'existing-card', retryable: requests.length === 1 } });
  });
  const submit = modal.getByRole('button', { name: '创建并同步', exact: true });
  await submit.click();
  await expect(modal.getByRole('button', { name: '正在保存…', exact: true })).toBeDisabled();
  expect(requests).toHaveLength(1);
  release();
  await expect(modal).toContainText('待同步');
  await expect(modal.getByLabel('背面')).toBeDisabled();
  await modal.getByRole('button', { name: '重试同步', exact: true }).click();
  await expect(modal).toContainText('已存在');
  expect(requests).toHaveLength(2);
  expect(requests[1]).toEqual(requests[0]);
});

for (const width of [1180, 390]) {
  test(`favorite flashcard permits a blank Back at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await install(page);
    const modal = await openCreate(page);
    await expect(modal.getByLabel('背面')).toHaveValue('');
    await modal.getByTestId('favorite-flashcard-user-mode').click();
  await modal.getByLabel('目标牌组').selectOption('用户整理/数量关系/数学运算/错题');
    await modal.getByLabel('确认写入所选目录').check();
    const submit = modal.getByRole('button', { name: '创建并同步', exact: true });
    await expect(submit).toBeEnabled();
    let body: unknown;
    await page.route(`**${root}/practice/questions/supported/flashcard`, async route => {
      body = route.request().postDataJSON();
      await route.fulfill({ json: { status: 'created_synced', target_deck: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', card_id: 'blank-back', retryable: false } });
    });
    await submit.click();
    await expect(modal).toContainText('已创建并同步');
    expect(body).toEqual({ creation_mode: 'user_organized', target_deck: '用户整理/数量关系/数学运算/错题', back: '', source_revision: 'source-v1', type_confirmed: true });
    await expectNoHorizontalOverflow(page);
  });
}

test('collection review prefers existing question AI analysis', async ({ page }) => {
  await install(page);
  await page.route(`**${root}/practice/questions/*/ai`, async route => {
    const question_id = new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({ json: { question_id, status: 'complete', error: null,
      messages: [{ role: 'assistant', content: '收藏题已有 AI 解析', created_at: 1 }] } });
  });
  const review = await openReview(page);
  await expect(review.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed', 'true');
  await expect(review.getByTestId('question-bank-ai-message')).toContainText('收藏题已有 AI 解析');
});

test('wrong review can delete saved question AI only after confirmation', async ({ page }) => {
  await install(page, 'wrong');
  let deleted = false;
  let deleteCsrf = '';
  await page.route(`**${root}/practice/questions/*/ai`, async route => {
    const question_id = new URL(route.request().url()).pathname.split('/').at(-2)!;
    if (route.request().method() === 'DELETE') {
      deleted = true;
      deleteCsrf = route.request().headers()['x-hyz-csrf'];
    }
    await route.fulfill({ json: { question_id, status: deleted ? 'idle' : 'complete', error: null,
      messages: deleted ? [] : [{ role: 'assistant', content: '错题已有解析', created_at: 1 }] } });
  });
  const review = await openReview(page);
  await expect(review.getByTestId('question-bank-ai-message')).toContainText('错题已有解析');
  await review.getByTestId('question-bank-ai-delete').click();
  await review.getByTestId('question-bank-ai-delete-cancel').click();
  expect(deleted).toBe(false);
  await review.getByTestId('question-bank-ai-delete').click();
  await review.getByTestId('question-bank-ai-delete-confirm').click();
  await expect(review.getByTestId('question-bank-ai-start')).toBeVisible();
  expect(deleted).toBe(true);
  expect(deleteCsrf).toBeTruthy();
});

for (const action of ['stop', 'delete'] as const) {
  test(`a delayed AI poll cannot overwrite ${action} result`, async ({ page }) => {
    await install(page);
    let gets = 0;
    let pollPending = false;
    let release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    const runningMessage = { role: 'assistant', content: '旧的生成内容', created_at: 1 };
    await page.route(`**${root}/practice/questions/*/ai**`, async route => {
      const path = new URL(route.request().url()).pathname;
      const question_id = path.split('/').at(path.endsWith('/stop') ? -3 : -2)!;
      const method = route.request().method();
      if (method === 'GET') {
        gets += 1;
        if (gets === 3) {
          pollPending = true;
          await pending;
        }
        await route.fulfill({ json: { question_id, status: 'running', error: null, messages: [runningMessage] } });
      } else if (method === 'DELETE') {
        await route.fulfill({ json: { question_id, status: 'idle', error: null, messages: [] } });
      } else {
        await route.fulfill({ json: { question_id, status: 'stopped', error: null, messages: [runningMessage] } });
      }
    });
    const review = await openReview(page);
    await expect(review.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
    await expect.poll(() => pollPending).toBe(true);
    const expectFinalState = async () => {
      if (action === 'stop') {
        await expect(review.getByTestId('question-bank-ai-status')).toHaveText('已停止');
        await expect(review.getByTestId('question-bank-ai-stop')).toHaveCount(0);
      } else {
        await expect(review.getByTestId('question-bank-ai-start')).toBeVisible();
        await expect(review.getByTestId('question-bank-ai-message')).toHaveCount(0);
      }
    };
    if (action === 'stop') {
      await review.getByTestId('question-bank-ai-stop').click();
    } else {
      await review.getByTestId('question-bank-ai-delete').click();
      await review.getByTestId('question-bank-ai-delete-confirm').click();
    }
    await expectFinalState();
    const oldResponse = page.waitForResponse(response => response.url().endsWith('/ai') && response.request().method() === 'GET');
    release();
    await oldResponse;
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expectFinalState();
  });
}

test('collection review displays a safe AI SVG diagram', async ({ page }) => {
  await install(page);
  await page.route(`**${root}/practice/questions/*/ai`, async route => {
    const question_id = new URL(route.request().url()).pathname.split('/').at(-2)!;
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 80"><line x1="10" y1="40" x2="150" y2="40" stroke="#333"/><text x="10" y="60">收藏题线段图</text></svg>';
    await route.fulfill({ json: { question_id, status: 'complete', error: null,
      messages: [{ role: 'assistant', content: '```svg\n'+svg+'\n```', created_at: 1 }] } });
  });
  const review = await openReview(page);
  const image = review.getByTestId('question-bank-ai-diagram');
  await expect(image).toBeVisible();
  await expect.poll(()=>image.evaluate((node:HTMLImageElement)=>node.complete && node.naturalWidth>0)).toBe(true);
});
