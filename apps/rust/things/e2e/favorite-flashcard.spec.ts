import { expect, test, type Page } from '@playwright/test';
import { expectNoHorizontalOverflow, loginAsAdmin, resetHarness } from './fixtures';

const root = '/api/v1/question-bank';
const rows = [
  { question_id: 'supported', module: '数量关系', category: '数学运算' },
  { question_id: 'unsupported', module: '常识判断', category: '法律' },
].map((row, index) => ({ ...row, paper_id: 'paper', paper_title: '收藏测试卷', year: 2026, region: '国考', number: index + 1 }));
const preview = (eligible = true) => ({
  eligible, reason: eligible ? null : '此模块暂不支持创建闪卡',
  front: '## 完整材料\n\n材料中的数量为 **1200**。\n\n求结果 $A=B$\n\n- A. 100\n- B. 200',
  correct_answer: ['B'], recommended_target: null,
  allowed_targets: eligible ? ['数量/工程效率/错题', '数量/概率/错题'] : [],
  requires_type_confirmation: true, source_revision: 'source-v1',
});

async function install(page: Page) {
  await loginAsAdmin(page);
  await page.route(`**${root}/**`, async route => {
    const url = new URL(route.request().url());
    const path = url.pathname.slice(root.length);
    if (path === '/papers') await route.fulfill({ json: [] });
    else if (path === '/practice/catalog') await route.fulfill({ json: { unclassified: 0, modules: rows.map(row => ({ name: row.module, count: 1, done: 0, uncategorized: 0, categories: [{ name: row.category, count: 1, done: 0 }] })) } });
    else if (path === '/practice/history') await route.fulfill({ json: [] });
    else if (path === '/practice/questions') await route.fulfill({ json: url.searchParams.get('mode') === 'favorite' ? rows : [] });
    else if (path.endsWith('/flashcard-preview')) await route.fulfill({ json: preview(!path.includes('/unsupported/')) });
    else await route.continue();
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await page.getByTestId('question-bank-collection-favorite').click();
}

async function openDetail(page: Page, number = 1) {
  await page.getByTestId('question-bank-collection-item').filter({ hasText: `第 ${number} 题` }).click();
  const detail = page.getByTestId('favorite-question-detail');
  await expect(detail).toBeVisible();
  return detail;
}

async function openCreate(page: Page) {
  await openDetail(page);
  await page.getByRole('button', { name: '创建 Flashcard', exact: true }).click();
  const modal = page.getByRole('dialog', { name: '创建 Flashcard', exact: true });
  await expect(modal).toBeVisible();
  return modal;
}

test.beforeEach(async ({ request }) => { await resetHarness(request); });

for (const width of [1180, 390]) {
  test(`favorite question detail creates a flashcard and preserves filters at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await install(page);
    await expect(page.getByRole('button', { name: '创建 Flashcard', exact: true })).toHaveCount(0);
    await page.getByTestId('question-bank-collection-module').filter({ hasText: '数量关系' }).click();
    await page.getByTestId('question-bank-collection-category').filter({ hasText: '数学运算' }).click();
    const detail = await openDetail(page);
    await expect(detail).toContainText('完整材料');
    await expect(detail.locator('strong')).toContainText(['1200']);
    await expect(detail.locator('.katex').first()).toBeVisible();
    await expect(detail.getByTestId('favorite-question-answer')).toHaveText('B');
    await detail.getByRole('button', { name: '创建 Flashcard', exact: true }).click();
    const modal = page.getByRole('dialog', { name: '创建 Flashcard', exact: true });
    await expect(modal.getByLabel('背面')).toHaveValue('');
    await modal.getByLabel('目标牌组').selectOption('数量/概率/错题');
    await expect(modal.getByRole('button', { name: '创建并同步', exact: true })).toBeDisabled();
    await modal.getByLabel('背面').fill('先列方程，再代入。');
    await modal.getByLabel('确认题型').check();
    let body: unknown;
    let csrf: string | undefined;
    await page.route(`**${root}/practice/favorites/supported/flashcard`, async route => {
      body = route.request().postDataJSON();
      csrf = route.request().headers()['x-hyz-csrf'];
      await route.fulfill({ json: { status: 'created_synced', target_deck: '数量/概率/错题', source_file: '数量/概率/错题/cards.md', card_id: 'new-card', retryable: false } });
    });
    await modal.getByRole('button', { name: '创建并同步', exact: true }).click();
    await expect(modal).toContainText('已创建');
    expect(body).toEqual({ target_deck: '数量/概率/错题', back: '先列方程，再代入。', source_revision: 'source-v1', type_confirmed: true });
    expect(csrf).toBeTruthy();
    await modal.getByRole('button', { name: '关闭', exact: true }).click();
    await detail.getByRole('button', { name: '返回收藏', exact: true }).click();
    await expect(page.getByTestId('question-bank-collection-item')).toHaveCount(1);
    await expect(page.getByTestId('question-bank-collection-category').filter({ hasText: '数学运算' })).toHaveAttribute('aria-pressed', 'true');
    await expectNoHorizontalOverflow(page);
  });
}

test('unsupported favorite still shows full question and answer without a create action', async ({ page }) => {
  await install(page);
  const detail = await openDetail(page, 2);
  await expect(detail).toContainText('完整材料');
  await expect(detail.getByTestId('favorite-question-answer')).toHaveText('B');
  await expect(detail).toContainText('此模块暂不支持创建闪卡');
  await expect(detail.getByRole('button', { name: '创建 Flashcard', exact: true })).toHaveCount(0);
});

test('favorite detail retries failed loading and ignores a closed question response', async ({ page }) => {
  await install(page);
  let reads = 0;
  await page.route(`**${root}/practice/favorites/supported/flashcard-preview`, async route => {
    reads += 1;
    await route.fulfill(reads === 1 ? { status: 503, json: { error: 'temporary-unavailable' } } : { json: preview() });
  });
  const detail = await openDetail(page);
  await detail.getByRole('button', { name: '重试加载', exact: true }).click();
  await expect(detail).toContainText('完整材料');
  expect(reads).toBe(2);
  await detail.getByRole('button', { name: '返回收藏', exact: true }).click();
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**${root}/practice/favorites/supported/flashcard-preview`, async route => {
    await gate;
    await route.fulfill({ json: { ...preview(), front: '过期响应不得显示' } });
  });
  await openDetail(page);
  await detail.getByRole('button', { name: '返回收藏', exact: true }).click();
  await openDetail(page, 2);
  release();
  await expect(detail).toContainText('此模块暂不支持创建闪卡');
  await expect(detail).not.toContainText('过期响应不得显示');
});

test('favorite flashcard sync recovery retries the identical request and blocks duplicate submits', async ({ page }) => {
  await install(page);
  const modal = await openCreate(page);
  await modal.getByLabel('背面').fill('恢复同步的背面');
  await modal.getByLabel('目标牌组').selectOption('数量/工程效率/错题');
  await modal.getByLabel('确认题型').check();
  const requests: unknown[] = [];
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**${root}/practice/favorites/supported/flashcard`, async route => {
    requests.push(route.request().postDataJSON());
    if (requests.length === 1) await gate;
    await route.fulfill({ json: { status: requests.length === 1 ? 'created_sync_pending' : 'already_exists', target_deck: '数量/工程效率/错题', source_file: '数量/工程效率/错题/cards.md', card_id: 'existing-card', retryable: requests.length === 1 } });
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
    await modal.getByLabel('目标牌组').selectOption('数量/工程效率/错题');
    await modal.getByLabel('确认题型').check();
    const submit = modal.getByRole('button', { name: '创建并同步', exact: true });
    await expect(submit).toBeEnabled();
    let body: unknown;
    await page.route(`**${root}/practice/favorites/supported/flashcard`, async route => {
      body = route.request().postDataJSON();
      await route.fulfill({ json: { status: 'created_synced', target_deck: '数量/工程效率/错题', source_file: '数量/工程效率/错题/cards.md', card_id: 'blank-back', retryable: false } });
    });
    await submit.click();
    await expect(modal).toContainText('已创建并同步');
    expect(body).toEqual({ target_deck: '数量/工程效率/错题', back: '', source_revision: 'source-v1', type_confirmed: true });
    await expectNoHorizontalOverflow(page);
  });
}
