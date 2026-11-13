import { expect, test } from '@playwright/test';
import { resetHarness } from './fixtures';

test.beforeEach(async ({request}) => { await resetHarness(request); });

const activity = (jobId: string) => ({
  job_id: jobId,
  seq: 3,
  status: 'ready',
  events: [
    {seq:1,kind:'tool_started',payload:{item_id:'out:cmd',tool:'commandExecution',details:{command:'cat flashcards/cards.md',source:'出题'}}},
    {seq:2,kind:'command_output_delta',payload:{item_id:'out:cmd',text:'卡片已读取'}},
    {seq:3,kind:'tool_finished',payload:{item_id:'out:cmd',status:'completed',details:{source:'出题'}}},
  ],
});

for (const [module, prefix, mode] of [
  ['logic-judgment','logic','training-mode-logic-judgment'],
  ['quantitative-relations','quant','training-mode-quantitative-relations'],
] as const) {
  test(module + ' shows Codex tool stream only until entering the paper', async ({page}) => {
    const paper = {
      id:'prepared-paper', status:'prepared', server_now_ms:Date.now(),
      started_at_ms:null, deadline_at_ms:null, duration_seconds:600,
      paused_remaining_ms:null,
      questions:Array.from({length:10},(_,n)=>({question:'题 '+(n+1),options:['甲','乙','丙','丁']})),
      answers:Array(10).fill(null),
    };
    await page.route('**/api/v1/study/'+module+'/**', async route => {
      const path = new URL(route.request().url()).pathname;
      if(path.endsWith('/prepare/my-job/events')) {
        await route.fulfill({
          status:200,
          contentType:'text/event-stream',
          headers:{'cache-control':'no-store'},
          body:'event: snapshot\ndata: '+JSON.stringify(activity('my-job'))+'\n\n',
        });
        return;
      }
      let value: unknown = {};
      if (path.endsWith('/status')) value={configured:true,eligible_cards:24,preparing_job_id:null};
      else if (path.endsWith('/user-prompt')) value={user_prompt:'默认',customized:false};
      else if (path.endsWith('/card-performance') || path.endsWith('/history')) value=[];
      else if (path.endsWith('/unfinished')) value=[paper];
      else if (path.endsWith('/prepare')) value={job_id:'my-job'};
      else if (path.endsWith('/prepare/my-job')) value={state:'ready',verified:10,message:'完成',run_id:paper.id};
      else if (path.endsWith('/start')) value={...paper,status:'active',started_at_ms:Date.now(),deadline_at_ms:Date.now()+600000};
      else if (path.endsWith('/runs/'+paper.id)) value=paper;
      await route.fulfill({json:value});
    });
    await page.goto('/#/study/training');
    await page.getByTestId(mode).click();
    await page.getByTestId(prefix+'-prepare').click();
    const panel=page.getByTestId(prefix+'-activity');
    await expect(panel).toContainText('AI 执行过程');
    await expect(panel).toContainText('cat flashcards/cards.md');
    await expect(panel).toContainText('卡片已读取');
    await expect(page.getByTestId(prefix+'-resume')).toBeVisible();
    await page.getByTestId(prefix+'-resume').click();
    await expect(panel).toHaveCount(0);
  });
}

test('data analysis shows parallel job activity and clears it when claimed', async ({page}) => {
  const groups=Array.from({length:4},(_,g)=>({
    material:{topic:'主题 '+(g+1),text:'材料内容'},
    questions:Array.from({length:5},(_,q)=>({question:'第'+(g*5+q+1)+'题',options:['甲','乙','丙','丁'],skill:'基期'})),
  }));
  const run={
    id:'analysis-paper',status:'active',server_now_ms:Date.now(),current_question:0,
    started_at_ms:Date.now(),deadline_at_ms:Date.now()+1200000,paused_remaining_ms:null,
    duration_seconds:1200,groups,answers:Array(20).fill(null),
  };
  await page.route('**/api/v1/study/data-analysis/**',async route=>{
    const path=new URL(route.request().url()).pathname;
    if(path.endsWith('/prepare/analysis-job/events')){
      await route.fulfill({status:200,contentType:'text/event-stream',
        body:'event: snapshot\ndata: '+JSON.stringify(activity('analysis-job'))+'\n\n'});
      return;
    }
    let value: unknown = {};
    if(path.endsWith('/status')) value={configured:true,model:'Codex App Server',cached_sources:1,verified_groups:4,preparing_job_id:null,answer_summary:{answered_count:0,correct_count:0},card_stats:[]};
    else if(path.endsWith('/user-prompt')) value={user_prompt:'默认',customized:false};
    else if(path.endsWith('/unfinished') || path.endsWith('/history')) value=[];
    else if(path.endsWith('/prepare')) value={job_id:'analysis-job'};
    else if(path.endsWith('/prepare/analysis-job')) value={state:'ready',verified:20,message:'完成',run_id:run.id};
    else if(path.endsWith('/claim')) value={run,control_token:'test-controller'};
    await route.fulfill({json:value});
  });
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-comprehensive').click();
  await page.getByTestId('analysis-start').click();
  const panel=page.getByTestId('analysis-activity');
  await expect(panel).toContainText('cat flashcards/cards.md');
  await expect(panel).toContainText('卡片已读取');
  await page.getByTestId('analysis-enter').click();
  await expect(panel).toHaveCount(0);
});
