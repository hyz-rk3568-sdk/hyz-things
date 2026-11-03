import { expect, test, type Page } from '@playwright/test';
import { loginAsAdmin, resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

for (const returning of [false, true]) {
  test(`paper list shows loading rather than an empty bank on ${returning ? 'navigation back' : 'first visit'}`, async ({ page }) => {
    await loginAsAdmin(page);
    if (returning) {
      await page.goto('/#/study');
      await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
      await page.goto('/#/');
      await expect(page.getByTestId('question-bank-papers')).toHaveCount(0);
    }
    let release!: () => void;
    const gate = new Promise<void>(resolve => { release = resolve; });
    let reads = 0;
    await page.route('**/api/v1/question-bank/papers', async route => {
      reads += 1;
      await gate;
      await route.continue();
    });
    try {
      await page.goto('/#/study');
      await expect.poll(() => reads).toBeGreaterThan(0);
      await expect(page.getByTestId('question-bank-papers')).not.toContainText('题库为空');
      await expect(page.getByTestId('question-bank-loading')).toBeVisible();
    } finally {
      release();
    }
    await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
  });
}

async function foreground(page: Page) {
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: false });
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
    document.dispatchEvent(new Event('visibilitychange'));
    window.dispatchEvent(new Event('focus'));
    window.dispatchEvent(new Event('pageshow'));
    window.dispatchEvent(new Event('online'));
  });
}

test('foreground refresh retains papers on failure and supports an explicit read-only retry', async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto('/#/study');
  const papers = page.getByTestId('question-bank-papers');
  await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
  const previous = await papers.innerText();
  let reads = 0;
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const writes: string[] = [];
  page.on('request', request => {
    if (request.url().includes('/question-bank/') && request.method() !== 'GET') writes.push(request.url());
  });
  await page.route('**/api/v1/question-bank/papers', async route => {
    reads += 1;
    if (reads === 1) {
      await gate;
      await route.fulfill({ status: 503, json: { error: 'temporary-unavailable' } });
    } else {
      await route.continue();
    }
  });
  try {
    await foreground(page);
    await expect.poll(() => reads).toBe(1);
    await expect(papers).toContainText(previous, { useInnerText: true });
    await expect(papers).not.toContainText('题库为空');
  } finally {
    release();
  }
  await expect(page.getByTestId('question-bank-load-error')).toBeVisible();
  await expect(papers).toContainText(previous, { useInnerText: true });
  await page.getByRole('button', { name: '重新加载题库', exact: true }).click();
  await expect.poll(() => reads).toBe(2);
  await expect(page.getByTestId('question-bank-load-error')).toHaveCount(0);
  await expect(papers).toHaveText(previous, { useInnerText: true });
  expect(writes).toEqual([]);
});

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>(done => { resolve = done; });
  return { promise, resolve };
}

async function paint(page: Page) {
  await page.evaluate(() => new Promise<void>(resolve => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  }));
}

const summary = (title: string) => [{
  id: 'resume-fixture', title, year: 2026, region: '恢复测试', variant: '', role: '', questions: 1,
}];

test('each foreground signal retries reads while hidden signals do not', async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto('/#/study');
  await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
  let reads = 0;
  await page.route('**/api/v1/question-bank/papers', async route => {
    reads += 1;
    await route.fulfill({ json: summary(`恢复请求 ${reads}`) });
  });
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: true });
    document.dispatchEvent(new Event('visibilitychange'));
    for (const event of ['focus', 'pageshow', 'online']) window.dispatchEvent(new Event(event));
  });
  await paint(page);
  expect(reads).toBe(0);
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: false });
  });
  for (const event of ['visibilitychange', 'pageshow', 'focus', 'online']) {
    const expected = reads + 1;
    await page.evaluate(event => {
      (event === 'visibilitychange' ? document : window).dispatchEvent(new Event(event));
    }, event);
    await expect.poll(() => reads).toBe(expected);
    await expect(page.getByTestId('question-bank-open')).toHaveCount(1);
    await expect(page.getByTestId('question-bank-open').first()).toContainText(`恢复请求 ${expected}`);
    expect(reads).toBe(expected);
  }
});

test('first-read failure offers a retry instead of claiming the bank is empty', async ({ page }) => {
  await loginAsAdmin(page);
  let reads = 0;
  await page.route('**/api/v1/question-bank/papers', async route => {
    reads += 1;
    if (reads === 1) await route.fulfill({ status: 503, json: { error: 'temporary-unavailable' } });
    else await route.continue();
  });
  await page.goto('/#/study');
  await expect(page.getByTestId('question-bank-load-error')).toContainText('题库加载失败');
  await expect(page.getByTestId('question-bank-loading')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-papers')).not.toContainText('题库为空');
  await page.getByRole('button', { name: '重新加载题库', exact: true }).click();
  await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
  await expect(page.getByTestId('question-bank-load-error')).toHaveCount(0);
  expect(reads).toBe(2);
});

test('foreground retry leaves the opened paper intact on failure and success', async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto('/#/study');
  await page.getByTestId('question-bank-open').first().click();
  const paper = page.getByTestId('question-bank-paper');
  await expect(paper).toBeVisible();
  const previous = await paper.innerText();
  const gate = deferred();
  let reads = 0;
  await page.route('**/api/v1/question-bank/papers', async route => {
    reads += 1;
    if (reads === 1) {
      await gate.promise;
      await route.fulfill({ status: 503, json: { error: 'temporary-unavailable' } });
    } else await route.continue();
  });
  try {
    await foreground(page);
    await expect.poll(() => reads).toBe(1);
    await expect(paper).toHaveText(previous, { useInnerText: true });
  } finally {
    gate.resolve();
  }
  await expect(page.getByTestId('question-bank-load-error')).toBeVisible();
  await expect(paper).toHaveText(previous, { useInnerText: true });
  await page.getByRole('button', { name: '重新加载题库', exact: true }).click();
  await expect(page.getByTestId('question-bank-load-error')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-loading')).toHaveCount(0);
  await expect(paper).toHaveText(previous, { useInnerText: true });
  expect(reads).toBe(2);
  await page.getByRole('button', { name: '返回试卷列表', exact: true }).click();
  await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
});

for (const stale of ['success', 'failure'] as const) {
  test(`a new foreground read overtakes a hanging initial read and ignores its late ${stale}`, async ({ page }) => {
    await loginAsAdmin(page);
    const gate = deferred();
    let reads = 0;
    await page.route('**/api/v1/question-bank/papers', async route => {
      reads += 1;
      if (reads === 1) {
        await gate.promise;
        if (stale === 'success') await route.fulfill({ json: summary('迟到旧列表') });
        else await route.fulfill({ status: 503, json: { error: 'stale-failure' } });
      } else await route.fulfill({ json: summary('最新列表') });
    });
    try {
      await page.goto('/#/study');
      await expect.poll(() => reads).toBe(1);
      await foreground(page);
      await expect.poll(() => reads).toBe(2);
      await expect(page.getByTestId('question-bank-open')).toContainText('最新列表');
      const response = page.waitForResponse(response => response.url().endsWith('/question-bank/papers'));
      gate.resolve();
      await (await response).finished();
      await paint(page);
      await expect(page.getByTestId('question-bank-open')).toContainText('最新列表');
      await expect(page.getByTestId('question-bank-papers')).not.toContainText('迟到旧列表');
      await expect(page.getByTestId('question-bank-load-error')).toHaveCount(0);
      await expect(page.getByTestId('question-bank-notice')).toHaveCount(0);
    } finally {
      gate.resolve();
    }
  });
}

test('unmount removes foreground listeners and a late read cannot replace a remounted list', async ({ page }) => {
  await loginAsAdmin(page);
  const gate = deferred();
  let reads = 0;
  await page.route('**/api/v1/question-bank/papers', async route => {
    reads += 1;
    if (reads === 1) {
      await gate.promise;
      await route.fulfill({ json: summary('卸载前旧列表') });
    } else await route.fulfill({ json: summary('重新进入列表') });
  });
  try {
    await page.goto('/#/study');
    await expect.poll(() => reads).toBe(1);
    await page.goto('/#/');
    await expect(page.getByTestId('question-bank-papers')).toHaveCount(0);
    await foreground(page);
    await paint(page);
    expect(reads).toBe(1);
    await page.goto('/#/study');
    await expect(page.getByTestId('question-bank-open')).toContainText('重新进入列表');
    const response = page.waitForResponse(response => response.url().endsWith('/question-bank/papers'));
    gate.resolve();
    await (await response).finished();
    await paint(page);
    await expect(page.getByTestId('question-bank-open')).toContainText('重新进入列表');
    expect(reads).toBe(2);
  } finally {
    gate.resolve();
  }
});

test('logout clears the mounted panel and a late paper response cannot cross into a new admin epoch', async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto('/#/study');
  await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
  const oldPaper = deferred();
  const newList = deferred();
  let paperReads = 0;
  let listReads = 0;
  let statusReads = 0;
  let paperUrl = '';
  await page.route('**/api/v1/question-bank/papers/*', async route => {
    const response = await route.fetch();
    paperReads += 1;
    paperUrl = route.request().url();
    await oldPaper.promise;
    await route.fulfill({ response });
  });
  try {
    await page.getByTestId('question-bank-open').first().click();
    await expect.poll(() => paperReads).toBe(1);
    // Invoke the real logout handler without navigating away from the mounted study panel.
    await page.getByRole('button', { name: '退出登录', includeHidden: true }).dispatchEvent('click');
    const panel = page.locator('section[aria-labelledby="question-bank-title"]');
    await expect(panel.getByRole('heading', { name: '管理员登录' })).toBeVisible();
    await expect(page.getByTestId('question-bank-open')).toHaveCount(0);
    await expect(page.getByTestId('question-bank-paper')).toHaveCount(0);
    await page.route('**/api/v1/question-bank/papers', async route => {
      listReads += 1;
      await newList.promise;
      await route.continue();
    });
    page.on('request', request => {
      if (request.url().endsWith('/question-bank/sync/status')) statusReads += 1;
    });
    await foreground(page);
    await paint(page);
    expect(listReads).toBe(0);
    expect(statusReads).toBe(0);
    await panel.getByLabel('密码', { exact: true }).fill('router-e2e-password');
    await panel.getByRole('button', { name: '登录', exact: true }).click();
    await expect.poll(() => listReads).toBe(1);
    await expect(page.getByTestId('question-bank-open')).toHaveCount(0);
    await expect(page.getByTestId('question-bank-paper')).toHaveCount(0);
    const response = page.waitForResponse(response => response.url() === paperUrl);
    oldPaper.resolve();
    await (await response).finished();
    await paint(page);
    await expect(page.getByTestId('question-bank-paper')).toHaveCount(0);
    await expect(page.getByTestId('question-bank-notice')).toHaveCount(0);
    newList.resolve();
    await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
  } finally {
    oldPaper.resolve();
    newList.resolve();
  }
});

for (const taskId of ['resume-completed', null]) {
  test(`one sync status poller reloads a completed job once using ${taskId ? 'task id' : 'summary fallback'}`, async ({ page }) => {
    await loginAsAdmin(page);
    const gate = deferred();
    let statusReads = 0;
    let paperReads = 0;
    await page.route('**/api/v1/question-bank/papers', async route => {
      paperReads += 1;
      await route.fulfill({ json: summary(`列表版本 ${paperReads}`) });
    });
    await page.route('**/api/v1/question-bank/sync/status', async route => {
      statusReads += 1;
      await gate.promise;
      await route.fulfill({ json: {
        task_id: taskId, stage: 'completed', running: false, error_code: null,
        summary: { papers: 738, questions: 1000, materials: 10, generated_ids: 0 },
      } });
    });
    try {
      await page.goto('/#/study');
      await expect(page.getByTestId('question-bank-open')).toContainText('列表版本 1');
      await paint(page);
      expect(statusReads).toBe(1);
      gate.resolve();
      await expect(page.getByTestId('question-bank-notice')).toContainText('已同步 738 份文件');
      await expect(page.getByTestId('question-bank-open')).toContainText('列表版本 2');
      await paint(page);
      expect(paperReads).toBe(2);
      expect(statusReads).toBe(1);
      for (let wake = 1; wake <= 2; wake += 1) {
        const response = page.waitForResponse(response => response.url().endsWith('/question-bank/sync/status'));
        await foreground(page);
        await (await response).finished();
        await expect(page.getByTestId('question-bank-open')).toContainText(`列表版本 ${2 + wake}`);
        await paint(page);
        expect(statusReads).toBe(1 + wake);
        expect(paperReads).toBe(2 + wake);
      }
    } finally {
      gate.resolve();
    }
  });
}

test('foreground resumes a disconnected sync status poll and reports a failed server job', async ({ page }) => {
  await loginAsAdmin(page);
  let statusReads = 0;
  let disconnected = true;
  await page.route('**/api/v1/question-bank/sync/status', async route => {
    statusReads += 1;
    if (disconnected) await route.fulfill({ status: 503, json: { error: 'temporary-unavailable' } });
    else await route.fulfill({ json: {
      task_id: 'resume-failed', stage: 'failed', running: false,
      summary: null, error_code: 'source-unavailable',
    } });
  });
  await page.goto('/#/study');
  await expect(page.getByTestId('question-bank-open').first()).toBeVisible();
  await expect(page.getByTestId('question-bank-notice')).toBeVisible();
  await paint(page);
  const previousReads = statusReads;
  disconnected = false;
  await foreground(page);
  await expect.poll(() => statusReads).toBe(previousReads + 1);
  await expect(page.getByTestId('question-bank-notice')).toContainText('题库同步失败');
  await expect(page.getByTestId('question-bank-notice')).toContainText('source-unavailable');
  await expect(page.getByTestId('question-bank-sync')).toBeEnabled();
});
