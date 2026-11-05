import {expect,test,type Page} from '@playwright/test';
import {expectNoHorizontalOverflow,harnessOrigin,loginAsAdmin,resetHarness} from './fixtures';

test.beforeEach(async({request})=>{await resetHarness(request)});

async function clockFixture(page:Page) {
  const events:string[]=[];
  let running=false,module='';
  const base=Date.now();
  const days=Array.from({length:35},(_,i)=>{
    const date=new Date(base-(34-i)*86400000).toISOString().slice(0,10);
    return {date,answered_count:i===34?12:i===33?3:0,answered_modules:i===34?{question_bank:10,logic:2}:i===33?{comprehensive:3}:{},
      question_bank:i===34?{modules:{'资料分析':{'比重':6,'增长':2}},papers:{'2024 国考行测':2}}:{},
      total_ms:i===34?3600000:i%7===0?1800000:0,
      modules:i===34?{calculation:3600000}:{},phases:i===34?{answer:3600000}:{}};
  });
  await page.route('**/api/v1/study/training-time/**', async route=>{
    const url=new URL(route.request().url());
    if(url.pathname.endsWith('/goals')) {await route.continue();return;}
    if(url.pathname.endsWith('/stats')) {await route.fulfill({json:{days}});return;}
    if(url.pathname.endsWith('/events')) {
      const body=route.request().postDataJSON();
      events.push(body.action);
      if(body.action==='enter'||body.action==='switch'){
        module=body.module;running=true;
      }else if(body.action==='leave'){running=false;}
    }
    await route.fulfill({json:{now_ms:Date.now(),today_ms:3600000,running,module}});
  });
  return events;
}

type QuestionBankBreakdown={modules:Record<string,Record<string,number>>,papers:Record<string,number>};
type PracticeDay={date:string,answered_count:number,answered_modules:Record<string,number>,question_bank:QuestionBankBreakdown};
async function readPracticeDays(page:Page,query=''):Promise<PracticeDay[]> {
  return page.evaluate(async query=>{
    const response=await fetch(`/api/v1/study/training-time/stats${query}`);
    if(!response.ok) throw new Error(`statistics failed: ${response.status}`);
    return (await response.json()).days;
  },query);
}

async function resetStudyPlan(page:Page) {
  await page.evaluate(async()=>{
    const panel=await (await fetch('/api/v1/panel')).json();
    const response=await fetch('/api/v1/study/plan',{method:'POST',
      headers:{'Content-Type':'application/json','X-HYZ-CSRF':panel.csrf_token},
      body:JSON.stringify({prediction_enabled:true,items:[]})});
    if(!response.ok) throw new Error(`reset plan failed: ${response.status}`);
  });
}

async function chooseFixturePaper(page:Page) {
  const region=page.getByTestId('question-bank-region').filter({hasText:'国考'});
  if (!(await region.evaluate(node=>(node.closest('details') as HTMLDetailsElement | null)?.open))) {
    await region.click();
  }
  const year=page.getByTestId('question-bank-year').filter({hasText:'2008'});
  if (!(await year.evaluate(node=>(node.closest('details') as HTMLDetailsElement).open))) await year.click();
  await page.getByTestId('question-bank-paper-option').filter({hasText:'行测真题训练端到端样卷'}).click();
}
async function openComprehensivePaper(page:Page) {
  const started=Date.now();
  const groups=Array.from({length:4},(_,group)=>({
    material:{topic:`材料 ${group+1}`,text:'材料正文'},
    questions:Array.from({length:5},(_,index)=>({
      question:`第${group*5+index+1}题`,options:['甲','乙','丙','丁'],skill:'基期量',
    })),
  }));
  const run={id:'time-run',status:'active',current_question:0,
    started_at_ms:started,deadline_at_ms:started+600000,paused_remaining_ms:null,
    duration_seconds:600,groups,answers:Array(20).fill(null)};
  await page.route('**/api/v1/study/data-analysis/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    let result:unknown={};
    if(path.endsWith('/status')) {
      result={configured:true,model:'mock-compatible-model',cached_sources:1,
        verified_groups:4,available_skills:[],preparing_job_id:null};
    } else if(path.endsWith('/unfinished')||path.endsWith('/history')) {
      result=[];
    } else if(path.endsWith('/prepare')) {
      result={job_id:'time-job'};
    } else if(path.endsWith('/prepare/time-job')) {
      result={state:'ready',verified:20,message:'完成',run_id:run.id};
    } else if(path.endsWith('/pause')) {
      result={...run,paused_remaining_ms:run.paused_remaining_ms===null?570000:null};
      Object.assign(run,result);
    } else if(path.endsWith('/claim')) {
      result={run,control_token:'time-token'};
    } else if(path.endsWith(`/runs/${run.id}`)) {
      result=run;
    }
    await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(result)});
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-comprehensive').click();
  await page.getByTestId('analysis-start').click();
  await page.getByTestId('analysis-enter').click();
}

test("sixth statistics text tab shows stored durations and heatmap",async({page})=>{
  await clockFixture(page);
  await page.goto('/#/study/training');
  const tab=page.getByTestId('training-mode-statistics');
  await expect(tab).toContainText('学习统计');
  await tab.click();
  await expect(page.getByTestId('training-statistics-content')).toBeVisible();
  await expect(page.getByTestId('study-heat-day')).toHaveCount(35);
  await page.getByTestId('study-heat-day').last().click();
  await expect(page.getByTestId('training-statistics-content')).toContainText('速算训练');
  await expect(page.getByTestId('training-header')).toBeVisible();
});

test("goal bars open embedded statistics without leaving the page",async({page})=>{
  const events=await clockFixture(page);
  await page.goto('/#/study/training');
  const bars=page.getByTestId('study-goal-bars');
  await expect(bars).toBeVisible();
  await expect(page.getByTestId('study-goal-questions')).toContainText('今日刷题');
  await expect(page.getByTestId('study-goal-time')).toContainText('今日时长');
  await bars.click();
  await expect(page.getByTestId('study-goal-sheet')).toBeVisible();
  await expect(page.getByTestId('study-goal-sheet').getByTestId('study-goals')).toBeVisible();
  await expect(page.getByTestId('study-goal-sheet').getByTestId('training-heatmap')).toBeVisible();
  await page.getByTestId('study-goal-sheet-close').click();
  await expect(page.getByTestId('study-goal-sheet')).toHaveCount(0);
  expect(events.filter(action=>action==='toggle')).toHaveLength(0);
  await expect(page).toHaveURL(/#\/study\/training/);
});

test("daily goals persist on the server and drive daily, weekly and 35-day bars",async({page})=>{
  await clockFixture(page);
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-goal-card')).toHaveCount(3);
  await expect(page.getByTestId('study-goals')).toContainText('/ 400 题');
  await page.getByTestId('study-goals-edit').click();
  await page.getByTestId('study-goal-questions-input').fill('500');
  await page.getByTestId('study-goal-hours-input').fill('10');
  await page.getByTestId('study-goals-save').click();
  await expect(page.getByTestId('study-goals-form')).toHaveCount(0);
  await expect(page.getByTestId('study-goals')).toContainText('/ 500 题');
  await expect(page.getByTestId('study-goals')).toContainText('/ 3500 题');
  await expect(page.getByTestId('study-goals')).toContainText('/ 10.0 小时');
  await expect(page.getByTestId('study-goal-questions-value')).toHaveText(/\d+\/500/);
  await page.reload();
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-goals')).toContainText('/ 500 题');
  await page.getByTestId('study-goals-edit').click();
  await page.getByTestId('study-goal-questions-input').fill('400');
  await page.getByTestId('study-goal-hours-input').fill('12');
  await page.getByTestId('study-goals-save').click();
  await expect(page.getByTestId('study-goals')).toContainText('/ 400 题');
  await expect(page.getByTestId('study-goal-questions-value')).toHaveText(/\d+\/400/);
});

test("study plan selects a parent module, toggles predictions and keeps the plan",async({page,request})=>{
  expect((await request.put(`${harnessOrigin}/reset-question-bank`)).ok()).toBeTruthy();
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await resetStudyPlan(page);
  await page.getByTestId('training-mode-statistics').click();
  const plan=page.getByTestId('study-plan');
  await expect(plan).toContainText('备考计划');
  await plan.getByTestId('study-plan-add').click();
  await expect(plan.getByTestId('study-plan-sheet')).toBeVisible();
  const module=plan.getByTestId('study-plan-module').filter({hasText:'资料分析'});
  await module.getByLabel('资料分析 全部').check();
  const sheetToggle=plan.getByTestId('study-plan-sheet-toggle');
  if(await sheetToggle.isChecked()) await sheetToggle.uncheck();
  await plan.getByTestId('study-plan-save').click();
  await expect(plan.getByTestId('study-plan-sheet')).toHaveCount(0);
  const item=plan.getByTestId('study-plan-item');
  await expect(item).toContainText('资料分析 · 全部');
  await expect(item).toContainText('还剩');
  await expect(plan.getByTestId('study-plan-summary')).toContainText('当前筛选');
  await expect(plan).toContainText('预测已关闭');
  await expect(item).not.toContainText('完整日');
  await plan.getByTestId('study-plan-toggle').check();
  await expect(item).toContainText('近 7 个完整日 · 样本不足');

  await plan.getByTestId('study-plan-manage').click();
  await plan.getByTestId('study-plan-module').filter({hasText:'资料分析'}).getByLabel('资料分析 全部').uncheck();
  await plan.getByTestId('study-plan-module').filter({hasText:'资料分析'}).getByLabel('资料分析 比重').check();
  await plan.getByTestId('study-plan-save').click();
  await expect(plan.getByTestId('study-plan-sheet')).toHaveCount(0);
  await expect(plan.getByTestId('study-plan-item')).toContainText('资料分析 · 比重');

  await page.setViewportSize({width:390,height:844});
  await expectNoHorizontalOverflow(page);
  await page.setViewportSize({width:1280,height:720});
  await resetStudyPlan(page);
  await page.reload();
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-plan-add')).toBeVisible();
});

test("refresh waits for the CSRF token before syncing study time",async({page})=>{
  const tokens:string[]=[];
  await page.route('**/api/v1/panel',async route=>{
    await new Promise(resolve=>setTimeout(resolve,800));
    await route.continue();
  });
  await page.route('**/api/v1/study/training-time/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname.endsWith('/events')){
      const csrf=route.request().headers()['x-hyz-csrf']??'';
      if(!csrf){
        await route.fulfill({status:403,contentType:'application/json',
          body:JSON.stringify({error:{code:'forbidden',message:'Control request rejected'}})});
        return;
      }
      tokens.push(csrf);
    }
    await route.fulfill({json:{now_ms:Date.now(),today_ms:0,running:false,module:''}});
  });
  await page.goto('/#/study/training');
  await expect(page.getByTestId('study-goal-bars')).toBeVisible();
  await expect.poll(()=>tokens.length,{timeout:5_000}).toBeGreaterThan(0);
  await expect(page.getByTestId('study-time-error')).toHaveCount(0);
});

test("returning after a long background soft-refreshes without reloading",async({page})=>{
  await page.addInitScript(()=>{
    const loads=Number(sessionStorage.getItem('training-page-loads')??'0');
    sessionStorage.setItem('training-page-loads',String(loads+1));
  });
  await page.clock.install();
  const events=await clockFixture(page);
  await page.goto('/#/study/training');
  await expect(page.getByTestId('study-goal-bars')).toBeVisible();
  await page.waitForTimeout(500);
  const leaves=()=>events.filter(action=>action==='leave').length;
  const leavesBefore=leaves();
  const setHidden=(hidden:boolean)=>page.evaluate(hidden=>{
    Object.defineProperty(document,'hidden',{configurable:true,get:()=>hidden});
    document.dispatchEvent(new Event('visibilitychange'));
  },hidden);
  await setHidden(true);
  await page.clock.fastForward(9_000);
  expect(leaves()).toBe(leavesBefore);
  await setHidden(false);
  await expect.poll(leaves).toBeGreaterThan(leavesBefore);
  const afterShortReturn=leaves();
  await setHidden(true);
  await page.clock.fastForward(11_000);
  expect(leaves()).toBe(afterShortReturn);
  await setHidden(false);
  await expect.poll(leaves).toBeGreaterThan(afterShortReturn);
  expect(await page.evaluate(()=>sessionStorage.getItem('training-page-loads'))).toBe('1');
});

test("foreground resume restores study time after the heartbeat grace expires",async({page})=>{
  await page.clock.install();
  const events=await clockFixture(page);
  await openComprehensivePaper(page);
  await expect(page.getByTestId('study-goal-bars')).toBeVisible();
  const clock=page.getByTestId('study-time-inline');
  await expect.poll(()=>events.filter(action=>action==='enter').length).toBeGreaterThan(0);
  const enters=()=>events.filter(action=>action==='enter').length;
  const initialEnters=enters();
  const setHidden=(hidden:boolean)=>page.evaluate(hidden=>{
    Object.defineProperty(document,'hidden',{configurable:true,get:()=>hidden});
    document.dispatchEvent(new Event('visibilitychange'));
  },hidden);
  await setHidden(true);
  await page.clock.fastForward(60_000);
  // The existing 45-second server heartbeat grace is preserved: no
  // background leave, extra heartbeat, or time-freezing request is sent.
  expect(enters()).toBe(initialEnters);
  await setHidden(false);
  await page.evaluate(()=>{
    window.dispatchEvent(new Event('pageshow'));
    window.dispatchEvent(new Event('focus'));
  });
  await expect.poll(enters).toBe(initialEnters+1);
  const restored=await clock.innerText();
  await page.clock.fastForward(3_000);
  await expect.poll(()=>clock.innerText()).not.toBe(restored);

  // The next app switch must also work: don't capture an obsolete Yew
  // state handle that only increments the wake generation once.
  const afterFirstReturn=enters();
  await setHidden(true);
  await page.clock.fastForward(2_000);
  // iPad occasionally sends pageshow/focus without a visible-state event.
  await page.evaluate(()=>{
    Object.defineProperty(document,'hidden',{configurable:true,get:()=>false});
    window.dispatchEvent(new Event('pageshow'));
    window.dispatchEvent(new Event('focus'));
  });
  await expect.poll(enters).toBe(afterFirstReturn+1);
});

test("question bank review keeps counting effective study time",async({page})=>{
  const events=await clockFixture(page);
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect.poll(()=>events.filter(action=>action==='enter').length).toBeGreaterThan(0);

  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByTestId('question-bank-submit-confirm').getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-practice-review-answer')).toBeVisible();
  await expect.poll(()=>events.at(-1)).toBe('enter');

  const clock=page.getByTestId('study-time-inline');
  const before=await clock.innerText();
  await page.waitForTimeout(1200);
  await expect.poll(()=>clock.innerText()).not.toBe(before);
});

test("paper pause stops the study clock and resuming restarts it",async({page})=>{
  const events=await clockFixture(page);
  await openComprehensivePaper(page);
  await expect.poll(()=>events.filter(action=>action==='enter').length).toBeGreaterThan(0);
  const leavesBefore=events.filter(action=>action==='leave').length;
  await page.getByTestId('analysis-clock').click();
  await expect(page.getByTestId('analysis-paused')).toBeVisible();
  await expect.poll(()=>events.filter(action=>action==='leave').length).toBeGreaterThan(leavesBefore);
  const entersBefore=events.filter(action=>action==='enter').length;
  await page.getByRole('button',{name:'继续计时'}).click();
  await expect(page.getByTestId('analysis-paused')).toHaveCount(0);
  await expect.poll(()=>events.filter(action=>action==='enter').length).toBeGreaterThan(entersBefore);
});


test("daily practice statistics show submitted counts and date-specific module totals",async({page})=>{
  await clockFixture(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-practice-today')).toHaveText('12 题');
  await expect(page.getByTestId('study-practice-week')).toHaveText('15 题');
  await expect(page.getByTestId('study-practice-total')).toHaveText('15 题');
  const detail=page.getByTestId('study-practice-detail');
  await expect(detail).toContainText('真题训练');
  await expect(detail).toContainText('10 题');
  const bankBreakdown=page.getByTestId('study-question-bank-breakdown');
  await expect(bankBreakdown).toContainText('模块刷题');
  await expect(page.getByTestId('study-question-bank-modules')).toContainText('资料分析');
  await expect(page.getByTestId('study-question-bank-modules')).toContainText('比重');
  await expect(page.getByTestId('study-question-bank-modules')).toContainText('6 题');
  await expect(page.getByTestId('study-question-bank-papers')).toContainText('2024 国考行测');
  await expect(page.getByTestId('study-question-bank-papers')).toContainText('2 题');
  await page.getByTestId('study-heat-day').nth(33).click();
  await expect(detail).toContainText('资料分析');
  await expect(detail).toContainText('3 题');
  await expect(detail).not.toContainText('真题训练');
  await page.getByTestId('study-heat-day').nth(32).click();
  await expect(detail).toContainText('当天暂无已交卷的作答记录');
  for(const viewport of [{width:1280,height:800},{width:390,height:844}]){
    await page.setViewportSize(viewport);
    await expectNoHorizontalOverflow(page);
    await page.getByTestId('study-heat-day').last().click();
    await expect(detail).toContainText('10 题');
  }
});

test("daily practice statistics count real submissions, exclude blanks and include repeats",async({page})=>{
  await loginAsAdmin(page);
  const readToday=async()=>(await readPracticeDays(page)).at(-1)!;
  const before=await readToday();
  expect(before.answered_count).toBeGreaterThanOrEqual(0);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  const choices=page.getByTestId('question-bank-practice-choice');
  await choices.first().click();
  await expect(choices.first()).toHaveAttribute('aria-pressed','true');
  expect((await readToday()).answered_count).toBe(before.answered_count);
  const submitted=page.waitForRequest(request=>request.method()==='POST' && /\/practice\/sessions\/[^/]+\/submit$/.test(request.url()));
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷',exact:true}).click();
  const submission=await submitted;
  await expect(page.getByTestId('question-bank-practice-score')).toBeVisible();
  expect((await readToday()).answered_count).toBe(before.answered_count+1);
  expect((await readToday()).answered_modules.question_bank).toBe((before.answered_modules.question_bank??0)+1);
  expect((await readToday()).question_bank.papers['行测真题训练端到端样卷']).toBeGreaterThanOrEqual(1);
  await expect(page.getByTestId('study-goal-questions-value')).toContainText(`${before.answered_count+1}/`);
  // Replaying the same authenticated submission must preserve the count.
  await page.evaluate(async({url,headers})=>{
    const response=await fetch(url,{method:'POST',headers,body:'{}'});
    if(!response.ok) throw new Error(`duplicate submission failed: ${response.status}`);
  },{url:submission.url(),headers:{'Content-Type':'application/json','X-HYZ-CSRF':submission.headers()['x-hyz-csrf']}});
  expect((await readToday()).answered_count).toBe(before.answered_count+1);
  await page.getByRole('button',{name:'← 返回训练',exact:true}).click();
  await page.getByTestId('question-bank-history-open').click();
  const id=submission.url().split('/').at(-2)!;
  // The completed session is the newest record; its repeat keeps the same paper.
  const repeatRequest=page.waitForRequest(request=>request.url().endsWith(`/sessions/${id}/repeat`));
  await page.getByTestId('question-bank-practice-history-item').first().getByRole('button',{name:'重做原题',exact:true}).click();
  await repeatRequest;
  await expect(choices.first()).toHaveAttribute('aria-pressed','false');
  await choices.nth(1).click();
  await expect(choices.nth(1)).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷',exact:true}).click();
  await expect(page.getByTestId('question-bank-practice-score')).toBeVisible();
  expect((await readToday()).answered_count).toBe(before.answered_count+2);
  await page.getByRole('button',{name:'← 返回训练',exact:true}).click();
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-practice-today')).toHaveText(`${before.answered_count+2} 题`);
  await page.reload();
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-practice-today')).toHaveText(`${before.answered_count+2} 题`);
});

test("daily practice statistics preserve historical retries and Beijing submission dates",async({page})=>{
  await loginAsAdmin(page);
  const days=await readPracticeDays(page,'?from=2026-01-01&to=2026-01-03');
  expect(days.map(({date,answered_count,answered_modules})=>({date,answered_count,answered_modules}))).toEqual([
    {date:'2026-01-01',answered_count:6,answered_modules:{comprehensive:2,logic:1,quantitative:3}},
    {date:'2026-01-02',answered_count:4,answered_modules:{comprehensive:1,logic:2,quantitative:1}},
    {date:'2026-01-03',answered_count:0,answered_modules:{}},
  ]);
  const next=await readPracticeDays(page,'?from=2026-01-02&to=2026-01-02');
  expect(next).toHaveLength(1);
  expect(next[0].answered_count).toBe(4);
});

test("daily practice statistics exclude calculation and flashcard practice completions",async({page})=>{
  await loginAsAdmin(page);
  const before=(await readPracticeDays(page)).at(-1)!.answered_count;
  await page.goto('/#/study/training');
  await page.getByTestId('calculation-start').click();
  await page.getByTestId('calculation-answer').first().click();
  await page.getByTestId('calculation-finish').click();
  await expect(page.getByTestId('calculation-summary')).toBeVisible();
  expect((await readPracticeDays(page)).at(-1)!.answered_count).toBe(before);
  await page.getByTestId('training-mode-flashcards').click();
  await page.getByTestId('practice-start').click();
  await page.getByTestId('practice-reveal').click();
  await page.getByTestId('practice-correct').click();
  await page.getByTestId('practice-finish').click();
  await expect(page.getByTestId('practice-summary')).toBeVisible();
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-practice-today')).toHaveText(`${before} 题`);
});

test("daily practice statistics show storage errors and recover on re-entry",async({page})=>{
  let failed=true;
  await page.route('**/api/v1/study/training-time/stats',route=>route.fulfill(failed?
    {status:503,json:{error:{code:'question_bank_unavailable',message:'题库共享或索引暂不可用'}}}:
    {json:{days:[]}}));
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByRole('alert')).toContainText('题库共享或索引暂不可用');
  await expect(page.getByTestId('study-practice-today')).toHaveCount(0);
  failed=false;
  await page.getByTestId('training-mode-calculation').click();
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('study-practice-today')).toHaveText('0 题');
  await expect(page.getByTestId('study-practice-detail')).toContainText('当天暂无已交卷的作答记录');
});

test("daily practice statistics require admin and validate the requested date range",async({page,request})=>{
  expect((await request.get('/api/v1/study/training-time/stats')).status()).toBe(401);
  await loginAsAdmin(page);
  const statuses=await page.evaluate(async()=>Promise.all([
    '?from=bad&to=2026-01-03','?from=2026-01-03&to=2026-01-01',
    '?from=2025-01-01&to=2026-01-03',
  ].map(async query=>(await fetch(`/api/v1/study/training-time/stats${query}`)).status)));
  expect(statuses).toEqual([400,400,400]);
});
