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

const existingCard = {
  card_id: '019a31b7-8c42-7c16-a6d1-0c1f0f34c821',
  deck_path: '真题练习/数量关系/数学运算/错题',
  source_file: '真题练习/数量关系/数学运算/错题/cards.md',
  front: `${material}\n\n- A. 100\n- B. 200`,
  back: '原来的背面',
  mode: 'practice',
};

async function mockExistingPreview(page: Page, cards: unknown[] = [existingCard]) {
  await page.route(`**${root}/practice/questions/supported/flashcard-preview`, route => route.fulfill({
    json: { ...preview(), existing: cards },
  }));
}

test('existing Flashcard opens its editor directly with a collapsed preview', async ({ page }) => {
  await install(page);
  await mockExistingPreview(page);
  const review = await openReview(page);
  await expect(review.getByTestId('collection-review-manage-flashcard')).toBeVisible();
  await expect(review.getByTestId('favorite-existing-card')).toHaveCount(0);
  await review.getByTestId('collection-review-manage-flashcard').click();
  const editor = page.getByTestId('favorite-flashcard-editor');
  await expect(editor).toBeVisible();
  await expect(page.getByTestId('favorite-existing-cards')).toHaveCount(0);
  await expect(editor.getByTestId('favorite-flashcard-path')).toContainText(existingCard.source_file);
  const toggle = editor.getByTestId('favorite-flashcard-preview-toggle');
  const front = editor.getByTestId('favorite-flashcard-front-preview');
  await expect(front).toBeHidden();
  await toggle.click();
  await expect(front).toBeVisible();
  await expect(front).toContainText('完整材料');
  await expect(editor.getByTestId('favorite-flashcard-back-preview')).toContainText('原来的背面');
  await toggle.click();
  await expect(front).toBeHidden();
  await page.getByRole('button', { name: '返回复盘' }).click();
  await expect(review.getByTestId('collection-review-manage-flashcard')).toBeVisible();
  await expect(review.getByTestId('collection-review-create-flashcard')).toHaveCount(0);
  await expect(review.getByTestId('collection-review-flashcard-unavailable')).toHaveCount(0);
});

test('editing an existing flashcard saves and syncs in one request', async ({ page }) => {
  await install(page);
  let current = { ...existingCard };
  await page.route(`**${root}/practice/questions/supported/flashcard-preview`, route => route.fulfill({
    json: { ...preview(), allowed_targets: ['真题练习/数量关系/数学运算/错题'], existing: [current] },
  }));
  let method = '';
  let body: unknown;
  let csrf = '';
  await page.route(`**${root}/practice/questions/supported/flashcard/${existingCard.card_id}`, async route => {
    method = route.request().method();
    body = route.request().postDataJSON();
    csrf = route.request().headers()['x-hyz-csrf'];
    current = { ...current, back: (body as { back: string }).back };
    await route.fulfill({ json: {
      status: 'updated_synced', card_id: current.card_id, source_file: current.source_file, retryable: false,
    } });
  });
  const review = await openReview(page);
  await review.getByTestId('collection-review-manage-flashcard').click();
  const editor = page.getByTestId('favorite-flashcard-editor');
  await expect(editor).toBeVisible();
  await expect(editor.getByTestId('favorite-flashcard-editor-back')).toHaveValue('原来的背面');
  await editor.getByTestId('favorite-flashcard-editor-back').fill('更新后的背面');
  await editor.getByTestId('favorite-flashcard-save').click();
  await expect(editor).toHaveCount(0);
  await expect(review.getByTestId('collection-review-manage-flashcard')).toBeVisible();
  expect(method).toBe('PUT');
  expect(body).toEqual({ back: '更新后的背面' });
  expect(csrf).toBeTruthy();
});

test('deleting an existing flashcard requires confirmation and syncs in one request', async ({ page }) => {
  await install(page);
  let current: typeof existingCard | null = { ...existingCard };
  await page.route(`**${root}/practice/questions/supported/flashcard-preview`, route => route.fulfill({
    json: { ...preview(), allowed_targets: ['真题练习/数量关系/数学运算/错题'], existing: current ? [current] : [] },
  }));
  let deletes = 0;
  let method = '';
  let csrf = '';
  await page.route(`**${root}/practice/questions/supported/flashcard/${existingCard.card_id}`, async route => {
    method = route.request().method();
    deletes += 1;
    csrf = route.request().headers()['x-hyz-csrf'];
    current = null;
    await route.fulfill({ json: {
      status: 'deleted_synced', card_id: existingCard.card_id, source_file: existingCard.source_file, retryable: false,
    } });
  });
  const review = await openReview(page);
  await review.getByTestId('collection-review-manage-flashcard').click();
  const editor = page.getByTestId('favorite-flashcard-editor');
  await editor.getByTestId('favorite-flashcard-delete').click();
  await expect(editor.getByTestId('favorite-flashcard-confirm-delete')).toBeVisible();
  await editor.getByTestId('favorite-flashcard-cancel-delete').click();
  expect(deletes).toBe(0);
  await editor.getByTestId('favorite-flashcard-delete').click();
  await editor.getByTestId('favorite-flashcard-confirm-delete').click();
  await expect(editor).toHaveCount(0);
  expect(method).toBe('DELETE');
  expect(deletes).toBe(1);
  expect(csrf).toBeTruthy();
  await expect(page.getByTestId('favorite-existing-card')).toHaveCount(0);
  await expect(page.getByTestId('collection-review-create-flashcard')).toBeVisible();
});

test('pending edit retries the same Back without changing the source again', async ({ page }) => {
  await install(page);
  await mockExistingPreview(page);
  const bodies: unknown[] = [];
  await page.route(`**${root}/practice/questions/supported/flashcard/${existingCard.card_id}`, async route => {
    bodies.push(route.request().postDataJSON());
    await route.fulfill({ json: {
      status: bodies.length === 1 ? 'updated_sync_pending' : 'updated_synced',
      card_id: existingCard.card_id,
      source_file: existingCard.source_file,
      retryable: bodies.length === 1,
    } });
  });
  const review = await openReview(page);
  await review.getByTestId('collection-review-manage-flashcard').click();
  const editor = page.getByTestId('favorite-flashcard-editor');
  await editor.getByTestId('favorite-flashcard-editor-back').fill('待同步的背面');
  await editor.getByTestId('favorite-flashcard-save').click();
  await expect(editor).toContainText('等待同步');
  await expect(editor.getByTestId('favorite-flashcard-save')).toHaveText('重试同步');
  await editor.getByTestId('favorite-flashcard-save').click();
  await expect(editor).toHaveCount(0);
  expect(bodies).toHaveLength(2);
  expect(bodies[1]).toEqual(bodies[0]);
});

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
    let created = false;
    await page.route(`**${root}/practice/questions/supported/flashcard-preview`, route => route.fulfill({
      json: { ...preview(), existing: created ? [{ ...existingCard, deck_path: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', back: '先列方程，再代入。', mode: 'user_organized' }] : [] },
    }));
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
      created = true;
      await route.fulfill({ json: { status: 'created_synced', target_deck: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', card_id: 'new-card', retryable: false } });
    });
    await card.getByRole('button', { name: '创建并同步', exact: true }).click();
    await expect(panel).toHaveCount(0);
    expect(body).toEqual({ creation_mode: 'user_organized', target_deck: '用户整理/数量关系/数学运算/错题', back: '先列方程，再代入。', source_revision: 'source-v1', type_confirmed: true });
    expect(csrf).toBeTruthy();
    await expect(review.getByTestId('collection-review-split')).toBeVisible();
    await expect(review.getByTestId('collection-review-manage-flashcard')).toBeVisible();
    await expect(review.getByTestId('favorite-existing-card')).toHaveCount(0);
    await review.getByTestId('collection-review-manage-flashcard').click();
    await expect(page.getByTestId('favorite-flashcard-editor')).toContainText('用户整理/数量关系/数学运算/错题/cards.md');
    await page.getByRole('button', { name:'返回复盘' }).click();
    await expect(review.getByTestId('collection-review-create-flashcard')).toHaveCount(0);
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

test('review keeps the scratchpad pen fixed at the far right', async ({ page }) => {
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
  expect(starBox!.x).toBeGreaterThanOrEqual(headingBox!.x + headingBox!.width);
  expect(penBox!.x).toBeGreaterThan(starBox!.x);
  const paneBox = await review.getByTestId('collection-review-question-pane').boundingBox();
  expect(penBox!.x + penBox!.width).toBeGreaterThanOrEqual(paneBox!.x + paneBox!.width - 16);
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-label', '关闭草稿');
  expect(Math.abs((await toggle.boundingBox())!.x - penBox!.x)).toBeLessThanOrEqual(1);
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
  await page.route(`**${root}/practice/questions/supported/flashcard-preview`, route => route.fulfill({
    json: { ...preview(), existing: requests.length >= 2 ? [{ ...existingCard, deck_path: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', back: '恢复同步的背面', mode: 'user_organized' }] : [] },
  }));
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
  await expect(modal).toHaveCount(0);
  expect(requests).toHaveLength(2);
  expect(requests[1]).toEqual(requests[0]);
  await expect(page.getByTestId('collection-review-manage-flashcard')).toBeVisible();
  await expect(page.getByTestId('collection-review-create-flashcard')).toHaveCount(0);
});

for (const width of [1180, 390]) {
  test(`favorite flashcard permits a blank Back at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await install(page);
    let created = false;
    await page.route(`**${root}/practice/questions/supported/flashcard-preview`, route => route.fulfill({
      json: { ...preview(), existing: created ? [{ ...existingCard, deck_path: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', back: '-', mode: 'user_organized' }] : [] },
    }));
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
      created = true;
      await route.fulfill({ json: { status: 'created_synced', target_deck: '用户整理/数量关系/数学运算/错题', source_file: '用户整理/数量关系/数学运算/错题/cards.md', card_id: 'blank-back', retryable: false } });
    });
    await submit.click();
    await expect(modal).toHaveCount(0);
    expect(body).toEqual({ creation_mode: 'user_organized', target_deck: '用户整理/数量关系/数学运算/错题', back: '', source_revision: 'source-v1', type_confirmed: true });
    await expect(page.getByTestId('collection-review-manage-flashcard')).toBeVisible();
    await expect(page.getByTestId('collection-review-create-flashcard')).toHaveCount(0);
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

test('question AI image picker uploads validated attachments and sends IDs', async ({ page }) => {
  await install(page);
  const starts: Array<{ instruction: string; attachments: string[] }> = [];
  let uploads = 0;
  await page.route(`**${root}/practice/questions/*/ai/attachments`, async route => {
    uploads += 1;
    expect(route.request().headers()['x-hyz-csrf']).toBeTruthy();
    expect(route.request().headers()['content-type']).toBe('image/png');
    await route.fulfill({ json: { id: `attachment-${uploads}` } });
  });
  await page.route(`**${root}/practice/questions/*/ai`, async route => {
    const question_id = new URL(route.request().url()).pathname.split('/').at(-2)!;
    if (route.request().method() === 'POST') {
      starts.push(route.request().postDataJSON());
      await route.fulfill({ status: 202, json: { question_id, status: 'running', error: null, messages: [], request: { request_id: route.request().postDataJSON().request_id, run_id: 'run-1', intent: 'initial', status: 'running', error: null, remote_uncertain: false, new_thread_requested: false, user_message_id: null, retry_of: null } } });
    } else {
      await route.fulfill({ json: { question_id, status: 'idle', error: null, messages: [] } });
    }
  });
  const review = await openReview(page);
  await review.getByTestId('question-bank-ai-tab').click();
  const composer = review.getByTestId('question-bank-ai-requirements-compose');
  await expect(composer.getByTestId('question-bank-ai-start-images')).toBeVisible();
  const picker = review.getByTestId('question-bank-ai-start-images');
  const input = picker.locator('input[type=file]');
  await input.setInputFiles({ name: 'note.png', mimeType: 'image/png', buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAFElEQVR4nGP8r9DHAANMDEgANwcAVT8BtalpBPoAAAAASUVORK5CYII=', 'base64') });
  await expect(picker.getByAltText('note.png')).toBeVisible();
  await expect.poll(() => picker.getByAltText('note.png').evaluate((node: HTMLImageElement) => node.naturalWidth)).toBe(4);
  const imageBox = await picker.getByAltText('note.png').boundingBox();
  const plusBox = await picker.getByLabel('添加图片').boundingBox();
  const textBox = await review.getByTestId('question-bank-ai-instruction').boundingBox();
  expect(imageBox!.x).toBeGreaterThan(plusBox!.x);
  expect(textBox!.x).toBeGreaterThan(imageBox!.x);
  expect(Math.abs(textBox!.y + textBox!.height / 2 - imageBox!.y - imageBox!.height / 2)).toBeLessThan(3);
  await input.setInputFiles({ name: 'note.gif', mimeType: 'image/gif', buffer: Buffer.from('GIF89a') });
  await expect(picker.getByRole('alert')).toContainText('仅支持 JPG 或 PNG');
  expect(uploads).toBe(1);
  await review.getByTestId('question-bank-ai-instruction').fill('看图讲题');
  await review.getByTestId('question-bank-ai-start').click();
  await expect.poll(() => starts.length).toBe(1);
  expect(starts[0]).toMatchObject({ instruction: '看图讲题', attachments: ['attachment-1'], request_id: expect.any(String) });
  await expect(picker.getByAltText('note.png')).toHaveCount(0);
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
  await review.getByRole('button',{name:'更多解析操作'}).click();
  await review.getByTestId('question-bank-ai-delete').click();
  await review.getByTestId('question-bank-ai-delete-cancel').click();
  expect(deleted).toBe(false);
  await review.getByRole('button',{name:'更多解析操作'}).click();
  await review.getByTestId('question-bank-ai-delete').click();
  await review.getByTestId('question-bank-ai-delete-confirm').click();
  await expect(review.getByTestId('question-bank-ai-start')).toBeVisible();
  expect(deleted).toBe(true);
  expect(deleteCsrf).toBeTruthy();
});

for (const action of ['stop', 'delete'] as const) {
  test(`a delayed AI poll cannot overwrite ${action} result`, async ({ page }) => {
    await install(page);
    await page.evaluate(() => {
      const streams: any[] = [];
      (window as any).__delayedStreams = streams;
      class FakeEventSource extends EventTarget {
        onmessage: ((event: MessageEvent) => void) | null = null;
        onerror: ((event: Event) => void) | null = null;
        constructor(public url: string) { super(); streams.push(this); }
        close() {}
      }
      (window as any).EventSource = FakeEventSource;
    });
    let gets = 0;
    let pollPending = false;
    let release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    const runningMessage = { role: 'assistant', content: '旧的生成内容', created_at: 1 };
    await page.route(`**${root}/practice/questions/*/ai**`, async route => {
      const path = new URL(route.request().url()).pathname;
      if (!path.endsWith('/ai') && !path.endsWith('/ai/stop')) return route.fallback();
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
    await expect.poll(() => page.evaluate(() => (window as any).__delayedStreams.length)).toBeGreaterThan(0);
    await page.evaluate(() => {
      const streams = (window as any).__delayedStreams;
      const stream = streams[streams.length - 1];
      const question_id = new URL(stream.url, location.origin).pathname.split('/').at(-3);
      stream.dispatchEvent(new MessageEvent('snapshot', { data: JSON.stringify({
        version: 1, question_id, stem_fingerprint: 'fingerprint', run_id: 'old-run',
        seq: 1, status: 'running', text: '', progress: []
      }) }));
    });
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
      await review.getByRole('button',{name:'更多解析操作'}).click();
      await review.getByTestId('question-bank-ai-delete').click();
      await review.getByTestId('question-bank-ai-delete-confirm').click();
    }
    await expectFinalState();
    // The SSE refresh may be aborted when the stream is replaced after the mutation.
    release();
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expectFinalState();
  });
}

test('create and edit share one form with a live Back preview', async ({ page }) => {
  await install(page);
  const card = (await openCreate(page)).getByTestId('favorite-flashcard-card');
  const preview = card.getByTestId('favorite-flashcard-back-preview');
  await expect(preview).toBeHidden();
  await card.getByTestId('favorite-flashcard-preview-toggle').click();
  await expect(preview).toBeVisible();
  await expect(preview).toContainText('背面预览');
  await card.getByLabel('背面').fill('设 $x^2$ 为 **最终答案**');
  await expect(preview).toContainText('最终答案');
  await expect(preview.locator('.katex').first()).toBeVisible();
});

test('editing an existing card uses the same form and previews Back while typing', async ({ page }) => {
  await install(page);
  await mockExistingPreview(page);
  const review = await openReview(page);
  await review.getByTestId('collection-review-manage-flashcard').click();
  const editor = page.getByTestId('favorite-flashcard-editor');
  await editor.getByTestId('favorite-flashcard-preview-toggle').click();
  const preview = editor.getByTestId('favorite-flashcard-back-preview');
  await expect(preview).toContainText('原来的背面');
  await editor.getByTestId('favorite-flashcard-editor-back').fill('改为 **新的背面**');
  await expect(preview).toContainText('新的背面');
  await expect(preview.locator('strong').first()).toHaveText('新的背面');
});

test('creating a flashcard can also favorite the question explicitly', async ({ page }) => {
  await install(page);
  let favoriteBody: unknown = null;
  await page.route(`**${root}/practice/favorites/supported`, async route => {
    favoriteBody = route.request().postDataJSON();
    await route.fulfill({ json: { question_id: 'supported', favorite: true } });
  });
  await page.route(`**${root}/practice/questions/supported/flashcard`, route => route.fulfill({ json: {
    status: 'created_synced',
    target_deck: '真题练习/数量关系/数学运算/错题',
    source_file: '真题练习/数量关系/数学运算/错题/cards.md',
    card_id: '019a31b7-8c42-7c16-a6d1-0c1f0f34c822',
    retryable: false,
  } }));
  const card = (await openCreate(page)).getByTestId('favorite-flashcard-card');
  const alsoFavorite = card.getByLabel('同时收藏本题');
  await expect(alsoFavorite).not.toBeChecked();
  await card.getByLabel('确认写入所选目录').check();
  await alsoFavorite.check();
  await card.getByRole('button', { name: '创建并同步', exact: true }).click();
  await expect.poll(() => favoriteBody).toEqual({ favorite: true });
});

test('wide Markdown stays inside the favorite form and scrolls locally', async ({ page }) => {
  const longInline = 'A'.repeat(180);
  const hash = 'a'.repeat(64);
  await page.setViewportSize({ width: 390, height: 844 });
  await install(page);
  await page.route(`**${root}/practice/questions/supported/flashcard-preview`, route => route.fulfill({
    json: { ...preview(), front: [
      `行内：\\(\\text{${longInline}}\\)`,
      '```',
      'B'.repeat(220),
      '```',
      `![图](hyz-bank-asset:${hash}.png)`,
    ].join('\n\n') },
  }));
  await page.route('**/api/v1/question-bank/assets/**', route => route.fulfill({
    contentType: 'image/svg+xml',
    body: '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="900"><rect width="1200" height="900" fill="#eee"/></svg>',
  }));
  const card = (await openCreate(page)).getByTestId('favorite-flashcard-card');
  await card.getByTestId('favorite-flashcard-preview-toggle').click();
  const front = card.getByTestId('favorite-flashcard-front-preview');
  const inlineMath = front.locator('.math-inline');
  const code = front.locator('pre');
  const media = front.locator('.markdown-media');
  await expect(inlineMath.locator('.katex')).toBeVisible();
  await expect(code).toContainText('B'.repeat(220));
  await expect.poll(() => media.locator('img').evaluate((image: HTMLImageElement) => image.complete && image.naturalWidth > 0)).toBe(true);
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBeLessThanOrEqual(1);
  await expect.poll(() => front.evaluate(node => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1);
  for (const target of [inlineMath, code, media]) {
    await expect.poll(() => target.evaluate(node => node.scrollWidth - node.clientWidth)).toBeGreaterThan(10);
    await target.evaluate(node => { node.scrollLeft = node.scrollWidth; });
    await expect.poll(() => target.evaluate(node => node.scrollLeft)).toBeGreaterThan(10);
  }
  await expect.poll(() => media.evaluate(node => node.scrollHeight - node.clientHeight)).toBeGreaterThan(10);
  await media.evaluate(node => { node.scrollTop = node.scrollHeight; });
  await expect.poll(() => media.evaluate(node => node.scrollTop)).toBeGreaterThan(10);
});

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
  const message = review.getByTestId('question-bank-ai-message').first();
  await message.getByRole('button', { name: '回复操作' }).click();
  await message.getByRole('button', { name: '查看源码' }).click();
  await expect(message.getByTestId('question-bank-ai-source')).toContainText('<svg');
  await expect(message.getByRole('button', { name: '回复操作' })).toHaveAttribute('aria-expanded', 'false');
  await message.getByRole('button', { name: '回复操作' }).click();
  await message.getByRole('button', { name: '查看解析' }).click();
  await expect(image).toBeVisible();
});

test('question AI pastes an image into the composer', async ({ page }) => {
  await install(page);
  await page.route(`**${root}/practice/questions/*/ai/attachments`, route => route.fulfill({ json: { id: 'pasted-image' } }));
  await page.route(`**${root}/practice/questions/*/ai`, route => route.fulfill({ json: { question_id: 'supported', status: 'idle', error: null, messages: [] } }));
  const review = await openReview(page);
  await review.getByTestId('question-bank-ai-tab').click();
  const input = review.getByTestId('question-bank-ai-instruction');
  await input.evaluate(element => {
    const data = new DataTransfer();
    data.items.add(new File(['image'], 'screenshot.png', { type: 'image/png' }));
    element.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: data }));
  });
  await expect(review.getByTestId('question-bank-ai-start-images').getByAltText('screenshot.png')).toBeVisible();
});

test('question AI exposes and copies each reply markdown source', async ({ page }) => {
  await install(page);
  const source = '**解题**\n\n$S=\\frac{1}{2}vT$\n\n```svg\n<svg></svg>\n```';
  await page.route(`**${root}/practice/questions/*/ai`, route => route.fulfill({ json: {
    question_id: 'supported', status: 'complete', error: null,
    messages: [{ role: 'assistant', content: source, created_at: 1 }, { role: 'user', content: '追问', created_at: 2 }, { role: 'assistant', content: '第二条 $x$', created_at: 3 }],
  } }));
  await page.evaluate(() => {
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: (text: string) => { (window as any).__copied = text; return Promise.resolve(); } } });
  });
  const review = await openReview(page);
  await review.getByTestId('question-bank-ai-tab').click();
  const messages = review.getByTestId('question-bank-ai-message');
  const first = messages.nth(0);
  await first.getByRole('button', { name: '回复操作' }).click();
  await first.getByRole('button', { name: '查看源码' }).click();
  await expect(first.getByTestId('question-bank-ai-source')).toHaveText(source);
  await first.getByRole('button', { name: '回复操作' }).click();
  await first.getByRole('button', { name: '复制源码' }).click();
  await expect.poll(() => page.evaluate(() => (window as any).__copied)).toBe(source);
  await expect(messages.nth(1).getByRole('button', { name: '查看源码' })).toHaveCount(0);
  await messages.nth(2).getByRole('button', { name: '回复操作' }).click();
  await messages.nth(2).getByRole('button', { name: '查看源码' }).click();
  await expect(messages.nth(2).getByTestId('question-bank-ai-source')).toHaveText('第二条 $x$');
  await first.getByRole('button', { name: '回复操作' }).click();
  await first.getByRole('button', { name: '查看解析' }).click();
  await expect(first.getByTestId('question-bank-ai-source')).toHaveCount(0);
});


test('flashcard creation previews pasted SVG Markdown', async ({ page }) => {
  await install(page);
  const card = (await openCreate(page)).getByTestId('favorite-flashcard-card');
  await card.getByTestId('favorite-flashcard-preview-toggle').click();
  await card.getByLabel('背面').fill('```svg\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50"><text x="5" y="25">图示</text></svg>\n```');
  await expect(card.getByTestId('favorite-flashcard-back-preview').getByTestId('question-bank-ai-diagram')).toBeVisible();
});

for (const viewport of [{width:1366,height:900},{width:1133,height:744},{width:390,height:844}]) {
  test(`collection review stem and action headers align at ${viewport.width}px`,async({page})=>{
    await page.setViewportSize(viewport);
    await install(page);
    await page.route(`**${root}/practice/questions/supported`,route=>{
      const value=detail(rows[0]);
      value.materials=[];
      value.question.materials=[];
      return route.fulfill({json:value});
    });
    await page.route(`**${root}/practice/questions/supported/ai`,route=>route.fulfill({json:{question_id:'supported',status:'complete',error:null,messages:[{id:1,role:'assistant',content:'解析',created_at:1}]}}));
    const review=await openReview(page);
    await expect(review.getByTestId('question-bank-ai-input')).toBeVisible();
    const left=review.getByTestId('collection-review-stem-pane').locator('.question-bank-pane-header');
    const pane=review.getByTestId('collection-review-question-pane');
    const right=pane.locator('.question-bank-pane-header');
    const leftBox=await left.boundingBox(),rightBox=await right.boundingBox();
    expect(Math.abs(leftBox!.height-rightBox!.height)).toBeLessThanOrEqual(1);
    if(viewport.width>=768) expect(Math.abs((leftBox!.y+leftBox!.height)-(rightBox!.y+rightBox!.height))).toBeLessThanOrEqual(1);
    const buttons=[pane.getByTestId('collection-review-favorite'),pane.getByTestId('question-bank-ai-tab'),pane.getByRole('button',{name:'更多解析操作'}),pane.getByTestId('calculation-scratchpad-toggle')];
    const sizes=await Promise.all(buttons.map(button=>button.boundingBox()));
    for(const size of sizes) {
      expect(Math.abs(size!.width-sizes[0]!.width)).toBeLessThanOrEqual(1);
      expect(Math.abs(size!.height-sizes[0]!.height)).toBeLessThanOrEqual(1);
    }
    const composer=await pane.getByTestId('question-bank-ai-compose').boundingBox();
    const send=await pane.getByTestId('question-bank-ai-send').boundingBox();
    expect(Math.abs((send!.y+send!.height/2)-(composer!.y+composer!.height/2))).toBeLessThanOrEqual(1);
    expect(send!.y).toBeGreaterThanOrEqual(composer!.y);
    expect(send!.y+send!.height).toBeLessThanOrEqual(composer!.y+composer!.height);
  });
}

for (const viewport of [{width:1133,height:744},{width:390,height:844}]) {
  test(`collection review splitter adjusts and persists at ${viewport.width}px`,async({page})=>{
    await page.setViewportSize(viewport);
    await install(page);
    const review=await openReview(page);
    const handle=review.getByTestId('training-split-handle');
    const horizontal=viewport.width>=768;
    await expect(handle).toHaveAttribute('aria-orientation',horizontal?'vertical':'horizontal');
    const before=Number(await handle.getAttribute('aria-valuenow'));
    await handle.focus();
    await page.keyboard.press(horizontal?'ArrowLeft':'ArrowUp');
    await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeLessThan(before);
    const adjusted=Number(await handle.getAttribute('aria-valuenow'));
    await review.getByTestId('collection-review-next').click();
    await review.getByTestId('collection-review-previous').click();
    await expect.poll(async()=>Number(await review.getByTestId('training-split-handle').getAttribute('aria-valuenow'))).toBeCloseTo(adjusted,0);
  });
}
