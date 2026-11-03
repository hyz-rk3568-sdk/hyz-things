import { expect, test, type Page } from '@playwright/test';
import { expectNoHorizontalOverflow, loginAsAdmin, resetHarness } from './fixtures';
import { inkSnapshot } from './ink-fixtures';

// Standalone capture reads this runner-free payload; keep it plain JavaScript.
export const referenceFixtureSource = String.raw`
const title = '2026 国考副省级 · 行政职业能力测验';
const years = (latest, count, copies = 2) => Array.from({ length: count }, (_, i) => [latest - i, copies]);
const paperPlan = [
  ['国考', [[2026, 3], [2025, 5], ...years(2024, 8)]],
  ['广东', [[2025, 4], ...years(2024, 7)]],
  ['江苏', years(2025, 8)], ['浙江', years(2025, 7)],
  ['湖北', [[2025, 4], ...years(2024, 8, 1)]],
  ['湖南', [[2025, 4], ...years(2024, 8, 1)]],
  ['福建', [[2025, 2], ...years(2024, 8, 1)]],
  ['贵州', [[2025, 2], ...years(2024, 8, 1)]],
  ['陕西', [[2025, 2], ...years(2024, 8, 1)]],
];
const papers = paperPlan.flatMap(([region, groups], r) => groups.flatMap(([year, copies]) =>
  Array.from({ length: copies }, (_, v) => {
    const variant = region === '国考' ? ['副省级', '地市级', '行政执法', '公安专项', '金融专项'][v]
      : region === '广东' ? ['县级', '乡镇卷', '行政执法', '公安专项'][v] : ['A 类', 'B 类', 'C 类', '乡镇卷'][v];
    return { id: 'paper-' + r + '-' + year + '-' + v,
      title: year + ' ' + region + variant + ' · 行政职业能力测验', year, region,
      variant: variant + ' · 网友回忆版', role: 'primary', questions: region === '国考' ? 130 : 100 };
  })));
const categoryPlan = [
  ['资料分析', [['比重', 1820, 36], ['增长率', 1460, 22], ['平均数', 980, 18], ['倍数', 760, 12], ['综合分析', 2448, 40]]],
  ['判断推理', [['图形推理', 4820, 2], ['定义判断', 2600, 1], ['逻辑判断', 5120, 1], ['综合分析', 2978, 0]]],
  ['数量关系', [['数学运算', 5953, 0]]],
  ['言语理解', [['逻辑填空', 5200, 0], ['片段阅读', 6000, 0], ['语句表达', 3015, 0]]],
];
const catalog = { unclassified: 0, modules: categoryPlan.map(([name, rows]) => ({
  name, count: rows.reduce((sum, row) => sum + row[1], 0), done: rows.reduce((sum, row) => sum + row[2], 0),
  uncategorized: 0, categories: rows.map(([name, count, done]) => ({ name, count, done })),
})) };
const names = catalog.modules.map(module => module.name);
function savedRows(scope) {
  const counts = scope === 'wrong' ? [[6, 4, 3, 2, 3], [4, 2, 3, 0], [4], [3, 0, 0]]
    : [[2, 1, 1, 1, 1], [1, 1, 1, 0], [2], [1, 0, 0]];
  let index = 0;
  return catalog.modules.flatMap((module, m) => module.categories.flatMap((category, c) =>
    Array.from({ length: counts[m][c] }, () => {
      const i = index++;
      const region = paperPlan[i % paperPlan.length][0];
      const paper = papers.find(paper => paper.region === region && paper.year === (i === 0 ? 2026 : 2025 - Math.floor(i / paperPlan.length)));
      return { question_id: 'q-' + i, paper_id: paper.id, paper_title: paper.title,
        year: paper.year, region: paper.region, number: i + 1, module: module.name, category: category.name };
    })));
}
const material = { id: 'material-1', name: '资料分析 · 一拖五', markdown:
  '2025 年，全国规模以上文化及相关产业企业实现营业收入 14.15 万亿元，比上年增长 6.0%。其中，文化新业态特征较为明显的 16 个行业小类实现营业收入 5.24 万亿元，增长 9.8%。\n\n' +
  '| 行业 | 营业收入（亿元） | 同比增速 | 占比 |\n| --- | ---: | ---: | ---: |\n' +
  '| 新闻信息服务 | 18,420 | 8.4% | 13.0% |\n| 内容创作生产 | 32,760 | 6.8% | 23.2% |\n| 创意设计服务 | 21,540 | 5.1% | 15.2% |\n| 文化传播渠道 | 16,980 | 9.2% | 12.0% |\n\n' +
  '分区域看，东部地区文化企业营业收入 10.02 万亿元，同比增长 6.7%；中部地区 2.01 万亿元，增长 4.9%；西部地区 1.62 万亿元，增长 5.2%；东北地区 0.50 万亿元，增长 3.8%。\n\n' +
  '材料较长时可独立上下滚动，题目栏保持不动。' };
const item = i => ({ question_id: 'q-' + i, paper_id: papers[0].id, source_hash: 'reference-v1',
  number: 1 + i, answer: [], grade: null, gradable: true });
const questions = Array.from({ length: 130 }, (_, i) => ({ item: item(i), paper_title: title,
  question: { id: 'q-' + i, number: 1 + i,
    stem: '2025 年，文化新业态特征较为明显的 16 个行业小类营业收入占全国规模以上文化及相关产业企业营业收入的比重约为：',
    options_markdown: '- A. 31.6%\n- B. 37.0%\n- C. 42.4%\n- D. 48.1%',
    materials: ['material-1'], module: '资料分析', category: '比重', topics: ['现期比重'], issues: [] },
  materials: [material], content_changed: false }));
questions[107].question.stem = '2025 年，东部地区文化企业营业收入约占全国规模以上文化企业营业收入的：';
questions[107].question.options_markdown = '- A. 61.8%\n- B. 66.4%\n- C. 70.8%\n- D. 72.1%';
const correctAnswer = id => id === 'q-107' ? 'C' : 'B';
function scoreItems(items) {
  return { total: items.length, correct: items.filter(i => i.grade === 'correct').length,
    wrong: items.filter(i => i.grade === 'wrong').length, unanswered: items.filter(i => i.grade === 'unanswered').length,
    ungradable: 0 };
}
function session(id, mode = 'paper', completed = false, count = 130) {
  const wrong = mode === 'paper' ? [0, 2, 6, 8, 10, 12] : mode === 'module' ? [2, 6, 8] : mode === 'favorite' ? [2, 6] : [];
  const unanswered = mode === 'paper' || mode === 'module' ? [4, 19] : mode === 'favorite' ? [4] : [];
  const items = Array.from({ length: count }, (_, i) => {
    const row = item(count <= 27 ? 103 + i : i);
    row.answer = completed ? unanswered.includes(i) ? [] : [wrong.includes(i) ? 'A' : correctAnswer(row.question_id)]
      : i < 47 ? ['B'] : [];
    row.grade = completed ? !row.answer.length ? 'unanswered' : row.answer[0] === correctAnswer(row.question_id) ? 'correct' : 'wrong' : null;
    return row;
  });
  return { id, mode, paper_id: mode === 'paper' ? papers[0].id : null,
    module: mode === 'paper' ? null : '资料分析', category: mode === 'paper' ? null : '比重',
    status: completed ? 'completed' : 'active', created_at: Date.parse('2026-11-03T13:42:00+08:00'),
    updated_at: Date.parse('2026-11-03T15:10:00+08:00'), elapsed_seconds: 1560, paused: true,
    revision: 1, items, score: completed ? scoreItems(items) : null };
}
async function install(page) {
  const runs = [session('resume'), ...['paper', 'module', 'wrong', 'favorite'].map(mode =>
    session('history-' + mode, mode, true, mode === 'wrong' ? 6 : mode === 'favorite' ? 12 : 20))];
  const completedTimes = [
    ['2026-11-03T14:08:00+08:00', 1122], ['2026-11-02T21:16:00+08:00', 725],
    ['2026-11-02T19:44:00+08:00', 318], ['2026-11-01T09:20:00+08:00', 540],
  ];
  runs.slice(1).forEach((run, i) => {
    run.created_at = Date.parse(completedTimes[i][0]);
    run.elapsed_seconds = completedTimes[i][1];
    run.updated_at = run.created_at + run.elapsed_seconds * 1000;
  });
  const moduleResume = session('resume-module', 'module', false, 20);
  moduleResume.items.forEach((item, i) => { if (i >= 5) item.answer = []; });
  moduleResume.elapsed_seconds = 240;
  moduleResume.created_at = Date.parse('2026-11-03T15:32:00+08:00');
  moduleResume.updated_at = moduleResume.created_at;
  runs.push(moduleResume);
  let active;
  let nextId = 0;
  const failures = [];
  await page.route('**/api/v1/question-bank/**', async route => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname.replace('/api/v1/question-bank', '');
    const method = request.method();
    let json;
    if (method === 'GET' && path === '/papers') json = papers;
    else if (method === 'GET' && path === '/practice/catalog') json = catalog;
    else if (method === 'GET' && path === '/practice/history') json = [...runs].sort((a, b) => b.created_at - a.created_at);
    else if (method === 'GET' && path === '/practice/questions') {
      json = savedRows(url.searchParams.get('mode')).filter(row =>
        ['paper_id', 'module', 'category'].every(key => !url.searchParams.get(key) || row[key] === url.searchParams.get(key)));
    } else if (method === 'GET' && /^\/practice\/favorites\//.test(path)) json = { question_id: path.split('/').pop(), favorite: false };
    else if (method === 'GET' && /^\/papers\/[^/]+\/modules$/.test(path)) json = { modules: names.map(name => ({ name, count: 30, ungradable: 0 })), unclassified: 10 };
    else if (method === 'GET' && /^\/papers\/[^/]+$/.test(path)) json = { ...papers.find(p => p.id === path.split('/').pop()), questions: questions.map(q => q.question), materials: [material] };
    else if (method === 'POST' && path === '/practice/sessions') {
      const body = request.postDataJSON();
      active = session('capture-active-' + nextId++, body.mode, false, body.mode === 'paper' ? 130 : 20);
      active.paused = false;
      active.elapsed_seconds = 372;
      active.items.forEach(i => { i.answer = []; });
      runs.unshift(active);
      json = active;
    } else if (/^\/practice\/sessions\//.test(path)) {
      const [, , , id, action, questionId] = path.split('/');
      const run = runs.find(r => r.id === id);
      if (!run) throw new Error('Unknown reference session: ' + id);
      const workspace = () => ({ session: run, questions: run.items.map(item => ({ ...questions[Number(item.question_id.slice(2))], item })) });
      if (method === 'GET' && !action) json = workspace();
      else if (method === 'GET' && action === 'review') json = { session: run, questions: workspace().questions.map(question => ({ question,
        correct_answer: [correctAnswer(question.item.question_id)], explanation: question.item.question_id === 'q-107'
          ? '原始解析\n\n东部地区营业收入 10.02 万亿元，全国 14.15 万亿元。10.02 ÷ 14.15 ≈ 70.8%，因此正确答案为 C。'
          : '原始解析\n\n现期比重 = 部分量 ÷ 整体量。5.24 ÷ 14.15 ≈ 37.0%，因此正确答案为 B。' })) };
      else if (method === 'POST' && action === 'answers') {
        run.items.find(i => i.question_id === questionId).answer = request.postDataJSON().answer; json = run;
      } else if (method === 'POST' && action === 'pause') {
        Object.assign(run, request.postDataJSON()); json = run;
      } else if (method === 'POST' && action === 'submit') {
        run.status = 'completed'; run.paused = true;
        run.items.forEach(i => { i.grade = !i.answer.length ? 'unanswered' : i.answer[0] === correctAnswer(i.question_id) ? 'correct' : 'wrong'; });
        run.score = scoreItems(run.items);
        json = run;
      }
    }
    if (json === undefined) {
      failures.push(method + ' ' + path);
      await route.fulfill({ status: 501, json: { error: 'unhandled-reference-seam', path } });
    } else await route.fulfill({ status: 200, json });
  });
  return failures;
}
async function choosePaper(page) {
  const region = page.getByTestId('question-bank-region').filter({ hasText: '国考' });
  if (!(await region.evaluate(node => node.closest('details').open))) await region.click();
  await page.getByTestId('question-bank-paper-option').filter({ hasText: title }).click();
}
async function open(page, mode = 'paper') {
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-' + mode).click();
  await page.getByTestId('question-bank-practice-setup').waitFor();
}
async function installTrainingTime(page) {
  let module = '';
  await page.route('**/api/v1/study/training-time/**', async route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname.endsWith('/stats')) {
      await route.fulfill({ json: { days: [{ date: '2026-11-03', total_ms: 724000,
        modules: { question_bank: 724000 }, phases: { answer: 724000 } }] } });
      return;
    }
    if (url.pathname.endsWith('/events') && request.method() === 'POST') {
      const body = request.postDataJSON();
      if (body.action === 'enter' || body.action === 'switch') module = body.module || '';
      if (body.action === 'leave') module = '';
    }
    // Freeze the study clock independently of practice timers and the local database.
    await route.fulfill({ json: { now_ms: Date.parse('2026-11-03T15:33:00+08:00'),
      today_ms: 724000, running: false, module } });
  });
}
function describeFixtures() {
  return {
    papers: { total: papers.length, yearMin: Math.min(...papers.map(paper => paper.year)),
      yearMax: Math.max(...papers.map(paper => paper.year)),
      regions: Object.fromEntries(paperPlan.map(([region]) => [region, papers.filter(paper => paper.region === region).length])) },
    modules: catalog.modules,
    collections: { wrong: savedRows('wrong').length, favorite: savedRows('favorite').length,
      wrongRatio: savedRows('wrong').filter(row => row.module === '资料分析' && row.category === '比重').length },
    trainingTime: { endpoint: '/api/v1/study/training-time/**', today_ms: 724000, running: false,
      note: 'HTTP fixture freezes the learning clock at 00:12:04; it does not read the local training database.' },
    material: 'Reference paragraphs, Markdown table, regional paragraph and independent-scroll note; no filler paragraphs.',
    review: { wrong: { index: 2, originalNumber: 106, answer: 'A', correctAnswer: 'B' },
      unanswered: { index: 4, originalNumber: 108, answer: null, correctAnswer: 'C' } },
    history: { total: 6, active: 2, completed: 4, paperScore: '12/20', paperElapsedSeconds: 1122 },
  };
}
module.exports = { install, choosePaper, open, installTrainingTime, describeFixtures };
`;

type ReferenceHelpers = {
  install(page: Page): Promise<string[]>;
  installTrainingTime(page: Page): Promise<void>;
  describeFixtures(): Record<string, unknown>;
  choosePaper(page: Page): Promise<void>;
  open(page: Page, mode?: string): Promise<void>;
};
const payload = { exports: {} as ReferenceHelpers };
new Function('module', referenceFixtureSource)(payload);
export const reference = payload.exports;

test.describe('question bank reference acceptance', () => {
  const seamFailures = new WeakMap<Page, string[]>();
  test.afterEach(async ({ page }) => { expect(seamFailures.get(page)).toEqual([]); });
  test.beforeEach(async ({ page, request }) => {
    await resetHarness(request);
    seamFailures.set(page, await reference.install(page));
    await reference.installTrainingTime(page);
    await loginAsAdmin(page);
    await reference.open(page);
  });

  test('collection buttons display separate 34 wrong and 12 favorite badges', async ({ page }) => {
    for (const [kind, count] of [['wrong', '34'], ['favorite', '12']]) {
      await expect(page.getByTestId('question-bank-collection-' + kind).getByTestId('question-bank-collection-count')).toHaveText(count);
    }
  });

  test('collection counts retain semantic colors inside outline buttons', async ({ page }) => {
    await expect(page.getByTestId('question-bank-collection-wrong').getByTestId('question-bank-collection-count')).toHaveCSS('color', 'rgb(153, 27, 27)');
    await expect(page.getByTestId('question-bank-collection-favorite').getByTestId('question-bank-collection-count')).toHaveCSS('color', 'rgb(79, 53, 230)');
  });

  test('resume card follows the question classification buttons', async ({ page }) => {
    const card = page.getByTestId('question-bank-resume-card');
    await expect(card).toContainText('47/130');
    const group = page.getByRole('group', { name: '题目浏览分类', exact: true });
    const groupBox = await group.boundingBox();
    const cardBox = await card.boundingBox();
    expect(cardBox!.y).toBeGreaterThanOrEqual(groupBox!.y + groupBox!.height);
    expect(await card.evaluate(node => !!(node.compareDocumentPosition(document.querySelector('[aria-label="题目浏览分类"]')!) & Node.DOCUMENT_POSITION_PRECEDING))).toBe(true);
  });

  test('selected original paper uses a pale purple surface, not a solid primary fill', async ({ page }) => {
    await reference.choosePaper(page);
    const selected = page.getByTestId('question-bank-paper-option').filter({ hasText: '2026 国考副省级' });
    await expect(selected).toHaveAttribute('aria-pressed', 'true');
    const color = await selected.evaluate(node => {
      const canvas = document.createElement('canvas');
      const ctx = canvas.getContext('2d')!;
      ctx.fillStyle = '#fff'; ctx.fillRect(0, 0, 1, 1);
      ctx.fillStyle = getComputedStyle(node).backgroundColor; ctx.fillRect(0, 0, 1, 1);
      return [...ctx.getImageData(0, 0, 1, 1).data];
    });
    expect(color[0]).toBeGreaterThan(220);
    expect(color[1]).toBeGreaterThan(215);
    expect(color[2]).toBeGreaterThan(color[1] + 3);
    expect(color[2]).toBeGreaterThan(color[0]);
  });

  test('module category selection belongs only to its parent module', async ({ page }) => {
    await page.getByTestId('training-mode-question-bank-module').click();
    await page.getByTestId('question-bank-cross-module').filter({ hasText: '资料分析' }).click();
    const ratio = page.getByRole('group', { name: '资料分析二级题型', exact: true }).getByTestId('question-bank-cross-category').filter({ hasText: '比重' });
    await ratio.click();
    await expect(ratio).toHaveAttribute('aria-pressed', 'true');
    const shared = (module: string) => page.getByRole('group', { name: module + '二级题型', exact: true })
      .getByTestId('question-bank-cross-category').filter({ hasText: '综合分析' });
    await shared('资料分析').click();
    await expect(shared('资料分析')).toHaveAttribute('aria-pressed', 'true');
    await expect(shared('判断推理')).toHaveAttribute('aria-pressed', 'false');
    await page.getByTestId('question-bank-cross-module').filter({ hasText: '判断推理' }).click();
    await expect(ratio).toHaveAttribute('aria-pressed', 'false');
    await expect(page.getByRole('group', { name: '判断推理二级题型', exact: true }).getByTestId('question-bank-cross-category').filter({ hasText: '全部' })).toHaveAttribute('aria-pressed', 'true');
  });

  for (const kind of ['wrong', 'favorite']) {
    test(kind + ' collection has a contextual title', async ({ page }) => {
      await page.getByTestId('training-mode-question-bank-module').click();
      await page.getByTestId('question-bank-collection-' + kind).click();
      await expect(page.getByRole('heading', { name: kind === 'wrong' ? '我的错题' : '我的收藏', exact: true })).toBeVisible();
      await expect(page.getByRole('heading', { name: '按模块查看', exact: true })).toBeVisible();
      await expect(page.getByRole('heading', { name: '筛选结果', exact: true })).toBeVisible();
    });

    test(kind + ' collection places results beside module cards', async ({ page }) => {
      await page.setViewportSize({ width: 1194, height: 834 });
      await page.getByTestId('training-mode-question-bank-module').click();
      await page.getByTestId('question-bank-collection-' + kind).click();
      await expect(page.getByTestId('question-bank-browse-list')).toBeVisible();
      const left = await page.getByTestId('question-bank-collection-module').first().boundingBox();
      const right = await page.getByTestId('question-bank-browse-list').boundingBox();
      expect(right!.x).toBeGreaterThan(left!.x + left!.width);
    });

    test(kind + ' results use flat divided rows instead of individual rounded cards', async ({ page }) => {
      await page.getByTestId('training-mode-question-bank-module').click();
      await page.getByTestId('question-bank-collection-' + kind).click();
      const row = page.getByTestId('question-bank-collection-item').first();
      await expect(row).toContainText('2026');
      await expect(row).toHaveCSS('border-radius', '0px');
      await expect(row).toHaveCSS('border-bottom-width', '1px');
    });
  }

  test('saved collection module cards follow practice catalog order', async ({ page }) => {
    await page.getByTestId('question-bank-collection-wrong').click();
    await expect(page.getByTestId('question-bank-collection-module').locator('strong')).toHaveText([
      '资料分析', '判断推理', '数量关系', '言语理解',
    ]);
  });

  test('workspace stem, material paragraphs and option text have normal weight', async ({ page }) => {
    await reference.choosePaper(page);
    await page.getByTestId('question-bank-practice-start').click();
    for (const locator of [page.getByTestId('question-bank-practice-stem'),
      page.getByTestId('question-bank-practice-material-pane').locator('.study-markdown p').first(),
      page.getByTestId('question-bank-practice-choice').first().locator('.study-markdown')]) {
      await expect(locator).toHaveCSS('font-weight', '400');
    }
    await expect(page.getByTestId('question-bank-practice-material-pane').getByRole('table')).toBeVisible();
  });

  test('question bank tables keep source alignment through CSP-safe classes', async ({ page }) => {
    await reference.choosePaper(page);
    await page.getByTestId('question-bank-practice-start').click();
    const table = page.getByTestId('question-bank-practice-material-pane').getByRole('table');
    await expect(table).toBeVisible();
    await expect(table.locator('[style]')).toHaveCount(0);
    await expect(table.getByRole('columnheader').nth(1)).toHaveCSS('text-align', 'right');
    await expect(table.getByRole('cell').first()).toHaveCSS('padding-top', '9px');
  });

  test('whole-paper reading shares safe table alignment and preserves literal code', async ({ page }) => {
    await page.route('**/api/v1/question-bank/sync/status', route => route.fulfill({ json: {
      task_id: null, stage: 'idle', running: false, summary: null, error_code: null,
    } }));
    await page.route('**/api/v1/question-bank/papers/paper-0-2026-0', route => route.fulfill({ json: {
      id: 'paper-0-2026-0', title: '2026 国考副省级 · 行政职业能力测验', year: 2026,
      region: '国考', variant: '副省级', role: 'primary',
      materials: [{ id: 'safe-table', name: '对齐材料',
        markdown: '| Left | Center | Right |\n| :--- | :---: | ---: |\n| 甲 | 乙 | 丙 |\n\n`style="text-align: right"`' }],
      questions: [{ id: 'reader-q', number: 1, stem: '阅读表格', options_markdown: '- A. 甲',
        materials: ['safe-table'], module: '资料分析', category: '比重', topics: [], issues: [] }],
    } }));
    await page.goto('/#/study');
    await page.getByTestId('question-bank-open').filter({ hasText: '2026 国考副省级' }).click();
    const paper = page.getByTestId('question-bank-paper');
    const table = paper.getByRole('table');
    await expect(table.locator('[style]')).toHaveCount(0);
    for (const [index, align] of ['left', 'center', 'right'].entries()) {
      await expect(table.getByRole('columnheader').nth(index)).toHaveCSS('text-align', align);
    }
    await expect(paper.locator('code')).toHaveText('style="text-align: right"');
    await page.getByRole('button', { name: '返回试卷列表', exact: true }).click();
    await expect(page.getByTestId('question-bank-papers')).toBeVisible();
  });

  test('question bank handwriting records both content panes and undo restores prior ink', async ({ page }) => {
    await reference.choosePaper(page);
    await page.getByTestId('question-bank-practice-start').click();
    await page.getByTestId('calculation-scratchpad-toggle').click();
    const canvas = page.getByTestId('calculation-scratchpad-canvas');
    await expect(canvas).toBeVisible();
    await expect.poll(() => canvas.evaluate(node => {
      const surface = node as HTMLCanvasElement;
      return surface.width === Math.round(surface.clientWidth * Math.min(3, window.devicePixelRatio));
    })).toBe(true);
    const before = await inkSnapshot(canvas);
    const draw = async (pane: string) => {
      const box = await page.getByTestId(pane).boundingBox();
      const x = box!.x + 30, y = box!.y + 35;
      await page.mouse.move(x, y);
      await page.mouse.down();
      await page.mouse.move(x + 45, y + 16, { steps: 12 });
      await page.mouse.up();
    };
    await draw('question-bank-practice-material-scroll');
    await expect(page.getByTestId('calculation-scratchpad-undo')).toBeEnabled();
    const materialInk = await inkSnapshot(canvas);
    expect(materialInk).not.toBe(before);
    await draw('question-bank-practice-question-scroll');
    expect(await inkSnapshot(canvas)).not.toBe(materialInk);
    await page.getByTestId('calculation-scratchpad-undo').click();
    await expect.poll(() => inkSnapshot(canvas)).toBe(materialInk);
  });

  test('completed question bank review shows the stored elapsed duration', async ({ page }) => {
    await page.getByTestId('question-bank-history-open').click();
    await page.getByTestId('question-bank-practice-history-item').filter({ hasText: '套卷训练' })
      .getByRole('button', { name: '查看原题与解析', exact: true }).click();
    await expect(page.getByTestId('question-bank-practice-timer')).toHaveText('00:18:42');
  });

  test('landscape workspace gives material three fifths of the usable pane width', async ({ page }) => {
    await page.setViewportSize({ width: 1194, height: 834 });
    await reference.choosePaper(page);
    await page.getByTestId('question-bank-practice-start').click();
    await expect(page.getByTestId('question-bank-practice-material-pane')).toBeVisible();
    const material = await page.getByTestId('question-bank-practice-material-pane').boundingBox();
    const question = await page.getByTestId('question-bank-practice-question-pane').boundingBox();
    expect(question!.x).toBeGreaterThan(material!.x + material!.width);
    const ratio = material!.width / (material!.width + question!.width);
    expect(ratio).toBeGreaterThanOrEqual(0.58);
    expect(ratio).toBeLessThanOrEqual(0.62);
    await expectNoHorizontalOverflow(page);
  });

  for (const viewport of [{ width: 1194, height: 834 }, { width: 834, height: 1194 }]) {
    test('setup width and fixed action at ' + viewport.width, async ({ page }) => {
      await page.setViewportSize(viewport);
      const setup = await page.getByTestId('question-bank-practice-setup').locator('.training-setup-content').boundingBox();
      expect(setup!.width).toBeGreaterThanOrEqual(viewport.width === 1194 ? 1000 : 750);
      expect(setup!.width).toBeLessThanOrEqual(1080);
      const action = page.getByTestId('training-setup-action');
      const before = await action.boundingBox();
      await page.getByTestId('training-setup-scroll').evaluate(node => { node.scrollTop = node.scrollHeight; });
      expect(await action.boundingBox()).toEqual(before);
      await expectNoHorizontalOverflow(page);
    });
  }
});
