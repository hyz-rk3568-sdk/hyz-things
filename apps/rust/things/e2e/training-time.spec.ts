import {expect,test,type Page} from '@playwright/test';
import {resetHarness} from './fixtures';

test.beforeEach(async({request})=>{await resetHarness(request)});

async function clockFixture(page:Page) {
  const events:string[]=[];
  let running=false,paused=false,module='';
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
        module=body.module;running=!paused;
      }else if(body.action==='toggle') {
        paused=!paused;running=!paused;
      }else if(body.action==='leave'){running=false;}
    }
    await route.fulfill({json:{now_ms:Date.now(),today_ms:3600000,running,paused,
      manual_paused:paused,module}});
  });
  return events;
}

test('sixth statistics text tab shows stored durations and heatmap',async({page})=>{
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

test('pink study clock toggles independently and remains compact',async({page})=>{
  const events=await clockFixture(page);
  await page.goto('/#/study/training');
  const clock=page.getByTestId('study-time-inline');
  await expect(clock).toHaveText('01:00:00');
  await expect(clock).toHaveClass(/text-primary/);
  await clock.click();
  await expect.poll(()=>events.filter(x=>x==='toggle').length).toBe(1);
  await expect(clock).toHaveAttribute('aria-label',/已暂停/);
  await clock.click();
  await expect.poll(()=>events.filter(x=>x==='toggle').length).toBe(2);
  await expect(clock).toHaveAttribute('aria-label',/点击暂停/);
});
