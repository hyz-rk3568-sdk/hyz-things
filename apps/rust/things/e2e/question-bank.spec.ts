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
  await expect(page.getByTestId('question-bank-practice-score')).toContainText('1 / 3 正确');
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
  await expect(page.getByTestId('collection-review-answer')).toContainText('B');
  await expect(page.getByTestId('collection-review-create-flashcard')).toBeVisible();
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

test('AI review defaults to source, stays running across navigation and restores follow-ups', async ({page}) => {
  type Msg = {role: string; content: string; created_at: number};
  type State = {session_id: string; question_id: string; status: string; error: null; messages: Msg[]};
  const records = new Map<string,State>();
  const actions: string[] = [];
  await page.route('**/api/v1/question-bank/practice/sessions/*/questions/*/ai**',async route => {
    const path = new URL(route.request().url()).pathname;
    const parts = path.split('/');
    const offset = parts.indexOf('sessions');
    const session_id = parts[offset+1];
    const question_id = parts[offset+3];
    const suffix = path.endsWith('/messages') ? '/messages' : path.endsWith('/stop') ? '/stop' : '';
    const key = session_id + ':' + question_id;
    const record = records.get(key) ?? {session_id,question_id,status:'idle',error:null,messages:[]};
    records.set(key,record);
    if(route.request().method()==='POST'){
      actions.push(suffix || 'start');
      if(suffix==='/stop') record.status='stopped';
      else if(suffix==='/messages'){
        const body=route.request().postDataJSON() as {message:string};
        record.messages.push({role:'user',content:body.message,created_at:1});
        record.status='running';
      } else record.status='running';
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
  expect(records.size).toBe(0); // AI tab is lazy, original remains the default.

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
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  expect(actions).toEqual(['start']); // no implicit interrupt on question navigation

  await page.getByRole('button',{name:/返回训练/}).click();
  await page.getByTestId('question-bank-history-open').click();
  await page.getByTestId('question-bank-practice-history-item').first()
    .getByRole('button',{name:'查看原题与解析'}).click();
  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  expect(actions).toEqual(['start']); // exiting review does not cancel

  const persisted=Array.from(records.values()).find(v=>v.status==='running')!;
  persisted.status='complete';
  persisted.messages.push({role:'assistant',content:'我的独立解法：优先使用速算。',created_at:2});
  await page.getByTestId('question-bank-original-tab').click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('我的独立解法');
  await page.getByTestId('question-bank-ai-input').fill('为什么不选 A？');
  const compose = page.getByTestId('question-bank-ai-compose');
  const inputBox = await page.getByTestId('question-bank-ai-input').boundingBox();
  const sendBox = await page.getByTestId('question-bank-ai-send').boundingBox();
  const composeBox = await compose.boundingBox();
  expect(inputBox!.width).toBeGreaterThan(composeBox!.width * 0.9);
  expect(sendBox!.x).toBeGreaterThan(inputBox!.x + inputBox!.width / 2);
  expect(sendBox!.y).toBeGreaterThan(inputBox!.y + inputBox!.height / 2);
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
  await expect(page.getByTestId('question-bank-ai-status')).toHaveText('正在生成');
  expect(actions).toEqual(['start','/messages','/stop','start']);
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
  await create.click();
  const dialog=page.getByTestId('question-bank-practice-flashcard-dialog');
  await expect(dialog).toBeVisible();
  const card=dialog.getByTestId('favorite-flashcard-card');
  await expect(card.getByRole('button',{name:'创建并同步'})).toBeDisabled();
  await expect(card.getByTestId('favorite-flashcard-path')).toHaveText('将追加或创建：flashcards/真题练习/资料分析/比重/错题/cards.md');
  await card.getByLabel('确认写入所选目录').check();
  await card.getByRole('button',{name:'创建并同步'}).click();
  await expect(card.getByRole('status')).toContainText('已创建并同步');
  await dialog.getByRole('button',{name:'返回复盘'}).first().click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第');
});
