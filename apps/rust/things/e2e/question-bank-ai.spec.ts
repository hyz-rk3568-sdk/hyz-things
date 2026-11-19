import {expect,test} from '@playwright/test';
import {resetHarness,webOrigin} from './fixtures';
import {aiRequestInfo,mockAiRequestStatus,openFixtureReview,openQuestionBankPaper,openSheet} from './helpers/question-bank';

test.beforeEach(async ({request})=>{await resetHarness(request)});

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
  await openFixtureReview(page);
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
  await openFixtureReview(page);
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
  await openFixtureReview(page);
  const message = page.getByTestId('question-bank-ai-message');
  await expect(message.locator('.katex').first()).toBeVisible();
  await expect(message.locator('.katex-display')).toHaveCount(1);
});

test('saved AI analysis opens by default and remains scoped to its question', async ({page}) => {
  let savedQuestion = '';
  const longAnalysis = `已经保存的 AI 解析\n\n${Array.from({length: 120}, (_, index) => `段落 ${index}：计算 $x+1=2$。`).join('\n\n')}\n\n解析末尾`;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    const parts = new URL(route.request().url()).pathname.split('/');
    const offset = parts.indexOf('questions');
    const question_id = parts[offset + 1];
    if (!savedQuestion) savedQuestion = question_id;
    const saved = question_id === savedQuestion;
    await route.fulfill({json: {
      question_id, status: saved ? 'complete' : 'idle', error: null,
      messages: saved ? [{role:'assistant', content:longAnalysis, created_at:1}] : [],
    }});
  });
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('已经保存的 AI 解析');
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('解析末尾');
  const more = page.getByRole('button',{name:'更多解析操作'});
  await more.click();
  await expect(page.getByTestId('question-bank-ai-delete')).toBeVisible();
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(more).toHaveAttribute('aria-expanded','false');
  await expect(page.getByTestId('question-bank-ai-delete')).toHaveCount(0);
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

test('opening saved AI analysis reuses the lookup response', async ({page}) => {
  let reads = 0;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/events', route => route.fulfill({status:503}));
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    reads++;
    const question_id = new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'已有解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('已有解析');
  await expect.poll(() => reads).toBe(1);
});

test('deleting saved AI analysis sends a same-origin JSON request', async ({page}) => {
  let deletedOnServer = false;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai', async route => {
    if (route.request().method() === 'DELETE') {
      // Preserve the real DELETE endpoint/CSRF checks, but make subsequent
      // mocked GETs reflect the server's committed state instead of reviving
      // an answer that was already deleted.
      const response = await route.fetch();
      if (response.ok()) deletedOnServer = true;
      await route.fulfill({response});
      return;
    }
    if (route.request().method() !== 'GET') {
      await route.continue();
      return;
    }
    const parts = new URL(route.request().url()).pathname.split('/');
    const question_id = parts[parts.indexOf('questions') + 1];
    await route.fulfill({json: {
      question_id, status: deletedOnServer ? 'idle' : 'complete', error: null,
      messages: deletedOnServer ? [] : [{role:'assistant', content:'待删除的 AI 解析', created_at:1}],
    }});
  });
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('待删除的 AI 解析');
  await page.getByRole('button',{name:'更多解析操作'}).click();
  await page.getByTestId('question-bank-ai-delete').click();
  const dialog = page.getByTestId('question-bank-ai-review').getByTestId('question-bank-ai-delete-dialog');
  await expect(dialog).toBeVisible();
  const panelBox = await page.getByTestId('question-bank-ai-review').boundingBox();
  const dialogBox = await dialog.boundingBox();
  expect(dialogBox!.x).toBeGreaterThanOrEqual(panelBox!.x);
  expect(dialogBox!.y).toBeGreaterThanOrEqual(panelBox!.y);
  expect(dialogBox!.x + dialogBox!.width).toBeLessThanOrEqual(panelBox!.x + panelBox!.width + 0.5);
  expect(dialogBox!.y + dialogBox!.height).toBeLessThanOrEqual(panelBox!.y + panelBox!.height + 0.5);
  const deleted = page.waitForResponse(response =>
    response.request().method() === 'DELETE' &&
    new URL(response.url()).pathname.endsWith('/ai'));
  await page.getByTestId('question-bank-ai-delete-confirm').click();
  expect((await deleted).status()).toBe(200);
  await expect(page.getByTestId('question-bank-ai-status')).toContainText('未生成');
  await expect(page.getByTestId('question-bank-ai-message')).toHaveCount(0);
});


test('AI delete 409 requires explicit uncertainty confirmation before clearing history',async({page})=>{
  const deletes:Array<{confirm_uncertain:boolean}>=[];
  let deleted=false;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    if(route.request().method()==='DELETE'){
      const body=route.request().postDataJSON() as {confirm_uncertain:boolean};
      deletes.push(body);
      if (!body.confirm_uncertain) {
        await route.fulfill({status:409,json:{error:{code:'question_bank_ai_confirmation_required',message:'远端运行状态不确定，请确认后重试'}}});
        return;
      }
      deleted=true;
    }
    await route.fulfill({json:{question_id,status:deleted?'idle':'stopped',error:null,messages:deleted?[]:[{id:1,role:'assistant',content:'保留的旧解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('保留的旧解析');
  await page.getByRole('button',{name:'更多解析操作'}).click();
  await page.getByTestId('question-bank-ai-delete').click();
  const submit=page.getByTestId('question-bank-ai-delete-confirm');
  await submit.click();
  await expect(page.getByTestId('question-bank-ai-delete-confirm-uncertain')).toBeVisible();
  await expect(submit).toBeDisabled();
  expect(deletes).toEqual([{confirm_uncertain:false}]);
  await page.getByTestId('question-bank-ai-delete-confirm-uncertain').check();
  await submit.click();
  await expect(page.getByTestId('question-bank-ai-message')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-ai-status')).toContainText('未生成');
  expect(deletes).toEqual([{confirm_uncertain:false},{confirm_uncertain:true}]);
});

test('AI delete busy conflict remains visible and cannot bypass uncertainty confirmation',async({page})=>{
  let count=0;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    if(route.request().method()==='DELETE'){
      count++;
      await route.fulfill({status:409,json:{error:{code:'question_bank_ai_busy',message:'当前题目正在生成或无法追问'}}});
      return;
    }
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'不可删除的解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  // AI history loads asynchronously and switches to its tab only once the
  // saved response arrives. Opening the action menu before that transition
  // can display the original-analysis menu, which has no delete action.
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('不可删除的解析');
  await expect(page.getByTestId('question-bank-ai-tab')).toHaveAttribute('aria-pressed','true');
  await page.getByRole('button',{name:'更多解析操作'}).click();
  await page.getByTestId('question-bank-ai-delete').click();
  await page.getByTestId('question-bank-ai-delete-confirm').click();
  await expect(page.getByTestId('question-bank-ai-delete-error')).toContainText('question_bank_ai_busy');
  await expect(page.getByTestId('question-bank-ai-delete-confirm-uncertain')).toHaveCount(0);
  expect(count).toBe(1);
  await page.getByTestId('question-bank-ai-delete-cancel').click();
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('不可删除的解析');
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
  await openQuestionBankPaper(page);
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
  const stop=page.getByTestId('question-bank-ai-compose').getByRole('button',{name:'停止生成'});
  await expect(stop).toBeVisible();
  await expect(stop.locator('svg rect')).toHaveCount(1);
  await expect(page.getByText('后台生成中，切换题目或退出复盘不会中断。')).toHaveCount(0);
  await stop.click();
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

test('AI SVG supports gradients, styles and local references',async({page})=>{
  const svg='<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><linearGradient id="g"><stop offset="0%" stop-color="red"/><stop offset="100%" stop-color="blue"/></linearGradient></defs><style>.shape { fill: url(#g); }</style><rect class="shape" width="100" height="80" transform="translate(10 5)"/></svg>';
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{role:'assistant',content:'```svg\n'+svg+'\n```',created_at:1}]}});
  });
  await openFixtureReview(page);
  const diagram=page.getByTestId('question-bank-ai-diagram');
  await expect(diagram).toBeVisible();
  await expect.poll(()=>diagram.evaluate((img:HTMLImageElement)=>img.complete&&img.naturalWidth>0)).toBe(true);
});

test('AI SVG diagrams reject unsafe or oversized markup and preserve its source',async({page})=>{
  const open='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">';
  const unsafe=[
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100" onload="alert(1)"><circle cx="20" cy="20" r="5"/></svg>`,
    `${open}<script>alert(1)</script></svg>`,
    `${open}<foreignObject><div xmlns="http://www.w3.org/1999/xhtml">bad</div></foreignObject></svg>`,
    `${open}<image href="https://example.invalid/private"/></svg>`,
    `${open}<style>@import "https://example.invalid/a";</style></svg>`,
    `${open}<style>rect { fill: url(https://example.invalid/a) }</style></svg>`,
    `${open}<style>rect { fill: u<![CDATA[rl(https://example.invalid/a)]]> }</style></svg>`,
    `${open}<g xml:base="https://example.invalid/"><use href="#x"/></g></svg>`,
    `${open}<rect style="fill: image-set('https://example.invalid/a' 1x)"/></svg>`,
    `${open}<rect style="fill: image('private.png')"/></svg>`,
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
  await expect(page.getByTestId('question-bank-ai-stop')).toBeVisible();
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
      await route.fulfill({json:{question_id,status:startBody===null?'idle':'running',error:null,messages:[]}});
    }
  });
  await openFixtureReview(page);
  const pane=page.getByTestId('question-bank-practice-question-pane');
  const menu=pane.getByRole('button',{name:'更多解析操作'});
  await expect(menu).toBeVisible();
  await menu.click();
  await expect(pane.getByText('暂无可用操作')).toBeVisible();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(menu).toBeVisible();
  await menu.click();
  await expect(pane.getByText('暂无可用操作')).toBeVisible();
  const box=page.getByTestId('question-bank-ai-instruction');
  await expect(box).toBeVisible();
  await expect(pane.getByText('这些要求会随本次解析一起发给 Codex。')).toHaveCount(0);
  const paneBox=await pane.boundingBox();
  const composerBox=await pane.getByTestId('question-bank-ai-requirements-compose').boundingBox();
  expect(paneBox!.width-composerBox!.width).toBeLessThanOrEqual(20);
  await box.fill('用线段图讲解，只讲速算');
  await page.getByTestId('question-bank-ai-start').click();
  await expect.poll(()=>startBody).toEqual(expect.objectContaining({instruction:'用线段图讲解，只讲速算',request_id:expect.stringMatching(/^[0-9a-f-]{36}$/)}));
  await expect(page.getByTestId('question-bank-ai-status')).toContainText('正在生成');
  await expect(page.getByTestId('question-bank-ai-compose').getByRole('button',{name:'停止生成'})).toBeVisible();
  await expect(page.getByTestId('question-bank-ai-send')).toHaveCount(0);
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

test('streamed SVG at the start survives subsequent updates and completion',async({page})=>{
  const errors:string[]=[];
  page.on('pageerror',error=>errors.push(error.message));
  await page.addInitScript(()=>{
    const streams:any[]=[];
    (window as any).__svgStreams=streams;
    class FakeEventSource extends EventTarget {
      onmessage:((event:MessageEvent)=>void)|null=null;
      onerror:((event:Event)=>void)|null=null;
      closed=false;
      constructor(public url:string){super();streams.push(this)}
      close(){this.closed=true}
      emit(type:string,data:unknown){
        if(this.closed)return;
        const event=new MessageEvent(type,{data:JSON.stringify(data)});
        if(type==='message')this.onmessage?.(event);else this.dispatchEvent(event);
      }
    }
    (window as any).EventSource=FakeEventSource;
  });
  const svg='<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="100" height="100" fill="blue"/></svg>';
  const answer='```svg\n'+svg+'\n```\n\n第一段';
  let final=false;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/runs**',route=>route.fulfill({json:{items:[],next_after:null,has_more:false}}));
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    return route.fulfill({json:{question_id,status:final?'complete':'running',error:null,messages:final?[{id:1,role:'assistant',content:answer+'\n\n后续内容',created_at:1}]:[]}});
  });
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-tab').click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__svgStreams.length)).toBeGreaterThan(0);
  const emit=(type:string,data:Record<string,unknown>)=>page.evaluate(({type,data})=>(window as any).__svgStreams.at(-1).emit(type,data),{type,data});
  const question_id=await page.evaluate(()=>new URL((window as any).__svgStreams.at(-1).url,location.origin).pathname.split('/').at(-3));
  await emit('snapshot',{version:1,question_id,stem_fingerprint:'fingerprint',run_id:'run-1',seq:1,status:'running',text:answer,progress:[]});
  await expect(page.getByTestId('question-bank-ai-stream-text').getByTestId('question-bank-ai-diagram')).toBeVisible();
  await page.getByTestId('question-bank-ai-diagram').evaluate(node=>{(window as any).__originalDiagram=node});
  await emit('message',{version:1,run_id:'run-1',seq:2,kind:'text_delta',timestamp_ms:1,payload:{text:'\n\n后续内容\n\n```text\n尚未结束'}});
  await expect(page.getByTestId('question-bank-ai-stream-text')).toContainText('尚未结束');
  await expect(page.getByTestId('question-bank-ai-stream-text').getByTestId('question-bank-ai-diagram')).toBeVisible();
  expect(await page.getByTestId('question-bank-ai-diagram').evaluate(node=>node===(window as any).__originalDiagram)).toBe(true);
  await emit('message',{version:1,run_id:'run-1',seq:3,kind:'text_delta',timestamp_ms:1,payload:{text:'\n```'}});
  await expect(page.getByTestId('question-bank-ai-stream-text').locator('code.language-text')).toContainText('尚未结束');
  expect(await page.getByTestId('question-bank-ai-diagram').evaluate(node=>node===(window as any).__originalDiagram)).toBe(true);
  await emit('message',{version:1,run_id:'run-1',seq:4,kind:'text_delta',timestamp_ms:1,payload:{text:'\n\n公式 \\(x+'}});
  await expect(page.getByTestId('question-bank-ai-stream-text')).toContainText('x+');
  expect(await page.getByTestId('question-bank-ai-diagram').evaluate(node=>node===(window as any).__originalDiagram)).toBe(true);
  await emit('message',{version:1,run_id:'run-1',seq:5,kind:'text_delta',timestamp_ms:1,payload:{text:'1\\)'}});
  await expect(page.getByTestId('question-bank-ai-stream-text').locator('.katex')).toBeVisible();
  expect(await page.getByTestId('question-bank-ai-diagram').evaluate(node=>node===(window as any).__originalDiagram)).toBe(true);
  final=true;
  await emit('message',{version:1,run_id:'run-1',seq:6,kind:'run_finished',timestamp_ms:1,payload:{}});
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('后续内容');
  await expect(page.getByTestId('question-bank-ai-message').getByTestId('question-bank-ai-diagram')).toBeVisible();
  await page.getByRole('button',{name:'下一题'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  expect(errors).toEqual([]);
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
  let reads=0, historyReads=0;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/runs**',async route=>{
    historyReads++;
    const url=new URL(route.request().url());
    const items=url.pathname.endsWith('/runs') ? [{run_id:'run-1',status:final?'complete':'running',assistant_message_id:final?1:null}] : [];
    await route.fulfill({json:{items,next_after:null,has_more:false}});
  });
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
  for (let seq=7; seq<=26; seq++) await emit('message',event(seq,'text_delta',{text:` 段落${seq}`}));
  await expect(page.getByTestId('question-bank-ai-stream-text')).toContainText('段落26');
  const before=reads, historyBefore=historyReads;
  final=true;
  await emit('message',event(27,'run_finished',{}));
  await expect.poll(()=>reads).toBeGreaterThan(before);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('持久化最终答案');
  await expect(page.getByTestId('question-bank-ai-stream-text')).toHaveCount(0);
  await expect.poll(()=>historyReads).toBeGreaterThan(historyBefore);
  const message=page.getByTestId('question-bank-ai-message');
  await expect.poll(()=>message.evaluate(node=>node.previousElementSibling?.getAttribute('data-testid'))).toBe('question-bank-ai-progress');
  await expect(page.getByTestId('question-bank-ai-progress').locator(':scope > button')).toHaveAttribute('aria-expanded','true');
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
  await expect(page.getByTestId('question-bank-ai-request-unknown')).toContainText('服务端不存在');
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


test('missing AI request can be discarded only after reconciling the server state',async({page})=>{
  let sentId='', retriedId='', requestCount=0;
  let currentStatus='complete';
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:currentStatus,error:null,messages:currentStatus==='idle'?[]:[{id:1,role:'assistant',content:'已有解析',created_at:1}]}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/events',route=>route.fulfill({status:503}));
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/messages',async route=>{
    requestCount++;
    if(requestCount===1){
      sentId=route.request().postDataJSON().request_id;
      currentStatus='running';
      await route.abort('connectionreset');
    } else {
      retriedId=route.request().postDataJSON().request_id;
      const question_id=new URL(route.request().url()).pathname.split('/').at(-3)!;
      await route.fulfill({status:202,json:{question_id,status:'running',error:null,messages:[],request:aiRequestInfo(retriedId,'follow_up','running')}});
    }
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/requests/*',async route=>{
    const id=new URL(route.request().url()).pathname.split('/').at(-1)!;
    if(id===sentId){
      await route.fulfill({status:404,json:{error:{code:'question_bank_ai_request_not_found',message:'AI 请求不存在'}}});
    } else await route.fulfill({json:aiRequestInfo(id,'follow_up','running')});
  });
  await openFixtureReview(page);
  await page.getByTestId('question-bank-ai-input').fill('保留原有追问草稿');
  // The server reports a running task when reconciling the lost request.
  await page.getByTestId('question-bank-ai-send').click();
  await expect(page.getByTestId('question-bank-ai-request-unknown')).toContainText('服务端不存在');
  await expect(page.getByTestId('question-bank-ai-request-discard')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-ai-send')).toHaveCount(0);
  currentStatus='complete';
  await page.getByTestId('question-bank-ai-request-check').click();
  await expect(page.getByTestId('question-bank-ai-request-discard')).toBeVisible();
  await page.getByTestId('question-bank-ai-request-discard').click();
  await expect(page.getByTestId('question-bank-ai-request-unknown')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-ai-input')).toHaveValue('保留原有追问草稿');
  await page.getByTestId('question-bank-ai-send').click();
  await expect.poll(()=>requestCount).toBe(2);
  expect(retriedId).not.toBe(sentId);
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
    const initiallyFavorite=await favorite.getAttribute('aria-label') === '取消收藏';
    if (!initiallyFavorite) {
      await favorite.click();
      await expect(favorite).toHaveAttribute('aria-label','取消收藏');
    }
    await page.getByRole('button',{name:/返回训练/}).click();
    await page.getByTestId('question-bank-collection-favorite').click();
    await page.getByTestId('question-bank-collection-item').first().click();
    await checkLayout('collection-review-question-pane');
    // The harness reuses one question bank database across tests, so leave the
    // fixture collection exactly as this test found it.
    if (!initiallyFavorite) {
      const restore=page.getByTestId('collection-review-favorite');
      await restore.click();
      await expect(restore).toHaveAttribute('aria-label','收藏题目');
    }
  });
}

test('AI activity keeps the collapsed tool summary within two lines and wraps expanded output',async({page})=>{
  const requests:string[]=[];
  const longCommand=`printf ${'x'.repeat(600)}`;
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
        ? {items:[event(1,'tool_started',{item_id:'cmd',tool:'命令',details:{command:longCommand}}),event(2,'command_output_delta',{item_id:'cmd',text:'第一部分'})],next_after:2,has_more:true}
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
  await expect(rounds).toHaveCount(2);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('已保存的回答');
  expect(await rounds.first().evaluate(node => !!(node.compareDocumentPosition(document.querySelector('[data-testid=question-bank-ai-message]')!) & Node.DOCUMENT_POSITION_FOLLOWING))).toBe(true);
  expect(requests.some(url=>url.includes('/events?'))).toBe(false);
  const firstRound=rounds.first();
  const toggle=firstRound.locator(':scope > button');
  await toggle.click();
  const tool=firstRound.getByTestId('question-bank-ai-tool');
  const command=tool.getByTestId('question-bank-ai-command');
  await expect(command).toHaveText(longCommand);
  await expect.poll(()=>requests.some(url=>url.includes('/run-a/events?after=2&limit=50'))).toBe(true);
  await expect(tool).toContainText('completed');
  await expect(tool.getByTestId('question-bank-ai-output')).toHaveText('第一部分第二部分');
  await toggle.click();
  const summary=firstRound.getByTestId('question-bank-ai-progress-summary');
  await expect(summary).toBeVisible();
  const summaryMetrics=await summary.evaluate(element=>{
    const style=getComputedStyle(element);
    return {
      lineClamp:style.webkitLineClamp,
      width:element.clientWidth,
      scrollWidth:element.scrollWidth,
      height:element.getBoundingClientRect().height,
      lineHeight:Number.parseFloat(style.lineHeight),
    };
  });
  expect(summaryMetrics.lineClamp).toBe('2');
  expect(summaryMetrics.height).toBeLessThanOrEqual(summaryMetrics.lineHeight*2+1);
  expect(summaryMetrics.scrollWidth).toBeLessThanOrEqual(summaryMetrics.width);
  await toggle.click();
  const commandMetrics=await command.evaluate(element=>{
    const style=getComputedStyle(element);
    return {
      whiteSpace:style.whiteSpace,
      wordBreak:style.wordBreak,
      width:element.clientWidth,
      scrollWidth:element.scrollWidth,
      height:element.getBoundingClientRect().height,
      lineHeight:Number.parseFloat(style.lineHeight),
    };
  });
  expect(commandMetrics.whiteSpace).toBe('pre-wrap');
  expect(commandMetrics.wordBreak).toBe('break-all');
  expect(commandMetrics.height).toBeGreaterThan(commandMetrics.lineHeight*2);
  expect(commandMetrics.scrollWidth).toBeLessThanOrEqual(commandMetrics.width);
  const paneMetrics=await page.getByTestId('question-bank-practice-question-pane').evaluate(element=>({
    width:element.clientWidth, scrollWidth:element.scrollWidth,
  }));
  expect(paneMetrics.scrollWidth).toBeLessThanOrEqual(paneMetrics.width);
  await expect(rounds.first().getByRole('button',{name:'加载更多输出'})).toHaveCount(0);
  await expect.poll(()=>requests.some(url=>url.includes('/run-a/events?after=2&limit=50'))).toBe(true);
  await expect(tool).toContainText('completed');
  await expect(page.getByRole('button',{name:'加载更多执行记录'})).toHaveCount(0);
  await expect(tool.getByTestId('question-bank-ai-command')).toHaveText(longCommand);
  await expect(tool.getByTestId('question-bank-ai-output')).toHaveText('第一部分第二部分');
  await page.getByRole('button',{name:'下一题',exact:true}).click();
  await page.getByRole('button',{name:'上一题',exact:true}).click();
  const restored=page.getByTestId('question-bank-ai-progress').first();
  await restored.getByRole('button').first().click();
  await expect(restored.getByTestId('question-bank-ai-output')).toHaveText('第一部分第二部分');
  await expect(restored.getByRole('button',{name:'加载更多输出'})).toHaveCount(0);
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
  const paneBox=await pane.boundingBox();
  expect(before!.x+before!.width).toBeGreaterThanOrEqual(paneBox!.x+paneBox!.width-16);
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

test('AI tool history retries a failed page and keeps already loaded output', async ({page}) => {
  let secondPageRequests=0;
  let firstPageRequests=0;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/runs**',async route=>{
    const url=new URL(route.request().url());
    if (url.pathname.endsWith('/runs')) {
      await route.fulfill({json:{items:[{run_id:'retry-run',status:'complete',assistant_message_id:1}],next_after:1,has_more:false}});
      return;
    }
    const event=(seq:number,kind:string,payload:unknown)=>({version:1,run_id:'retry-run',seq,kind,payload});
    if (url.searchParams.get('after')==='0') {
      firstPageRequests++;
      await route.fulfill({json:{items:[event(1,'tool_started',{item_id:'cmd',tool:'命令',details:{command:'printf'}}),event(2,'command_output_delta',{item_id:'cmd',text:'第一部分'})],next_after:2,has_more:true}});
    } else if (++secondPageRequests===1) {
      await route.fulfill({status:500,json:{error:'暂时不可用'}});
    } else {
      await route.fulfill({json:{items:[event(3,'command_output_delta',{item_id:'cmd',text:'第二部分'})],next_after:3,has_more:false}});
    }
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  const round=page.getByTestId('question-bank-ai-progress').first();
  await round.getByRole('button').first().click();
  await expect(round.getByTestId('question-bank-ai-output')).toHaveText('第一部分');
  await expect(round.getByRole('alert')).toBeVisible();
  await round.getByRole('button',{name:'重试加载输出'}).click();
  await expect(round.getByTestId('question-bank-ai-output')).toHaveText('第一部分第二部分');
  await expect(round.getByRole('alert')).toHaveCount(0);
  expect(firstPageRequests).toBe(1);
  expect(secondPageRequests).toBe(2);
});

for (const viewport of [{width:1366,height:900},{width:1133,height:744},{width:1024,height:768},{width:390,height:844}]) {
  test(`AI composer and split pane headers align at ${viewport.width}px`,async({page})=>{
    await page.setViewportSize(viewport);
    await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
      const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
      await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:11,role:'assistant',content:'解析',created_at:1}]}});
    });
    await openFixtureReview(page);
    const pane=page.getByTestId('question-bank-practice-question-pane');
    const input=pane.getByTestId('question-bank-ai-input');
    await expect(input).toBeVisible();
    const composer=pane.getByTestId('question-bank-ai-compose');
    const send=pane.getByTestId('question-bank-ai-send');
    for (const text of ['', '第一行\n第二行\n第三行']) {
      await input.fill(text);
      const outer=await composer.boundingBox();
      const button=await send.boundingBox();
      expect(button!.x).toBeGreaterThanOrEqual(outer!.x);
      expect(button!.x+button!.width).toBeLessThanOrEqual(outer!.x+outer!.width);
      expect(button!.y).toBeGreaterThanOrEqual(outer!.y);
      expect(button!.y+button!.height).toBeLessThanOrEqual(outer!.y+outer!.height);
      expect(Math.abs((button!.y+button!.height/2)-(outer!.y+outer!.height/2))).toBeLessThanOrEqual(1);
    }
    const left=page.getByTestId('question-bank-practice-material-pane').locator('.question-bank-pane-header');
    const right=pane.locator('.question-bank-pane-header');
    const leftBox=await left.boundingBox(), rightBox=await right.boundingBox();
    expect(Math.abs(leftBox!.height-rightBox!.height)).toBeLessThanOrEqual(1);
    if (viewport.width>=768) expect(Math.abs((leftBox!.y+leftBox!.height)-(rightBox!.y+rightBox!.height))).toBeLessThanOrEqual(1);
    const controls=[pane.getByTestId('question-bank-favorite'),pane.getByTestId('question-bank-ai-tab'),pane.getByRole('button',{name:'更多解析操作'}),pane.getByTestId('calculation-scratchpad-toggle')];
    const flashcard=pane.locator('[data-testid="question-bank-practice-create-flashcard"], [data-testid="question-bank-practice-manage-flashcard"]');
    if (await flashcard.count()) controls.push(flashcard);
    const sizes=await Promise.all(controls.map(control=>control.boundingBox()));
    for (const size of sizes) {
      expect(Math.abs(size!.width-sizes[0]!.width)).toBeLessThanOrEqual(1);
      expect(Math.abs(size!.height-sizes[0]!.height)).toBeLessThanOrEqual(1);
    }
  });
}

test('deleting an AI review clears its tool history immediately and after revisiting',async({page})=>{
  let deleted=false, historyReads=0;
  await page.addInitScript(()=>{ (window as any).EventSource=class extends EventTarget { onmessage=null; onerror=null; close() {} }; });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai/runs**',async route=>{
    historyReads++;
    const url=new URL(route.request().url());
    const items=url.pathname.endsWith('/runs') && !deleted ? [{run_id:'old-run',status:'complete',assistant_message_id:11}] : [];
    await route.fulfill({json:{items,next_after:null,has_more:false}});
  });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    if (route.request().method()==='DELETE') deleted=true;
    await route.fulfill({json:{question_id,status:deleted?'idle':'complete',error:null,messages:deleted?[]:[{id:11,role:'assistant',content:'旧解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-ai-message')).toContainText('旧解析');
  await expect(page.getByTestId('question-bank-ai-progress')).toHaveCount(1);
  const before=historyReads;
  await page.getByRole('button',{name:'更多解析操作'}).click();
  await page.getByTestId('question-bank-ai-delete').click();
  await page.getByTestId('question-bank-ai-delete-confirm').click();
  await expect(page.getByTestId('question-bank-ai-message')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-ai-progress')).toHaveCount(0);
  await expect.poll(()=>historyReads).toBeGreaterThan(before);
  await page.getByTestId('question-bank-original-tab').click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(page.getByTestId('question-bank-ai-progress')).toHaveCount(0);
});
