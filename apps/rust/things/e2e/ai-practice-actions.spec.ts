import { expect, test } from '@playwright/test';
import { resetHarness } from './fixtures';

const session = '019a31b7-8c42-7c16-a6d1-0c1f0f34c821';
const card = '019a31b7-8c42-7c16-a6d1-0c1f0f34c822';
const otherCard = '019a31b7-8c42-7c16-a6d1-0c1f0f34c823';
test.beforeEach(async ({ request }) => { await resetHarness(request); });

for (const [prefix, mode, endpoint, module] of [
  ['analysis', 'comprehensive', 'data-analysis', 'data_analysis'],
  ['logic', 'logic-judgment', 'logic-judgment', 'logic_judgment'],
  ['quant', 'quantitative', 'quantitative-relations', 'quantitative_relations'],
] as const) {
  test(`${prefix} favorites and agent share the exercise session`, async ({ page }) => {
    let favorites: any[] = [];
    const conversations = new Map<string, any[]>();
    const sentMessages: any[] = [];
    const uploads: string[] = [];
    const key = (id: any) => `${id.module}:${id.session_id}:${id.question}`;
    await page.route('**/api/v1/study/ai-practice/favorites', async route => {
      if (route.request().method() === 'POST') {
        const { id, favorite } = route.request().postDataJSON();
        favorites = favorites.filter(item => key(item) !== key(id));
        if (favorite) favorites.push(id);
        return route.fulfill({json:{favorite}});
      }
      return route.fulfill({json:favorites});
    });
    await page.route('**/api/v1/study/ai-practice/agent**', async route => {
      const url = new URL(route.request().url());
      const id = { module:url.searchParams.get('module'), session_id:url.searchParams.get('session_id'), question:Number(url.searchParams.get('question')) };
      if (url.pathname.endsWith('/events')) {
        // Exercise the polling fallback; the dedicated storage spec covers SSE.
        return route.fulfill({status:204});
      }
      if (route.request().method() === 'DELETE') {
        conversations.delete(key(id));
        return route.fulfill({json:{deleted:true}});
      }
      if (url.pathname.endsWith('/attachments')) {
        uploads.push(route.request().headers()['content-type']);
        return route.fulfill({json:{id:'attachment-1'}});
      }
      if (route.request().method() === 'GET') {
        const messages = conversations.get(key(id));
        return route.fulfill({json:{status:messages?'complete':'idle',messages:messages ?? [],partial:'',error:null}});
      }
      const body = route.request().postDataJSON();
      if (url.pathname.endsWith('/messages')) {
        sentMessages.push(body);
        conversations.set(key(body.id), [...(conversations.get(key(body.id)) ?? []), {role:'user',content:body.message}, {role:'assistant',content:'',tool:{name:'command',status:'completed',detail:'rg 卡片'}}, {role:'assistant',content:'按关联方法卡解题'}]);
      }
      return route.fulfill({json:{accepted:true}});
    });
    await page.route('**/api/v1/study/cards/*/current', route => route.fulfill({json:{source_file:'卡片/cards.md',front_markdown:'方法卡',back_markdown:'方法'}}));
    await page.route(`**/api/v1/study/${endpoint}/**`, async route => {
      const path = new URL(route.request().url()).pathname;
      let data: unknown = [];
      if (path.endsWith('/status')) data = prefix === 'analysis' ? {configured:true, model:'fixture',preparing_job_id:null,answer_summary:{answered_count:0,correct_count:0},card_stats:[]} : {configured:true,eligible_cards:20,preparing_job_id:null};
      else if (path.endsWith('/user-prompt')) data={user_prompt:'',customized:false};
      else if (path.endsWith('/history')) data=[{id:session,correct:10,total:10,answered:10,ended_at_ms:Date.now(),started_at_ms:Date.now(),elapsed_ms:1000,topics:['材料']}];
      else if (path.endsWith('/result')) {
        const questions=Array.from({length:prefix==='analysis'?20:10},(_,i)=>({number:i+1,question:`题目 ${i+1}`,options:['甲','乙','丙','丁'],chosen:0,correct_index:0,explanation:'解析',skill:'方法',evidence:'材料',card_id:i===1?otherCard:card,card_path:`用户整理/资料分析/方法${i===1?'B':'A'}/cards.md`,front:'旧卡',back:'旧卡'}));
        data={id:session,correct:10,total:questions.length,answered:10,questions,materials:Array.from({length:4},()=>({topic:'材料',text:'统计材料'}))};
      }
      await route.fulfill({json:data});
    });
    await page.goto('/#/study/training');
    await page.getByTestId(`training-mode-${mode}`).click();
    await page.getByTestId(`${prefix}-history-open`).click();
    await page.getByRole('button',{name:'查看原题与解析',exact:true}).click();
    const answerPane = page.getByTestId(prefix === 'analysis' ? 'analysis-answer-header' : `${prefix}-answer-pane`);
    await expect(answerPane.getByTestId('ai-practice-favorite')).toBeVisible();
    await expect(answerPane.getByTestId('ai-practice-agent-tab')).toBeVisible();
    await expect(page.getByTestId(`ai-practice-favorite-${prefix === 'analysis' ? 'material' : 'stem'}`)).toHaveCount(0);
    await expect(page.getByTestId('training-workspace-header').getByTestId('ai-practice-favorite')).toHaveCount(0);
    const star = page.getByTestId('ai-practice-favorite');
    await expect(star).toHaveAttribute('aria-label','收藏题目');
    await star.click();
    await expect(star).toHaveAttribute('aria-label','取消收藏');
    expect(favorites).toEqual([{module,session_id:session,question:0}]);
    const tab = page.getByTestId('ai-practice-agent-tab');
    await tab.click();
    await expect(tab).toHaveAttribute('aria-pressed','true');
    await expect(page.getByTestId('ai-practice-agent-page')).toBeVisible();
    await expect(page.getByTestId('ai-practice-agent-dialog')).toHaveCount(0);
    await expect(page.getByTestId('ai-practice-agent-question')).toHaveCount(0);
    // The AI panel renders only the conversation; the question belongs to the panes.
    await expect(page.getByTestId('ai-practice-agent-page').getByText('题目 1',{exact:true})).toHaveCount(0);
    for (const header of prefix === 'analysis'
      ? ['analysis-material-header','analysis-answer-header']
      : [`${prefix}-question-header`,`${prefix}-answer-header`]) {
      await expect(page.getByTestId(header)).toHaveCSS('border-bottom-width','1px');
    }
    await page.getByTestId('ai-practice-agent-input-field').fill('请按方法卡讲解');
    await page.getByTestId('ai-practice-agent-send').click();
    await expect(page.getByTestId('ai-practice-agent-page')).toContainText('请按方法卡讲解');
    const tools = page.getByTestId('ai-practice-agent-tools').first();
    await expect(tools).toContainText('工具调用 · 完成');
    await expect(tools).toContainText('rg 卡片');
    await tools.getByRole('button').click();
    await expect(tools.getByTestId('ai-practice-agent-tool')).toContainText('command · 完成');
    await expect(tools.getByTestId('ai-practice-agent-command')).toHaveText('rg 卡片');
    const finalReply = page.getByTestId('ai-practice-agent-page').locator('div.study-markdown[data-ai-diagram-source="true"]').last();
    await expect(finalReply).toContainText('按关联方法卡解题');
    await expect(finalReply).toHaveClass(/study-markdown/);
    await expect(finalReply).toHaveClass(/text-base/);
    await expect(page.getByTestId('ai-practice-agent-page')).toContainText('按关联方法卡解题');
    await page.getByTestId('ai-practice-agent-input-field').fill('为什么这样计算？');
    await page.getByTestId('ai-practice-agent-send').click();
    await expect(page.getByTestId('ai-practice-agent-page')).toContainText('为什么这样计算？');
    const picker = page.getByTestId('ai-practice-agent-images');
    await expect(picker).toBeVisible();
    await picker.locator('input[type=file]').setInputFiles({
      name: 'note.png',
      mimeType: 'image/png',
      buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==','base64'),
    });
    await expect(picker.getByRole('img')).toBeVisible();
    await page.getByTestId('ai-practice-agent-send').click();
    await expect.poll(() => sentMessages.at(-1)?.attachments).toEqual(['attachment-1']);
    expect(uploads).toEqual(['image/png']);
    await page.getByRole('button',{name:'下一题',exact:true}).click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','收藏题目');
    await page.getByTestId('ai-practice-favorite').click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','取消收藏');
    await page.getByRole('button',{name:'上一题',exact:true}).click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','取消收藏');
    await expect(page.getByTestId('ai-practice-agent-page')).toContainText('为什么这样计算？');
    await page.getByRole('button',{name:'查看练习记录',exact:true}).click();
    await expect(page.getByTestId(prefix === 'analysis' ? 'analysis-history' : `${prefix}-history`)).toBeVisible();
    const favoritesButton=page.getByTestId('ai-practice-favorites');
    await expect(favoritesButton).toContainText('我的收藏');
    await expect(page.getByTestId('ai-practice-favorites-count')).toHaveText('2');
    await favoritesButton.click();
    const collection=page.getByTestId('ai-practice-favorites-page');
    await expect(collection).toBeVisible();
    await expect(page.getByTestId('ai-practice-favorites-page-content')).toContainText('题目 1');
    await expect(collection).toContainText('题目 2');
    await page.getByTestId('ai-practice-favorites-back').click();
    await expect(page.getByTestId(prefix === 'analysis' ? 'analysis-history' : `${prefix}-history`)).toBeVisible();
    await favoritesButton.click();
    await expect(collection).toBeVisible();
    await collection.getByTestId('ai-practice-favorite-category').filter({hasText:'方法B'}).click();
    await expect(collection.getByTestId('ai-practice-favorite-item')).toHaveCount(1);
    await expect(collection.getByTestId('ai-practice-favorite-item')).toContainText('题目 2');
    await collection.getByTestId('ai-practice-favorite-category').filter({hasText:'方法A'}).click();
    await collection.getByRole('button',{name:/题目 1/}).click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','取消收藏');
    const openedTab = page.getByTestId('ai-practice-agent-tab');
    if (await openedTab.getAttribute('aria-pressed') !== 'true') await openedTab.click();
    await expect(page.getByTestId('ai-practice-agent-page')).toContainText('为什么这样计算？');
    await page.getByTestId('ai-practice-favorite').click();
    await expect.poll(()=>favorites.length).toBe(1);
    expect(favorites[0].question).toBe(1);
    await page.getByTestId('ai-practice-agent-more').click();
    await page.getByTestId('ai-practice-agent-delete').click();
    const dialog = page.getByTestId('ai-practice-agent-page').getByTestId('ai-practice-agent-delete-dialog');
    await expect(dialog).toBeVisible();
    const panelBox = await page.getByTestId('ai-practice-agent-page').boundingBox();
    const dialogBox = await dialog.boundingBox();
    expect(dialogBox!.x).toBeGreaterThanOrEqual(panelBox!.x);
    expect(dialogBox!.y).toBeGreaterThanOrEqual(panelBox!.y);
    expect(dialogBox!.x + dialogBox!.width).toBeLessThanOrEqual(panelBox!.x + panelBox!.width + 0.5);
    expect(dialogBox!.y + dialogBox!.height).toBeLessThanOrEqual(panelBox!.y + panelBox!.height + 0.5);
    await expect(page.getByTestId('ai-practice-agent-more')).toBeVisible();
    await page.getByTestId('ai-practice-agent-delete-confirm').click();
    await expect(page.getByTestId('ai-practice-agent-delete-dialog')).toHaveCount(0);
    await expect(page.getByTestId('ai-practice-agent-page')).not.toContainText('为什么这样计算？');
    await expect(page.getByTestId('ai-practice-agent-status')).toHaveText('未生成');
    const answerHeader = page.getByTestId(`${prefix}-answer-header`);
    await page.getByTestId(`${prefix}-navigation`).getByRole('button', {name: '下一题'}).click();
    await expect(answerHeader).toContainText('第 2/');
    await expect(page.getByTestId('ai-practice-agent-tab')).toHaveAttribute('aria-pressed', 'true');
    const messages = await page.getByTestId('ai-practice-agent-messages').boundingBox();
    await page.evaluate(({x, y}) => {
      const target = document.elementFromPoint(x, y)!;
      const event = (type: string, clientX: number) => new PointerEvent(type, {
        bubbles: true, cancelable: true, pointerId: 7, pointerType: 'touch', isPrimary: true,
        clientX, clientY: y, button: 0, buttons: 1,
      });
      target.dispatchEvent(event('pointerdown', x));
      target.dispatchEvent(event('pointermove', x - 140));
      target.dispatchEvent(event('pointerup', x - 140));
    }, {x: messages!.x + messages!.width / 2, y: messages!.y + messages!.height / 2});
    await expect(answerHeader).toContainText('第 3/');
  });
}
