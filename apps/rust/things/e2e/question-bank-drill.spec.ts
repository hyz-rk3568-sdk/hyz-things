import {expect,test} from '@playwright/test';
import {loginAsAdmin,resetHarness} from './fixtures';

test.beforeEach(async ({request})=>{await resetHarness(request)});

test('drill collection practice keeps ungradable questions and reports separate progress',async({page})=>{
  await page.route('**/api/v1/question-bank/ai/rate-limits', route => route.fulfill({json: {
    five_hour: {remaining_percent: 72, resets_at: 1900000000},
    week: {remaining_percent: 38, resets_at: 1900400000},
  }}));
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-drill').click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await expect(page.getByTestId('question-bank-drill-setup')).toContainText('小P 2026 数量关系讲义');

  const collection=page.getByTestId('question-bank-drill-collection').filter({hasText:'小P 2026 数量关系讲义'});
  await expect(collection).toContainText('2 题');
  await collection.click();
  await expect(page.getByTestId('question-bank-drill-estimate')).toHaveCount(0);
  await page.getByTestId('question-bank-drill-paper').filter({hasText:'方程关系'}).click();
  await page.getByTestId('question-bank-practice-start').click();

  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('training-workspace-header')).toBeVisible();
  await expect(page.getByTestId('codex-limit-five-hour')).toContainText('72%');
  await expect(page.getByTestId('codex-limit-week')).toContainText('38%');
  await page.getByRole('button', {name: 'Codex 限额详情'}).click();
  await expect(page.getByTestId('codex-limit-details')).toContainText('重置');
  await page.getByRole('button', {name: 'Codex 限额详情'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/2 题');
  await page.getByTestId('training-answer-sheet-toggle').click();
  const group=page.getByTestId('question-bank-practice-group');
  await expect(group).toHaveCount(1);
  await expect(group).toHaveAttribute('data-group-label','方程关系');
  await expect(group.getByTestId('question-bank-practice-group-module')).toHaveText('数量关系');
  await page.getByTestId('training-answer-sheet-toggle').click();
  await expect(page.getByTestId('question-bank-practice-choice').nth(1)).toBeVisible();
  await page.getByTestId('question-bank-practice-choice').nth(1).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();

  await expect(page.getByTestId('question-bank-practice-choice').nth(1)).toHaveAttribute('data-state', 'correct');
  const score = page.getByTestId('question-bank-practice-score');
  await expect(score.getByLabel('正确 1')).toBeVisible();
  await expect(score.getByLabel('错误 0')).toBeVisible();
  await expect(score.getByLabel('不可判分 1')).toBeVisible();
  await expect(score.getByLabel(/未答/)).toHaveCount(0);
  await expect(score.getByLabel('正确 1')).toHaveCSS('flex-direction', 'column');
  await page.setViewportSize({width:390,height:844});
  await expect(page.getByTestId('codex-limit-five-hour')).toBeVisible();
  await expect(score.getByLabel('正确 1')).toBeVisible();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.setViewportSize({width:1280,height:720});

  if (!(await page.getByTestId('training-answer-sheet').isVisible())) {
    await page.getByTestId('training-answer-sheet-toggle').click();
  }
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('question-bank-practice-review-answer')).toContainText('暂无可靠答案');
  const create=page.getByTestId('question-bank-practice-create-flashcard');
  await expect(create).toBeVisible();
  await create.click();
  const card=page.getByTestId('favorite-flashcard-card');
  await expect(card).toContainText('原题暂无标准答案');
  await card.getByLabel('确认写入所选目录').check();
  await expect(card.getByRole('button',{name:'创建并同步'})).toBeDisabled();
  await card.getByPlaceholder('写下解题思路或复习提示').fill('自行整理的答案');
  await card.getByRole('button',{name:'创建并同步'}).click();
  await expect(page.getByTestId('question-bank-practice-flashcard-dialog')).toHaveCount(0);
  await expect(create).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-manage-flashcard')).toBeVisible();
  await page.getByTestId('question-bank-practice-manage-flashcard').click();
  const editor=page.getByTestId('favorite-flashcard-editor');
  await expect(editor).toBeVisible();
  await expect(editor.getByTestId('favorite-flashcard-editor-back')).toHaveValue('自行整理的答案');
  await page.getByRole('button',{name:'返回复盘'}).click();
  const image=page.getByTestId('question-bank-practice-stem').locator('img');
  await expect(image).toHaveAttribute('src','/api/v1/question-bank/assets/431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460.png');
  expect(await image.evaluate((node:HTMLImageElement)=>node.naturalWidth)).toBeGreaterThan(0);
  await page.getByTestId('training-answer-sheet-toggle').click();

  await page.getByTestId('question-bank-practice-records').click();
  await expect(page.getByTestId('question-bank-practice-history')).toContainText('专项练习');
  await expect(page.getByTestId('question-bank-history-filter-paper')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-history-item').first()).toContainText('小P 2026 数量关系讲义');

  await page.goto('/#/study/training');
  await page.reload();
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('drill-progress-card')).toHaveCount(0);
});

test('exam paper list does not include drill collections',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-setup')).not.toContainText('小P 2026 数量关系讲义');
  const response=await page.request.get('/api/v1/question-bank/papers');
  const papers=await response.json() as {title:string}[];
  expect(papers.some(paper=>paper.title==='小P 2026 数量关系讲义')).toBe(false);
});

for (const scenario of [
  {width:1280,height:900,cancel:false},
  {width:390,height:844,cancel:false},
  {width:1280,height:900,cancel:true},
]) {
test(`drill history restores scroll after saved AI analysis loads at ${scenario.width}px cancel=${scenario.cancel}`,async({page})=>{
  await page.setViewportSize({width:scenario.width,height:scenario.height});
  let firstQuestion='';
  let armed=false;
  let releaseReload:()=>void=()=>{};
  let reloadArrived:()=>void=()=>{};
  const reloadGate=new Promise<void>(resolve=>{releaseReload=resolve});
  const reloadStarted=new Promise<void>(resolve=>{reloadArrived=resolve});
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    if (!firstQuestion) firstQuestion=id;
    if (armed && id===firstQuestion) {reloadArrived();await reloadGate;}
    await route.fulfill({json:{question_id:id,status:'complete',error:null,
      messages:[{role:'assistant',content:Array.from({length:80},(_,i)=>`第 ${i+1} 段：专项训练 AI 解析。`).join('\n\n'),created_at:1}]}});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-drill').click();
  await page.getByTestId('question-bank-drill-collection').filter({hasText:'小P 2026 数量关系讲义'}).click();
  await page.getByTestId('question-bank-drill-paper').filter({hasText:'方程关系'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await page.getByTestId('question-bank-practice-records').click();
  await page.getByTestId('question-bank-practice-history-item').first().getByRole('button',{name:'查看原题与解析'}).click();
  await expect(page.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('第 80 段');
  armed=true;
  const question=page.getByTestId('question-bank-ai-messages');
  await question.evaluate(node=>{node.scrollTop=450});
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(450);
  const swipe=async(direction:'next'|'previous')=>{
    await page.getByTestId('training-swipe-surface').evaluate((node,direction)=>{
      const [start,end]=direction==='next'?[500,250]:[250,500];
      const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerType:'touch',pointerId:17,isPrimary:true,clientX:x,clientY:230}));
      emit('pointerdown',start);emit('pointermove',end);emit('pointerup',end);
    },direction);
  };
  await swipe('next');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 2 题');
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('第 80 段');
  await swipe('previous');
  await reloadStarted;
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 1 题');
  if (scenario.cancel) {
    await expect(question).toHaveAttribute('data-review-scroll-restoring','');
    await question.dispatchEvent('wheel',{deltaY:40});
  }
  armed=false;
  releaseReload();
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('第 80 段');
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(scenario.cancel?0:450);
  if (!scenario.cancel) {
    await question.dispatchEvent('wheel',{deltaY:30});
    await question.evaluate(node=>{node.scrollTop=220;node.append(document.createElement('span'))});
    await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(220);
  }
});
}

test('drill review displays a safe AI SVG diagram',async({page})=>{
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    const svg='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 80"><rect x="10" y="10" width="100" height="30" fill="#ddd"/><text x="15" y="30">专项示意图</text></svg>';
    await route.fulfill({json:{question_id,status:'complete',error:null,
      messages:[{role:'assistant',content:'```svg\n'+svg+'\n```',created_at:1}]}});
  });
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-drill').click();
  await page.getByTestId('question-bank-drill-collection').filter({hasText:'小P 2026 数量关系讲义'}).click();
  await page.getByTestId('question-bank-drill-paper').filter({hasText:'方程关系'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  const image=page.getByTestId('question-bank-ai-diagram');
  await expect(image).toBeVisible();
  await expect.poll(()=>image.evaluate((node:HTMLImageElement)=>node.complete&&node.naturalWidth>0)).toBe(true);
});


test('Codex limits show unavailable windows without blocking practice', async ({page}) => {
  await page.route('**/api/v1/question-bank/ai/rate-limits', route => route.fulfill({status: 503}));
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-drill').click();
  await page.getByTestId('question-bank-drill-collection').filter({hasText:'小P 2026 数量关系讲义'}).click();
  await page.getByTestId('question-bank-drill-paper').filter({hasText:'方程关系'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('codex-limit-five-hour')).toContainText('—');
  await expect(page.getByTestId('codex-limit-week')).toContainText('—');
  await expect(page.getByTestId('question-bank-practice-submit')).toBeEnabled();
});
