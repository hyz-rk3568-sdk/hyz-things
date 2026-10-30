import { expect, test, type Page } from '@playwright/test';
import { expectNoHorizontalOverflow, resetHarness } from './fixtures';

test.beforeEach(async ({ request }) => { await resetHarness(request); });

async function subjectFixture(page: Page, logicPromptGate?: Promise<void>) {
  const now = Date.now();
  const reads: string[] = [];
  await page.route('**/api/v1/study/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (!/logic-judgment|quantitative-relations/.test(path)) return route.continue();
    expect(route.request().method()).toBe('GET');
    reads.push(path);
    const quantitative = path.includes('quantitative-relations');
    const subject = quantitative ? '数量' : '判断';
    const run = {
      id: quantitative ? 'quant-paper' : 'logic-paper', status: 'active',
      deadline_at_ms: now + (quantitative ? 900_000 : 600_000), paused_remaining_ms: null,
      server_now_ms: now, duration_seconds: quantitative ? 900 : 600,
      questions: Array.from({ length: 10 }, (_, i) => ({ question: `${subject}题 ${i + 1}`, options: ['甲', '乙', '丙', '丁'] })),
      answers: Array(10).fill(null),
    };
    let data: unknown = [];
    if (path.endsWith('/status')) data = { configured: true, eligible_cards: quantitative ? 22 : 11, preparing_job_id: null };
    else if (path.endsWith('/user-prompt')) {
      if (!quantitative && logicPromptGate) await logicPromptGate;
      data = { user_prompt: `${subject}专用提示`, customized: true };
    }
    else if (path.endsWith('/unfinished') || path.includes('/runs/')) data = path.endsWith('/unfinished') ? [run] : run;
    else if (path.endsWith('/history')) data = [{ id: `${subject}-history`, correct: quantitative ? 8 : 3, answered: 10, total: 10, ended_at_ms: now }];
    else if (path.endsWith('/card-performance')) data = [{
      card_id: `${subject}-card`, category: subject, card_path: `${subject}/cards.md`,
      front: `${subject}方法卡`, back: `${subject}解题方法`, answered_count: 10, correct_count: quantitative ? 8 : 3,
    }];
    await route.fulfill({ json: data });
  });
  return reads;
}

test('training subjects reload their own prompts, cards, history and unfinished papers on every switch', async ({ page }) => {
  const reads = await subjectFixture(page);
  await page.goto('/#/study/training');
  for (const subject of ['logic', 'quant', 'logic', 'quant']) {
    const quantitative = subject === 'quant';
    await page.getByTestId(`training-mode-${quantitative ? 'quantitative' : 'logic-judgment'}`).click();
    const setup = page.getByTestId(`${subject}-setup`);
    await expect(setup).toContainText(`当前有效卡片：${quantitative ? 22 : 11} 张`);
    await expect(page.getByTestId(`${subject}-unfinished`)).toBeVisible();
    const prompt = page.getByTestId(`${subject}-user-prompt`);
    await prompt.locator('summary').click();
    await expect(prompt.locator('textarea')).toHaveValue(`${quantitative ? '数量' : '判断'}专用提示`);
    await page.getByTestId(`${subject}-statistics`).locator('summary').click();
    await expect(page.getByTestId(`${subject}-card-item`)).toContainText(`${quantitative ? '数量' : '判断'}方法卡`);
    await page.getByTestId(`${subject}-history-toggle`).click();
    await expect(page.getByTestId(`${subject}-history`)).toContainText(`${quantitative ? 8 : 3}/10题`);
    await page.getByRole('button', { name: '返回训练', exact: true }).click();
  }
  await page.getByTestId('quant-resume').click();
  await expect(page.getByTestId('quant-question')).toHaveText('数量题 1');
  await expect(page.getByTestId('quant-clock')).toHaveText('15:00');
  expect(reads.filter(path => path === '/api/v1/study/logic-judgment/status')).toHaveLength(2);
  expect(reads.filter(path => path === '/api/v1/study/quantitative-relations/status')).toHaveLength(2);
  expect(reads.filter(path => path.includes('/runs/'))).toEqual(['/api/v1/study/quantitative-relations/runs/quant-paper']);
});

test('late replies from the previous training subject cannot overwrite the selected subject', async ({ page }) => {
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await subjectFixture(page, gate);
  await page.goto('/#/study/training');
  const logicPrompt = page.waitForRequest('**/logic-judgment/user-prompt');
  await page.getByTestId('training-mode-logic-judgment').click();
  await logicPrompt;
  await page.getByTestId('training-mode-quantitative').click();
  await expect(page.getByTestId('quant-setup')).toContainText('当前有效卡片：22 张');
  const lateReply = page.waitForResponse('**/logic-judgment/user-prompt');
  release();
  await lateReply;
  await page.getByTestId('quant-user-prompt').locator('summary').click();
  await expect(page.getByTestId('quant-user-prompt').locator('textarea')).toHaveValue('数量专用提示');
  await expect(page.getByTestId('quant-setup')).toContainText('当前有效卡片：22 张');
  await expect(page.getByTestId('logic-setup')).toHaveCount(0);
});

test('training setup fixes navigation and the start action while settings scroll internally', async ({page})=>{
  await page.setViewportSize({width:390,height:640});
  await page.goto('/#/study/training');
  const header=page.getByTestId('training-header');
  const modes=page.getByTestId('training-modes');
  const scroll=page.getByTestId('training-setup-scroll');
  const action=page.getByTestId('training-setup-action');
  await expect(header).toBeVisible();
  await expect(modes).toBeVisible();
  await expect(action.getByTestId('calculation-start')).toBeVisible();
  const before={header:await header.boundingBox(),modes:await modes.boundingBox(),action:await action.boundingBox()};
  await scroll.evaluate(el=>el.scrollTop=el.scrollHeight);
  expect(await scroll.evaluate(el=>el.scrollTop)).toBeGreaterThan(0);
  expect(await header.boundingBox()).toEqual(before.header);
  expect(await modes.boundingBox()).toEqual(before.modes);
  expect(await action.boundingBox()).toEqual(before.action);
  expect(await page.evaluate(()=>window.scrollY)).toBe(0);
  await page.getByTestId('calculation-count-50').click();
  await expect(action).toContainText('50');
  await page.getByTestId('calc-kind-div').uncheck();
  await page.getByTestId('calc-kind-mul').uncheck();
  await page.getByTestId('calc-kind-percent').uncheck();
  await expect(page.getByTestId('calculation-start')).toBeDisabled();
  await page.getByTestId('calc-kind-div').check();
  await page.getByTestId('calculation-start').click();
  await expect(page.getByTestId('calculation-question')).toBeVisible();
  await expect(header).toHaveCount(0);
  await page.getByTestId('calculation-back').click();
  await expect(header).toBeVisible();
  await expect(page.getByTestId('calculation-count-50')).toHaveAttribute('aria-pressed','true');
});

test('every training mode keeps its primary setup action in the shared fixed footer', async ({page})=>{
  await subjectFixture(page);
  await page.setViewportSize({width:390,height:640});
  await page.goto('/#/study/training');
  for(const [mode,setup,button] of [
    ['flashcards','practice-setup','practice-start'],
    ['comprehensive','analysis-setup','analysis-start'],
    ['logic-judgment','logic-setup','logic-prepare'],
    ['quantitative','quant-setup','quant-prepare'],
  ]) {
    await page.getByTestId(`training-mode-${mode}`).click();
    await expect(page.getByTestId(setup)).toBeVisible();
    const action=page.getByTestId('training-setup-action');
    await expect(action.getByTestId(button)).toBeVisible();
    const before=await action.boundingBox();
    await page.getByTestId('training-setup-scroll').evaluate(el=>el.scrollTop=el.scrollHeight);
    expect(await action.boundingBox()).toEqual(before);
    await expect(page.getByTestId('training-header')).toBeVisible();
    await expect(page.getByTestId('training-modes')).toBeVisible();
    expect(await page.evaluate(()=>window.scrollY)).toBe(0);
  }
});

test('setup and history stay inside safe areas while prompt editing and viewport height change', async ({page})=>{
  await subjectFixture(page);
  await page.route('**/logic-judgment/history',route=>route.fulfill({json:Array.from({length:18},(_,i)=>({id:`long-history-${i}`,correct:3,answered:10,total:10,ended_at_ms:Date.now()-i*60000}))}));
  await page.goto('/#/study/training');
  await page.evaluate(()=>{const s=document.documentElement.style;s.setProperty('--app-safe-top','28px');s.setProperty('--app-safe-bottom','22px');s.setProperty('--app-safe-left','10px');s.setProperty('--app-safe-right','10px');});
  for(const viewport of [{width:1133,height:744},{width:834,height:1112},{width:390,height:640}]) {
    await page.setViewportSize(viewport);
    await page.getByTestId('training-mode-logic-judgment').click();
    const action=page.getByTestId('training-setup-action');
    const start=await page.getByTestId('logic-prepare').boundingBox();
    expect(start!.y+start!.height).toBeLessThanOrEqual(viewport.height-22);
    expect((await page.getByTestId('training-back').boundingBox())!.y).toBeGreaterThanOrEqual(28);
    const prompt=page.getByTestId('logic-user-prompt');
    await prompt.locator('summary').click();
    await prompt.locator('textarea').fill('希望增加计算步骤的训练');
    await prompt.locator('textarea').focus();
    await expect(action.getByTestId('logic-prepare')).toBeVisible();
    await prompt.locator('summary').click();
    await page.getByTestId('logic-history-toggle').click();
    const back=page.getByRole('button',{name:'返回训练',exact:true});
    const before=await back.boundingBox();
    await page.getByTestId('training-setup-scroll').evaluate(el=>el.scrollTop=el.scrollHeight);
    expect(await page.getByTestId('training-setup-scroll').evaluate(el=>el.scrollTop)).toBeGreaterThan(0);
    expect(await back.boundingBox()).toEqual(before);
    await expectNoHorizontalOverflow(page);
    await back.click();
    expect(await page.getByTestId('training-setup-scroll').evaluate(el=>el.scrollTop)).toBe(0);
  }
  await page.setViewportSize({width:390,height:430});
  await page.getByTestId('logic-user-prompt').locator('summary').click();
  await page.getByTestId('logic-user-prompt').locator('textarea').focus();
  await expect(page.getByTestId('training-setup-action').getByTestId('logic-prepare')).toBeVisible();
  await page.locator('summary[aria-label="应用选项"]').click();
  await expect(page.getByRole('button',{name:'重新加载',exact:true})).toBeVisible();
  await page.locator('summary[aria-label="应用选项"]').click();
  await page.getByTestId('training-back').click();
  await expect(page).toHaveURL(/#\/study$/);
  await expect(page.locator('nav[aria-label="主导航"]')).toBeVisible();
});

test('empty practice pool retains a visible disabled action and exposes recovery controls', async ({page})=>{
  await page.route('**/api/v1/study/cards?**',route=>route.fulfill({json:{cards:[]}}));
  await page.setViewportSize({width:390,height:640});
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-flashcards').click();
  await expect(page.getByTestId('practice-empty')).toBeVisible();
  await expect(page.getByTestId('training-setup-action').getByTestId('practice-start')).toBeDisabled();
  await page.getByTestId('practice-count-50').click();
  await expect(page.getByTestId('practice-count-50')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('practice-deck-filter')).toBeVisible();
  await expectNoHorizontalOverflow(page);
});

test('training document scrolling lock is released on returning to study', async ({page})=>{
  await page.goto('/#/study/training');
  await expect(page.locator('html')).toHaveCSS('overflow-y','hidden');
  await expect(page.locator('body')).toHaveCSS('overflow-y','hidden');
  await page.getByTestId('training-back').click();
  await expect(page).toHaveURL(/#\/study$/);
  await expect(page.locator('html')).not.toHaveCSS('overflow-y','hidden');
  await expect(page.locator('body')).not.toHaveCSS('overflow-y','hidden');
});

test('training shell follows the keyboard-visible viewport without resizing the document', async ({page})=>{
  await page.setViewportSize({width:390,height:844});
  await page.addInitScript(()=>{
    const viewport=Object.assign(new EventTarget(),{height:window.innerHeight,width:window.innerWidth,offsetTop:0,offsetLeft:0,scale:1});
    Object.defineProperty(window,'visualViewport',{configurable:true,value:viewport});
  });
  await subjectFixture(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-logic-judgment').click();
  await page.getByTestId('logic-user-prompt').locator('summary').click();
  await page.getByTestId('logic-user-prompt').locator('textarea').focus();
  await expect(page.getByTestId('logic-user-prompt').locator('textarea')).toHaveCSS('font-size','16px');
  await page.evaluate(()=>{Object.assign(window.visualViewport!,{height:430,offsetTop:60});window.visualViewport!.dispatchEvent(new Event('resize'));});
  const shell=page.getByTestId('training-layout');
  await expect.poll(async()=>Math.round((await shell.boundingBox())!.height)).toBe(430);
  expect(Math.round((await shell.boundingBox())!.y)).toBe(60);
  expect(await page.evaluate(()=>window.innerHeight)).toBe(844);
  const action=await page.getByTestId('logic-prepare').boundingBox();
  expect(action!.y+action!.height).toBeLessThanOrEqual(490);
  await page.evaluate(()=>{Object.assign(window.visualViewport!,{height:215,width:195,scale:2});window.visualViewport!.dispatchEvent(new Event('resize'));});
  expect(Math.round((await shell.boundingBox())!.height)).toBe(430);
  await page.evaluate(()=>{Object.assign(window.visualViewport!,{height:844,width:390,offsetTop:0,scale:1});window.visualViewport!.dispatchEvent(new Event('resize'));});
  await expect.poll(async()=>Math.round((await shell.boundingBox())!.height)).toBe(844);
});

test('footerless history keeps its last entry above the bottom safe area', async ({page})=>{
  await subjectFixture(page);
  await page.route('**/logic-judgment/history',route=>route.fulfill({json:Array.from({length:18},(_,i)=>({id:`safe-history-${i}`,correct:3,answered:10,total:10,ended_at_ms:Date.now()-i*60000}))}));
  await page.setViewportSize({width:390,height:640});
  await page.goto('/#/study/training');
  await page.evaluate(()=>document.documentElement.style.setProperty('--app-safe-bottom','80px'));
  await page.getByTestId('training-mode-logic-judgment').click();
  await page.getByTestId('logic-history-toggle').click();
  await page.getByTestId('training-setup-scroll').evaluate(el=>el.scrollTop=el.scrollHeight);
  const last=await page.getByTestId('logic-history').locator('article').last().boundingBox();
  expect(last!.y+last!.height).toBeLessThanOrEqual(640-80);
});

test('training shell retains CSS viewport sizing when VisualViewport is unavailable', async ({page})=>{
  await page.setViewportSize({width:390,height:640});
  await page.addInitScript(()=>Object.defineProperty(window,'visualViewport',{configurable:true,value:undefined}));
  await page.goto('/#/study/training');
  const shell=await page.getByTestId('training-layout').boundingBox();
  expect(shell!.height).toBe(640);
  expect(shell!.y).toBe(0);
  await expect(page.getByTestId('calculation-start')).toBeEnabled();
  await page.getByTestId('training-back').click();
  await expect(page).toHaveURL(/#\/study$/);
});

test('setup markers and primary action align with their reserved edges', async ({page})=>{
  await page.goto('/#/study/training');
  for(const viewport of [{width:1133,height:744},{width:390,height:640}]) {
    await page.setViewportSize(viewport);
    await expect.poll(async()=>Math.round((await page.getByTestId('training-layout').boundingBox())!.height)).toBe(viewport.height);
    const slot=await page.locator('.training-primary-action').boundingBox();
    const start=await page.getByTestId('calculation-start').boundingBox();
    expect.soft(Math.abs(slot!.width-start!.width)).toBeLessThanOrEqual(1);
    for(const card of await page.locator('.training-type-card').all()) {
      const box=await card.boundingBox();
      const input=card.locator('input');
      const marker=await input.boundingBox();
      expect.soft(await input.evaluate(el=>getComputedStyle(el).position)).toBe('absolute');
      expect.soft(box!.x+box!.width-marker!.x-marker!.width).toBeLessThanOrEqual(16);
      expect.soft(marker!.x).toBeGreaterThanOrEqual(box!.x);
    }
  }
});
