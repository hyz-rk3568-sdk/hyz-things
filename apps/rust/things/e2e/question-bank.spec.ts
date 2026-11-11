import {expect,test} from '@playwright/test';
import {expectNoHorizontalOverflow,harnessOrigin,loginAsAdmin,resetHarness,webOrigin} from './fixtures';

test.beforeEach(async ({request})=>{await resetHarness(request)});

async function chooseFixturePaper(page:import('@playwright/test').Page) {
  const region=page.getByTestId('question-bank-region').filter({hasText:'国考'});
  if (!(await region.evaluate(node=>(node.closest('details') as HTMLDetailsElement | null)?.open))) {
    await region.click();
  }
  const year=page.getByTestId('question-bank-year').filter({hasText:'2008'});
  if (!(await year.evaluate(node=>(node.closest('details') as HTMLDetailsElement).open))) await year.click();
  await page.getByTestId('question-bank-paper-option').filter({hasText:'行测真题训练端到端样卷'}).click();
}
async function openSheet(page:import('@playwright/test').Page) {
  if (!(await page.getByTestId('training-answer-sheet').isVisible())) {
    await page.getByTestId('training-answer-sheet-toggle').click();
  }
  await expect(page.getByTestId('training-answer-sheet')).toBeVisible();
}



async function openFixtureReview(page:Page) {
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
}

function aiRequestInfo(request_id:string,intent='follow_up',status='complete',error:string|null=null,remote_uncertain=false) {
  return {request_id,run_id:'run-1',intent,status,error,remote_uncertain,new_thread_requested:false,user_message_id:null,retry_of:null};
}
async function mockAiRequestStatus(page:import('@playwright/test').Page) {
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*',async route=>{
    const request_id=new URL(route.request().url()).pathname.split('/').at(-1)!;
    await route.fulfill({json:aiRequestInfo(request_id)});
  });
}

test('answer sheet keeps its scroll position when reopened',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  // Constructable stylesheets work under the portal's strict CSP and persist
  // when the conditional answer-sheet element is unmounted/recreated.
  await page.evaluate(() => {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync('[data-testid="training-answer-sheet"] { max-height: 100px !important; }');
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  });
  await openSheet(page);
  const sheet=page.getByTestId('training-answer-sheet');
  await expect(sheet).toHaveCSS('max-height', '100px');
  await expect.poll(()=>sheet.evaluate(node=>node.scrollHeight-node.clientHeight)).toBeGreaterThan(75);
  await sheet.evaluate(node=>{node.scrollTop=60});
  await expect.poll(()=>sheet.evaluate(node=>node.scrollTop)).toBe(60);
  await page.getByTestId('training-answer-sheet-toggle').click();
  await expect(sheet).toHaveCount(0);
  await page.getByTestId('training-answer-sheet-toggle').click();
  await expect.poll(()=>sheet.evaluate(node=>node.scrollTop)).toBe(60);
});

test('paper practice hides numbering and scroll tools until they are needed',async({page,request})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await chooseFixturePaper(page);
  await expect(page.getByTestId('question-bank-mode')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-browse-count')).toHaveCount(0);
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/3 题');
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('材料中的正确选项');
  await expect(page.getByTestId('question-bank-practice-stem')).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('question-bank-practice-stem')).toHaveCSS('font-weight','400');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await expect(page.getByRole('button',{name:'题目向下滚动'})).toHaveCount(0);
  await expect(page.getByRole('button',{name:'材料向下滚动'})).toHaveCount(0);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect(page.getByRole('button',{name:'题目向下滚动'})).toHaveCount(1);
  await expect(page.getByRole('button',{name:'材料向下滚动'})).toHaveCount(1);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect(page.getByRole('button',{name:'题目向下滚动'})).toHaveCount(0);
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(3);
  await expect(page.getByTestId('question-bank-practice-number').first()).toContainText('26');
  await expect(page.getByTestId('question-bank-practice-scope')).toContainText('资料分析 · 比重');
  const groups = page.getByTestId('question-bank-practice-group');
  await expect(groups).toHaveCount(2);
  await expect(groups.nth(0)).toHaveAttribute('data-group-label', '资料分析');
  await expect(groups.nth(1)).toHaveAttribute('data-group-label', '判断推理');
  await page.getByTestId('training-answer-sheet-toggle').click();
  const sessionId = await page.evaluate(async()=>{
    const res=await fetch('/api/v1/question-bank/practice/history');
    const sessions=await res.json();
    return sessions[0].id as string;
  });
  const response=await request.get(`/api/v1/question-bank/practice/sessions/${sessionId}`,{headers:{Cookie:(await page.context().cookies()).map(c=>`${c.name}=${c.value}`).join('; ')}});
  expect(response.status()).toBe(200);
  expect(await response.text()).not.toContain('原始解析');
  expect((await request.get(`/api/v1/question-bank/practice/sessions/${sessionId}/review`,
    {headers:{Cookie:(await page.context().cookies()).map(c=>`${c.name}=${c.value}`).join('; ')}})).status()).toBe(409);
  await page.getByTestId('question-bank-practice-choice').first().click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-practice-pause')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-timer')).toBeEnabled();
  await page.getByTestId('question-bank-practice-timer').click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.getByTestId('question-bank-practice-timer').click();
  await expect(page.getByTestId('question-bank-practice-paused')).toHaveCount(0);
  await page.getByTestId('question-bank-practice-timer').click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.reload();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-resume-card')).toContainText('继续练习');
  await page.getByTestId('question-bank-resume-card').getByRole('button',{name:'继续练习'}).click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.getByRole('button',{name:'继续练习'}).click();
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('第二道题');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 2/3 题');
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await page.getByTestId('question-bank-practice-choice').nth(2).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-submit-confirm')).toBeVisible();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('第二题解析');
  await expect(page.getByTestId('question-bank-practice-question-pane').locator('.study-markdown').filter({hasText:'第二题解析'})).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('question-bank-practice-review-answer')).toHaveCSS('font-size','16px');
  await expect(page.getByRole('region', {name:'答案统计'})).toHaveClass(/rounded-box/);
  await expect(page.getByRole('region', {name:'答案统计'})).toContainText('你的答案');
  const reviewResponse=await request.get(`/api/v1/question-bank/practice/sessions/${sessionId}/review`,
    {headers:{Cookie:(await page.context().cookies()).map(c=>`${c.name}=${c.value}`).join('; ')}});
  expect(reviewResponse.status()).toBe(200);
  const reviewBody:{questions:{question:{item:{number:number}},correct_count:number,wrong_count:number}[]}=await reviewResponse.json();
  const reviewCounts=(number:number)=>reviewBody.questions.find(q=>q.question.item.number===number)!;
  const attemptCounts=page.getByTestId('question-bank-practice-attempt-counts');
  await expect(attemptCounts).toHaveAttribute('data-correct',String(reviewCounts(27).correct_count));
  await expect(attemptCounts).toHaveAttribute('data-wrong',String(reviewCounts(27).wrong_count));
  await expect(attemptCounts).toContainText(`做对 ${reviewCounts(27).correct_count} 次`);
  await expect(attemptCounts).toContainText(`做错 ${reviewCounts(27).wrong_count} 次`);
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('B 是正确项');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('现期比重');
  await expect(attemptCounts).toHaveAttribute('data-correct',String(reviewCounts(26).correct_count));
  await expect(attemptCounts).toHaveAttribute('data-wrong',String(reviewCounts(26).wrong_count));
  await expect(attemptCounts).toContainText(`做对 ${reviewCounts(26).correct_count} 次`);
  await expect(attemptCounts).toContainText(`做错 ${reviewCounts(26).wrong_count} 次`);
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveClass(/border-error/);
  await expect(page.getByTestId('question-bank-practice-choice').nth(1)).toHaveClass(/border-success/);
  await expect(page.getByTestId('question-bank-practice-choice').first()).not.toContainText('正确答案');
  await expect(page.getByTestId('question-bank-practice-choice').first()).not.toContainText('你的错选');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(2).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 28 题');
  await expect(page.getByTestId('question-bank-practice-scope')).toContainText('判断推理 · 图形推理');
  await expect(page.getByTestId('question-bank-practice-issues')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('待核验');
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('未核验');
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveClass(/border-success/);
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-score').getByLabel('正确 1')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-records')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-repeat')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-ungradable')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-score')).not.toContainText('待核验');
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('ungradable');
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('正确率');
  await expect(page.getByTestId('question-bank-practice-number').first()).toHaveClass(/btn-error/);
  await expect(page.getByTestId('question-bank-practice-number').nth(1)).toHaveClass(/btn-success/);
  await expect(page.getByTestId('question-bank-practice-number').nth(2)).not.toHaveClass(/btn-(success|error)/);
  await page.getByTestId('training-answer-sheet-toggle').click();
  const star=await page.getByTestId('question-bank-favorite').boundingBox();
  const title=await page.getByTestId('question-bank-practice-heading').boundingBox();
  expect(star!.x).toBeLessThan(title!.x);
  await page.getByTestId('question-bank-favorite').click();
  await expect(page.getByTestId('question-bank-favorite')).toHaveAttribute('aria-label','取消收藏');
  // Restore the fixture state so the following test starts with an empty favorite set.
  await page.getByTestId('question-bank-favorite').click();
  await expect(page.getByTestId('question-bank-favorite')).toHaveAttribute('aria-label','收藏题目');
  await expect(page.getByTestId('question-bank-favorite')).not.toHaveAttribute('aria-label','取消收藏');
});

test('question bank reading omits source verification labels',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study');
  const paperButton=page.getByTestId('question-bank-open').filter({hasText:'行测真题训练端到端样卷'});
  await expect(paperButton).toBeVisible();
  await paperButton.click();
  const paper=page.getByTestId('question-bank-paper');
  await expect(paper).toContainText('原卷第 28 题');
  await expect(paper).not.toContainText('待核');
  await expect(paper).not.toContainText('未核验');
});

test('saved collections are classified by module and category before practicing',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-choice').first().click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-submit-confirm')).toBeVisible();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  await page.getByTestId('question-bank-favorite').click();
  await expect(page.getByTestId('question-bank-favorite')).toHaveAttribute('aria-label','取消收藏');
  await page.getByRole('button',{name:/返回训练/}).click();
  await page.getByTestId('question-bank-collection-wrong').click();
  await expect(page.getByTestId('question-bank-browse-count')).toContainText('1题');
  await expect(page.getByTestId('question-bank-collection-module').filter({hasText:'资料分析'})).toBeVisible();
  await page.getByTestId('question-bank-collection-category').filter({hasText:'比重'}).click();
  await expect(page.getByTestId('question-bank-browse-count')).toContainText('1题');
  await expect(page.getByTestId('question-bank-collection-item')).toContainText(/2008\s+国考\s*·\s*第\s*26\s*题/);
  await expect(page.getByTestId('question-bank-browse-preview')).toHaveCount(0);
  await page.getByTestId('question-bank-collection-item').first().click();
  await expect(page.getByTestId('collection-review')).toBeVisible();
  await expect(page.getByTestId('collection-review-scope')).toContainText('错题复盘 · 第 1/1 题');
  await expect(page.getByTestId('collection-review-favorite')).toHaveAttribute('aria-label','取消收藏');
  await expect(page.getByTestId('collection-review-material-pane')).toContainText('两道题共用材料');
  await expect(page.getByTestId('collection-review-choice').first()).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('collection-review-explanation')).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('collection-review-answer')).toContainText('B');
  const collectionCounts=page.getByTestId('collection-review-attempt-counts');
  await expect(collectionCounts).toContainText('本题累计');
  await expect(collectionCounts).toHaveAttribute('data-wrong',/^[1-9]\d*$/);
  const collectionCreate=page.getByTestId('collection-review-create-flashcard');
  await expect(collectionCreate).toBeVisible();
  await expect(collectionCreate).toHaveAttribute('aria-label','为当前题目创建 Flashcard');
  await expect(collectionCreate).toContainText('+');
  await expect(page.locator('.question-bank-pane-header').getByTestId('collection-review-create-flashcard')).toBeVisible();
  await collectionCreate.click();
  await expect(page.getByTestId('favorite-flashcard-card')).toBeVisible();
  await page.getByRole('button',{name:'返回复盘'}).click();
  await page.getByTestId('collection-review-back').click();
  await expect(page.getByTestId('question-bank-collection-item')).toBeVisible();
  await page.getByTestId('question-bank-practice-start').click();
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(1);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/1 题');
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-choice').nth(1).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('正确答案');
  await page.getByRole('button',{name:/返回训练/}).click();
  await page.getByTestId('question-bank-collection-favorite').click();
  await expect(page.getByTestId('question-bank-browse-count')).toContainText('1题');
  await expect(page.getByTestId('question-bank-collection-item')).toBeVisible();
  await page.getByTestId('question-bank-collection-item').first().click();
  await expect(page.getByTestId('collection-review')).toBeVisible();
  await expect(page.getByTestId('collection-review-scope')).toContainText('收藏复盘 · 第 1/1 题');
  await expect(page.getByTestId('collection-review-favorite')).toHaveAttribute('aria-label','取消收藏');
  await expect(page.getByTestId('collection-review-answer')).toContainText('B');
  await expect(collectionCounts).toHaveAttribute('data-correct',/^[1-9]\d*$/);
  await page.getByTestId('collection-review-back').click();
  await expect(page.getByTestId('question-bank-collection-item')).toBeVisible();
  await page.getByTestId('question-bank-practice-start').click();
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(1);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/1 题');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
});

test('question bank practice mutations enforce admin, origin and CSRF',async({request})=>{
  const panel=await request.get('/api/v1/panel');
  const csrf=(await panel.json() as {csrf_token:string}).csrf_token;
  const body={mode:'paper',paper_id:null,module:null};
  const noAuth=await request.post('/api/v1/question-bank/practice/sessions',{
    headers:{Origin:webOrigin,'X-HYZ-CSRF':csrf},data:body,
  });
  expect(noAuth.status()).toBe(401);
  const invalidCsrf=await request.post('/api/v1/question-bank/practice/sessions',{
    headers:{Origin:webOrigin,'X-HYZ-CSRF':'invalid'},data:body,
  });
  expect(invalidCsrf.status()).toBe(403);
  const read=await request.get('/api/v1/question-bank/practice/history');
  expect(read.status()).toBe(401);
  expect((await request.get('/api/v1/question-bank/practice/questions/019a3d8e-1b32-7a60-9c83-f34217a6e210')).status()).toBe(401);
  expect((await request.get('/api/v1/question-bank/practice/questions?mode=favorite')).status()).toBe(401);
});

test('question bank module drill filters real categories across original papers',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').first()).toBeVisible();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('question-bank-cross-category').filter({hasText:'比重'}).click();
  await expect(page.getByTestId('question-bank-cross-category').filter({hasText:'比重'})).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/3 题');
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await openSheet(page);
  // The 比重 category selects the whole M1 material group, so its 增长 sibling
  // stays with it; the second paper contributes one solo question.
  const numbers=page.getByTestId('question-bank-practice-number');
  await expect(numbers).toHaveCount(3);
  await expect(numbers.filter({hasText:'12'})).toHaveCount(1);
  await expect(numbers.filter({hasText:'26'})).toHaveCount(1);
  await expect(numbers.filter({hasText:'27'})).toHaveCount(1);
  const sheetOrder=await numbers.allTextContents();
  expect(sheetOrder.indexOf('26')).toBeLessThan(sheetOrder.indexOf('27'));
  await numbers.filter({hasText:'12'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-workspace')).toContainText('第二份跨卷模块练习卷');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('第二份试卷的比重题');
  await openSheet(page);
  await numbers.filter({hasText:'26'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('材料中的正确选项');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await openSheet(page);
  await numbers.filter({hasText:'27'}).click();
  await expect(page.getByTestId('question-bank-practice-scope')).toContainText('资料分析 · 增长');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
});

test('switching training modes preserves the module selection',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-calculation').click();
  const requests:string[]=[];
  const record=(request:import('@playwright/test').Request)=>requests.push(new URL(request.url()).pathname);
  page.on('request',record);
  const version=page.waitForResponse(response=>response.url().includes('/question-bank/data-version'));
  await page.getByTestId('training-mode-question-bank-module').click();
  await version;
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.evaluate(()=>new Promise(requestAnimationFrame));
  page.off('request',record);
  expect(requests.filter(path=>path.endsWith('/papers')||path.endsWith('/practice/catalog')||path.endsWith('/practice/history'))).toEqual([]);
});

test('starting a cached drill waits for the current version check',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-calculation').click();
  let release!:()=>void;
  const gate=new Promise<void>(resolve=>{release=resolve});
  let arrived!:()=>void;
  const requested=new Promise<void>(resolve=>{arrived=resolve});
  await page.route('**/api/v1/question-bank/data-version',async route=>{arrived();await gate;await route.continue()});
  await page.getByTestId('training-mode-question-bank-module').click();
  await requested;
  await expect(page.getByTestId('question-bank-practice-start')).toBeDisabled();
  release();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
});

test('version check failures refresh data and clear after recovery',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toBeVisible();
  await page.getByTestId('training-mode-calculation').click();
  await page.route('**/api/v1/question-bank/data-version',route=>route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({error:{code:'unavailable',message:'版本不可用'}})}));
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-error')).toBeVisible();
  await page.unroute('**/api/v1/question-bank/data-version');
  await page.getByTestId('training-mode-calculation').click();
  const version=page.waitForResponse(response=>response.url().includes('/question-bank/data-version')&&response.ok());
  await page.getByTestId('training-mode-question-bank-module').click();
  await version;
  await expect(page.getByTestId('question-bank-error')).toHaveCount(0);
});

test('switching between module and paper retains both selections',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
});

test('returning to module training updates progress from another device',async({page,browser,request})=>{
  expect((await request.put(`${harnessOrigin}/reset-question-bank`)).ok()).toBeTruthy();
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('0/3');
  await page.getByTestId('training-mode-calculation').click();
  const other=await browser.newContext();
  try {
    await other.addCookies(await page.context().cookies());
    const second=await other.newPage();
    await second.goto(`${webOrigin}/#/study/training`);
    await second.getByTestId('training-mode-question-bank-module').click();
    await second.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
    await second.getByTestId('question-bank-practice-start').click();
    await expect(second.getByTestId('question-bank-practice-workspace')).toBeVisible();
    const saved=second.waitForResponse(response=>response.url().includes('/answers/')&&response.request().method()==='POST');
    await second.getByTestId('question-bank-practice-choice').first().click();
    await saved;
    await second.getByTestId('question-bank-practice-submit').click();
    if(await second.getByTestId('question-bank-submit-confirm').isVisible()) await second.getByRole('button',{name:'确认交卷'}).click();
    await expect(second.getByTestId('question-bank-practice-score')).toBeVisible();
    await page.getByTestId('training-mode-question-bank-module').click();
    await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('1/3');
  } finally { await other.close(); }
});

test('returning to module training picks up settings changed on another device',async({page,browser})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('training-mode-calculation').click();
  const other=await browser.newContext();
  try {
    await other.addCookies(await page.context().cookies());
    const second=await other.newPage();
    await second.goto(`${webOrigin}/#/study/training`);
    await second.getByTestId('training-mode-question-bank-module').click();
    await second.getByTestId('module-drill-settings-open').click();
    await second.getByTestId('module-filter-count').filter({hasText:'10题'}).click();
    await second.getByTestId('module-filter-save').click();
    await expect(second.getByTestId('module-drill-settings')).toBeHidden();
    await page.getByTestId('training-mode-question-bank-module').click();
    await expect(page.getByTestId('module-drill-filter-summary')).toContainText('10 题');
    await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  } finally {
    await other.close();
  }
});

test('module training counts and selects cross-paper duplicates once',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  const module=page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'});
  await expect(module).toContainText('/3');
  await module.click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('1/3');
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(3);
});

test('module filter settings are shared across browser contexts',async({page,browser})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('module-drill-settings-open').click();
  await page.getByTestId('module-filter-count').filter({hasText:'10题'}).click();
  await page.getByTestId('module-filter-save').click();
  await expect(page.getByTestId('module-drill-settings')).toBeHidden();
  const context=await browser.newContext();
  try {
    await context.addCookies(await page.context().cookies());
    const other=await context.newPage();
    await other.goto(`${webOrigin}/#/study/training`);
    await other.getByTestId('training-mode-question-bank-module').click();
    await expect(other.getByTestId('module-drill-filter-summary')).toContainText('10 题');
  } finally {
    await context.close();
  }
});

test('module filter save errors remain visible in the settings dialog',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('module-drill-settings-open').click();
  await page.route('**/api/v1/question-bank/module-settings',async route=>{
    if (route.request().method()==='POST') await route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({error:{code:'unavailable',message:'保存失败'}})});
    else await route.continue();
  });
  await page.getByTestId('module-filter-save').click();
  await expect(page.getByTestId('module-drill-settings').getByRole('alert')).toBeVisible();
  await expect(page.getByTestId('module-drill-settings')).toBeVisible();
});

test('module filter settings persist, apply server-side, and reflect answered/total progress',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('/3');
  await page.getByTestId('module-drill-settings-open').click();
  await expect(page.getByTestId('module-drill-settings')).toBeVisible();
  await page.getByTestId('module-filter-custom-year-toggle').click();
  await page.getByTestId('module-filter-year-min').fill('2018');
  await page.getByTestId('module-filter-year-max').fill('2018');
  await page.getByTestId('module-filter-region').filter({hasText:'广东'}).click();
  await page.getByTestId('module-filter-count').filter({hasText:'10题'}).click();
  await page.getByTestId('module-filter-save').click();
  await expect(page.getByTestId('module-drill-settings')).toBeHidden();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('/1');
  await page.reload();
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('module-drill-filter-summary')).toContainText('2018');
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('/1');
  await page.getByTestId('module-drill-settings-open').click();
  await expect(page.getByTestId('module-filter-custom-year-toggle')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('module-filter-year-min')).toHaveValue('2018');
  await expect(page.getByTestId('module-filter-year-max')).toHaveValue('2018');
  await expect(page.getByTestId('module-filter-region').filter({hasText:'广东'})).toHaveAttribute('aria-pressed','true');
  await page.getByRole('button',{name:'← 返回'}).click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-workspace')).toContainText('第二份跨卷模块练习卷');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('1/1');
  const answerSaved=page.waitForResponse(response=>response.url().includes('/answers/')&&response.request().method()==='POST');
  await page.getByTestId('question-bank-practice-choice').first().click();
  await answerSaved;
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-practice-score')).toBeVisible();
  const today=await page.evaluate(async()=>{
    const response=await fetch('/api/v1/study/training-time/stats');
    if(!response.ok) throw new Error(`statistics failed: ${response.status}`);
    return (await response.json()).days.at(-1);
  });
  expect(today.question_bank.modules['资料分析']['比重']).toBeGreaterThanOrEqual(1);
  await page.getByRole('button',{name:/← 返回训练/}).click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('1/1');
});
test('swipe follows the finger before release and reveals the neighbouring page',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.emulateMedia({reducedMotion:'no-preference'});
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  const surface=page.getByTestId('training-swipe-surface');
  const heading=page.getByTestId('question-bank-practice-heading');
  const sheet=page.getByTestId('training-answer-sheet-toggle');
  const stem=page.getByTestId('question-bank-practice-stem');
  const outgoing=page.locator('[data-page-turn="out"]');
  await expect(sheet).toContainText('1/3');

  // Keep the finger down: the pages must already be sliding and both must be readable.
  const firstDrag=await surface.evaluate((node)=>{
    const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'touch',pointerId:17,isPrimary:true,clientX:x,clientY:230
    }));
    emit('pointerdown',500);emit('pointermove',460);emit('pointermove',400);emit('pointermove',300);
    const out=document.querySelector('[data-page-turn="out"]') as HTMLElement | null;
    const rect=out?.getBoundingClientRect();
    return out ? {
      text:out.textContent ?? '',
      direction:out.getAttribute('data-direction'),
      transform:out.style.transform,
      animations:out.getAnimations().length,
      duplicateTestIds:out.querySelectorAll('[data-testid]').length,
      left:rect?.left ?? 0,
      right:rect?.right ?? 0,
    } : null;
  });
  expect(firstDrag).not.toBeNull();
  expect(firstDrag?.text).toContain('材料中的正确选项');
  expect(firstDrag?.direction).toBe('next');
  expect(firstDrag?.transform).toContain('translateX(-200px)');
  expect(firstDrag?.animations).toBe(0);
  expect(firstDrag?.duplicateTestIds).toBe(0);
  expect(firstDrag!.left).toBeLessThan(0);
  expect(firstDrag!.right).toBeGreaterThan(0);
  await expect(heading).toContainText('第 2/3 题');
  await expect(sheet).toContainText('2/3');
  await expect(stem).toContainText('第二道题');
  await expect(outgoing).toHaveCount(1);

  // Releasing past the threshold finishes the turn.
  await surface.evaluate((node)=>{
    node.dispatchEvent(new PointerEvent('pointerup',{
      bubbles:true,pointerType:'touch',pointerId:17,isPrimary:true,clientX:300,clientY:230
    }));
  });
  await expect(outgoing).toHaveCount(0,{timeout:2000});
  await expect(heading).toContainText('第 2/3 题');
  await expect(stem).toHaveCount(1);
  await expect(stem).toContainText('第二道题');

  // A short drag previews the next page, then snaps back when released.
  await surface.evaluate((node)=>{
    const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'touch',pointerId:18,isPrimary:true,clientX:x,clientY:230
    }));
    emit('pointerdown',500);emit('pointermove',470);emit('pointermove',450);
  });
  await expect(heading).toContainText('第 3/3 题');
  await expect(stem).toContainText('这是一道判断推理题');
  const cancelDrag=await surface.evaluate(()=>document.querySelector('[data-page-turn="out"]')?.textContent ?? '');
  expect(cancelDrag).toContain('第二道题');
  await surface.evaluate((node)=>{
    node.dispatchEvent(new PointerEvent('pointerup',{
      bubbles:true,pointerType:'touch',pointerId:18,isPrimary:true,clientX:450,clientY:230
    }));
  });
  await expect(heading).toContainText('第 2/3 题');
  await expect(outgoing).toHaveCount(0,{timeout:2000});
  await expect(stem).toContainText('第二道题');

  // Pen drags follow the same live path.
  await surface.evaluate((node)=>{
    const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'pen',pointerId:19,isPrimary:true,clientX:x,clientY:230
    }));
    emit('pointerdown',250);emit('pointermove',350);emit('pointermove',450);
    const out=document.querySelector('[data-page-turn="out"]') as HTMLElement | null;
    return out ? {
      text:out.textContent ?? '',
      direction:out.getAttribute('data-direction'),
      transform:out.style.transform,
    } : null;
  }).then((penDrag)=>{
    expect(penDrag).not.toBeNull();
    expect(penDrag?.text).toContain('第二道题');
    expect(penDrag?.direction).toBe('previous');
    expect(penDrag?.transform).toContain('translateX(200px)');
  });
  await expect(heading).toContainText('第 1/3 题');
  await expect(stem).toContainText('材料中的正确选项');
  await surface.evaluate((node)=>{
    node.dispatchEvent(new PointerEvent('pointerup',{
      bubbles:true,pointerType:'pen',pointerId:19,isPrimary:true,clientX:450,clientY:230
    }));
  });
  await expect(outgoing).toHaveCount(0,{timeout:2000});
  await expect(sheet).toContainText('1/3');

  // A vertical gesture never starts a page turn.
  await surface.evaluate((node)=>{
    const emit=(type:string,x:number,y:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'touch',pointerId:20,isPrimary:true,clientX:x,clientY:y
    }));
    emit('pointerdown',300,180);emit('pointermove',304,270);emit('pointerup',300,380);
  });
  await expect(outgoing).toHaveCount(0);
  await expect(sheet).toContainText('1/3');
  const overflow=await page.evaluate(()=>document.documentElement.scrollHeight-document.documentElement.clientHeight);
  expect(overflow).toBeLessThanOrEqual(1);
});

test('a delayed AI lookup respects a manually selected original explanation', async ({page}) => {
  let requested = false;
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    const parts = new URL(route.request().url()).pathname.split('/');
    const offset = parts.indexOf('questions');
    requested = true;
    await pending;
    await route.fulfill({json: {
      question_id: parts[offset + 1], status:'complete', error:null,
      messages:[{role:'assistant',content:'已保存的解析',created_at:1}],
    }});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect.poll(() => requested).toBe(true);
  await page.getByTestId('question-bank-original-tab').click();
  const response = page.waitForResponse(response => response.url().endsWith('/ai'));
  release();
  await response;
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
});

test('a delayed AI lookup cannot switch a different question', async ({page}) => {
  let firstQuestion = '';
  let secondRequested = false;
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    const parts = new URL(route.request().url()).pathname.split('/');
    const offset = parts.indexOf('questions');
    const question_id = parts[offset + 1];
    if (!firstQuestion) firstQuestion = question_id;
    const first = question_id === firstQuestion;
    if (first) await pending;
    else secondRequested = true;
    await route.fulfill({json: {
      question_id, status:first ? 'complete' : 'idle', error:null,
      messages:first ? [{role:'assistant',content:'上一题的解析',created_at:1}] : [],
    }});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect.poll(() => firstQuestion.length > 0).toBe(true);
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect.poll(() => secondRequested).toBe(true);
  const response = page.waitForResponse(response => response.url().includes(firstQuestion) && response.url().endsWith('/ai'));
  release();
  await response;
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
});

test('practice header keeps one row on tablets and reveals the full title on tap', async ({page}) => {
  await page.setViewportSize({width:1024,height:768});
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  const header = page.getByTestId('question-bank-practice-workspace').locator('header');
  const title = page.getByTestId('question-bank-practice-title');
  const bars = page.getByTestId('study-goal-bars');
  await expect(title).toHaveAttribute('aria-expanded','false');
  const before = (await header.boundingBox())!.height;
  const titleBox = (await title.boundingBox())!;
  const barsBox = (await bars.boundingBox())!;
  expect(Math.abs((titleBox.y + titleBox.height/2) - (barsBox.y + barsBox.height/2))).toBeLessThan(24);
  await title.click();
  await expect(page.getByTestId('question-bank-practice-title-popover')).toContainText('行测真题训练端到端样卷');
  expect(Math.abs((await header.boundingBox())!.height - before)).toBeLessThanOrEqual(1);
  await title.click();
  await expect(page.getByTestId('question-bank-practice-title-popover')).toHaveCount(0);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test('completed practice header actions stay inside a phone viewport', async ({page}) => {
  await page.setViewportSize({width:390,height:844});
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  const records = page.getByTestId('question-bank-practice-records');
  await expect(records).toBeVisible();
  const repeat = page.getByTestId('question-bank-practice-repeat');
  await expect(repeat).toBeVisible();
  for (const control of [records, repeat]) {
    const box = await control.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  }
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test('AI analysis renders LaTeX delimiters with KaTeX', async ({page}) => {
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    const parts = new URL(route.request().url()).pathname.split('/');
    const question_id = parts[parts.indexOf('questions') + 1];
    await route.fulfill({json: {
      question_id, status: 'complete', error: null,
      messages: [{
        role: 'assistant',
        content: '设 \\(x\\) 元。\n\n\\[\n0.7x-25=48.5\n\\]\n\n\\(x=105\\)',
        created_at: 1,
      }],
    }});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  const message = page.getByTestId('question-bank-ai-message');
  await expect(message.locator('.katex').first()).toBeVisible();
  await expect(message.locator('.katex-display')).toHaveCount(1);
});

test('saved AI analysis opens by default and remains scoped to its question', async ({page}) => {
  let savedQuestion = '';
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    const parts = new URL(route.request().url()).pathname.split('/');
    const offset = parts.indexOf('questions');
    const question_id = parts[offset + 1];
    if (!savedQuestion) savedQuestion = question_id;
    const saved = question_id === savedQuestion;
    await route.fulfill({json: {
      question_id, status: saved ? 'complete' : 'idle', error: null,
      messages: saved ? [{role:'assistant', content:'已经保存的 AI 解析', created_at:1}] : [],
    }});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('已经保存的 AI 解析');
  await page.getByTestId('question-bank-original-tab').click();
  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect(page.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('已经保存的 AI 解析');
});

test('deleting saved AI analysis sends a same-origin JSON request', async ({page}) => {
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    if (route.request().method() !== 'GET') {
      await route.continue();
      return;
    }
    const parts = new URL(route.request().url()).pathname.split('/');
    const question_id = parts[parts.indexOf('questions') + 1];
    await route.fulfill({json: {
      question_id, status: 'complete', error: null,
      messages: [{role:'assistant', content:'待删除的 AI 解析', created_at:1}],
    }});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('待删除的 AI 解析');
  await page.getByRole('button',{name:'更多解析操作'}).click();
  await page.getByTestId('question-bank-ai-delete').click();
  const deleted = page.waitForResponse(response =>
    response.request().method() === 'DELETE' &&
    new URL(response.url()).pathname.endsWith('/ai'));
  await page.getByTestId('question-bank-ai-delete-confirm').click();
  expect((await deleted).status()).toBe(200);
  await expect(page.getByTestId('question-bank-ai-status')).toContainText('未生成');
  await expect(page.getByTestId('question-bank-ai-message')).toHaveCount(0);
});

test('AI review keeps running across navigation and restores follow-ups', async ({page}) => {
  type Msg = {role: string; content: string; created_at: number};
  type State = {question_id: string; status: string; error: null; messages: Msg[]};
  const records = new Map<string,State>();
  const actions: string[] = [];
  const requests=new Map<string,{question_id:string,intent:string}>();
  await page.route('**/api/v1/question-bank/practice/questions/*/ai**',async route => {
    const path = new URL(route.request().url()).pathname;
    const parts = path.split('/');
    const offset = parts.indexOf('questions');
    const question_id = parts[offset+1];
    const suffix = path.endsWith('/messages') ? '/messages' : path.endsWith('/stop') ? '/stop' : path.endsWith('/retry') ? '/retry' : '';
    const requestOffset=parts.indexOf('requests');
    const key = question_id;
    const record = records.get(key) ?? {question_id,status:'idle',error:null,messages:[]};
    records.set(key,record);
    if(requestOffset>=0 && route.request().method()==='GET') {
      const request_id=parts[requestOffset+1];
      const saved=requests.get(request_id)!;
      await route.fulfill({json:aiRequestInfo(request_id,saved.intent,record.status)});
      return;
    }
    if(route.request().method()==='POST'){
      actions.push(suffix || 'start');
      if(suffix==='/stop') record.status='stopped';
      else if(suffix==='/messages'){
        const body=route.request().postDataJSON() as {message:string};
        record.messages.push({role:'user',content:body.message,created_at:1});
        record.status='running';
      } else record.status='running';
      if(suffix!=='/stop') {
        const request_id=route.request().postDataJSON().request_id as string;
        const intent=suffix==='/messages' || suffix==='/retry' ? 'follow_up' : 'initial';
        requests.set(request_id,{question_id,intent});
        await route.fulfill({json:{...record,request:aiRequestInfo(request_id,intent,record.status)}});
        return;
      }
    }
    await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(record)});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-choice').first().click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();

  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  await expect(page.getByTestId('question-bank-ai-review')).toHaveCount(0);
  await expect.poll(() => records.size).toBe(1); // Check for an existing analysis without starting one.

  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-start')).toBeVisible();
  await page.getByTestId('question-bank-ai-start').click();
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  expect(actions).toEqual(['start']);

  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect(page.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  expect(actions).toEqual(['start']); // no implicit interrupt on question navigation

  await page.getByRole('button',{name:/返回训练/}).click();
  await page.getByTestId('question-bank-history-open').click();
  await page.getByTestId('question-bank-practice-history-item').first()
    .getByRole('button',{name:'查看原题与解析'}).click();
  await expect(page.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  expect(actions).toEqual(['start']); // exiting review does not cancel

  const persisted=Array.from(records.values()).find(v=>v.status==='running')!;
  persisted.status='complete';
  persisted.messages.push({role:'assistant',content:'我的独立解法：优先使用速算。',created_at:2});
  await page.getByTestId('question-bank-original-tab').click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('我的独立解法');
  await expect(page.getByTestId('question-bank-ai-message').locator('.study-markdown')).toHaveCSS('font-size','16px');
  await page.getByTestId('question-bank-ai-input').fill('为什么不选 A？');
  const compose = page.getByTestId('question-bank-ai-compose');
  const inputBox = await page.getByTestId('question-bank-ai-input').boundingBox();
  const sendBox = await page.getByTestId('question-bank-ai-send').boundingBox();
  const composeBox = await compose.boundingBox();
  expect(inputBox!.width).toBeGreaterThan(composeBox!.width * 0.5);
  expect(sendBox!.x).toBeGreaterThanOrEqual(inputBox!.x + inputBox!.width);
  expect(Math.abs((sendBox!.y + sendBox!.height / 2) - (inputBox!.y + inputBox!.height / 2))).toBeLessThan(5);
  expect(sendBox!.x + sendBox!.width).toBeLessThanOrEqual(composeBox!.x + composeBox!.width);
  expect(sendBox!.y + sendBox!.height).toBeLessThanOrEqual(composeBox!.y + composeBox!.height);
  await page.getByTestId('question-bank-ai-send').click();
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  await expect(page.getByTestId('question-bank-ai-message').last()).toContainText('为什么不选 A？');
  await page.getByTestId('question-bank-ai-stop').click();
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('已停止');
  expect(actions).toEqual(['start','/messages','/stop']);
  await page.getByTestId('question-bank-original-tab').click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-message')).toHaveCount(2);
  await expect(page.getByTestId('question-bank-ai-restart')).toBeVisible();
  await page.getByTestId('question-bank-ai-restart').click();
  await page.getByTestId('question-bank-ai-retry-confirm').click();
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  expect(actions).toEqual(['start','/messages','/stop','/retry']);
});


test('any original-paper review can create a category flashcard without first favoriting',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-create-flashcard')).toHaveCount(0);
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  const create=page.getByTestId('question-bank-practice-create-flashcard');
  await expect(create).toBeVisible();
  await expect(create).toHaveAttribute('aria-label','为当前题目创建 Flashcard');
  await expect(create).toContainText('+');
  await expect(page.locator('.question-bank-pane-header').getByTestId('question-bank-practice-create-flashcard')).toBeVisible();
  await create.click();
  const dialog=page.getByTestId('question-bank-practice-flashcard-dialog');
  await expect(dialog).toBeVisible();
  await expect(dialog).not.toHaveAttribute('role', 'dialog');
  await expect(dialog).not.toHaveCSS('position', 'fixed');
  const card=dialog.getByTestId('favorite-flashcard-card');
  const form=card.getByTestId('favorite-flashcard-form');
  const borderBefore=await form.evaluate(node=>getComputedStyle(node).borderColor);
  await card.getByPlaceholder('写下解题思路或复习提示').focus();
  await expect.poll(()=>form.evaluate(node=>getComputedStyle(node).borderColor)).not.toBe(borderBefore);
  await page.setViewportSize({width:390,height:844});
  await expectNoHorizontalOverflow(page);
  await page.setViewportSize({width:1280,height:720});
  await expect(card.getByRole('button',{name:'创建并同步'})).toBeDisabled();
  await expect(card.getByTestId('favorite-flashcard-path')).toHaveText('将追加或创建：flashcards/真题练习/资料分析/比重/错题/cards.md');
  await card.getByLabel('确认写入所选目录').check();
  await card.getByRole('button',{name:'创建并同步'}).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-create-flashcard')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-manage-flashcard')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-manage-flashcard')).toHaveAttribute('aria-label','管理当前题目的 1 张 Flashcard');
  await expect(page.getByTestId('question-bank-practice-manage-flashcard')).toContainText('1');
  await page.getByTestId('question-bank-practice-manage-flashcard').click();
  const editor=page.getByTestId('favorite-flashcard-editor');
  await expect(editor.getByTestId('favorite-flashcard-path')).toContainText('真题练习/资料分析/比重/错题/cards.md');
  await editor.getByTestId('favorite-flashcard-preview-toggle').click();
  await expect(editor.getByTestId('favorite-flashcard-front-preview')).toBeVisible();
  await page.getByRole('button',{name:'返回复盘'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第');
});

for (const viewport of [{width:1280,height:720},{width:390,height:844}]) {
test(`review remembers each question and material scroll position at ${viewport.width}px`,async({page})=>{
  await page.setViewportSize(viewport);
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await page.evaluate(()=>document.styleSheets[0].insertRule('[data-testid="question-bank-practice-material-scroll"]::after, [data-testid="question-bank-practice-question-scroll"]::after { content: ""; display: block; height: 1600px; }'));
  const material=page.getByTestId('question-bank-practice-material-scroll');
  const question=page.getByTestId('question-bank-practice-question-scroll');
  await material.evaluate(node=>{node.scrollTop=180});
  await question.evaluate(node=>{node.scrollTop=270});
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(180);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(270);
  const swipe=async(direction:'next'|'previous')=>{
    await page.getByTestId('training-swipe-surface').evaluate((node,direction)=>{
      const [start,end]=direction==='next'?[500,250]:[250,500];
      const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerType:'touch',pointerId:17,isPrimary:true,clientX:x,clientY:230}));
      emit('pointerdown',start);emit('pointermove',end);emit('pointerup',end);
    },direction);
  };
  await swipe('next');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(0);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(0);
  await material.evaluate(node=>{node.scrollTop=90});
  await question.evaluate(node=>{node.scrollTop=130});
  await swipe('previous');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(180);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(270);
  await swipe('next');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(90);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(130);
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(180);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(270);
  await page.getByTestId('question-bank-practice-records').click();
  await page.getByRole('button',{name:'返回套卷'}).click();
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(0);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(0);
});
}

test('long unbroken AI explanation wraps inside the review pane',async({page})=>{
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,
      messages:[{role:'assistant',content:'解析：'+('A'.repeat(400)),created_at:1}]}});
  });
  await page.setViewportSize({width:1280,height:720});
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('A'.repeat(400));
  const pane=page.getByTestId('question-bank-ai-messages');
  await expect.poll(()=>pane.evaluate(node=>node.scrollWidth-node.clientWidth)).toBeLessThanOrEqual(1);
});

test('long inline and display equations stay readable within the review pane',async({page,context,browserName})=>{
  const inline='A'.repeat(120), display='B'.repeat(120);
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,
      messages:[{role:'assistant',content:`行内：\\(\\text{${inline}}\\) 结束。\n\n\\[\\text{${display}}\\]`,created_at:1}]}});
  });
  await page.setViewportSize({width:390,height:844});
  await openFixtureReview(page);
  const message=page.getByTestId('question-bank-ai-message');
  const inlineMath=message.locator('.math-inline');
  const displayMath=message.locator('.math-display');
  await expect(inlineMath.locator('.katex')).toBeVisible();
  await expect(displayMath.locator('.katex')).toBeVisible();
  const pane=page.getByTestId('question-bank-ai-messages');
  await expect.poll(()=>pane.evaluate(node=>node.scrollWidth-node.clientWidth)).toBeLessThanOrEqual(1);
  for(const formula of [inlineMath,displayMath]) {
    await expect.poll(()=>formula.evaluate(node=>node.scrollWidth-node.clientWidth)).toBeGreaterThan(10);
    await formula.evaluate(node=>{node.scrollLeft=node.scrollWidth});
    await expect.poll(()=>formula.evaluate(node=>node.scrollLeft)).toBeGreaterThan(10);
  }
  if (browserName==='chromium') {
    await displayMath.scrollIntoViewIfNeeded();
    await displayMath.evaluate(node=>{node.scrollLeft=0});
    const box=await displayMath.boundingBox();
    if (!box) throw new Error('display formula not visible');
    const x=Math.round(box.x+box.width*0.8),y=Math.round(box.y+box.height*0.5);
    const cdp=await context.newCDPSession(page);
    await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x,y,id:1}]});
    for(let i=1;i<=8;i++) {
      await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:x-i*15,y,id:1}]});
    }
    await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    await expect.poll(()=>displayMath.evaluate(node=>node.scrollLeft)).toBeGreaterThan(10);
    await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  }
});

test('wide explanation tables scroll locally without turning the page',async({page,context,browserName})=>{
  const row='|'+Array.from({length:20},(_,i)=>` 列${i+1} `).join('|')+'|';
  const separator='|'+Array.from({length:20},()=> ' --- ').join('|')+'|';
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,
      messages:[{role:'assistant',content:`表格解析\n\n${row}\n${separator}\n${row}`,created_at:1}]}});
  });
  await page.setViewportSize({width:390,height:844});
  await openFixtureReview(page);
  const pane=page.getByTestId('question-bank-ai-messages');
  const table=page.getByTestId('question-bank-ai-message').locator('table');
  await expect(table).toBeVisible();
  await expect.poll(()=>pane.evaluate(node=>node.scrollWidth-node.clientWidth)).toBeLessThanOrEqual(1);
  await expect.poll(()=>table.evaluate(node=>node.scrollWidth-node.clientWidth)).toBeGreaterThan(10);
  await table.evaluate(node=>{node.scrollLeft=node.scrollWidth});
  await expect.poll(()=>table.evaluate(node=>node.scrollLeft)).toBeGreaterThan(10);
  await table.evaluate(node=>{node.scrollLeft=0});
  await table.scrollIntoViewIfNeeded();
  if(browserName==='chromium') {
    const box=await table.boundingBox();
    if(!box) throw new Error('table not visible');
    const x=Math.round(box.x+box.width*0.8),y=Math.round(box.y+box.height*0.5);
    const cdp=await context.newCDPSession(page);
    await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x,y,id:1}]});
    for(let i=1;i<=8;i++) await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:x-i*15,y,id:1}]});
    await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    await expect.poll(()=>table.evaluate(node=>node.scrollLeft)).toBeGreaterThan(10);
    await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  }
});

for (const viewport of [{width:1280,height:720},{width:390,height:844}]) {
test(`AI analysis displays a safe SVG diagram at ${viewport.width}px`,async({page})=>{
  await page.setViewportSize(viewport);
  const svg='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 240 90"><line x1="20" y1="45" x2="220" y2="45" stroke="#333333" stroke-width="2"/><circle cx="90" cy="45" r="4" fill="#ff0080"/><text x="90" y="70" text-anchor="middle">已知数量</text></svg>';
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,
      messages:[{role:'assistant',content:`先画线段图。\n\n\`\`\`svg\n${svg}\n\`\`\`\n\n再求结果。`,created_at:1}]}});
  });
  await openFixtureReview(page);
  const diagram=page.getByTestId('question-bank-ai-diagram');
  await expect(diagram).toBeVisible();
  await expect(diagram).toHaveAttribute('alt','AI 解析示意图');
  await expect.poll(()=>diagram.evaluate((img:HTMLImageElement)=>img.complete&&img.naturalWidth>0)).toBe(true);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('再求结果');
  await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth-window.innerWidth)).toBeLessThanOrEqual(0);
  await page.getByRole('button',{name:'下一题'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  await expect(diagram).toHaveCount(1);
  await page.getByRole('button',{name:'上一题'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect(diagram).toHaveCount(1);
});
}

test('AI SVG diagrams reject unsafe or oversized markup and preserve its source',async({page})=>{
  const open='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">';
  const unsafe=[
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100" onload="alert(1)"><circle cx="20" cy="20" r="5"/></svg>`,
    `${open}<script>alert(1)</script></svg>`,
    `${open}<foreignObject><div xmlns="http://www.w3.org/1999/xhtml">bad</div></foreignObject></svg>`,
    `${open}<image href="https://example.invalid/private"/></svg>`,
    `${open}<rect x="0" y="0" width="20" height="20" fill="url(https://example.invalid/a)"/></svg>`,
    `<!DOCTYPE svg [<!ENTITY x "bad">]>${open}<text x="1" y="10">&x;</text></svg>`,
    `${open}<rect constructor="bad"/></svg>`,
    `${open}${'<circle cx="1" cy="1" r="1"/>'.repeat(257)}</svg>`,
    `${open}<path d="${'M1 1 '.repeat(7000)}"/></svg>`,
  ];
  const code=(value:string)=>'```svg\n'+value+'\n```';
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[
      ...unsafe.map(content=>({role:'assistant',content:code(content),created_at:1})),
      {role:'user',content:code(`${open}<circle cx="10" cy="10" r="5"/></svg>`),created_at:1},
    ]}});
  });
  const external:string[]=[];
  page.on('request',request=>{if(request.url().startsWith('https://example.invalid/')) external.push(request.url())});
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-diagram-error')).toHaveCount(unsafe.length);
  await expect(page.getByTestId('question-bank-ai-diagram')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-ai-message').locator('pre code.language-svg')).toHaveCount(unsafe.length+1);
  await expect(page.getByTestId('question-bank-ai-message').locator('svg,script,foreignObject')).toHaveCount(0);
  expect(external).toEqual([]);
});

test('an existing AI review can draw a diagram in response to a follow-up',async({page})=>{
  await mockAiRequestStatus(page);
  const svg='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 240 90"><line x1="20" y1="45" x2="220" y2="45" stroke="#333"/><text x="40" y="70">线段图</text></svg>';
  const messages=[{role:'assistant',content:'已有文字解析',created_at:1}];
  let received='';
  const state=(question_id:string)=>({question_id,status:'complete',error:null,messages});
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:state(question_id)});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/messages',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-3)!;
    received=route.request().postDataJSON().message;
    messages.push({role:'user',content:received,created_at:2},
      {role:'assistant',content:'线段图如下：\n\n```svg\n'+svg+'\n```',created_at:3});
    await route.fulfill({status:202,json:{...state(question_id),request:aiRequestInfo(route.request().postDataJSON().request_id)}});
  });
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('已有文字解析');
  await page.getByTestId('question-bank-ai-input').fill('请画一张线段图');
  await page.getByTestId('question-bank-ai-send').click();
  await expect(page.getByTestId('question-bank-ai-diagram')).toBeVisible();
  await expect(page.getByTestId('question-bank-ai-message').filter({hasText:'请画一张线段图'})).toBeVisible();
  expect(received).toBe('请画一张线段图');
});

test('keeps question drafts and shows pending follow-up without blocking the editor',async({page})=>{
  let running=false;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*',async route=>{
    const id=new URL(route.request().url()).pathname.split('/').at(-1)!;
    await route.fulfill({json:aiRequestInfo(id,'follow_up',running?'running':'complete')});
  });
  const messages=[{role:'assistant',content:'已有文字解析',created_at:1}];
  let release!:()=>void;
  const gate=new Promise<void>(resolve=>{release=resolve});
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:running?'running':'complete',error:null,messages}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/messages',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-3)!;
    const message=route.request().postDataJSON().message;
    await gate;
    running=true;
    messages.push({role:'user',content:message,created_at:2});
    await route.fulfill({json:{question_id,status:'running',error:null,messages,request:aiRequestInfo(route.request().postDataJSON().request_id,'follow_up','running')}});
  });
  await openFixtureReview(page);
  const input=page.getByTestId('question-bank-ai-input');
  await input.fill('第一题的草稿');
  await page.getByTestId('question-bank-original-tab').click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(input).toHaveValue('第一题的草稿');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(input).toHaveValue('');
  await input.fill('第二题的草稿');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect(input).toHaveValue('第一题的草稿');
  await page.getByTestId('question-bank-ai-send').click();
  await expect(page.getByTestId('question-bank-ai-pending')).toContainText('第一题的草稿');
  await expect(page.getByTestId('question-bank-ai-send')).toBeDisabled();
  await input.fill('下一条草稿');
  release();
  await expect(page.getByTestId('question-bank-ai-pending')).toHaveCount(0);
  await expect(input).toHaveValue('下一条草稿');
  await expect(page.getByTestId('question-bank-ai-send')).toBeDisabled();
});

test('sends an AI follow-up with Ctrl+Enter but not plain Enter',async({page})=>{
  await mockAiRequestStatus(page);
  const messages=[{role:'assistant',content:'已有文字解析',created_at:1}];
  const received:string[]=[];
  const state=(question_id:string)=>({question_id,status:'complete',error:null,messages});
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:state(question_id)});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/messages',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-3)!;
    const message=route.request().postDataJSON().message;
    received.push(message);
    messages.push({role:'user',content:message,created_at:2});
    await route.fulfill({status:202,json:{...state(question_id),request:aiRequestInfo(route.request().postDataJSON().request_id)}});
  });
  await openFixtureReview(page);
  const input=page.getByTestId('question-bank-ai-input');
  await input.fill('第一行');
  await input.press('Enter');
  await expect.poll(()=>received.length).toBe(0);
  await input.press('Control+Enter');
  await expect.poll(()=>received).toEqual(['第一行']);
  await expect(input).toHaveValue('');
  await input.fill('第二行');
  await input.press('Meta+Enter');
  await expect.poll(()=>received).toEqual(['第一行','第二行']);
});

for (const viewport of [{width:1280,height:720},{width:390,height:844}]) {
test(`starts an AI review with the optional requirements text at ${viewport.width}px`,async({page})=>{
  await mockAiRequestStatus(page);
  await page.setViewportSize(viewport);
  let startBody:unknown=null;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    if(route.request().method()==='POST') {
      startBody=route.request().postDataJSON();
      await route.fulfill({status:202,json:{question_id,status:'running',error:null,messages:[],request:aiRequestInfo(route.request().postDataJSON().request_id,'initial','running')}});
    } else {
      await route.fulfill({json:{question_id,status:'idle',error:null,messages:[]}});
    }
  });
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-tab').click();
  const box=page.getByTestId('question-bank-ai-instruction');
  await expect(box).toBeVisible();
  await box.fill('用线段图讲解，只讲速算');
  await page.getByTestId('question-bank-ai-start').click();
  await expect.poll(()=>startBody).toEqual(expect.objectContaining({instruction:'用线段图讲解，只讲速算',request_id:expect.stringMatching(/^[0-9a-f-]{36}$/)}));
  await expect(page.getByTestId('question-bank-ai-status')).toContainText('正在生成');
  await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth-window.innerWidth)).toBeLessThanOrEqual(0);
});
}

test('starts an AI review with Ctrl+Enter but not plain Enter',async({page})=>{
  await mockAiRequestStatus(page);
  const starts:unknown[]=[];
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    if(route.request().method()==='POST') {
      starts.push(route.request().postDataJSON());
      await route.fulfill({status:202,json:{question_id,status:'running',error:null,messages:[],request:aiRequestInfo(route.request().postDataJSON().request_id,'initial','running')}});
    } else {
      await route.fulfill({json:{question_id,status:'idle',error:null,messages:[]}});
    }
  });
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-tab').click();
  const box=page.getByTestId('question-bank-ai-instruction');
  await box.fill('用线段图讲解');
  await box.press('Enter');
  await expect.poll(()=>starts.length).toBe(0);
  await box.press('Control+Enter');
  await expect.poll(()=>starts).toEqual([expect.objectContaining({instruction:'用线段图讲解',request_id:expect.stringMatching(/^[0-9a-f-]{36}$/)})]);
  await expect(page.getByTestId('question-bank-ai-status')).toContainText('正在生成');
});

test('AI SSE replaces snapshots, ignores duplicates, reconnects on gaps and refreshes terminal history',async({page})=>{
  await page.addInitScript(()=>{
    const streams: any[]=[];
    (window as any).__aiStreams=streams;
    class FakeEventSource extends EventTarget {
      onmessage: ((event: MessageEvent)=>void) | null=null;
      onerror: ((event: Event)=>void) | null=null;
      closed=false;
      constructor(public url:string) { super(); streams.push(this); }
      close() { this.closed=true; }
      emit(type:string,data:unknown) {
        if (this.closed) return;
        const event=new MessageEvent(type,{data:JSON.stringify(data)});
        if (type==='message') this.onmessage?.(event);
        else this.dispatchEvent(event);
      }
    }
    (window as any).EventSource=FakeEventSource;
  });
  let final=false;
  let reads=0;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    reads++;
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:final?'complete':'running',error:null,messages:final?
      [{id:1,role:'assistant',content:'持久化最终答案',created_at:1}]:[]}});
  });
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-tab').click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__aiStreams.length)).toBeGreaterThan(0);
  const emit=async(type:string, data:Record<string,unknown>)=>page.evaluate(({type,data})=>{
    const streams=(window as any).__aiStreams;
    streams[streams.length-1].emit(type,data);
  },{type,data});
  const question_id=await page.evaluate(()=>{
    const streams=(window as any).__aiStreams;
    return new URL(streams[streams.length-1].url,location.origin).pathname.split('/').at(-3);
  });
  await emit('snapshot',{version:1,question_id,stem_fingerprint:'fingerprint',run_id:'run-1',seq:1,status:'running',text:'第一段',progress:[]});
  await expect(page.getByTestId('question-bank-ai-stream-text')).toContainText('第一段');
  const event=(seq:number,kind:string,payload:Record<string,unknown>)=>({version:1,run_id:'run-1',seq,kind,timestamp_ms:1,payload});
  await emit('message',event(2,'tool_started',{item_id:'tool-1',tool:'读取资料'}));
  await emit('message',event(3,'text_delta',{text:' **重点**'}));
  await emit('message',event(3,'text_delta',{text:'重复'}));
  await expect(page.getByTestId('question-bank-ai-stream-text')).toContainText('重点');
  await expect(page.getByTestId('question-bank-ai-stream-text')).not.toContainText('重复');
  await page.getByTestId('question-bank-ai-progress').locator(':scope > button').click();
  await expect(page.getByTestId('question-bank-ai-progress')).toContainText('读取资料');
  await emit('message',event(4,'tool_finished',{item_id:'tool-1',tool:'读取资料',status:'completed'}));
  await expect(page.getByTestId('question-bank-ai-progress')).toContainText('completed');
  await emit('message',event(6,'text_delta',{text:'缺口'}));
  await expect.poll(()=>page.evaluate(()=>(window as any).__aiStreams.length)).toBeGreaterThan(1);
  await emit('snapshot',{version:1,question_id,stem_fingerprint:'fingerprint',run_id:'run-1',seq:6,status:'running',text:'快照恢复',progress:[]});
  await expect(page.getByTestId('question-bank-ai-stream-text')).toContainText('快照恢复');
  const before=reads;
  final=true;
  await emit('message',event(7,'run_finished',{}));
  await expect.poll(()=>reads).toBeGreaterThan(before);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('持久化最终答案');
  await expect(page.getByTestId('question-bank-ai-stream-text')).toHaveCount(0);
});

test('AI follow-up accepts an image without text',async({page})=>{
  await mockAiRequestStatus(page);
  let sent:unknown=null;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'已有解析',created_at:1}]}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/attachments',route=>route.fulfill({json:{id:'image-1'}}));
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/messages',async route=>{
    sent=route.request().postDataJSON();
    const question_id=new URL(route.request().url()).pathname.split('/').at(-3)!;
    await route.fulfill({json:{question_id,status:'running',error:null,messages:[],request:aiRequestInfo(route.request().postDataJSON().request_id,'follow_up','running')}});
  });
  await openFixtureReview(page);
  const picker=page.getByTestId('question-bank-ai-follow-up-images');
  await picker.locator('input[type=file]').setInputFiles({name:'example.png',mimeType:'image/png',buffer:Buffer.from([137,80,78,71])});
  await expect(picker.getByAltText('example.png')).toBeVisible();
  await page.getByTestId('question-bank-ai-send').click();
  await expect.poll(()=>sent).toEqual(expect.objectContaining({message:'',attachments:['image-1'],request_id:expect.stringMatching(/^[0-9a-f-]{36}$/)}));
});

test('AI unknown send keeps the same request ID for status lookup across tab changes',async({page})=>{
  let sentId='';
  let allowLookup=false;
  const lookups:string[]=[];
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'已有解析',created_at:1}]}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/events',route=>route.fulfill({status:503}));
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/messages',async route=>{
    sentId=route.request().postDataJSON().request_id;
    await route.abort('connectionreset');
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*',async route=>{
    const id=new URL(route.request().url()).pathname.split('/').at(-1)!;
    lookups.push(id);
    if (!allowLookup) await route.fulfill({status:404,json:{error:{code:'question_bank_ai_request_not_found',message:'请求不存在'}}});
    else await route.fulfill({json:aiRequestInfo(id,'follow_up','running')});
  });
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-input').fill('保留这次追问');
  await page.getByTestId('question-bank-ai-send').click();
  await expect(page.getByTestId('question-bank-ai-request-unknown')).toContainText('尚未确认');
  expect(sentId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  await expect(page.getByTestId('question-bank-ai-send')).toBeDisabled();
  await page.getByTestId('question-bank-original-tab').click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-request-unknown')).toBeVisible();
  allowLookup=true;
  await page.getByTestId('question-bank-ai-request-check').click();
  await expect(page.getByTestId('question-bank-ai-request-unknown')).toHaveCount(0);
  expect(lookups.length).toBeGreaterThan(1);
  expect(lookups.every(id=>id===sentId)).toBe(true);
});

test('AI retry requires uncertainty and new-thread confirmation and replaces expired images',async({page})=>{
  let sourceId='';
  let failed=false;
  const retries:Array<{source:string; body:any}>=[];
  const info=(id:string)=>aiRequestInfo(id,'follow_up','failed','无法恢复原 Codex Thread，原有上下文不可用',true);
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:failed?'failed':'complete',error:null,messages:[{id:1,role:'assistant',content:'已有解析',created_at:1}]}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/events',route=>route.fulfill({status:503}));
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/messages',async route=>{
    sourceId=route.request().postDataJSON().request_id;
    failed=true;
    const question_id=new URL(route.request().url()).pathname.split('/').at(-3)!;
    await route.fulfill({json:{question_id,status:'failed',error:null,messages:[{id:1,role:'assistant',content:'已有解析',created_at:1}],request:info(sourceId)}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*',async route=>{
    const id=new URL(route.request().url()).pathname.split('/').at(-1)!;
    await route.fulfill({json:info(id)});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*/retry',async route=>{
    const parts=new URL(route.request().url()).pathname.split('/');
    const body=route.request().postDataJSON();
    retries.push({source:parts.at(-2)!,body});
    if (retries.length===1) {
      await route.fulfill({status:400,json:{error:{code:'question_bank_ai_attachment_expired',message:'临时图片已失效'}}});
    } else {
      const question_id=parts[parts.indexOf('questions')+1];
      await route.fulfill({json:{question_id,status:'running',error:null,messages:[],request:aiRequestInfo(body.request_id,'follow_up','running')}});
    }
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/attachments',route=>route.fulfill({json:{id:'replacement'}}));
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-input').fill('继续分析');
  await page.getByTestId('question-bank-ai-send').click();
  await page.getByTestId('question-bank-ai-restart').click();
  const confirm=page.getByTestId('question-bank-ai-retry-confirm');
  await expect(confirm).toBeDisabled();
  await page.getByTestId('question-bank-ai-confirm-uncertain').check();
  await expect(confirm).toBeDisabled();
  await page.getByTestId('question-bank-ai-confirm-new-thread').check();
  await confirm.click();
  await expect(page.getByTestId('question-bank-ai-retry-images')).toBeVisible();
  await page.getByTestId('question-bank-ai-retry-images').locator('input[type=file]').setInputFiles({name:'replacement.png',mimeType:'image/png',buffer:Buffer.from([137,80,78,71])});
  await expect(page.getByTestId('question-bank-ai-retry-images').getByAltText('replacement.png')).toBeVisible();
  await confirm.click();
  await expect.poll(()=>retries.length).toBe(2);
  expect(retries.map(value=>value.source)).toEqual([sourceId,sourceId]);
  expect(retries[0].body).toEqual(expect.objectContaining({confirm_uncertain:true,confirm_new_thread:true}));
  expect(retries[1].body.replacement_attachments).toEqual(['replacement']);
  expect(retries[1].body.request_id).not.toBe(sourceId);
  expect(retries[1].body.request_id).not.toBe(retries[0].body.request_id);
});

test('AI retries the failed initial intent through the request retry endpoint',async({page})=>{
  let sourceId='';
  let retryBody:any;
  let failed=false;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    if (route.request().method()==='POST') {
      sourceId=route.request().postDataJSON().request_id;
      failed=true;
      await route.fulfill({json:{question_id,status:'failed',error:'失败',messages:[],request:aiRequestInfo(sourceId,'initial','failed')}});
    } else await route.fulfill({json:{question_id,status:failed?'failed':'idle',error:failed?'失败':null,messages:[]}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/events',route=>route.fulfill({status:503}));
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*',async route=>{
    const id=new URL(route.request().url()).pathname.split('/').at(-1)!;
    await route.fulfill({json:aiRequestInfo(id,'initial','failed')});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*/retry',async route=>{
    const parts=new URL(route.request().url()).pathname.split('/');
    expect(parts.at(-2)).toBe(sourceId);
    retryBody=route.request().postDataJSON();
    const question_id=parts[parts.indexOf('questions')+1];
    await route.fulfill({json:{question_id,status:'running',error:null,messages:[],request:aiRequestInfo(retryBody.request_id,'initial','running')}});
  });
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-tab').click();
  await page.getByTestId('question-bank-ai-instruction').fill('原要求');
  await page.getByTestId('question-bank-ai-start').click();
  await expect(page.getByTestId('question-bank-ai-restart')).toHaveText('重试首次解析');
  await page.getByTestId('question-bank-ai-restart').click();
  await page.getByTestId('question-bank-ai-retry-confirm').click();
  await expect.poll(()=>retryBody).not.toBeUndefined();
  expect(retryBody.request_id).not.toBe(sourceId);
  expect(retryBody).toEqual(expect.objectContaining({confirm_uncertain:false,confirm_new_thread:false}));
});

for (const viewport of [{width:1280,height:800},{width:390,height:720}]) {
  test(`AI review keeps its composer at the panel bottom in both routes at ${viewport.width}px`,async({page})=>{
    await page.setViewportSize(viewport);
    await page.addInitScript(()=>{
      const streams:any[]=[];
      (window as any).__layoutAiStreams=streams;
      class FakeEventSource extends EventTarget {
        onmessage:((event:MessageEvent)=>void)|null=null;
        onerror:((event:Event)=>void)|null=null;
        closed=false;
        constructor(public url:string) { super(); streams.push(this); }
        close() { this.closed=true; }
        emit(type:string,data:unknown) {
          if (this.closed) return;
          const event=new MessageEvent(type,{data:JSON.stringify(data)});
          if (type==='message') this.onmessage?.(event); else this.dispatchEvent(event);
        }
      }
      (window as any).EventSource=FakeEventSource;
    });
    await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
      const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
      await route.fulfill({json:{question_id,status:'complete',error:null,messages:[
        {role:'assistant',content:Array.from({length:80},(_,i)=>`解析段落 ${i}：详细说明。`).join('\n\n'),created_at:1},
      ]}});
    });
    await openFixtureReview(page);
    const checkLayout=async(panelId:string)=>{
      const panel=page.getByTestId(panelId);
      const messages=page.getByTestId('question-bank-ai-messages');
      const composer=page.getByTestId('question-bank-ai-composer-area');
      await expect(page.getByTestId('question-bank-ai-input')).toBeVisible();
      const bounds=await panel.boundingBox();
      const inputBounds=await composer.boundingBox();
      expect(bounds && inputBounds && Math.abs(bounds.y+bounds.height-inputBounds.y-inputBounds.height)).toBeLessThan(3);
      await expect.poll(()=>messages.evaluate(node=>node.scrollHeight-node.clientHeight)).toBeGreaterThan(100);
      await expect(messages).toHaveCSS('max-height','none');
      await messages.evaluate(node=>{node.scrollTop=0; node.dispatchEvent(new Event('scroll'));});
      await expect.poll(()=>messages.evaluate(node=>node.scrollTop)).toBe(0);
      await expect(messages.getByTestId('question-bank-ai-question-content')).toContainText('正确答案');
      const choices=messages.locator('[data-testid$="-choice"]');
      await expect(choices.first()).toBeVisible();
      const stream=await page.evaluate(()=>{
        const streams=(window as any).__layoutAiStreams;
        return streams[streams.length-1].url;
      });
      const question_id=new URL(stream,webOrigin).pathname.split('/').at(-3);
      await page.evaluate(({question_id})=>{
        const streams=(window as any).__layoutAiStreams;
        streams[streams.length-1].emit('snapshot',{version:1,question_id,stem_fingerprint:'fingerprint',run_id:'layout',seq:1,status:'running',text:'新的回复',progress:[{version:1,run_id:'layout',seq:1,kind:'tool_started',payload:{tool:'读取资料'}}]});
      },{question_id});
      const arrow=page.getByTestId('question-bank-ai-scroll-bottom');
      await expect(arrow).toBeVisible();
      await expect.poll(()=>messages.evaluate(node=>node.scrollTop)).toBe(0);
      const arrowBounds=await arrow.boundingBox();
      expect(arrowBounds && inputBounds && arrowBounds.y+arrowBounds.height).toBeLessThanOrEqual(inputBounds!.y+2);
      await arrow.click();
      await expect(arrow).toHaveCount(0);
      await expect.poll(()=>messages.evaluate(node=>node.scrollHeight-node.clientHeight-node.scrollTop)).toBeLessThanOrEqual(2);
      const progress=page.getByTestId('question-bank-ai-progress');
      await expect(progress.locator(':scope > button')).toContainText('读取资料');
      await progress.locator(':scope > button').click();
      await expect(progress).toContainText('读取资料');
      await page.getByTestId('question-bank-ai-input').focus();
      await expect(page.getByTestId('question-bank-ai-input')).toBeFocused();
      if (viewport.width < 768) {
        await page.setViewportSize({width:viewport.width,height:480});
        const box=await page.getByTestId('question-bank-ai-input').boundingBox();
        expect(box && box.y+box.height).toBeLessThanOrEqual(480);
        await page.getByTestId('question-bank-ai-input').blur();
        await page.setViewportSize(viewport);
      }
    };
    await checkLayout('question-bank-practice-question-pane');
    const favorite=page.getByTestId('question-bank-favorite');
    if (await favorite.getAttribute('aria-label') === '收藏题目') {
      await favorite.click();
      await expect(favorite).toHaveAttribute('aria-label','取消收藏');
    }
    await page.getByRole('button',{name:/返回训练/}).click();
    await page.getByTestId('question-bank-collection-favorite').click();
    await page.getByTestId('question-bank-collection-item').first().click();
    await checkLayout('collection-review-question-pane');
  });
}

test('AI activity loads every history page and expands full command output',async({page})=>{
  const requests:string[]=[];
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/runs**',async route=>{
    const url=new URL(route.request().url());
    requests.push(url.pathname+url.search);
    const after=url.searchParams.get('after');
    if (url.pathname.endsWith('/runs')) {
      await route.fulfill({json:after==='0'
        ? {items:[{run_id:'run-a',status:'complete',assistant_message_id:1}],next_after:1,has_more:true}
        : {items:[{run_id:'run-b',status:'complete'}],next_after:2,has_more:false}});
    } else if (url.pathname.includes('/run-a/')) {
      const event=(seq:number,kind:string,payload:unknown)=>({version:1,run_id:'run-a',seq,kind,payload});
      await route.fulfill({json:after==='0'
        ? {items:[event(1,'tool_started',{item_id:'cmd',tool:'命令',details:{command:'printf full-output'}}),event(2,'command_output_delta',{item_id:'cmd',text:'第一部分'})],next_after:2,has_more:true}
        : {items:[event(3,'command_output_delta',{item_id:'cmd',text:'第二部分'}),event(4,'tool_finished',{item_id:'cmd',tool:'命令',status:'completed'})],next_after:4,has_more:false}});
    } else {
      await route.fulfill({json:{items:[],next_after:null,has_more:false}});
    }
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'已保存的回答',created_at:1}]}});
  });
  await openFixtureReview(page);
  const rounds=page.getByTestId('question-bank-ai-progress');
  await expect(rounds).toHaveCount(1);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('已保存的回答');
  expect(await rounds.first().evaluate(node => !!(node.compareDocumentPosition(document.querySelector('[data-testid=question-bank-ai-message]')!) & Node.DOCUMENT_POSITION_FOLLOWING))).toBe(true);
  expect(requests.some(url=>url.includes('/events?'))).toBe(false);
  await rounds.first().locator(':scope > button').click();
  const tool=rounds.first().getByTestId('question-bank-ai-tool');
  await expect(tool.getByTestId('question-bank-ai-command')).toHaveText('printf full-output');
  await rounds.first().getByRole('button',{name:'加载更多输出'}).click();
  await expect.poll(()=>requests.some(url=>url.includes('/run-a/events?after=2&limit=50'))).toBe(true);
  await expect(tool).toContainText('completed');
  await page.getByRole('button',{name:'加载更多执行记录'}).click();
  await expect(rounds).toHaveCount(2);
  await expect(tool.getByTestId('question-bank-ai-command')).toHaveText('printf full-output');
  await expect(tool.getByTestId('question-bank-ai-output')).toHaveText('第一部分第二部分');
});

for (const width of [1280, 390]) {
test(`compact review composer grows and header controls preserve scratchpad anchor at ${width}px`, async ({page}) => {
  await page.setViewportSize({width,height:844});
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'解析正文',created_at:1}]}});
  });
  await openFixtureReview(page);
  const pane=page.getByTestId('question-bank-practice-question-pane');
  const input=pane.getByTestId('question-bank-ai-input');
  await expect(input).toBeVisible();
  const initial=await input.boundingBox();
  if (!initial) throw new Error('input missing');
  expect(initial.height).toBeLessThan(45);
  await input.fill('第一行\n第二行\n第三行');
  await expect.poll(async()=>(await input.boundingBox())!.height).toBeGreaterThan(initial.height);
  await input.fill('');
  await expect.poll(async()=>(await input.boundingBox())!.height).toBeLessThanOrEqual(initial.height+1);
  const pen=pane.getByTestId('calculation-scratchpad-toggle');
  const before=await pen.boundingBox();
  await pen.click();
  await expect(pen).toHaveAttribute('aria-label','关闭草稿');
  await expect(pane.getByTestId('calculation-scratchpad-toolbar')).toBeVisible();
  expect(Math.abs((await pen.boundingBox())!.x-before!.x)).toBeLessThanOrEqual(1);
  await pen.click();
  await expect(pane.getByTestId('question-bank-ai-tab')).toBeVisible();
  await pane.getByRole('button',{name:'更多解析操作'}).click();
  await pane.getByTestId('question-bank-ai-delete').click();
  await expect(pane.getByTestId('question-bank-ai-delete-confirm')).toBeVisible();
  await pane.getByTestId('question-bank-ai-delete-cancel').click();
});
}
