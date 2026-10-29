import { expect, test } from '@playwright/test';
import { resetHarness, expectNoHorizontalOverflow } from './fixtures';

const DEFAULT_LOGIC_PROMPT='默认出题要求';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

test("logic judgment persists answers, recovers deadline and reviews exact-paper retry", async ({ page }) => {
  const now = Date.now();
  let run = {id:'logic-paper',status:'prepared',started_at_ms:null as number|null,deadline_at_ms:null as number|null,server_now_ms:now,duration_seconds:600,questions:Array.from({length:10},(_,i)=>({question:`独立逻辑题 ${i+1}`,options:['甲','乙','丙','丁']})),answers:Array(10).fill(null)};
  let submitted = false;
  let prepared = 0;
  await page.route('**/api/v1/study/logic-judgment/**', async route => {
    const path = new URL(route.request().url()).pathname;
    const body = route.request().postDataJSON();
    let data: unknown = {};
    const result = {id:run.id,correct:1,total:10,answered:1,questions:run.questions.map((q,i)=>({...q,chosen:run.answers[i],correct_index:0,explanation:'仅交卷后可见的解析',card_id:`card-${i}`,card_path:'判断/形式推理/S1-直言命题/cards.md',front:'关联方法',back:'方法详解'}))};
    if(path.endsWith('/status')) data={configured:true,eligible_cards:24,preparing_job_id:null};
    else if(path.endsWith('/user-prompt')) data={user_prompt:'',customized:false};
    else if(path.endsWith('/card-performance')) data=[];
    else if(path.endsWith('/unfinished')) data=submitted?[]:[run];
    else if(path.endsWith('/history')) data=submitted?[{id:run.id,correct:1,total:10,answered:1,ended_at_ms:now}]:[];
    else if(path.endsWith('/prepare')) { prepared++; data={job_id:'job'}; }
    else if(path.endsWith('/prepare/job')) data={state:'ready',verified:10,message:'完成',run_id:run.id};
    else if(path.endsWith('/start')) {run={...run,status:'active',started_at_ms:now,deadline_at_ms:now+600000}; data=run;}
    else if(path.endsWith('/answers')) {run.answers[body.question]=body.choice; data={saved:true};}
    else if(path.endsWith('/submit')) {submitted=true; run.status='submitted'; data=result;}
    else if(path.endsWith('/result')) data=result;
    else if(path.endsWith('/retry')) {submitted=false; run={...run,id:'logic-retry',status:'prepared',answers:Array(10).fill(null),deadline_at_ms:null}; data={run_id:run.id};}
    else if(path.includes('/runs/')) data=run;
    await route.fulfill({json:data});
  });
  await page.setViewportSize({width:834,height:1112});
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-logic-judgment').click();
  await expect(page.getByTestId('training-mode-logic-judgment')).toHaveClass(/btn-primary/);
  await expect(page.getByTestId('logic-setup')).toContainText('24');
  await page.getByTestId('logic-resume').click();
  await expect(page.getByTestId('logic-clock')).toHaveText('10:00');
  await expect(page.getByText('仅交卷后可见的解析')).toHaveCount(0);
  await page.getByTestId('logic-choice').first().click();
  await expect(page.getByTestId('logic-choice').first()).toHaveClass(/btn-primary/);
  await page.getByRole('button',{name:'下一题',exact:true}).click();
  await expect(page.getByTestId('logic-question')).toHaveText('独立逻辑题 2');
  await expectNoHorizontalOverflow(page);
  await page.reload();
  await page.getByTestId('training-mode-logic-judgment').click();
  await page.getByTestId('logic-resume').click();
  await expect(page.getByTestId('logic-choice').first()).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect(page.getByTestId('calculation-scratchpad-canvas')).toBeVisible();
  await page.getByTestId('logic-submit').click();
  await expect(page.getByTestId('logic-result')).toContainText('1 / 10');
  await expect(page.getByTestId('logic-result')).toContainText('仅交卷后可见的解析');
  await page.getByRole('button',{name:'← 返回训练',exact:true}).click();
  await page.getByTestId('logic-history-toggle').click();
  await expect(page.getByTestId('logic-history')).toContainText('1/10题');
  await page.getByRole('button',{name:'查看原题与解析',exact:true}).click();
  await expect(page.getByTestId('logic-result')).toContainText('1 / 10');
  await page.getByTestId('logic-retry').click();
  await expect(page.getByTestId('logic-run')).toBeVisible();
  expect(prepared).toBe(0);
});

async function logicFixture(page: import('@playwright/test').Page) {
  const now = Date.now();
  const state = {
    run: {id:'logic-boundary',status:'active',deadline_at_ms:now+600000 as number|null,paused_remaining_ms:null as number|null,server_now_ms:now,duration_seconds:600,
      questions:Array.from({length:10},(_,i)=>({question:`单题 ${i+1}`,options:['甲','乙','丙','丁']})),answers:Array<number|null>(10).fill(null)},
    job: null as string|null, ready:false, polls:0, prepared:0, submitted:0,
    available:true, started:0, eligible:24, configured:true,
    failAnswer:false, saveGate:null as Promise<void>|null,
    prompt:'原提示', promptWrites:[] as unknown[],
  };
  await page.route('**/api/v1/study/logic-judgment/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    const body=route.request().postDataJSON();
    let data:unknown={};
    if(path.endsWith('/status')) data={configured:state.configured,eligible_cards:state.eligible,preparing_job_id:state.job};
    else if(path.endsWith('/user-prompt')) {
      if(route.request().method()==='POST') {
        state.promptWrites.push(body.user_prompt);
        if(body.user_prompt==='') { await route.fulfill({status:400,json:{error:{code:'invalid_prompt',message:'空白提示必须传 null'}}}); return; }
        state.prompt=body.user_prompt??DEFAULT_LOGIC_PROMPT;
      }
      data={user_prompt:state.prompt,customized:!!state.prompt};
    }
    else if(path.endsWith('/card-performance')) data=[{card_id:'method-card',category:'形式推理',card_path:'判断/形式推理/S1-直言命题/cards.md',front:'方法卡\n\n$$\nA\\rightarrow B\n$$',back:'方法解析',answered_count:3,correct_count:2}];
    else if(path.endsWith('/unfinished')) data=state.submitted || !state.available || (state.job && !state.ready)?[]:[state.run];
    else if(path.endsWith('/history')) data=[];
    else if(path.endsWith('/prepare')) { state.prepared++; state.job='recover-job'; data={job_id:state.job}; }
    else if(path.includes('/prepare/')) {
      state.polls++;
      data={state:state.ready?'ready':'running',verified:state.ready?10:3,message:state.ready?'完成':'正在验证',run_id:state.ready?state.run.id:null};
      if(state.ready) {state.job=null; state.available=true;}
    }
    else if(path.endsWith('/start')) {state.started++; state.run.status='active'; data=state.run;}
    else if(path.endsWith('/pause')) {
      if(state.run.paused_remaining_ms===null) {
        state.run.paused_remaining_ms=Math.max(0,(state.run.deadline_at_ms??state.run.server_now_ms)-state.run.server_now_ms);
        state.run.deadline_at_ms=null;
      } else {
        state.run.deadline_at_ms=state.run.server_now_ms+state.run.paused_remaining_ms;
        state.run.paused_remaining_ms=null;
      }
      data=state.run;
    }
    else if(path.endsWith('/answers')) {
      if(state.saveGate) await state.saveGate;
      if(state.failAnswer) { await route.fulfill({status:503,json:{error:{code:'save_failed',message:'保存失败，请重试'}}}); return; }
      state.run.answers[body.question]=body.choice; data={saved:true};
    }
    else if(path.endsWith('/submit') || path.endsWith('/result')) {
      if(path.endsWith('/submit')) state.submitted++;
      data={id:state.run.id,correct:0,total:10,answered:state.run.answers.filter(a=>a!==null).length,
        questions:state.run.questions.map((q,i)=>({...q,chosen:state.run.answers[i],correct_index:1,explanation:'交卷解析：$A \\rightarrow B$',card_id:`card-${i}`,card_path:'判断/形式推理/S1-直言命题/cards.md',front:'方法 $A \\rightarrow B$',back:'说明'}))};
    }
    else if(path.includes('/runs/')) data=state.run;
    await route.fulfill({json:data});
  });
  const open=async()=>{
    await page.goto('/#/study/training');
    await page.getByTestId('training-mode-logic-judgment').click();
    await expect(page.getByTestId('logic-setup')).toContainText('当前有效卡片');
  };
  return {state,open};
}

test("logic judgment typesets KaTeX in stems, choices, review and source cards", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  state.run.questions[0].question='已知 $A \\rightarrow B$，推断哪项成立？';
  state.run.questions[0].options[0]='选项 $\\neg B$';
  await open();
  await page.getByTestId('logic-statistics').locator('summary').click();
  await page.getByTestId('logic-card-item').first().click();
  await expect(page.getByTestId('logic-card-detail').locator('.katex')).toBeVisible();
  await page.getByTestId('logic-card-detail').getByRole('button',{name:'← 返回'}).click();
  await page.getByTestId('logic-resume').click();
  await expect(page.getByTestId('logic-question').locator('.katex')).toBeVisible();
  await expect(page.getByTestId('logic-choice').first().locator('.katex')).toBeVisible();
  await page.getByTestId('logic-submit').click();
  const review=page.getByTestId('logic-review-content');
  await expect(review.locator('.katex').first()).toBeVisible();
  await review.locator('summary').click();
  await expect(review.locator('details .katex').first()).toBeVisible();
});

test("deadline waits for the in-flight answer before automatically submitting once", async ({ page }) => {
  await page.clock.install();
  const {state,open}=await logicFixture(page);
  state.run.deadline_at_ms=state.run.server_now_ms+3000;
  let release!:()=>void;
  state.saveGate=new Promise<void>(resolve=>{release=resolve;});
  await open();
  await page.getByTestId('logic-resume').click();
  const saving=page.waitForRequest('**/answers');
  await page.getByTestId('logic-choice').first().click();
  await saving;
  await page.clock.fastForward(4000);
  await expect(page.getByTestId('logic-clock')).toHaveText('00:00');
  await expect(page.getByTestId('logic-choice').first()).toBeDisabled();
  release();
  await expect(page.getByTestId('logic-result')).toBeVisible();
  expect(state.submitted).toBe(1);
  expect(state.run.answers[0]).toBe(0);
});

test("user prompt clears through null instead of sending invalid blank text", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  await open();
  await page.getByTestId('logic-user-prompt').locator('summary').click();
  const prompt=page.getByLabel('补充偏好（保留题型范围与 Codex 自检要求）');
  await expect(prompt).toHaveValue('原提示');
  await prompt.fill('');
  await page.getByRole('button',{name:'保存用户提示',exact:true}).click();
  await expect.poll(()=>state.promptWrites).toEqual([null]);
  await expect(prompt).toHaveValue(DEFAULT_LOGIC_PROMPT);
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.reload();
  await page.getByTestId('training-mode-logic-judgment').click();
  await page.getByTestId('logic-user-prompt').locator('summary').click();
  await expect(prompt).toHaveValue(DEFAULT_LOGIC_PROMPT);
});

test("preparing task resumes polling after reload without creating another paper", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  state.job='recover-job';
  await open();
  await expect(page.getByTestId('logic-progress')).toContainText('3/10');
  await page.reload();
  await page.getByTestId('training-mode-logic-judgment').click();
  await expect(page.getByTestId('logic-progress')).toContainText('3/10');
  state.ready=true;
  await expect(page.getByTestId('logic-resume')).toBeVisible();
  expect(state.prepared).toBe(0);
  expect(state.polls).toBeGreaterThanOrEqual(3);
});

test("failed answer stays unselected and can be retried without losing the paper", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  state.failAnswer=true;
  await open();
  await page.getByTestId('logic-resume').click();
  const choice=page.getByTestId('logic-choice').first();
  await choice.click();
  await expect(page.getByRole('alert')).toContainText('保存失败');
  await expect(choice).toHaveAttribute('aria-pressed','false');
  expect(state.run.answers[0]).toBeNull();
  state.failAnswer=false;
  await choice.click();
  await expect(choice).toHaveAttribute('aria-pressed','true');
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test("server clock compensates a slow device and pause survives exit and resume", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  state.run.deadline_at_ms=state.run.server_now_ms+127000;
  await page.clock.setFixedTime(new Date(state.run.server_now_ms-240000));
  await open();
  await page.getByTestId('logic-resume').click();
  await expect(page.getByTestId('logic-clock')).toHaveText('02:07');
  await page.getByRole('button',{name:'← 暂停并退出',exact:true}).click();
  expect(state.run.paused_remaining_ms).toBe(127000);
  state.run.server_now_ms+=60000;
  await page.reload();
  await page.getByTestId('training-mode-logic-judgment').click();
  await page.getByTestId('logic-resume').click();
  await expect(page.getByTestId('logic-paused')).toBeVisible();
  await expect(page.getByTestId('logic-clock')).toHaveText('02:07');
  await expect(page.getByTestId('logic-choice')).toHaveCount(0);
  await page.getByTestId('logic-continue').click();
  await expect(page.getByTestId('logic-run')).toBeVisible();
  await expect(page.getByTestId('logic-clock')).toHaveText('02:07');
  expect(state.run.deadline_at_ms).toBe(state.run.server_now_ms+127000);
});

test("review question tabs retain correct and wrong colours even on the selected question", async ({ page }) => {
  const {open}=await logicFixture(page);
  await open();
  await page.getByTestId('logic-resume').click();
  await page.getByTestId('logic-choice').nth(1).click(); // Correct.
  await page.getByRole('button',{name:'下一题',exact:true}).click();
  await page.getByTestId('logic-choice').first().click(); // Wrong.
  await page.getByTestId('logic-submit').click();
  const tabs=page.getByTestId('logic-question-tab');
  await expect(tabs).toHaveCount(10);
  await expect(tabs.nth(0)).toHaveClass(/btn-success/);
  await expect(tabs.nth(1)).toHaveClass(/btn-error/);
  await tabs.nth(0).click();
  await expect(tabs.nth(0)).toHaveClass(/btn-success/);
  await expect(tabs.nth(0)).toHaveAttribute('aria-current','true');
  await tabs.nth(1).click();
  await expect(tabs.nth(1)).toHaveClass(/btn-error/);
  await tabs.nth(2).click();
  await expect(tabs.nth(2)).toHaveClass(/btn-primary/);
  await expect(tabs.nth(2)).toHaveAttribute('aria-label','第3题 未答');
});

test("mobile single-question ink is transparent and survives question navigation and reload", async ({ page }) => {
  await page.setViewportSize({width:390,height:844});
  const {open}=await logicFixture(page);
  await open();
  await page.getByTestId('logic-resume').click();
  await expect(page.getByRole('group',{name:'训练模式'})).toHaveCount(0);
  await expect(page.getByTestId('analysis-material')).toHaveCount(0);
  await expectNoHorizontalOverflow(page);
  const toggle=page.getByTestId('calculation-scratchpad-toggle');
  const canvas=page.getByTestId('calculation-scratchpad-canvas');
  const save=page.getByTestId('calculation-scratchpad-save-status');
  const snapshot=()=>canvas.evaluate(element=>(element as HTMLCanvasElement).toDataURL());
  const hasInk=()=>canvas.evaluate(element=>{
    const bitmap=element as HTMLCanvasElement;
    const data=bitmap.getContext('2d')!.getImageData(0,0,bitmap.width,bitmap.height).data;
    for(let i=3;i<data.length;i+=4) if(data[i]!==0) return true;
    return false;
  });
  await toggle.click();
  await expect(save).toHaveText('草稿已保存到本机');
  const blank=await snapshot();
  const box=await canvas.boundingBox();
  expect(box).not.toBeNull();
  for(const [type,x,y] of [['pointerdown',50,120],['pointermove',110,150],['pointerup',110,150]] as const) {
    await canvas.dispatchEvent(type,{pointerId:7,pointerType:'pen',isPrimary:true,button:0,buttons:type==='pointerup'?0:1,clientX:box!.x+x,clientY:box!.y+y});
  }
  await expect.poll(snapshot).not.toEqual(blank);
  const marked=await snapshot();
  await expect(save).toHaveText('草稿已保存到本机');
  await expect(page.getByTestId('logic-question')).toBeVisible();
  await page.getByRole('button',{name:'下一题',exact:true}).click();
  await toggle.click();
  await expect(save).toHaveText('草稿已保存到本机');
  expect(await snapshot()).toEqual(blank);
  await page.getByRole('button',{name:'上一题',exact:true}).click();
  await toggle.click();
  await expect.poll(snapshot).toEqual(marked);
  await page.reload();
  await page.getByTestId('training-mode-logic-judgment').click();
  await page.getByTestId('logic-resume').click();
  await toggle.click();
  await expect.poll(snapshot).toEqual(marked);
  await page.getByTestId('logic-submit').click();
  await expect(page.getByTestId('logic-result')).toBeVisible();
  await expect(save).toHaveText('草稿已保存到本机');
  await toggle.click();
  await expect.poll(hasInk).toBe(true);
  // The review and live-paper canvas can have different pixel heights.
  // Verify actual drawn alpha instead of comparing size-dependent PNG data.
  await page.getByRole('button',{name:'下一题',exact:true}).click();
  await toggle.click();
  await expect(save).toHaveText('草稿已保存到本机');
  await expect.poll(hasInk).toBe(false);
  await page.getByRole('button',{name:'上一题',exact:true}).click();
  await toggle.click();
  await expect.poll(hasInk).toBe(true);
});


test("opening an already expired server-submitted paper goes directly to its result", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  await open();
  state.run.status='submitted';
  state.run.deadline_at_ms=state.run.server_now_ms-1000;
  await page.getByTestId('logic-resume').click();
  await expect(page.getByTestId('logic-result')).toContainText('交卷解析');
  await expect(page.getByTestId('logic-run')).toHaveCount(0);
  expect(state.submitted).toBe(0);
});


test("newly generated paper stays untimed until explicit entry and statistics remain separate", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  state.available=false;
  state.run.status='prepared';
  await open();
  await page.getByTestId('logic-statistics').locator('summary').click();
  await expect(page.getByTestId('logic-statistics')).toContainText('方法卡');
  await page.getByTestId('logic-card-item').click();
  await expect(page.getByTestId('logic-card-detail')).toContainText('方法解析');
  await page.getByRole('button',{name:'← 返回',exact:true}).click();
  await page.getByTestId('logic-prepare').click();
  await expect(page.getByTestId('logic-progress')).toContainText('3/10');
  expect(state.prepared).toBe(1);
  state.ready=true;
  await expect(page.getByTestId('logic-resume')).toBeVisible();
  await expect(page.getByTestId('logic-clock')).toHaveCount(0);
  expect(state.started).toBe(0);
  await page.getByTestId('logic-resume').click();
  await expect(page.getByTestId('logic-run')).toBeVisible();
  expect(state.started).toBe(1);
});

test("insufficient card pool and unconfigured generation cannot create fake papers", async ({ page }) => {
  const {state,open}=await logicFixture(page);
  state.eligible=9;
  state.configured=false;
  await open();
  await expect(page.getByTestId('logic-setup')).toContainText('至少需要10张');
  await expect(page.getByTestId('logic-setup')).toContainText('请先配置');
  await expect(page.getByTestId('logic-prepare')).toBeDisabled();
  expect(state.prepared).toBe(0);
});

test("transparent ink controls do not cover the single question stem on mobile", async ({ page }) => {
  await page.setViewportSize({width:390,height:844});
  const {state,open}=await logicFixture(page);
  state.run.questions[0].question='如果甲参加，则乙不参加。已知乙参加，以下哪项必然成立？'.repeat(3);
  await open();
  await page.getByTestId('logic-resume').click();
  await page.getByTestId('calculation-scratchpad-toggle').click();
  const question=await page.getByTestId('logic-question').boundingBox();
  const controls=await page.getByTestId('calculation-scratchpad-controls').boundingBox();
  expect(question).not.toBeNull();
  expect(controls).not.toBeNull();
  expect(question!.y).toBeGreaterThanOrEqual(controls!.y+controls!.height+4);
});

test("logic judgment keeps controls inside PWA safe areas across viewports", async ({ page }) => {
  const {open}=await logicFixture(page);
  await open();
  await page.getByTestId('logic-resume').click();
  await page.evaluate(()=>{
    const style=document.documentElement.style;
    style.setProperty('--app-safe-top','32px');
    style.setProperty('--app-safe-bottom','24px');
    style.setProperty('--app-safe-left','12px');
    style.setProperty('--app-safe-right','12px');
  });
  const screen=page.getByTestId('logic-run');
  const next=page.getByRole('button',{name:'下一题',exact:true});
  for(const viewport of [{width:1180,height:820},{width:820,height:1180},{width:360,height:800}]) {
    await page.setViewportSize(viewport);
    expect((await screen.locator('header').boundingBox())!.y).toBeGreaterThanOrEqual(40);
    const box=(await next.boundingBox())!;
    expect(box.y+box.height).toBeLessThanOrEqual(viewport.height-24);
    await expectNoHorizontalOverflow(page);
    await next.click();
    await expect(page.getByTestId('logic-question')).toHaveText('单题 2');
    await page.getByRole('button',{name:'上一题',exact:true}).click();
  }
});
