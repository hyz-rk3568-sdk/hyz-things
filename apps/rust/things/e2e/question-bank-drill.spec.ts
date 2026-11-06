import {expect,test} from '@playwright/test';
import {loginAsAdmin,resetHarness} from './fixtures';

test.beforeEach(async ({request})=>{await resetHarness(request)});

test('drill collection practice keeps ungradable questions and reports separate progress',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-drill').click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await expect(page.getByTestId('question-bank-drill-setup')).toContainText('小P 2026 数量关系讲义');

  const collection=page.getByTestId('question-bank-drill-collection').filter({hasText:'小P 2026 数量关系讲义'});
  await expect(collection).toContainText('2 题');
  await collection.click();
  await expect(page.getByTestId('question-bank-drill-estimate')).toHaveCount(0);
  await page.getByTestId('question-bank-drill-paper').filter({hasText:'方程关系'}).click();
  await page.getByTestId('question-bank-practice-start').click();

  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/2 题');
  await expect(page.getByTestId('question-bank-practice-choice').nth(1)).toBeVisible();
  await page.getByTestId('question-bank-practice-choice').nth(1).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();

  await expect(page.getByTestId('question-bank-practice-score')).toContainText('1 / 1 正确');
  await expect(page.getByTestId('question-bank-practice-score')).toContainText('1 题暂不可判分');

  if (!(await page.getByTestId('training-answer-sheet').isVisible())) {
    await page.getByTestId('training-answer-sheet-toggle').click();
  }
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('question-bank-practice-review-answer')).toContainText('暂无可靠答案');
  const image=page.getByTestId('question-bank-practice-stem').locator('img');
  await expect(image).toHaveAttribute('src','/api/v1/question-bank/assets/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png');
  expect(await image.evaluate((node:HTMLImageElement)=>node.naturalWidth)).toBeGreaterThan(0);
  await page.getByTestId('training-answer-sheet-toggle').click();

  await page.getByTestId('question-bank-practice-records').click();
  await expect(page.getByTestId('question-bank-practice-history')).toContainText('专项练习');
  await page.getByTestId('question-bank-history-filter-drill').click();
  await expect(page.getByTestId('question-bank-practice-history-item').first()).toContainText('小P 2026 数量关系讲义');

  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-statistics').click();
  await expect(page.getByTestId('drill-progress-card')).toContainText('小P 2026 数量关系讲义');
  await expect(page.getByTestId('drill-progress-card')).toContainText('已做 1/2 题');
  await expect(page.getByTestId('drill-progress-card')).not.toContainText('预计');
});

test('exam paper list does not include drill collections',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-setup')).not.toContainText('小P 2026 数量关系讲义');
  const response=await page.request.get('/api/v1/question-bank/papers');
  const papers=await response.json() as {title:string}[];
  expect(papers.some(paper=>paper.title==='小P 2026 数量关系讲义')).toBe(false);
});
