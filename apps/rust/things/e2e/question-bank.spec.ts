import {expect,test} from '@playwright/test';
import {inkSnapshot} from './ink-fixtures';
import {expectNoHorizontalOverflow,harnessOrigin,loginAsAdmin,resetHarness,webOrigin} from './fixtures';
import {chooseFixturePaper,openFixtureReview,openQuestionBankPaper,openSheet} from './helpers/question-bank';

test.beforeEach(async ({request})=>{await resetHarness(request)});

test('training question uses the shared detail layout',async({page})=>{
  await openQuestionBankPaper(page);
  const detail=page.getByTestId('question-detail-layout');
  await expect(detail.getByTestId('study-goal-bars')).toBeVisible();
  await expect(detail.getByTestId('codex-limit-bars')).toBeVisible();
  await expect(detail.getByTestId('question-bank-practice-split')).toBeVisible();
  await expect(detail.getByTestId('training-answer-sheet-toggle')).toBeVisible();
});

test('answer sheet keeps its scroll position when reopened',async({page})=>{
  await openQuestionBankPaper(page);
  // Constructable stylesheets work under the portal's strict CSP and persist
  // when the conditional answer-sheet element is unmounted/recreated.
  await page.evaluate(() => {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync('[data-testid="training-answer-sheet"] { max-height: 100px !important; }');
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  });
  await openSheet(page);
  const sheet=page.getByTestId('training-answer-sheet');
  await expect(sheet).toHaveCSS('max-height', '100px');
  await expect.poll(()=>sheet.evaluate(node=>node.scrollHeight-node.clientHeight)).toBeGreaterThan(75);
  await sheet.evaluate(node=>{node.scrollTop=60});
  await expect.poll(()=>sheet.evaluate(node=>node.scrollTop)).toBe(60);
  await page.getByTestId('training-answer-sheet-toggle').click();
  await expect(sheet).toHaveCount(0);
  await page.getByTestId('training-answer-sheet-toggle').click();
  await expect.poll(()=>sheet.evaluate(node=>node.scrollTop)).toBe(60);
});

test('flashcard editor occupies the options pane and keeps the answer sheet',async({page})=>{
  await openFixtureReview(page);
  await page.evaluate(() => {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync('[data-testid="training-answer-sheet"] { max-height: 100px !important; }');
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  });
  await openSheet(page);
  const sheet=page.getByTestId('training-answer-sheet');
  await expect.poll(()=>sheet.evaluate(node=>node.scrollHeight-node.clientHeight)).toBeGreaterThan(75);
  await sheet.evaluate(node=>{node.scrollTop=60});
  await expect.poll(()=>sheet.evaluate(node=>node.scrollTop)).toBe(60);
  await page.getByTestId('question-bank-practice-create-flashcard').click();
  await expect(page.getByTestId('question-bank-practice-flashcard-pane')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-heading')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-material-pane')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-question-scroll')).toHaveCount(0);
  await expect(sheet).toBeVisible();
  await expect.poll(()=>sheet.evaluate(node=>node.scrollTop)).toBe(60);
  await page.getByRole('button',{name:'返回复盘'}).click();
  await expect(page.getByTestId('question-bank-practice-question-scroll')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-flashcard-pane')).toHaveCount(0);
});

test('paper practice hides numbering and scroll tools until they are needed',async({page,request})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await chooseFixturePaper(page);
  await expect(page.getByTestId('question-bank-mode')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-browse-count')).toHaveCount(0);
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/3 题');
  const emptyMenu=page.getByTestId('question-bank-practice-question-pane').getByRole('button',{name:'更多解析操作'});
  await expect(emptyMenu).toBeVisible();
  await emptyMenu.click();
  await expect(page.getByText('暂无可用操作')).toBeVisible();
  await emptyMenu.click();
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('材料中的正确选项');
  await expect(page.getByTestId('question-bank-practice-stem')).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('question-bank-practice-stem')).toHaveCSS('font-weight','400');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await expect(page.getByRole('button',{name:'题目向下滚动'})).toHaveCount(0);
  await expect(page.getByRole('button',{name:'材料向下滚动'})).toHaveCount(0);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect(page.getByRole('button',{name:'题目向下滚动'})).toHaveCount(1);
  await expect(page.getByRole('button',{name:'材料向下滚动'})).toHaveCount(1);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect(page.getByRole('button',{name:'题目向下滚动'})).toHaveCount(0);
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(3);
  await expect(page.getByTestId('question-bank-practice-number').first()).toContainText('26');
  await expect(page.getByTestId('question-bank-practice-scope')).toContainText('资料分析 · 比重');
  const groups = page.getByTestId('question-bank-practice-group');
  await expect(groups).toHaveCount(2);
  await expect(groups.nth(0)).toHaveAttribute('data-group-label', '资料分析');
  await expect(groups.nth(1)).toHaveAttribute('data-group-label', '判断推理');
  await page.getByTestId('training-answer-sheet-toggle').click();
  const sessionId = await page.evaluate(async()=>{
    const res=await fetch('/api/v1/question-bank/practice/history');
    const sessions=await res.json();
    return sessions[0].id as string;
  });
  const response=await request.get(`/api/v1/question-bank/practice/sessions/${sessionId}`,{headers:{Cookie:(await page.context().cookies()).map(c=>`${c.name}=${c.value}`).join('; ')}});
  expect(response.status()).toBe(200);
  expect(await response.text()).not.toContain('原始解析');
  expect((await request.get(`/api/v1/question-bank/practice/sessions/${sessionId}/review`,
    {headers:{Cookie:(await page.context().cookies()).map(c=>`${c.name}=${c.value}`).join('; ')}})).status()).toBe(409);
  await page.getByTestId('question-bank-practice-choice').first().click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('question-bank-practice-pause')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-timer')).toBeEnabled();
  await page.getByTestId('question-bank-practice-timer').click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.getByTestId('question-bank-practice-timer').click();
  await expect(page.getByTestId('question-bank-practice-paused')).toHaveCount(0);
  await page.getByTestId('question-bank-practice-timer').click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.reload();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-resume-card')).toContainText('继续练习');
  await page.getByTestId('question-bank-resume-card').getByRole('button',{name:'继续练习'}).click();
  await expect(page.getByTestId('question-bank-practice-paused')).toBeVisible();
  await page.getByRole('button',{name:'继续练习'}).click();
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('第二道题');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 2/3 题');
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await page.getByTestId('question-bank-practice-choice').nth(2).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-submit-confirm')).toBeVisible();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('第二题解析');
  await expect(page.getByTestId('question-bank-practice-question-pane').locator('.study-markdown').filter({hasText:'第二题解析'})).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('question-bank-practice-review-answer')).toHaveCSS('font-size','16px');
  await expect(page.getByRole('region', {name:'答案统计'})).toHaveClass(/rounded-box/);
  await expect(page.getByRole('region', {name:'答案统计'})).toContainText('你的答案');
  const reviewResponse=await request.get(`/api/v1/question-bank/practice/sessions/${sessionId}/review`,
    {headers:{Cookie:(await page.context().cookies()).map(c=>`${c.name}=${c.value}`).join('; ')}});
  expect(reviewResponse.status()).toBe(200);
  const reviewBody:{questions:{question:{item:{number:number}},correct_count:number,wrong_count:number}[]}=await reviewResponse.json();
  const reviewCounts=(number:number)=>reviewBody.questions.find(q=>q.question.item.number===number)!;
  const attemptCounts=page.getByTestId('question-bank-practice-attempt-counts');
  await expect(attemptCounts).toHaveAttribute('data-correct',String(reviewCounts(27).correct_count));
  await expect(attemptCounts).toHaveAttribute('data-wrong',String(reviewCounts(27).wrong_count));
  await expect(attemptCounts).toContainText(`做对 ${reviewCounts(27).correct_count} 次`);
  await expect(attemptCounts).toContainText(`做错 ${reviewCounts(27).wrong_count} 次`);
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('B 是正确项');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('现期比重');
  await expect(attemptCounts).toHaveAttribute('data-correct',String(reviewCounts(26).correct_count));
  await expect(attemptCounts).toHaveAttribute('data-wrong',String(reviewCounts(26).wrong_count));
  await expect(attemptCounts).toContainText(`做对 ${reviewCounts(26).correct_count} 次`);
  await expect(attemptCounts).toContainText(`做错 ${reviewCounts(26).wrong_count} 次`);
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveClass(/border-error/);
  await expect(page.getByTestId('question-bank-practice-choice').nth(1)).toHaveClass(/border-success/);
  await expect(page.getByTestId('question-bank-practice-choice').first()).not.toContainText('正确答案');
  await expect(page.getByTestId('question-bank-practice-choice').first()).not.toContainText('你的错选');
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').nth(2).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 28 题');
  await expect(page.getByTestId('question-bank-practice-scope')).toContainText('判断推理 · 图形推理');
  await expect(page.getByTestId('question-bank-practice-issues')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('待核验');
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('未核验');
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveClass(/border-success/);
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-score').getByLabel('正确 1')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-records')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-repeat')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-ungradable')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-score').last()).not.toContainText('待核验');
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('ungradable');
  await expect(page.getByTestId('question-bank-practice-workspace')).not.toContainText('正确率');
  await expect(page.getByTestId('question-bank-practice-number').first()).toHaveClass(/btn-error/);
  await expect(page.getByTestId('question-bank-practice-number').nth(1)).toHaveClass(/btn-success/);
  await expect(page.getByTestId('question-bank-practice-number').nth(2)).not.toHaveClass(/btn-(success|error)/);
  await page.getByTestId('training-answer-sheet-toggle').click();
  const star=await page.getByTestId('question-bank-favorite').boundingBox();
  const title=await page.getByTestId('question-bank-practice-heading').boundingBox();
  expect(star!.x).toBeGreaterThanOrEqual(title!.x + title!.width);
  expect((await page.getByTestId('calculation-scratchpad-toggle').boundingBox())!.x).toBeGreaterThan(star!.x);
  const favorite=page.getByTestId('question-bank-favorite');
  // Test-level sharding can place a favorite-mutating test before this one, so
  // restore the observed fixture state instead of assuming an empty set.
  const initiallyFavorite=await favorite.getAttribute('aria-label') === '取消收藏';
  if (initiallyFavorite) {
    await favorite.click();
    await expect(favorite).toHaveAttribute('aria-label','收藏题目');
  }
  await favorite.click();
  await expect(favorite).toHaveAttribute('aria-label','取消收藏');
  await favorite.click();
  await expect(favorite).toHaveAttribute('aria-label','收藏题目');
  await expect(favorite).not.toHaveAttribute('aria-label','取消收藏');
  if (initiallyFavorite) {
    await favorite.click();
    await expect(favorite).toHaveAttribute('aria-label','取消收藏');
  }
});

test('question bank reading omits source verification labels',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study');
  const paperButton=page.getByTestId('question-bank-open').filter({hasText:'行测真题训练端到端样卷'});
  await expect(paperButton).toBeVisible();
  await paperButton.click();
  const paper=page.getByTestId('question-bank-paper');
  await expect(paper).toContainText('原卷第 28 题');
  await expect(paper).not.toContainText('待核');
  await expect(paper).not.toContainText('未核验');
});

test('saved collections are classified by module and category before practicing',async({page})=>{
  await openQuestionBankPaper(page);
  await page.getByTestId('question-bank-practice-choice').first().click();
  await expect(page.getByTestId('question-bank-practice-choice').first()).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-submit-confirm')).toBeVisible();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('原始解析');
  const favorite=page.getByTestId('question-bank-favorite');
  const initiallyFavorite=await favorite.getAttribute('aria-label') === '取消收藏';
  if (initiallyFavorite) {
    await favorite.click();
    await expect(favorite).toHaveAttribute('aria-label','收藏题目');
  }
  await favorite.click();
  await expect(favorite).toHaveAttribute('aria-label','取消收藏');
  await page.getByRole('button',{name:/返回训练/}).click();
  await page.getByTestId('question-bank-collection-wrong').click();
  await expect(page.getByTestId('question-bank-browse-count')).toContainText('1题');
  await expect(page.getByTestId('question-bank-collection-module').filter({hasText:'资料分析'})).toBeVisible();
  await page.getByTestId('question-bank-collection-category').filter({hasText:'比重'}).click();
  await expect(page.getByTestId('question-bank-browse-count')).toContainText('1题');
  await expect(page.getByTestId('question-bank-collection-item')).toContainText(/2008\s+国考\s*·\s*第\s*26\s*题/);
  await expect(page.getByTestId('question-bank-browse-preview')).toHaveCount(0);
  await page.getByTestId('question-bank-collection-item').first().click();
  await expect(page.getByTestId('collection-review')).toBeVisible();
  await expect(page.getByTestId('collection-review-scope')).toContainText('错题 · 第 1/1 题');
  await expect(page.getByTestId('collection-review-favorite')).toHaveAttribute('aria-label','取消收藏');
  await expect(page.getByTestId('collection-review-material-pane')).toContainText('两道题共用材料');
  await expect(page.getByTestId('collection-review-choice').first()).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('collection-review-explanation')).toHaveCSS('font-size','16px');
  await expect(page.getByTestId('collection-review-answer')).toContainText('B');
  const collectionCounts=page.getByTestId('collection-review-attempt-counts');
  await expect(collectionCounts).toContainText('本题累计');
  await expect(collectionCounts).toHaveAttribute('data-wrong',/^[1-9]\d*$/);
  const collectionCreate=page.getByTestId('collection-review-create-flashcard');
  await expect(collectionCreate).toBeVisible();
  await expect(collectionCreate).toHaveAttribute('aria-label','为当前题目创建 Flashcard');
  await expect(collectionCreate).toContainText('+');
  await expect(page.locator('.question-bank-pane-header').getByTestId('collection-review-create-flashcard')).toBeVisible();
  await collectionCreate.click();
  await expect(page.getByTestId('favorite-flashcard-card')).toBeVisible();
  await page.getByRole('button',{name:'返回复盘'}).click();
  await page.getByTestId('collection-review-back').click();
  await expect(page.getByTestId('question-bank-collection-item')).toBeVisible();
  await page.getByTestId('question-bank-practice-start').click();
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(1);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/1 题');
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-choice').nth(1).click();
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect(page.getByTestId('question-bank-practice-question-pane')).toContainText('正确答案');
  await page.getByRole('button',{name:/返回训练/}).click();
  await page.getByTestId('question-bank-collection-favorite').click();
  await expect(page.getByTestId('question-bank-browse-count')).toContainText('1题');
  await expect(page.getByTestId('question-bank-collection-item')).toBeVisible();
  await page.getByTestId('question-bank-collection-item').first().click();
  await expect(page.getByTestId('collection-review')).toBeVisible();
  await expect(page.getByTestId('collection-review-scope')).toContainText('收藏 · 第 1/1 题');
  await expect(page.getByTestId('collection-review-favorite')).toHaveAttribute('aria-label','取消收藏');
  await expect(page.getByTestId('collection-review-answer')).toContainText('B');
  await expect(collectionCounts).toHaveAttribute('data-correct',/^[1-9]\d*$/);
  await page.getByTestId('collection-review-back').click();
  await expect(page.getByTestId('question-bank-collection-item')).toBeVisible();
  await page.getByTestId('question-bank-practice-start').click();
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(1);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/1 题');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  if (!initiallyFavorite) {
    await page.getByTestId('question-bank-favorite').click();
    await expect(page.getByTestId('question-bank-favorite')).toHaveAttribute('aria-label','收藏题目');
  }
});

test('my cards lists Xingce flashcards and opens the source question editor',async({page})=>{
  await loginAsAdmin(page);
  const source='card-source-question';
  const deck='真题练习/数量关系/方程比例与应用/错题';
  const card=(id:string,front:string,back:string,source_question_id:string|null)=>({
    id,source_question_id,source_file:`${deck}/cards.md`,source_order:0,deck_path:deck,
    front_markdown:front,back_markdown:back,tags:[],active:true,due_at_ms:null,last_reviewed_at_ms:null,
    state:'new',review_count:0,lapse_count:0,is_new:true,
  });
  const existing=[card('card-xingce-1','第一张卡片','第一张卡片背面',source),card('card-xingce-2','第二张卡片','第二张卡片背面',source)];
  let cardRequests=0;
  await page.route('**/api/v1/study/cards**',route=>{cardRequests+=1;return route.fulfill({json:{cards:[
    ...existing,
    {...card('card-shenlun','申论卡片','申论卡片背面',null),deck_path:'真题练习/申论/归纳概括/错题',source_file:'真题练习/申论/归纳概括/错题/cards.md'},
    {...card('card-shenlun-2','申论卡片二','申论卡片二背面',null),deck_path:'用户整理/申论/归纳概括',source_file:'用户整理/申论/归纳概括/cards.md'},
    {...card('card-manual','手工卡片','手工卡片背面',null),deck_path:'用户整理/数量关系/方程比例与应用',source_file:'用户整理/数量关系/方程比例与应用/cards.md'},
  ]}})});
  await page.route(`**/api/v1/question-bank/practice/questions/${source}`,route=>route.fulfill({json:{
    item:{question_id:source,paper_id:'card-paper',paper_title:'行测卡片来源卷',year:2026,region:'国考',number:4,module:'数量关系',category:'方程比例与应用'},
    question:{id:source,number:4,stem:'某工程由甲乙合作完成。',options_markdown:'- A. 10\n- B. 12',materials:['M1'],module:'数量关系',category:'方程比例与应用',topics:[],answer_type:'single',question_type:'choice',grading_mode:'exact',max_score:1},
    materials:[{id:'M1',name:'材料',markdown:'甲乙合作完成一项工程。'}],correct_answer:['B'],explanation:'先算效率。',correct_count:0,wrong_count:0,
  }}));
  await page.route(`**/api/v1/question-bank/practice/questions/${source}/flashcard-preview`,route=>route.fulfill({json:{
    eligible:true,reason:null,front:'某工程由甲乙合作完成。',correct_answer:['B'],recommended_target:null,
    allowed_targets:[],requires_type_confirmation:false,source_revision:'card-source-v1',
    existing:existing.map(value=>({card_id:value.id,deck_path:value.deck_path,source_file:value.source_file,front:value.front_markdown,back:value.back_markdown,mode:'practice'})),
  }}));
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-cards-count')).toHaveText('2');
  const [cardsCountColor, cardsTabColor] = await page.getByTestId('question-bank-cards-count').evaluate(element => {
    const tab = element.closest('button')!;
    return [getComputedStyle(element).color, getComputedStyle(tab).color];
  });
  expect(cardsCountColor).not.toBe(cardsTabColor);
  await page.getByTestId('question-bank-collection-cards').click();
  const selectedCardsCountColor = await page.getByTestId('question-bank-cards-count')
    .evaluate(element => getComputedStyle(element).color);
  expect(selectedCardsCountColor).toBe(cardsCountColor);
  const cards=page.getByTestId('question-bank-card-item');
  await expect(cards).toHaveCount(2);
  await expect(page.getByTestId('question-bank-collection-module').filter({hasText:'数量关系'})).toBeVisible();
  await expect(page.getByTestId('question-bank-cards')).not.toContainText('申论');
  await expect(page.getByTestId('question-bank-cards')).not.toContainText('手工卡片');
  const requestsAfterLoad=cardRequests;
  await page.getByTestId('question-bank-collection-favorite').click();
  await page.getByTestId('question-bank-collection-cards').click();
  await expect(page.getByTestId('question-bank-card-item')).toHaveCount(2);
  expect(cardRequests).toBe(requestsAfterLoad);
  await cards.filter({hasText:'第二张卡片'}).click();
  await expect(page.getByTestId('collection-review')).toBeVisible();
  await expect(page.getByTestId('collection-review-material-pane')).toBeVisible();
  await expect(page.getByTestId('collection-review-flashcard-pane')).toBeVisible();
  await expect(page.getByTestId('favorite-flashcard-editor-back')).toHaveValue('第二张卡片背面');
  await expect(page.getByTestId('collection-review-scope')).toContainText('第 2/2 题');
  await page.getByRole('button', { name: '上一题' }).click();
  await expect(page.getByTestId('favorite-flashcard-editor-back')).toHaveValue('第一张卡片背面');
  await page.getByRole('button', { name: '下一题' }).click();
  await expect(page.getByTestId('favorite-flashcard-editor-back')).toHaveValue('第二张卡片背面');
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-number').nth(0).click();
  await expect(page.getByTestId('favorite-flashcard-editor-back')).toHaveValue('第一张卡片背面');
  await page.getByTestId('training-answer-sheet-toggle').click();
  await page.getByTestId('question-bank-practice-number').nth(1).click();
  await page.getByTestId('favorite-flashcard-editor-back').fill('未保存的编辑');
  await expect(page.getByRole('button', { name: '上一题' })).toBeDisabled();
  await expect(page.getByTestId('collection-review').getByTestId('calculation-scratchpad-toggle')).toBeVisible();
});

test('question bank practice mutations enforce admin, origin and CSRF',async({request})=>{
  const panel=await request.get('/api/v1/panel');
  const csrf=(await panel.json() as {csrf_token:string}).csrf_token;
  const body={mode:'paper',paper_id:null,module:null};
  const noAuth=await request.post('/api/v1/question-bank/practice/sessions',{
    headers:{Origin:webOrigin,'X-HYZ-CSRF':csrf},data:body,
  });
  expect(noAuth.status()).toBe(401);
  const invalidCsrf=await request.post('/api/v1/question-bank/practice/sessions',{
    headers:{Origin:webOrigin,'X-HYZ-CSRF':'invalid'},data:body,
  });
  expect(invalidCsrf.status()).toBe(403);
  const read=await request.get('/api/v1/question-bank/practice/history');
  expect(read.status()).toBe(401);
  expect((await request.get('/api/v1/question-bank/practice/questions/019a3d8e-1b32-7a60-9c83-f34217a6e210')).status()).toBe(401);
  expect((await request.get('/api/v1/question-bank/practice/questions?mode=favorite')).status()).toBe(401);
});

test('question bank module drill filters real categories across original papers',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').first()).toBeVisible();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('question-bank-cross-category').filter({hasText:'比重'}).click();
  await expect(page.getByTestId('question-bank-cross-category').filter({hasText:'比重'})).toHaveAttribute('aria-pressed','true');
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('第 1/3 题');
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await openSheet(page);
  // The 比重 category selects the whole M1 material group, so its 增长 sibling
  // stays with it; the second paper contributes one solo question.
  const numbers=page.getByTestId('question-bank-practice-number');
  await expect(numbers).toHaveCount(3);
  await expect(numbers.filter({hasText:'12'})).toHaveCount(1);
  await expect(numbers.filter({hasText:'26'})).toHaveCount(1);
  await expect(numbers.filter({hasText:'27'})).toHaveCount(1);
  const sheetOrder=await numbers.allTextContents();
  expect(sheetOrder.indexOf('26')).toBeLessThan(sheetOrder.indexOf('27'));
  await numbers.filter({hasText:'12'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-workspace')).toContainText('第二份跨卷模块练习卷');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('第二份试卷的比重题');
  await openSheet(page);
  await numbers.filter({hasText:'26'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).not.toContainText('原卷第');
  await expect(page.getByTestId('question-bank-practice-stem')).toContainText('材料中的正确选项');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
  await openSheet(page);
  await numbers.filter({hasText:'27'}).click();
  await expect(page.getByTestId('question-bank-practice-scope')).toContainText('资料分析 · 增长');
  await expect(page.getByTestId('question-bank-practice-material-pane')).toContainText('两道题共用材料');
});

test('switching training modes preserves the module selection',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-calculation').click();
  const requests:string[]=[];
  const record=(request:import('@playwright/test').Request)=>requests.push(new URL(request.url()).pathname);
  page.on('request',record);
  const version=page.waitForResponse(response=>response.url().includes('/question-bank/data-version'));
  await page.getByTestId('training-mode-question-bank-module').click();
  await version;
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.evaluate(()=>new Promise(requestAnimationFrame));
  page.off('request',record);
  expect(requests.filter(path=>path.endsWith('/papers')||path.endsWith('/practice/catalog')||path.endsWith('/practice/history'))).toEqual([]);
});

test('starting a cached drill waits for the current version check',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-calculation').click();
  let release!:()=>void;
  const gate=new Promise<void>(resolve=>{release=resolve});
  let arrived!:()=>void;
  const requested=new Promise<void>(resolve=>{arrived=resolve});
  await page.route('**/api/v1/question-bank/data-version',async route=>{arrived();await gate;await route.continue()});
  await page.getByTestId('training-mode-question-bank-module').click();
  await requested;
  await expect(page.getByTestId('question-bank-practice-start')).toBeDisabled();
  release();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
});

test('version check failures refresh data and clear after recovery',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toBeVisible();
  await page.getByTestId('training-mode-calculation').click();
  await page.route('**/api/v1/question-bank/data-version',route=>route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({error:{code:'unavailable',message:'版本不可用'}})}));
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-error')).toBeVisible();
  await page.unroute('**/api/v1/question-bank/data-version');
  await page.getByTestId('training-mode-calculation').click();
  const version=page.waitForResponse(response=>response.url().includes('/question-bank/data-version')&&response.ok());
  await page.getByTestId('training-mode-question-bank-module').click();
  await version;
  await expect(page.getByTestId('question-bank-error')).toHaveCount(0);
});

test('switching between module and paper retains both selections',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
});

test('returning to module training updates progress from another device',async({page,browser,request})=>{
  expect((await request.put(`${harnessOrigin}/reset-question-bank`)).ok()).toBeTruthy();
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('0/3');
  await page.getByTestId('training-mode-calculation').click();
  const other=await browser.newContext();
  try {
    await other.addCookies(await page.context().cookies());
    const second=await other.newPage();
    await second.goto(`${webOrigin}/#/study/training`);
    await second.getByTestId('training-mode-question-bank-module').click();
    await second.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
    await second.getByTestId('question-bank-practice-start').click();
    await expect(second.getByTestId('question-bank-practice-workspace')).toBeVisible();
    const saved=second.waitForResponse(response=>response.url().includes('/answers/')&&response.request().method()==='POST');
    await second.getByTestId('question-bank-practice-choice').first().click();
    await saved;
    await second.getByTestId('question-bank-practice-submit').click();
    if(await second.getByTestId('question-bank-submit-confirm').isVisible()) await second.getByRole('button',{name:'确认交卷'}).click();
    await expect(second.getByTestId('question-bank-practice-score').last()).toBeVisible();
    await page.getByTestId('training-mode-question-bank-module').click();
    await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('1/3');
  } finally { await other.close(); }
});

test('returning to module training picks up settings changed on another device',async({page,browser})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('training-mode-calculation').click();
  const other=await browser.newContext();
  try {
    await other.addCookies(await page.context().cookies());
    const second=await other.newPage();
    await second.goto(`${webOrigin}/#/study/training`);
    await second.getByTestId('training-mode-question-bank-module').click();
    await second.getByTestId('module-drill-settings-open').click();
    await second.getByTestId('module-filter-count').filter({hasText:'10题'}).click();
    await second.getByTestId('module-filter-save').click();
    await expect(second.getByTestId('module-drill-settings')).toBeHidden();
    await page.getByTestId('training-mode-question-bank-module').click();
    await expect(page.getByTestId('module-drill-filter-summary')).toContainText('10 题');
    await expect(page.getByTestId('question-bank-practice-start')).toBeEnabled();
  } finally {
    await other.close();
  }
});

test('module training counts and selects cross-paper duplicates once',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  const module=page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'});
  await expect(module).toContainText('/3');
  await module.click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('1/3');
  await openSheet(page);
  await expect(page.getByTestId('question-bank-practice-number')).toHaveCount(3);
});

test('module filter settings are shared across browser contexts',async({page,browser})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('module-drill-settings-open').click();
  await page.getByTestId('module-filter-count').filter({hasText:'10题'}).click();
  await page.getByTestId('module-filter-save').click();
  await expect(page.getByTestId('module-drill-settings')).toBeHidden();
  const context=await browser.newContext();
  try {
    await context.addCookies(await page.context().cookies());
    const other=await context.newPage();
    await other.goto(`${webOrigin}/#/study/training`);
    await other.getByTestId('training-mode-question-bank-module').click();
    await expect(other.getByTestId('module-drill-filter-summary')).toContainText('10 题');
  } finally {
    await context.close();
  }
});

test('module filter save errors remain visible in the settings dialog',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await page.getByTestId('module-drill-settings-open').click();
  await page.route('**/api/v1/question-bank/module-settings',async route=>{
    if (route.request().method()==='POST') await route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({error:{code:'unavailable',message:'保存失败'}})});
    else await route.continue();
  });
  await page.getByTestId('module-filter-save').click();
  await expect(page.getByTestId('module-drill-settings').getByRole('alert')).toBeVisible();
  await expect(page.getByTestId('module-drill-settings')).toBeVisible();
});

test('module filter settings persist, apply server-side, and reflect answered/total progress',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('/3');
  await page.getByTestId('module-drill-settings-open').click();
  await expect(page.getByTestId('module-drill-settings')).toBeVisible();
  await page.getByTestId('module-filter-custom-year-toggle').click();
  await page.getByTestId('module-filter-year-min').fill('2018');
  await page.getByTestId('module-filter-year-max').fill('2018');
  await page.getByTestId('module-filter-region').filter({hasText:'广东'}).click();
  await page.getByTestId('module-filter-count').filter({hasText:'10题'}).click();
  await page.getByTestId('module-filter-save').click();
  await expect(page.getByTestId('module-drill-settings')).toBeHidden();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('/1');
  await page.reload();
  await page.getByTestId('training-mode-question-bank-module').click();
  await expect(page.getByTestId('module-drill-filter-summary')).toContainText('2018');
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('/1');
  await page.getByTestId('module-drill-settings-open').click();
  await expect(page.getByTestId('module-filter-custom-year-toggle')).toHaveAttribute('aria-pressed','true');
  await expect(page.getByTestId('module-filter-year-min')).toHaveValue('2018');
  await expect(page.getByTestId('module-filter-year-max')).toHaveValue('2018');
  await expect(page.getByTestId('module-filter-region').filter({hasText:'广东'})).toHaveAttribute('aria-pressed','true');
  await page.getByRole('button',{name:'← 返回'}).click();
  await page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'}).click();
  await page.getByTestId('question-bank-practice-start').click();
  await expect(page.getByTestId('question-bank-practice-workspace')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-workspace')).toContainText('第二份跨卷模块练习卷');
  await expect(page.getByTestId('training-answer-sheet-toggle')).toContainText('1/1');
  const answerSaved=page.waitForResponse(response=>response.url().includes('/answers/')&&response.request().method()==='POST');
  await page.getByTestId('question-bank-practice-choice').first().click();
  await answerSaved;
  await page.getByTestId('question-bank-practice-submit').click();
  await expect(page.getByTestId('question-bank-practice-score').last()).toBeVisible();
  const today=await page.evaluate(async()=>{
    const response=await fetch('/api/v1/study/training-time/stats');
    if(!response.ok) throw new Error(`statistics failed: ${response.status}`);
    return (await response.json()).days.at(-1);
  });
  expect(today.question_bank.modules['资料分析']['比重']).toBeGreaterThanOrEqual(1);
  await page.getByRole('button',{name:/← 返回训练/}).click();
  await expect(page.getByTestId('question-bank-practice-setup')).toBeVisible();
  await expect(page.getByTestId('question-bank-cross-module').filter({hasText:'资料分析'})).toContainText('1/1');
});
test('swipe follows the finger before release and reveals the neighbouring page',async({page})=>{
  await loginAsAdmin(page);
  await page.goto('/#/study/training');
  await page.emulateMedia({reducedMotion:'no-preference'});
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  const surface=page.getByTestId('training-swipe-surface');
  const heading=page.getByTestId('question-bank-practice-heading');
  const sheet=page.getByTestId('training-answer-sheet-toggle');
  const stem=page.getByTestId('question-bank-practice-stem');
  const outgoing=page.locator('[data-page-turn="out"]');
  await expect(sheet).toContainText('1/3');

  // Keep the finger down: the pages must already be sliding and both must be readable.
  const firstDrag=await surface.evaluate((node)=>{
    const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'touch',pointerId:17,isPrimary:true,clientX:x,clientY:230
    }));
    emit('pointerdown',500);emit('pointermove',460);emit('pointermove',400);emit('pointermove',300);
    const out=document.querySelector('[data-page-turn="out"]') as HTMLElement | null;
    const rect=out?.getBoundingClientRect();
    return out ? {
      text:out.textContent ?? '',
      direction:out.getAttribute('data-direction'),
      transform:out.style.transform,
      animations:out.getAnimations().length,
      duplicateTestIds:out.querySelectorAll('[data-testid]').length,
      left:rect?.left ?? 0,
      right:rect?.right ?? 0,
    } : null;
  });
  expect(firstDrag).not.toBeNull();
  expect(firstDrag?.text).toContain('材料中的正确选项');
  expect(firstDrag?.direction).toBe('next');
  expect(firstDrag?.transform).toContain('translateX(-200px)');
  expect(firstDrag?.animations).toBe(0);
  expect(firstDrag?.duplicateTestIds).toBe(0);
  expect(firstDrag!.left).toBeLessThan(0);
  expect(firstDrag!.right).toBeGreaterThan(0);
  await expect(heading).toContainText('第 2/3 题');
  await expect(sheet).toContainText('2/3');
  await expect(stem).toContainText('第二道题');
  await expect(outgoing).toHaveCount(1);

  // Releasing past the threshold finishes the turn.
  await surface.evaluate((node)=>{
    node.dispatchEvent(new PointerEvent('pointerup',{
      bubbles:true,pointerType:'touch',pointerId:17,isPrimary:true,clientX:300,clientY:230
    }));
  });
  await expect(outgoing).toHaveCount(0,{timeout:2000});
  await expect(heading).toContainText('第 2/3 题');
  await expect(stem).toHaveCount(1);
  await expect(stem).toContainText('第二道题');

  // A short drag previews the next page, then snaps back when released.
  await surface.evaluate((node)=>{
    const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'touch',pointerId:18,isPrimary:true,clientX:x,clientY:230
    }));
    emit('pointerdown',500);emit('pointermove',470);emit('pointermove',450);
  });
  await expect(heading).toContainText('第 3/3 题');
  await expect(stem).toContainText('这是一道判断推理题');
  const cancelDrag=await surface.evaluate(()=>document.querySelector('[data-page-turn="out"]')?.textContent ?? '');
  expect(cancelDrag).toContain('第二道题');
  await surface.evaluate((node)=>{
    node.dispatchEvent(new PointerEvent('pointerup',{
      bubbles:true,pointerType:'touch',pointerId:18,isPrimary:true,clientX:450,clientY:230
    }));
  });
  await expect(heading).toContainText('第 2/3 题');
  await expect(outgoing).toHaveCount(0,{timeout:2000});
  await expect(stem).toContainText('第二道题');

  // Pen drags follow the same live path.
  await surface.evaluate((node)=>{
    const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'pen',pointerId:19,isPrimary:true,clientX:x,clientY:230
    }));
    emit('pointerdown',250);emit('pointermove',350);emit('pointermove',450);
    const out=document.querySelector('[data-page-turn="out"]') as HTMLElement | null;
    return out ? {
      text:out.textContent ?? '',
      direction:out.getAttribute('data-direction'),
      transform:out.style.transform,
    } : null;
  }).then((penDrag)=>{
    expect(penDrag).not.toBeNull();
    expect(penDrag?.text).toContain('第二道题');
    expect(penDrag?.direction).toBe('previous');
    expect(penDrag?.transform).toContain('translateX(200px)');
  });
  await expect(heading).toContainText('第 1/3 题');
  await expect(stem).toContainText('材料中的正确选项');
  await surface.evaluate((node)=>{
    node.dispatchEvent(new PointerEvent('pointerup',{
      bubbles:true,pointerType:'pen',pointerId:19,isPrimary:true,clientX:450,clientY:230
    }));
  });
  await expect(outgoing).toHaveCount(0,{timeout:2000});
  await expect(sheet).toContainText('1/3');

  // A vertical gesture never starts a page turn.
  await surface.evaluate((node)=>{
    const emit=(type:string,x:number,y:number)=>node.dispatchEvent(new PointerEvent(type,{
      bubbles:true,pointerType:'touch',pointerId:20,isPrimary:true,clientX:x,clientY:y
    }));
    emit('pointerdown',300,180);emit('pointermove',304,270);emit('pointerup',300,380);
  });
  await expect(outgoing).toHaveCount(0);
  await expect(sheet).toContainText('1/3');
  const overflow=await page.evaluate(()=>document.documentElement.scrollHeight-document.documentElement.clientHeight);
  expect(overflow).toBeLessThanOrEqual(1);
});

test('practice header keeps one row on tablets and reveals the full title on tap', async ({page}) => {
  await page.setViewportSize({width:1024,height:768});
  await openQuestionBankPaper(page);
  const header = page.getByTestId('question-bank-practice-workspace').locator('header');
  const title = page.getByTestId('question-bank-practice-title');
  const bars = page.getByTestId('study-goal-bars');
  await expect(title).toHaveAttribute('aria-expanded','false');
  const before = (await header.boundingBox())!.height;
  const titleBox = (await title.boundingBox())!;
  const barsBox = (await bars.boundingBox())!;
  expect(Math.abs((titleBox.y + titleBox.height/2) - (barsBox.y + barsBox.height/2))).toBeLessThan(24);
  await title.click();
  await expect(page.getByTestId('question-bank-practice-title-popover')).toContainText('行测真题训练端到端样卷');
  expect(Math.abs((await header.boundingBox())!.height - before)).toBeLessThanOrEqual(1);
  await title.click();
  await expect(page.getByTestId('question-bank-practice-title-popover')).toHaveCount(0);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test('completed practice header actions stay inside a phone viewport', async ({page}) => {
  await page.setViewportSize({width:390,height:844});
  await openFixtureReview(page);
  const records = page.getByTestId('question-bank-practice-records');
  await expect(records).toBeVisible();
  const repeat = page.getByTestId('question-bank-practice-repeat');
  await expect(repeat).toBeVisible();
  for (const control of [records, repeat]) {
    const box = await control.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  }
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test('any original-paper review can create a category flashcard without first favoriting',async({page})=>{
  await openQuestionBankPaper(page);
  await expect(page.getByTestId('question-bank-practice-create-flashcard')).toHaveCount(0);
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  const create=page.getByTestId('question-bank-practice-create-flashcard');
  await expect(create).toBeVisible();
  await expect(create).toHaveAttribute('aria-label','为当前题目创建 Flashcard');
  await expect(create).toContainText('+');
  await expect(page.locator('.question-bank-pane-header').getByTestId('question-bank-practice-create-flashcard')).toBeVisible();
  await create.click();
  const pane=page.getByTestId('question-bank-practice-flashcard-pane');
  await expect(pane).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-heading')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-material-pane')).toBeVisible();
  const card=pane.getByTestId('favorite-flashcard-card');
  const form=card.getByTestId('favorite-flashcard-form');
  const borderBefore=await form.evaluate(node=>getComputedStyle(node).borderColor);
  await card.getByPlaceholder('写下解题思路或复习提示').focus();
  await expect.poll(()=>form.evaluate(node=>getComputedStyle(node).borderColor)).not.toBe(borderBefore);
  await page.setViewportSize({width:390,height:844});
  await expectNoHorizontalOverflow(page);
  await page.setViewportSize({width:1280,height:720});
  await expect(card.getByRole('button',{name:'创建并同步'})).toBeDisabled();
  await expect(card.getByTestId('favorite-flashcard-path')).toHaveText('将追加或创建：flashcards/真题练习/资料分析/比重/错题/cards.md');
  await card.getByLabel('确认写入所选目录').check();
  await card.getByRole('button',{name:'创建并同步'}).click();
  await expect(pane).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-create-flashcard')).toHaveCount(0);
  await expect(page.getByTestId('question-bank-practice-manage-flashcard')).toBeVisible();
  await expect(page.getByTestId('question-bank-practice-manage-flashcard')).toHaveAttribute('aria-label','管理当前题目的 1 张 Flashcard');
  await expect(page.getByTestId('question-bank-practice-manage-flashcard')).toContainText('1');
  await page.getByTestId('question-bank-practice-manage-flashcard').click();
  const editor=page.getByTestId('favorite-flashcard-editor');
  await expect(editor.getByTestId('favorite-flashcard-path')).toContainText('真题练习/资料分析/比重/错题/cards.md');
  await editor.getByTestId('favorite-flashcard-preview-toggle').click();
  await expect(editor.getByTestId('favorite-flashcard-front-preview')).toBeVisible();
  await page.getByRole('button',{name:'返回复盘'}).click();
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第');
});

for (const viewport of [{width:1280,height:720},{width:390,height:844}]) {
test(`review remembers each question and material scroll position at ${viewport.width}px`,async({page})=>{
  await page.setViewportSize(viewport);
  await openFixtureReview(page);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await page.evaluate(()=>document.styleSheets[0].insertRule('[data-testid="question-bank-practice-material-scroll"]::after, [data-testid="question-bank-practice-question-scroll"]::after { content: ""; display: block; height: 1600px; }'));
  const material=page.getByTestId('question-bank-practice-material-scroll');
  const question=page.getByTestId('question-bank-practice-question-scroll');
  await material.evaluate(node=>{node.scrollTop=180});
  await question.evaluate(node=>{node.scrollTop=270});
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(180);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(270);
  const swipe=async(direction:'next'|'previous')=>{
    await page.getByTestId('training-swipe-surface').evaluate((node,direction)=>{
      const [start,end]=direction==='next'?[500,250]:[250,500];
      const emit=(type:string,x:number)=>node.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerType:'touch',pointerId:17,isPrimary:true,clientX:x,clientY:230}));
      emit('pointerdown',start);emit('pointermove',end);emit('pointerup',end);
    },direction);
  };
  await swipe('next');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(0);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(0);
  await material.evaluate(node=>{node.scrollTop=90});
  await question.evaluate(node=>{node.scrollTop=130});
  await swipe('previous');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(180);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(270);
  await swipe('next');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 27 题');
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(90);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(130);
  await openSheet(page);
  await page.getByTestId('question-bank-practice-number').first().click();
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(180);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(270);
  await page.getByTestId('question-bank-practice-records').click();
  await page.getByRole('button',{name:'返回套卷'}).click();
  await expect.poll(()=>material.evaluate(node=>node.scrollTop)).toBe(0);
  await expect.poll(()=>question.evaluate(node=>node.scrollTop)).toBe(0);
});
}

test('review pane headers and all header buttons have equal sizes',async({page})=>{
  await page.setViewportSize({width:1366,height:900});
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:11,role:'assistant',content:'解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  const pane=page.getByTestId('question-bank-practice-question-pane');
  const left=page.getByTestId('question-bank-practice-material-pane').locator('.question-bank-pane-header');
  const right=pane.locator('.question-bank-pane-header');
  const leftBox=await left.boundingBox(), rightBox=await right.boundingBox();
  expect(Math.abs(leftBox!.height-rightBox!.height)).toBeLessThanOrEqual(1);
  const buttons=[pane.getByTestId('question-bank-favorite'),pane.getByTestId('question-bank-ai-tab'),pane.getByRole('button',{name:'更多解析操作'}),pane.getByTestId('calculation-scratchpad-toggle')];
  const sizes=await Promise.all(buttons.map(button=>button.boundingBox()));
  for (const size of sizes) {
    expect(Math.abs(size!.width-sizes[0]!.width)).toBeLessThanOrEqual(1);
    expect(Math.abs(size!.height-sizes[0]!.height)).toBeLessThanOrEqual(1);
  }
  const pen=pane.getByTestId('calculation-scratchpad-toggle');
  const penBox=await pen.boundingBox();
  const moreBox=await pane.getByRole('button',{name:'更多解析操作'}).boundingBox();
  const paneBox=await pane.boundingBox();
  expect(penBox!.x).toBeGreaterThanOrEqual(moreBox!.x+moreBox!.width);
  expect(penBox!.x+penBox!.width).toBeGreaterThanOrEqual(paneBox!.x+paneBox!.width-16);
  await pen.click();
  const clear=pane.getByTestId('calculation-scratchpad-clear');
  await expect(clear).toBeVisible();
  const expandedPenBox=await pen.boundingBox();
  const clearBox=await clear.boundingBox();
  expect(expandedPenBox!.x).toBeGreaterThan(clearBox!.x);
  await pen.click();
  await expect(clear).toHaveCount(0);
});

test('review split resizes horizontally on iPad with a mouse drag',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  await openFixtureReview(page);
  const split=page.getByTestId('question-bank-practice-split');
  const handle=split.getByTestId('training-split-handle');
  await expect(handle).toHaveAttribute('aria-orientation','vertical');
  const left=page.getByTestId('question-bank-practice-material-pane');
  const before=await left.boundingBox(), grip=await handle.boundingBox();
  if (!before || !grip) throw new Error('split geometry is missing');
  await page.mouse.move(grip.x+grip.width/2,grip.y+grip.height/2);
  await page.mouse.down();
  await page.mouse.move(grip.x+grip.width/2+100,grip.y+grip.height/2,{steps:5});
  await page.mouse.up();
  await expect.poll(async()=> (await left.boundingBox())!.width).toBeGreaterThan(before.width+60);
  const afterMouse=(await left.boundingBox())!.width;
  await page.clock.install();
  await handle.evaluate(node=>{const r=node.getBoundingClientRect();node.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'touch',pointerId:78,isPrimary:true,button:0,clientX:r.x+r.width/2,clientY:r.y+r.height/2}));});
  await page.clock.runFor(300);
  await handle.evaluate(node=>{const r=node.getBoundingClientRect();for(const type of ['pointermove','pointerup']) node.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerType:'touch',pointerId:78,isPrimary:true,button:0,clientX:r.x+r.width/2-60,clientY:r.y+r.height/2}));});
  await expect.poll(async()=> (await left.boundingBox())!.width).toBeLessThan(afterMouse-30);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
});

test('review split resizes vertically after a touch hold on a phone',async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await openFixtureReview(page);
  const split=page.getByTestId('question-bank-practice-split');
  const handle=split.getByTestId('training-split-handle');
  await expect(handle).toHaveAttribute('aria-orientation','horizontal');
  const top=page.getByTestId('question-bank-practice-material-pane');
  const before=await top.boundingBox();
  if (!before) throw new Error('top pane is missing');
  await page.clock.install();
  await handle.evaluate(node=>{const rect=node.getBoundingClientRect();node.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'touch',pointerId:77,isPrimary:true,button:0,clientX:rect.x+rect.width/2,clientY:rect.y+rect.height/2}));});
  await page.clock.runFor(300);
  await handle.evaluate(node=>{const rect=node.getBoundingClientRect();for(const type of ['pointermove','pointerup']) node.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerType:'touch',pointerId:77,isPrimary:true,button:0,clientX:rect.x+rect.width/2,clientY:rect.y+rect.height/2-70}));});
  await expect.poll(async()=> (await top.boundingBox())!.height).toBeLessThan(before.height-20);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
});

test('splitter keyboard controls persist separate horizontal and vertical ratios',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  await openFixtureReview(page);
  const handle=page.getByTestId('training-split-handle');
  await expect(handle).toHaveAttribute('aria-orientation','vertical');
  const initial=Number(await handle.getAttribute('aria-valuenow'));
  await handle.focus();
  await page.keyboard.press('ArrowLeft');
  await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeLessThan(initial);
  const horizontal=Number(await handle.getAttribute('aria-valuenow'));
  await page.getByRole('button',{name:'下一题',exact:true}).click();
  await page.getByRole('button',{name:'上一题',exact:true}).click();
  await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeCloseTo(horizontal,0);
  await page.setViewportSize({width:390,height:844});
  await expect(handle).toHaveAttribute('aria-orientation','horizontal');
  const vertical=Number(await handle.getAttribute('aria-valuenow'));
  await handle.focus();
  await page.keyboard.press('ArrowUp');
  await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeLessThan(vertical);
  const savedVertical=Number(await handle.getAttribute('aria-valuenow'));
  await page.setViewportSize({width:1133,height:744});
  await expect(handle).toHaveAttribute('aria-orientation','vertical');
  await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeCloseTo(horizontal,0);
  await handle.dblclick();
  await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBe(60);
  await page.setViewportSize({width:390,height:844});
  await expect(handle).toHaveAttribute('aria-orientation','horizontal');
  await expect.poll(async()=>Number(await handle.getAttribute('aria-valuenow'))).toBeCloseTo(savedVertical,0);
});

test('splitter cancels a short touch and clamps large drags',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  await openFixtureReview(page);
  const handle=page.getByTestId('training-split-handle');
  const left=page.getByTestId('question-bank-practice-material-pane');
  const right=page.getByTestId('question-bank-practice-question-pane');
  await expect(handle).toHaveAttribute('aria-orientation','vertical');
  const before=(await left.boundingBox())!.width;
  await page.clock.install();
  await handle.evaluate(node=>{const r=node.getBoundingClientRect();for(const [type,dx] of [['pointerdown',0],['pointermove',35]] as const) node.dispatchEvent(new PointerEvent(type,{bubbles:true,pointerType:'touch',pointerId:79,isPrimary:true,button:0,clientX:r.x+r.width/2+dx,clientY:r.y+r.height/2}));});
  await page.clock.runFor(300);
  await handle.evaluate(node=>{const r=node.getBoundingClientRect();node.dispatchEvent(new PointerEvent('pointerup',{bubbles:true,pointerType:'touch',pointerId:79,isPrimary:true,button:0,clientX:r.x+r.width/2+35,clientY:r.y+r.height/2}));});
  expect((await left.boundingBox())!.width).toBeCloseTo(before,0);
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
  const grip=(await handle.boundingBox())!;
  await page.mouse.move(grip.x+grip.width/2,grip.y+grip.height/2);
  await page.mouse.down();
  await page.mouse.move(grip.x+grip.width/2+4000,grip.y+grip.height/2);
  await page.mouse.up();
  expect((await right.boundingBox())!.width).toBeGreaterThanOrEqual(119);
  expect(Number(await handle.getAttribute('aria-valuenow'))).toBeLessThanOrEqual(Number(await handle.getAttribute('aria-valuemax')));
});

test('splitter works with the scratchpad open and keeps ink attached to the question',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  await openFixtureReview(page);
  const pen=page.getByTestId('calculation-scratchpad-toggle');
  await pen.click();
  const canvas=page.getByTestId('calculation-scratchpad-canvas');
  await expect(canvas).toBeVisible();
  const question=page.getByTestId('question-bank-practice-question-scroll');
  const q=(await question.boundingBox())!;
  const readInk=async()=>canvas.evaluate((node:HTMLCanvasElement)=>{
    const ctx=node.getContext('2d')!, pixels=ctx.getImageData(0,0,node.width,node.height).data;
    let min=node.width,max=-1; for(let y=0;y<node.height;y++) for(let x=0;x<node.width;x++) if(pixels[(y*node.width+x)*4+3]>0){min=Math.min(min,x);max=Math.max(max,x);}
    return {min,max,width:node.width};
  });
  await page.mouse.move(q.x+45,q.y+45); await page.mouse.down(); await page.mouse.move(q.x+85,q.y+70,{steps:5}); await page.mouse.up();
  await expect.poll(async()=> (await readInk()).max).toBeGreaterThan(0);
  const before=await readInk();
  const handle=page.getByTestId('training-split-handle'), grip=(await handle.boundingBox())!;
  await page.mouse.move(grip.x+grip.width/2,grip.y+grip.height/2); await page.mouse.down();
  await page.mouse.move(grip.x+grip.width/2+80,grip.y+grip.height/2,{steps:4}); await page.mouse.up();
  await expect.poll(async()=> (await readInk()).min).toBeGreaterThan(before.min+40);
  await expect(pen).toHaveAttribute('aria-label','关闭草稿');
  await expect(page.getByTestId('question-bank-practice-heading')).toContainText('原卷第 26 题');
});

test('phone splitter preserves the AI input focus layout and draft',async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  const pane=page.getByTestId('question-bank-practice-material-pane');
  const input=page.getByTestId('question-bank-ai-input');
  await input.fill('保留的追问草稿');
  await expect.poll(async()=> (await pane.boundingBox())!.height).toBeLessThanOrEqual(2);
  await input.blur();
  await expect.poll(async()=> (await pane.boundingBox())!.height).toBeGreaterThan(20);
  await expect(input).toHaveValue('保留的追问草稿');
  await expect(page.getByTestId('training-split-handle')).toHaveAttribute('aria-orientation','horizontal');
});

test('dragging the split keeps both pane scroll positions',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  await openFixtureReview(page);
  // Explicitly select the original review before measuring scroll. An AI
  // history response may otherwise auto-switch tabs mid-drag and replace
  // the question scroll element, invalidating the identity under test.
  await page.getByTestId('question-bank-original-tab').click();
  await expect(page.getByTestId('question-bank-original-tab')).toHaveAttribute('aria-pressed','true');
  const left=page.getByTestId('question-bank-practice-material-scroll');
  const right=page.getByTestId('question-bank-practice-question-scroll');
  await page.evaluate(()=>{
    const sheet=new CSSStyleSheet();
    sheet.replaceSync('[data-testid="question-bank-practice-material-scroll"]::after,[data-testid="question-bank-practice-question-scroll"]::after {content:"";display:block;height:1800px}');
    document.adoptedStyleSheets=[...document.adoptedStyleSheets,sheet];
  });
  await expect(page.getByTestId('training-split-handle')).toHaveAttribute('aria-orientation','vertical');
  await expect.poll(()=>left.evaluate(node=>node.scrollHeight-node.clientHeight)).toBeGreaterThan(1000);
  await expect.poll(()=>right.evaluate(node=>node.scrollHeight-node.clientHeight)).toBeGreaterThan(1000);
  await left.evaluate(node=>{node.scrollTop=240;});
  await right.evaluate(node=>{node.scrollTop=200;});
  await expect.poll(()=>left.evaluate(node=>node.scrollTop)).toBe(240);
  await expect.poll(()=>right.evaluate(node=>node.scrollTop)).toBe(200);
  const handle=page.getByTestId('training-split-handle'), grip=(await handle.boundingBox())!;
  await page.mouse.move(grip.x+grip.width/2,grip.y+grip.height/2); await page.mouse.down();
  await page.mouse.move(grip.x+grip.width/2+80,grip.y+grip.height/2,{steps:4}); await page.mouse.up();
  await expect.poll(()=>left.evaluate(node=>node.scrollTop)).toBe(240);
  await expect.poll(()=>right.evaluate(node=>node.scrollTop)).toBe(200);
});

test('split ratio is restored after reloading and opening another review',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  await openFixtureReview(page);
  const handle=page.getByTestId('training-split-handle');
  await handle.focus(); await page.keyboard.press('ArrowLeft'); await page.keyboard.press('ArrowLeft');
  const saved=Number(await handle.getAttribute('aria-valuenow'));
  expect(saved).toBeLessThan(60);
  await page.reload();
  await page.getByTestId('training-mode-question-bank-paper').click();
  await chooseFixturePaper(page);
  await page.getByTestId('question-bank-practice-start').click();
  await page.getByTestId('question-bank-practice-submit').click();
  await page.getByRole('button',{name:'确认交卷'}).click();
  await expect.poll(async()=>Number(await page.getByTestId('training-split-handle').getAttribute('aria-valuenow'))).toBeCloseTo(saved,0);
});


test('AI review scratchpad writes in the options pane and follows its scrolling',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  await page.addInitScript(()=>{ (window as any).EventSource=class extends EventTarget { onmessage=null; onerror=null; close() {} }; });
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:Array.from({length:100},(_,i)=>`解析第 ${i} 行`).join('\n\n'),created_at:1}]}});
  });
  await openFixtureReview(page);
  const messages=page.getByTestId('question-bank-ai-messages');
  await expect(messages).toBeVisible();
  await expect(messages).toContainText('解析第 99 行');
  await messages.evaluate(node=>{node.dispatchEvent(new WheelEvent('wheel',{bubbles:true,deltaY:-10000}));node.scrollTop=0;});
  await expect.poll(()=>messages.evaluate(node=>node.scrollTop)).toBe(0);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  const canvas=page.getByTestId('calculation-scratchpad-canvas');
  await expect(canvas).toBeVisible();
  const blank=await inkSnapshot(canvas), pane=(await messages.boundingBox())!;
  await page.mouse.move(pane.x+55,pane.y+180); await page.mouse.down();
  await page.mouse.move(pane.x+115,pane.y+200,{steps:6}); await page.mouse.up();
  await expect(page.getByTestId('calculation-scratchpad-undo')).toBeEnabled();
  await expect.poll(()=>inkSnapshot(canvas)).not.toBe(blank);
  const drawn=await inkSnapshot(canvas);
  const down=page.getByRole('button',{name:'题目向下滚动'});
  await expect(down).toBeEnabled();
  await down.click();
  await expect.poll(()=>messages.evaluate(node=>node.scrollTop)).toBe(32);
  await expect.poll(()=>inkSnapshot(canvas)).not.toBe(drawn);
  expect(await inkSnapshot(canvas)).not.toBe(blank);
  await page.getByRole('button',{name:'题目向上滚动'}).click();
  await expect.poll(()=>messages.evaluate(node=>node.scrollTop)).toBe(0);
  await expect.poll(()=>inkSnapshot(canvas)).toBe(drawn);
  const aiChoiceOffset=await page.getByTestId('question-bank-practice-choice').first().evaluate(node=>node.getBoundingClientRect().top-node.closest('[data-testid="question-bank-ai-messages"]')!.getBoundingClientRect().top);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await page.getByTestId('question-bank-original-tab').click();
  const originalChoiceOffset=await page.getByTestId('question-bank-practice-choice').first().evaluate(node=>node.getBoundingClientRect().top-node.closest('[data-testid="question-bank-practice-question-scroll"]')!.getBoundingClientRect().top);
  expect(Math.abs(aiChoiceOffset-originalChoiceOffset)).toBeLessThanOrEqual(1);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect.poll(()=>inkSnapshot(canvas)).not.toBe(blank);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await page.getByTestId('question-bank-ai-tab').click();
  await expect(messages).toContainText('解析第 99 行');
  await messages.evaluate(node=>{node.dispatchEvent(new WheelEvent('wheel',{bubbles:true,deltaY:-10000}));node.scrollTop=0;});
  await expect.poll(()=>messages.evaluate(node=>node.scrollTop)).toBe(0);
  await page.getByTestId('calculation-scratchpad-toggle').click();
  await expect.poll(()=>inkSnapshot(canvas)).not.toBe(blank);
});

test('open scratchpad follows a live switch from original to AI review',async({page})=>{
  await page.setViewportSize({width:1133,height:744});
  let release!:()=>void;
  const gate=new Promise<void>(resolve=>{release=resolve;});
  let requests=0;
  await page.route('**/api/v1/question-bank/practice/questions/*/ai',async route=>{
    requests++;
    await gate;
    const question_id=new URL(route.request().url()).pathname.split('/').at(-2)!;
    await route.fulfill({json:{question_id,status:'complete',error:null,messages:[{id:1,role:'assistant',content:'简短解析',created_at:1}]}});
  });
  await openFixtureReview(page);
  await expect.poll(()=>requests).toBeGreaterThan(0);
  const original=page.getByTestId('question-bank-practice-question-scroll');
  await expect(original).toBeVisible();
  await page.evaluate(()=>{
    const sheet=new CSSStyleSheet();
    sheet.replaceSync('[data-testid="question-bank-practice-question-scroll"]::after {content:"";display:block;height:1400px}');
    document.adoptedStyleSheets=[...document.adoptedStyleSheets,sheet];
  });
  await page.getByTestId('calculation-scratchpad-toggle').click();
  const canvas=page.getByTestId('calculation-scratchpad-canvas');
  const blank=await inkSnapshot(canvas), pane=(await original.boundingBox())!;
  await page.mouse.move(pane.x+55,pane.y+180); await page.mouse.down();
  await page.mouse.move(pane.x+115,pane.y+200,{steps:6}); await page.mouse.up();
  await expect.poll(()=>inkSnapshot(canvas)).not.toBe(blank);
  const drawn=await inkSnapshot(canvas);
  const inkPosition=()=>canvas.evaluate((node:HTMLCanvasElement)=>{
    const pixels=node.getContext('2d')!.getImageData(0,0,node.width,node.height).data;
    let x=node.width,y=node.height;
    for(let i=3;i<pixels.length;i+=4) if(pixels[i]) { const point=(i-3)/4; x=Math.min(x,point%node.width); y=Math.min(y,Math.floor(point/node.width)); }
    return {x,y,scale:node.width/node.clientWidth};
  });
  const originalInk=await inkPosition();
  await expect.poll(()=>original.evaluate(node=>node.scrollHeight-node.clientHeight)).toBeGreaterThan(500);
  await original.evaluate(node=>{node.scrollTop=80;});
  await expect.poll(()=>original.evaluate(node=>node.scrollTop)).toBe(80);
  await expect.poll(()=>inkSnapshot(canvas)).not.toBe(drawn);
  release();
  const aiMessages=page.getByTestId('question-bank-ai-messages');
  await expect(aiMessages).toBeVisible();
  await expect.poll(async()=>{
    const position=await inkPosition();
    const scroll=await aiMessages.evaluate(node=>node.scrollTop);
    return Math.max(Math.abs(position.x-originalInk.x),Math.abs(position.y+scroll*position.scale-originalInk.y));
  }).toBeLessThanOrEqual(2);
  await expect(page.getByTestId('calculation-scratchpad-toggle')).toHaveAttribute('aria-label','关闭草稿');
});
