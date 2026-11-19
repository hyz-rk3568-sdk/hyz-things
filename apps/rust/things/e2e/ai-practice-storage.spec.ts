import { expect, test } from '@playwright/test';
import { harnessOrigin, loginAsAdmin, resetHarness, webOrigin } from './fixtures';

test.beforeEach(async ({ request }) => {
  await resetHarness(request);
  expect((await request.put(`${harnessOrigin}/ai-practice-fixtures`)).ok()).toBeTruthy();
});
test.afterEach(async ({ request }) => {
  expect((await request.delete(`${harnessOrigin}/ai-practice-fixtures`)).ok()).toBeTruthy();
});

test('three modules persist favorite tags and independent Agent conversations', async ({ page, request }) => {
  await loginAsAdmin(page);
  const seed = await (await request.put(`${harnessOrigin}/ai-practice-fixtures`)).json() as {sessions:string[],card:string};
  const csrf = (await (await page.request.get('/api/v1/panel')).json()).csrf_token as string;
  const headers = {Origin:webOrigin,'X-HYZ-CSRF':csrf};
  const root='/api/v1/study/ai-practice';
  const modules=['data_analysis','logic_judgment','quantitative_relations'];
  for (let index=0;index<modules.length;index++) {
    const id={module:modules[index],session_id:seed.sessions[index],question:0};
    const query=new URLSearchParams({module:id.module,session_id:id.session_id,question:'0'}).toString();
    expect((await page.request.post(`${root}/favorites`,{headers,data:{id,favorite:true}})).status()).toBe(200);
    expect((await page.request.post(`${root}/favorites`,{headers,data:{id,favorite:true}})).status()).toBe(200);
    const before=await (await page.request.get(`${root}/agent?${query}`)).json();
    expect(before.messages).toEqual([]);
    expect((await page.request.post(`${root}/agent/messages`,{headers,data:{id,message:'请解答本题'}})).status()).toBe(200);
    await expect.poll(async()=> (await (await page.request.get(`${root}/agent?${query}`)).json()).status).toBe('complete');
    const state=await (await page.request.get(`${root}/agent?${query}`)).json();
    expect(state.messages.map((entry:any)=>entry.role)).toEqual(['user','assistant']);
    expect(state.messages[1].content).toBe('按当前方法卡求解。');
    const otherQuery=new URLSearchParams({module:id.module,session_id:id.session_id,question:'1'}).toString();
    expect((await (await page.request.get(`${root}/agent?${otherQuery}`)).json()).messages).toEqual([]);
  }
  const favorites=await (await page.request.get(`${root}/favorites`)).json();
  expect(favorites.filter((id:any)=>seed.sessions.includes(id.session_id))).toHaveLength(3);
  const prompts=await (await request.get(`${harnessOrigin}/ai-practice-prompts`)).json() as string[];
  expect(prompts).toHaveLength(3);
  for (const prompt of prompts) {
    expect(prompt).toContain(seed.card);
    expect(prompt).not.toContain('旧卡片正文');
    expect(prompt).not.toContain('基期量公式');
  }
  expect(prompts[0]).toContain('共享统计材料');
  const current=await page.request.get(`/api/v1/study/cards/${seed.card}/current`);
  expect(current.status()).toBe(200);
  expect((await current.json()).front_markdown).toContain('基期量公式');
  await page.reload();
  const query=new URLSearchParams({module:modules[0],session_id:seed.sessions[0],question:'0'}).toString();
  expect((await (await page.request.get(`${root}/agent?${query}`)).json()).messages).toHaveLength(2);
  const id={module:modules[0],session_id:seed.sessions[0],question:0};
  expect((await page.request.post(`${root}/agent/messages`,{headers,data:{id,message:'为什么？'}})).status()).toBe(200);
  await expect.poll(async()=> (await (await page.request.get(`${root}/agent?${query}`)).json()).messages.length).toBe(4);
  const blocking={...id,question:1};
  const blockingQuery=new URLSearchParams({module:blocking.module,session_id:blocking.session_id,question:'1'}).toString();
  expect((await page.request.post(`${root}/agent/messages`,{headers,data:{id:blocking,message:'阻塞模式'}})).status()).toBe(200);
  await expect.poll(async()=> (await (await page.request.get(`${root}/agent?${blockingQuery}`)).json()).partial).toContain('已开始解题');
  expect((await page.request.post(`${root}/agent/messages`,{headers,data:{id:blocking,message:'重复发送'}})).status()).toBe(409);
  expect((await page.request.post(`${root}/agent/stop`,{headers,data:{id:blocking}})).status()).toBe(200);
  await expect.poll(async()=> (await (await page.request.get(`${root}/agent?${blockingQuery}`)).json()).status).toBe('stopped');
  const stopped=await (await page.request.get(`${root}/agent?${blockingQuery}`)).json();
  expect(stopped.messages[1].content).toContain('已开始解题');
  expect((await page.request.post(`${root}/favorites`,{headers,data:{id,favorite:false}})).status()).toBe(200);
  expect((await (await page.request.get(`${root}/favorites`)).json()).filter((item:any)=>item.session_id===id.session_id)).toHaveLength(0);
  expect((await (await page.request.get(`${root}/agent?${query}`)).json()).messages).toHaveLength(4);
});
