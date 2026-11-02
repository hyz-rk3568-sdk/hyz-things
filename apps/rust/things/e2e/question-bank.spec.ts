import {expect,test} from '@playwright/test';
import {loginAsAdmin,resetHarness,webOrigin} from './fixtures';

test.beforeEach(async ({request})=>{await resetHarness(request)});

async function chooseFixturePaper(page:import('@playwright/test').Page) {
  await page.getByTestId('question-bank-region').filter({hasText:'国考'}).click();
  await page.getByTestId('question-bank-year').filter({hasText:'2008'}).click();
  await page.getByTestId('question-bank-paper-option').filter({hasText:'行测真题训练端到端样卷'}).click();
}
async function openSheet(page:import('@playwright/test').Page) {
  if (!(await page.getByTestId('training-answer-sheet').isVisible())) {
    await page.getByTestId('training-answer-sheet-toggle').click();
  }
  await expect(page.getByTestId('training-answer-sheet')).toBeVisible();
}


test('question bank paper module practice keeps source numbering and hides answers until submission',async({page,request})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank').click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-mode').filter({hasText:'原卷模块练习'}).click();
  await expect(page.getByTestId('question-bank-select-module').filter({hasText:'资料分析'})).toBeVisible();
  await page.getByTestId('question-bank-select-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('材料中的正确选项');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(2);
  await expect(page.getByTestId('question-bank-practice-number').first()).toContainText('26');
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
  await page.getByTestId('training-mode-question-bank').click();
  await expect(page.getByTestId('question-bank-resume-card')).toContainText('继续练习');
  await page.getByTestId('question-bank-resume-card').getByRole('button',{name:'继续练习'}).click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.getByRole('button',{name:'继续练习'}).click();
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').last().click();
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('第二道题');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await page.getByTestId('question-bank-practice-choice').nth(2).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('第二题解析');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('B 是正确项');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('现期比重');
  await page.getByTestId('question-bank-favorite').click();
  await expect(page.getByTestId('question-bank-favorite')).toContainText('已收藏');
  // Restore the fixture state so the following test starts with an empty favorite set.
  await page.getByTestId('question-bank-favorite').click();
  await expect(page.getByTestId('question-bank-favorite')).toContainText('收藏');
  await expect(page.getByTestId('question-bank-favorite')).not.toContainText('已收藏');
});

test('question bank wrong and favorite mode reuses fixed source items',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-choice').first().click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-submit-confirm')).toBeVisible();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  await page.getByTestId('question-bank-favorite').click();
  await expect(page.getByTestId('question-bank-favorite')).toContainText('已收藏');
  await page.getByRole('button',{name:/暂停并退出/}).click();
  await page.getByTestId('question-bank-mode').filter({hasText:'错题训练'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(1);
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原卷第 26 题');
  await page.getByTestId('question-bank-practice-choice').nth(1).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('正确答案');
  await page.getByRole('button',{name:/暂停并退出/}).click();
  await page.getByTestId('question-bank-mode').filter({hasText:'收藏训练'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(1);
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原卷第 26 题');
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
});

test('question bank module drill filters real categories across original papers',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank').click();
  await page.getByTestId('question-bank-mode').filter({hasText:'模块刷题'}).click();
  await expect(page.getByTestId('question-bank-cross-module')).toBeVisible();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByText('筛选二级题型（可选）').click();
  await page.getByTestId('question-bank-cross-category').filter({hasText:'比重'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(2);
  await expect(page.getByTestId('question-bank-practice-workspace')).toContainText('第二份跨卷模块练习卷');
  await expect(page.getByTestId('question-bank-practice-number').first()).toContainText('12');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').last().click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原卷第 26 题');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('材料中的正确选项');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
});

test('module filter settings persist, apply server-side, and reflect answered/total progress',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank').click();
  await page.getByTestId('question-bank-mode').filter({hasText:'模块刷题'}).click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('0/3');
  await page.getByTestId('module-drill-settings-open').click();
  await expect(page.getByTestId('module-drill-settings')).toBeVisible();
  await page.getByTestId('module-filter-custom-year-toggle').click();
  await page.getByTestId('module-filter-year-min').fill('2018');
  await page.getByTestId('module-filter-year-max').fill('2018');
  await page.getByTestId('module-filter-region').filter({hasText:'广东'}).click();
  await page.getByTestId('module-filter-count').filter({hasText:'10题'}).click();
  await page.getByTestId('module-filter-save').click();
  await expect(page.getByTestId('module-drill-settings')).toBeHidden();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('0/1');
  await page.reload();
  await page.getByTestId('training-mode-question-bank').click();
  await page.getByTestId('question-bank-mode').filter({hasText:'模块刷题'}).click();
  await expect(page.getByTestId('module-drill-filter-summary')).toContainText('2018');
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('0/1');
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('第二份跨卷模块练习卷');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('1/1');
  await page.getByTestId('question-bank-practice-choice').first().click();
  await page.getByRole('button',{name:/暂停并退出/}).click();
  await page.getByTestId('question-bank-mode').filter({hasText:'模块刷题'}).click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('1/1');
});
test('swipe changes questions only for horizontal finger gesture and keeps document locked',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-mode').filter({hasText:'原卷模块练习'}).click();
  await page.getByTestId('question-bank-select-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  const surface=page.getByTestId('training-swipe-surface');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('1/2');
  await surface.evaluate((node)=>{
    const emit=(type:string,pointerType:string,x:number,y:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType,pointerId:17,isPrimary:true,clientX:x,clientY:y
    }));
    emit('pointerdown','touch',500,230);emit('pointerup','touch',250,230);
  });
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('2/2');
  await surface.evaluate((node)=>{
    const emit=(type:string,pointerType:string,x:number,y:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType,pointerId:18,isPrimary:true,clientX:x,clientY:y
    }));
    emit('pointerdown','pen',250,230);emit('pointerup','pen',500,230);
  });
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('2/2');
  const overflow=await page.evaluate(()=>document.documentElement.scrollHeight-document.documentElement.clientHeight);
  expect(overflow).toBeLessThanOrEqual(1);
});
