import { expect, test, type Page } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

async function startFixturePaper(page: Page) {
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  for (const [id, text] of [
    ['question-bank-region', '国考'],
    ['question-bank-year', '2008'],
  ]) {
    const summary = page.getByTestId(id).filter({ hasText: text });
    if (!(await summary.evaluate(node => (node.closest('details') as HTMLDetailsElement).open))) {
      await summary.click();
    }
  }
  await page.getByTestId('question-bank-paper-option').filter({ hasText: '行测真题训练端到端样卷' }).click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/3 题');
}

test('question bank pending answer rolls back to the saved choice on failure and can be retried without downloading the workspace', async ({ page }) => {
  await startFixturePaper(page);
  const choices = page.getByTestId('question-bank-practice-choice');
  const answerEndpoint = '**/api/v1/question-bank/practice/sessions/*/answers/*';
  const workspaceDownloads: string[] = [];
  page.on('request', request => {
    if (request.method() === 'GET'
        && /\/api\/v1\/question-bank\/practice\/sessions\/[^/]+$/.test(new URL(request.url()).pathname)) {
      workspaceDownloads.push(request.url());
    }
  });

  const savedResponse = page.waitForResponse(response => response.request().method() === 'POST'
    && response.url().includes('/answers/') && response.ok());
  await choices.nth(0).click();
  await savedResponse;
  await expect(choices.nth(0)).toHaveAttribute('aria-pressed', 'true');
  await expect(choices.nth(1)).toBeEnabled();

  const failureGate = deferred();
  let answerUrl = '';
  let attempts = 0;
  await page.route(answerEndpoint, async route => {
    attempts += 1;
    answerUrl = route.request().url();
    await failureGate.promise;
    await route.fulfill({ status: 503, json: { error: 'temporary-unavailable' } });
  });
  try {
    await choices.nth(1).click();
    await expect.poll(() => attempts).toBe(1);
    // The response is held until these assertions finish; no wall-clock delay is involved.
    await expect(choices.nth(1)).toHaveAttribute('aria-pressed', 'true');
    await expect(choices.nth(1)).toHaveAttribute('aria-busy', 'true');
    await expect(choices.nth(0)).toHaveAttribute('aria-pressed', 'false');
  } finally {
    failureGate.release();
  }
  await expect(page.getByTestId('question-bank-error')).toBeVisible();
  await expect(choices.nth(0)).toHaveAttribute('aria-pressed', 'true');
  await expect(choices.nth(1)).toHaveAttribute('aria-pressed', 'false');
  await expect(choices.nth(1)).not.toHaveAttribute('aria-busy', 'true');
  await expect(choices.nth(1)).toBeEnabled();
  expect(attempts).toBe(1);

  const persistedAnswer = () => page.evaluate(async url => {
    const parsed = new URL(url);
    const [, sessionId, questionId] = parsed.pathname.match(/\/sessions\/([^/]+)\/answers\/([^/]+)$/)!;
    const response = await fetch(`/api/v1/question-bank/practice/sessions/${sessionId}`);
    if (!response.ok) throw new Error(`workspace read failed: ${response.status}`);
    const workspace = await response.json();
    return workspace.session.items.find((item: { question_id: string }) => item.question_id === questionId).answer;
  }, answerUrl);
  // Check the performance invariant before the explicit persistence verification GET.
  expect(workspaceDownloads).toEqual([]);
  expect(await persistedAnswer()).toEqual(['A']);
  workspaceDownloads.length = 0;

  await page.unroute(answerEndpoint);
  const retryResponse = page.waitForResponse(response => response.url() === answerUrl
    && response.request().method() === 'POST' && response.ok());
  await choices.nth(1).click();
  await retryResponse;
  await expect(choices.nth(1)).toHaveAttribute('aria-pressed', 'true');
  await expect(choices.nth(1)).toBeEnabled();
  await expect(choices.nth(1)).not.toHaveAttribute('aria-busy', 'true');
  await expect(page.getByTestId('question-bank-error')).toHaveCount(0);
  expect(workspaceDownloads).toEqual([]);
  expect(await persistedAnswer()).toEqual(['B']);
});

test('question bank saving freezes navigation and multi-select until the answer is persisted', async ({ page }) => {
  await startFixturePaper(page);
  const next = page.getByRole('button', { name: '下一题', exact: true });
  const previous = page.getByRole('button', { name: '上一题', exact: true });
  await next.click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 2/3 题');
  await page.getByTestId('training-answer-sheet-toggle').click();
  const numbers = page.getByTestId('question-bank-practice-number');
  await expect(numbers).toHaveCount(3);
  const multiSelect = page.getByRole('checkbox', { name: '多选作答（题目明确允许多选时开启）' });
  const gate = deferred();
  const writes: { url: string; answer: string[] }[] = [];
  await page.route('**/api/v1/question-bank/practice/sessions/*/answers/*', async route => {
    writes.push({ url: route.request().url(), answer: route.request().postDataJSON().answer });
    await gate.promise;
    await route.continue();
  });
  try {
    await page.getByTestId('question-bank-practice-choice').first().click();
    await expect.poll(() => writes.length).toBe(1);
    await expect(next).toBeDisabled();
    await expect(previous).toBeDisabled();
    for (let i = 0; i < 3; i += 1) await expect(numbers.nth(i)).toBeDisabled();
    await expect(multiSelect).toBeDisabled();
    // Synthetic events also exercise the handlers rather than only disabled hit-testing.
    for (const label of ['上一题', '下一题']) {
      await page.getByRole('button', { name: label, exact: true }).dispatchEvent('click');
      await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 2/3 题');
    }
    for (let i = 0; i < 3; i += 1) {
      await numbers.nth(i).dispatchEvent('click');
      await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 2/3 题');
    }
    for (const [start, end] of [[250, 50], [50, 250]]) {
      await page.getByTestId('training-swipe-surface').evaluate((surface, { start, end }) => {
        surface.dispatchEvent(new PointerEvent('pointerdown', {
          bubbles: true, pointerId: 51, pointerType: 'touch', isPrimary: true, clientX: start, clientY: 100,
        }));
        surface.dispatchEvent(new PointerEvent('pointerup', {
          bubbles: true, pointerId: 51, pointerType: 'touch', isPrimary: true, clientX: end, clientY: 100,
        }));
      }, { start, end });
      await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 2/3 题');
    }
    await multiSelect.evaluate(node => {
      const checkbox = node as HTMLInputElement;
      checkbox.checked = true;
      checkbox.dispatchEvent(new Event('change', { bubbles: true }));
    });
    await expect(multiSelect).not.toBeChecked();
    expect(writes).toHaveLength(1);
  } finally {
    gate.release();
  }
  await expect(next).toBeEnabled();
  await expect(previous).toBeEnabled();
  await expect(multiSelect).toBeEnabled();
  await page.getByTestId('training-answer-sheet-toggle').click();
  await next.click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 3/3 题');
  await previous.click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed', 'true');
  await multiSelect.check();
  await expect(multiSelect).toBeChecked();
});

test('question bank same-turn answer and navigation clicks cannot create overlapping or misdirected writes', async ({ page }) => {
  await startFixturePaper(page);
  const gate = deferred();
  const writes: { url: string; answer: string[] }[] = [];
  await page.route('**/api/v1/question-bank/practice/sessions/*/answers/*', async route => {
    writes.push({ url: route.request().url(), answer: route.request().postDataJSON().answer });
    await gate.promise;
    await route.continue();
  });
  try {
    await page.evaluate(() => {
      const choices = document.querySelectorAll('[data-testid="question-bank-practice-choice"]');
      const next = [...document.querySelectorAll('[data-testid="question-bank-practice-navigation"] button')]
        .find(button => button.textContent === '下一题')!;
      for (const target of [choices[0], next, choices[1], choices[0]]) {
        target.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      }
    });
    await expect.poll(() => writes.length).toBe(1);
    await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/3 题');
    await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed', 'true');
    expect(writes[0].answer).toEqual(['A']);
  } finally {
    gate.release();
  }
  const next = page.getByRole('button', { name: '下一题', exact: true });
  await expect(next).toBeEnabled();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toBeEnabled();
  expect(writes).toHaveLength(1);
  const saved = await page.evaluate(async url => {
    const [, sessionId, questionId] = new URL(url).pathname.match(/\/sessions\/([^/]+)\/answers\/([^/]+)$/)!;
    const response = await fetch(`/api/v1/question-bank/practice/sessions/${sessionId}`);
    if (!response.ok) throw new Error(`workspace read failed: ${response.status}`);
    return { questionId, workspace: await response.json() };
  }, writes[0].url);
  expect(saved.workspace.session.items[0].question_id).toBe(saved.questionId);
  expect(saved.workspace.session.items[0].answer).toEqual(['A']);
  expect(saved.workspace.session.items.slice(1).every((item: { answer: string[] }) => item.answer.length === 0)).toBe(true);
  await next.click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 2/3 题');
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed', 'false');
});

test('question bank collection request budget reuses initialized lists across collection and category chips', async ({ page }) => {
  const started = { wrong: 0, favorite: 0 };
  const completed = { wrong: 0, favorite: 0 };
  const sizes = { wrong: 0, favorite: 0 };
  await page.route('**/api/v1/question-bank/practice/questions?*', async route => {
    const scope = new URL(route.request().url()).searchParams.get('mode');
    if (scope !== 'wrong' && scope !== 'favorite') {
      await route.continue();
      return;
    }
    started[scope] += 1;
    const response = await route.fetch();
    expect(response.ok()).toBe(true);
    sizes[scope] = (await response.json()).length;
    await route.fulfill({ response });
    completed[scope] += 1;
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect.poll(() => completed).toEqual({ wrong: 1, favorite: 1 });
  await expect(page.getByTestId('question-bank-cross-module').filter({ hasText: '资料分析' })).toBeVisible();
  for (const scope of ['wrong', 'favorite', 'wrong', 'favorite'] as const) {
    await page.getByTestId(`question-bank-collection-${scope}`).click();
    await expect(page.getByTestId(`question-bank-collection-${scope}`)).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByTestId('question-bank-browse-count')).toContainText(`${sizes[scope]}题`);
    await page.getByTestId('question-bank-collection-module').first().click();
  }
  await page.getByTestId('question-bank-collection-all').click();
  await page.getByTestId('question-bank-cross-category').filter({ hasText: '比重' }).click();
  await expect(page.getByTestId('training-setup-action')).toContainText('资料分析 · 比重');
  expect(started).toEqual({ wrong: 1, favorite: 1 });
});

type FavoriteSnapshot = { question_id: string; favorite: boolean };

async function startWithFavoriteSnapshot(page: Page): Promise<FavoriteSnapshot> {
  const pending = page.waitForResponse(response => response.request().method() === 'GET'
    && /\/practice\/favorites\/[^/]+$/.test(new URL(response.url()).pathname));
  await startFixturePaper(page);
  const response = await pending;
  expect(response.ok()).toBe(true);
  const state = await response.json() as FavoriteSnapshot;
  await expect(page.getByTestId('question-bank-favorite')).toHaveAttribute('aria-label', state.favorite ? '取消收藏' : '收藏题目');
  return state;
}

async function setFavoriteViaApi(page: Page, state: FavoriteSnapshot) {
  await page.evaluate(async state => {
    const panel = await fetch('/api/v1/panel');
    if (!panel.ok) throw new Error(`panel read failed: ${panel.status}`);
    const { csrf_token } = await panel.json();
    const response = await fetch(`/api/v1/question-bank/practice/favorites/${state.question_id}`, {
      method: 'POST', headers: { 'Content-Type': 'application/json', 'X-HYZ-CSRF': csrf_token },
      body: JSON.stringify({ favorite: state.favorite }),
    });
    if (!response.ok) throw new Error(`favorite seed failed: ${response.status}`);
  }, state);
}

function deferred() {
  let release!: () => void;
  const promise = new Promise<void>(resolve => { release = resolve; });
  return { promise, release };
}

test('question bank a slow wrong list does not block the favorite chip or list', async ({ page }) => {
  const gate = deferred();
  let wrongStarted = false;
  let wrongCompleted = false;
  let favoriteSize: number | undefined;
  await page.route('**/api/v1/question-bank/practice/questions?*', async route => {
    const scope = new URL(route.request().url()).searchParams.get('mode');
    if (scope === 'wrong') {
      wrongStarted = true;
      await gate.promise;
      await route.continue();
      wrongCompleted = true;
    } else if (scope === 'favorite') {
      const response = await route.fetch();
      expect(response.ok()).toBe(true);
      favoriteSize = (await response.json()).length;
      await route.fulfill({ response });
    } else {
      await route.continue();
    }
  });
  try {
    await loginAsAdmin(page);
    await page.goto('/#/study/training');
    await page.getByTestId('training-mode-question-bank-module').click();
    await expect.poll(() => wrongStarted).toBe(true);
    await expect.poll(() => favoriteSize).not.toBeUndefined();
    await expect(page.getByTestId('question-bank-collection-favorite').getByTestId('question-bank-collection-count')).toHaveText(String(favoriteSize));
    await page.getByTestId('question-bank-collection-favorite').click();
    await expect(page.getByTestId('question-bank-browse-count')).toContainText(`${favoriteSize}题`);
    expect(wrongCompleted).toBe(false);
  } finally {
    gate.release();
  }
});

test('question bank a favorite mutation refreshes only the favorite list', async ({ page }) => {
  const reads = { papers: 0, history: 0, catalog: 0, wrong: 0, favorite: 0 };
  page.on('requestfinished', request => {
    if (request.method() !== 'GET') return;
    const url = new URL(request.url());
    if (url.pathname === '/api/v1/question-bank/papers') reads.papers += 1;
    if (url.pathname === '/api/v1/question-bank/practice/history') reads.history += 1;
    if (url.pathname === '/api/v1/question-bank/practice/catalog') reads.catalog += 1;
    if (url.pathname === '/api/v1/question-bank/practice/questions') {
      const scope = url.searchParams.get('mode');
      if (scope === 'wrong' || scope === 'favorite') reads[scope] += 1;
    }
  });
  const original = await startWithFavoriteSnapshot(page);
  try {
    await expect.poll(() => reads).toEqual({ papers: 1, history: 1, catalog: 1, wrong: 1, favorite: 1 });
    await page.getByTestId('question-bank-favorite').click();
    await expect(page.getByTestId('question-bank-favorite')).toHaveAttribute('aria-label', original.favorite ? '收藏题目' : '取消收藏');
    await expect.poll(() => reads.favorite).toBe(2);
    expect(reads).toEqual({ papers: 1, history: 1, catalog: 1, wrong: 1, favorite: 2 });
  } finally {
    await setFavoriteViaApi(page, original);
  }
});

test('question bank collection refresh retains the successful snapshot while pending or failed and can retry', async ({ page }) => {
  const original = await startWithFavoriteSnapshot(page);
  const gate = deferred();
  let attempts = 0;
  try {
    await setFavoriteViaApi(page, { ...original, favorite: true });
    await page.getByRole('button', { name: '← 暂停并退出', exact: true }).click();
    await page.getByTestId('question-bank-collection-favorite').click();
    await expect(page.getByTestId('question-bank-collection-item').first()).toBeVisible();
    const snapshot = await page.getByTestId('question-bank-collection-item').allTextContents();
    expect(snapshot.length).toBeGreaterThan(0);
    await page.getByTestId('question-bank-practice-start').click();
    await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
    await page.route('**/api/v1/question-bank/practice/questions?*', async route => {
      if (new URL(route.request().url()).searchParams.get('mode') !== 'favorite') {
        await route.continue();
        return;
      }
      attempts += 1;
      if (attempts === 1) {
        await gate.promise;
        await route.fulfill({ status: 503, json: { error: 'temporary-unavailable' } });
      } else {
        await route.continue();
      }
    });
    await page.getByRole('button', { name: '← 暂停并退出', exact: true }).click();
    await expect.poll(() => attempts).toBe(1);
    await expect(page.getByTestId('question-bank-collection-item')).toHaveText(snapshot);
    await expect(page.getByTestId('question-bank-collection-loading')).toContainText('正在刷新');
    gate.release();
    await expect(page.getByTestId('question-bank-error')).toBeVisible();
    await expect(page.getByTestId('question-bank-collection-item')).toHaveText(snapshot);
    await page.getByTestId('question-bank-collection-retry').click();
    await expect.poll(() => attempts).toBe(2);
    await expect(page.getByTestId('question-bank-collection-loading')).toHaveCount(0);
    await expect(page.getByTestId('question-bank-collection-retry')).toHaveCount(0);
    await expect(page.getByTestId('question-bank-collection-item')).toHaveText(snapshot);
    await expect(page.getByTestId('question-bank-error')).toHaveCount(0);
  } finally {
    gate.release();
    await setFavoriteViaApi(page, original);
  }
});

test('question bank an unconfirmed collection shows loading rather than a confirmed empty count', async ({ page }) => {
  const gate = deferred();
  let started = false;
  await page.route('**/api/v1/question-bank/practice/questions?*', async route => {
    if (new URL(route.request().url()).searchParams.get('mode') === 'favorite') {
      started = true;
      await gate.promise;
    }
    await route.continue();
  });
  try {
    await loginAsAdmin(page);
    await page.goto('/#/study/training');
    await page.getByTestId('training-mode-question-bank-module').click();
    await expect.poll(() => started).toBe(true);
    await page.getByTestId('question-bank-collection-favorite').click();
    await expect(page.getByTestId('question-bank-collection-favorite').getByTestId('question-bank-collection-count')).toHaveText('…');
    await expect(page.getByTestId('question-bank-collection-loading')).toContainText('正在加载');
    await expect(page.getByTestId('question-bank-browse-count')).not.toContainText('0题');
    await expect(page.getByTestId('question-bank-collection-browser')).not.toContainText('这个分类还没有题目');
  } finally {
    gate.release();
  }
});

test('question bank favorite state must load before toggling and a failed state read can retry', async ({ page }) => {
  const gate = deferred();
  let original: FavoriteSnapshot | undefined;
  let reads = 0;
  let failNext = false;
  const writes: boolean[] = [];
  await page.route('**/api/v1/question-bank/practice/favorites/*', async route => {
    if (route.request().method() === 'POST') {
      writes.push(route.request().postDataJSON().favorite);
      await route.continue();
      return;
    }
    reads += 1;
    if (reads === 1) {
      const before = await route.fetch();
      expect(before.ok()).toBe(true);
      original = await before.json() as FavoriteSnapshot;
      await gate.promise;
    } else if (failNext) {
      failNext = false;
      await route.fulfill({ status: 503, json: { error: 'temporary-unavailable' } });
      return;
    }
    const response = await route.fetch();
    await route.fulfill({ response });
  });
  try {
    await startFixturePaper(page);
    await expect.poll(() => original).not.toBeUndefined();
    await setFavoriteViaApi(page, { ...original!, favorite: true });
    writes.length = 0;
    const star = page.getByTestId('question-bank-favorite');
    await expect(star).toBeDisabled();
    await expect(star).toHaveAttribute('aria-busy', 'true');
    await star.evaluate(node => {
      node.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      node.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(writes).toEqual([]);
    gate.release();
    await expect(star).toBeEnabled();
    await expect(star).toHaveAttribute('aria-label', '取消收藏');
    await star.click();
    await expect(star).toHaveAttribute('aria-label', '收藏题目');
    expect(writes).toEqual([false]);
    failNext = true;
    await page.getByRole('button', { name: '下一题', exact: true }).click();
    await expect(page.getByTestId('question-bank-favorite-error')).toBeVisible();
    await expect(star).toBeDisabled();
    await expect(star).not.toHaveAttribute('aria-busy', 'true');
    await page.getByTestId('question-bank-favorite-retry').click();
    await expect(star).toBeEnabled();
    await expect(page.getByTestId('question-bank-favorite-error')).toHaveCount(0);
    expect(reads).toBe(3);
    expect(writes).toEqual([false]);
  } finally {
    gate.release();
    if (original) await setFavoriteViaApi(page, original);
  }
});
