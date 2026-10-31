import {expect,test,type Page} from '@playwright/test';
import {resetHarness} from './fixtures';

test.beforeEach(async({request})=>{await resetHarness(request)});

async function clockFixture(page:Page) {
  const events:string[]=[];
  let running=false,module='';
  const base=Date.now();
  const days=Array.from({length:35},(_,i)=>{
    const date=new Date(base-(34-i)*86400000).toISOString().slice(0,10);
    return {date,total_ms:i===34?3600000:i%7===0?1800000:0,
      modules:i===34?{calculation:3600000}:{},phases:i===34?{answer:3600000}:{}};
  });
  await page.route('**/api/v1/study/training-time/**', async route=>{
    const url=new URL(route.request().url());
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

test("pink study clock is display only and keeps its compact layout",async({page})=>{
  const events=await clockFixture(page);
  await page.goto('/#/study/training');
  const clock=page.getByTestId('study-time-inline');
  await expect(clock).toHaveText('01:00:00');
  await expect(clock).toHaveClass(/text-primary/);
  await expect(clock).toHaveJSProperty('tagName','SPAN');
  await expect(clock).toHaveAttribute('aria-label','今日有效学习时长');
  await clock.click();
  await page.waitForTimeout(200);
  expect(events.filter(action=>action==='toggle')).toHaveLength(0);
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
  await expect(page.getByTestId('study-time-inline')).toBeVisible();
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
  await expect(page.getByTestId('study-time-inline')).toBeVisible();
  await page.waitForTimeout(500);
  const leaves=()=>events.filter(action=>action==='leave').length;
  const leavesBefore=leaves();
  const setHidden=(hidden:boolean)=>page.evaluate(hidden=>{
    Object.defineProperty(document,'hidden',{configurable:true,get:()=>hidden});
    document.dispatchEvent(new Event('visibilitychange'));
  },hidden);
  await setHidden(true);
  await page.clock.fastForward(9_000);
  await setHidden(false);
  await page.waitForTimeout(300);
  expect(leaves()).toBe(leavesBefore);
  await setHidden(true);
  await page.clock.fastForward(11_000);
  await setHidden(false);
  await expect.poll(leaves).toBeGreaterThan(leavesBefore);
  expect(await page.evaluate(()=>sessionStorage.getItem('training-page-loads'))).toBe('1');
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
