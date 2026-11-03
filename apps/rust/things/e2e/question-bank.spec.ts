import {expect,test} from '@playwright/test';
import {loginAsAdmin,resetHarness,webOrigin} from './fixtures';

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
  await page.getByTestId('question-bank-practice-pause').click();
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
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('B 是正确项');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('现期比重');
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
  await expect(page.getByTestId('question-bank-practice-score')).toContainText('正确 1');
  await expect(page.getByTestId('question-bank-practice-score')).toContainText('错误 1');
  await expect(page.getByTestId('question-bank-practice-score')).toContainText('未答 1');
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
  await expect(page.getByTestId('question-bank-practice-workspace')).toContainText('行测真题训练端到端样卷');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('材料中的正确选项');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await openSheet(page);
  await numbers.filter({hasText:'27'}).click();
  await expect(page.getByTestId('question-bank-practice-scope')).toContainText('资料分析 · 增长');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
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
  await page.getByTestId('question-bank-practice-choice').first().click();
  await page.getByRole('button',{name:/暂停并退出/}).click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('1/1');
});
test('swipe changes questions only for horizontal finger gesture and keeps document locked',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  const surface=page.getByTestId('training-swipe-surface');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('1/3');
  await surface.evaluate((node)=>{
    const emit=(type:string,pointerType:string,x:number,y:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType,pointerId:17,isPrimary:true,clientX:x,clientY:y
    }));
    emit('pointerdown','touch',500,230);emit('pointerup','touch',250,230);
  });
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('2/3');
  await surface.evaluate((node)=>{
    const emit=(type:string,pointerType:string,x:number,y:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType,pointerId:18,isPrimary:true,clientX:x,clientY:y
    }));
    emit('pointerdown','pen',250,230);emit('pointerup','pen',500,230);
  });
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('2/3');
  const overflow=await page.evaluate(()=>document.documentElement.scrollHeight-document.documentElement.clientHeight);
  expect(overflow).toBeLessThanOrEqual(1);
});
