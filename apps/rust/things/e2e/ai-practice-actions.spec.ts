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
      if (route.request().method() === 'GET') {
        const id = { module:url.searchParams.get('module'), session_id:url.searchParams.get('session_id'), question:Number(url.searchParams.get('question')) };
        return route.fulfill({json:{status:'complete',messages:conversations.get(key(id)) ?? [],partial:'',error:null}});
      }
      const {id, message} = route.request().postDataJSON();
      if (url.pathname.endsWith('/messages')) {
        conversations.set(key(id), [...(conversations.get(key(id)) ?? []), {role:'user',content:message}, {role:'assistant',content:'按关联方法卡解题'}]);
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
    const star = page.getByTestId('ai-practice-favorite');
    await expect(star).toHaveAttribute('aria-label','收藏题目');
    await star.click();
    await expect(star).toHaveAttribute('aria-label','取消收藏');
    expect(favorites).toEqual([{module,session_id:session,question:0}]);
    await page.getByRole('button',{name:'解题 Agent'}).click();
    await page.getByRole('button',{name:'开始解题'}).click();
    await expect(page.getByTestId('ai-practice-agent-dialog')).toContainText('按关联方法卡解题');
    await page.getByLabel('追问解题 Agent').fill('为什么这样计算？');
    await page.getByRole('button',{name:'发送追问'}).click();
    await expect(page.getByTestId('ai-practice-agent-dialog')).toContainText('为什么这样计算？');
    await page.getByRole('button',{name:'关闭解题 Agent'}).click();
    await page.getByRole('button',{name:'下一题',exact:true}).click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','收藏题目');
    await page.getByTestId('ai-practice-favorite').click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','取消收藏');
    await page.getByRole('button',{name:'上一题',exact:true}).click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','取消收藏');
    await page.getByRole('button',{name:'解题 Agent'}).click();
    await expect(page.getByTestId('ai-practice-agent-dialog')).toContainText('为什么这样计算？');
    await page.getByRole('button',{name:'关闭解题 Agent'}).click();
    await page.getByRole('button',{name:'查看练习记录',exact:true}).click();
    const collection=page.getByTestId('ai-practice-favorites');
    await expect(collection).toContainText('题目 1');
    await expect(collection).toContainText('题目 2');
    await collection.getByTestId('ai-practice-favorite-category').filter({hasText:'方法B'}).click();
    await expect(collection.getByTestId('ai-practice-favorite-item')).toHaveCount(1);
    await expect(collection.getByTestId('ai-practice-favorite-item')).toContainText('题目 2');
    await collection.getByTestId('ai-practice-favorite-category').filter({hasText:'方法A'}).click();
    await collection.getByRole('button',{name:/题目 1/}).click();
    await expect(page.getByTestId('ai-practice-favorite')).toHaveAttribute('aria-label','取消收藏');
    await page.getByRole('button',{name:'解题 Agent'}).click();
    await expect(page.getByTestId('ai-practice-agent-dialog')).toContainText('为什么这样计算？');
    await page.getByRole('button',{name:'关闭解题 Agent'}).click();
    await page.getByTestId('ai-practice-favorite').click();
    await expect.poll(()=>favorites.length).toBe(1);
    expect(favorites[0].question).toBe(1);
  });
}
