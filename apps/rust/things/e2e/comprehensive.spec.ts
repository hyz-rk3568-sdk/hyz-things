import { expect, test } from "@playwright/test";
import { expectNoHorizontalOverflow, resetHarness } from "./fixtures";

test.beforeEach(async ({ request }) => { await resetHarness(request); });

test("comprehensive generates with zero selected wrong categories", async ({ page }) => {
  let submittedSkills: string[] | null = null;
  await page.route("**/api/v1/study/data-analysis/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    let result: unknown = {};
    if (path.endsWith("/status")) {
      result = {configured:true,model:"mock-compatible-model",cached_sources:1,verified_groups:0,
        available_skills:["乘积增长率"],preparing_job_id:null};
    } else if (path.endsWith("/unfinished") || path.endsWith("/history")) {
      result = [];
    } else if (path.endsWith("/prepare") && route.request().method() === "POST") {
      submittedSkills = (route.request().postDataJSON() as {manual_skills:string[]}).manual_skills;
      result = {job_id:"zero-selection-task"};
    } else if (path.endsWith("/prepare/zero-selection-task")) {
      result = {state:"ready",verified:20,message:"出题完成",run_id:"zero-selection-run"};
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(result)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-setup")).toContainText("错题保底分类（可选）");
  await expect(page.getByTestId("analysis-setup")).toContainText("可不选");
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await page.getByTestId("analysis-start").click();
  await expect.poll(() => submittedSkills).toEqual([]);
  await expect(page.getByTestId("analysis-ready")).toBeVisible();
});

test("iPad comprehensive 20-question flow with answer save and submit", async ({ page }) => {
  await page.setViewportSize({width: 1180,height: 820});
  const started = Date.now();
  const answers: (number | null)[] = Array(20).fill(null);
  let paused=false;
  let submittedResult:unknown={};
  const retryAnswers:(number|null)[]=Array(20).fill(null);
  let repeatCreated=0;
  let manualSkillsSubmitted:string[]=[];
  const topics = ["宏观经济","农业","工业能源","交通物流与服务"];
  const groups = Array.from({length:4},(_,g)=>({
    material: {

      topic: topics[g],

      text: Array.from({length:30},()=> "2025年某项业务量为1200亿元，同比增长15.2%。\n\n其中地区一为480亿元，地区二为720亿元。").join("\n\n"),

    },
    questions: Array.from({length:5},(_,j)=>({
      question:"第"+String(g*5+j+1)+"题：上一年的业务量约为多少？",
      options:["1000亿元","1042亿元","1100亿元","1200亿元"],
      skill:"基期量",
    })),
  }));
  await page.route("**/api/v1/study/data-analysis/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    let result:unknown = {};
    if (path.endsWith("/unfinished")) {
      result=[];
    } else if (path.endsWith("/history")) {
      result=submittedResult && Object.keys(submittedResult as object).length>0 ? [
        {id:"fixture-run",started_at_ms:started,ended_at_ms:started+30000,elapsed_ms:30000,correct:1,total:20,topics:["经济","农业","工业","服务业"]}
      ] : [];
    } else if (path.endsWith("/status")) {
      result={configured:true,model:"mock-compatible-model",cached_sources:1,verified_groups:4,available_skills:["乘积增长率","混合增长率"],preparing_job_id:null};
    } else if (path.endsWith("/prepare") && route.request().method()==="POST") {
      manualSkillsSubmitted=(route.request().postDataJSON() as {manual_skills:string[]}).manual_skills;
      result={job_id:"fixture-task"};
    } else if (path.endsWith("/prepare/fixture-task")) {
      result={state:"ready",verified:20,message:"已完成20题校验",run_id:"fixture-run"};
    } else if (path.endsWith("/runs/fixture-run/retry") && route.request().method()==="POST") {
      repeatCreated++;
      result={run_id:"fixture-retry"};
    } else if (path.endsWith("/runs/fixture-retry/claim") && route.request().method()==="POST") {
      result={run:{id:"fixture-retry",status:"active",current_question:0,started_at_ms:Date.now(),deadline_at_ms:Date.now()+1200000,paused_remaining_ms:null,duration_seconds:1200,groups,answers:retryAnswers},control_token:"retry-token"};
    } else if (path.endsWith("/runs/fixture-retry") && route.request().method()==="GET") {
      result={id:"fixture-retry",status:"active",started_at_ms:Date.now(),deadline_at_ms:Date.now()+1200000,paused_remaining_ms:null,duration_seconds:1200,groups,answers:retryAnswers};
    } else if (path.includes("/runs/fixture-retry/answers/")) {
      const number=Number(path.split("/").pop()!);
      const data=route.request().postDataJSON() as {choice:number};
      retryAnswers[number-1]=data.choice;
      result={saved:true};
    } else if (path.endsWith("/runs/fixture-retry/submit")) {
      result={id:"fixture-retry",correct:0,total:20,materials:groups.map(g=>g.material),
        questions:Array.from({length:20},(_,i)=>({
          number:i+1,question:groups[Math.floor(i/5)].questions[i%5].question,
          options:["1000亿元","1042亿元","1100亿元","1200亿元"],chosen:retryAnswers[i],
          correct_index:0,answer:"1000亿元",explanation:"基期量为现期量除以增长因子",
          topic:topics[Math.floor(i/5)],skill:"基期量",evidence:"业务量1200亿元",
        }))};
    } else if (path.endsWith("/runs/fixture-run/claim") && route.request().method()==="POST") {
      result={run:{id:"fixture-run",status:"active",current_question:0,started_at_ms:started,deadline_at_ms:paused?null:started+1200000,paused_remaining_ms:paused?1100000:null,duration_seconds:1200,groups,answers},control_token:"ipad-token"};
    } else if (path.endsWith("/runs/fixture-run") && route.request().method()==="GET") {
      result={id:"fixture-run",status:"active",started_at_ms:started,deadline_at_ms:paused?null:started+1200000,paused_remaining_ms:paused?1100000:null,duration_seconds:1200,groups,answers};
    } else if (path.endsWith("/position") && route.request().method()==="POST") {
      result={saved:true};
    } else if (path.endsWith("/pause") && route.request().method()==="POST") {
      paused=!paused;
      result={id:"fixture-run",status:"active",started_at_ms:started,deadline_at_ms:paused?null:started+1200000,paused_remaining_ms:paused?1100000:null,duration_seconds:1200,groups,answers};
    } else if (path.endsWith("/runs/fixture-run/result")) {
      result=submittedResult;
    } else if (path.includes("/answers/")) {
      const number=Number(path.split("/").pop()!);
      const data=route.request().postDataJSON() as {choice:number};
      answers[number-1]=data.choice;
      result={saved:true};
    } else if (path.endsWith("/submit")) {
      result={id:"fixture-run",correct:1,total:20,materials:groups.map(g=>g.material),
        questions:Array.from({length:20},(_,i)=>({
          number:i+1,question:groups[Math.floor(i/5)].questions[i%5].question,
          options:["1000亿元","1042亿元","1100亿元","1200亿元"],
          chosen:answers[i],correct_index:0,answer:"1000亿元",
          explanation:"基期量为现期量除以增长因子",topic:topics[Math.floor(i/5)],
          skill:"基期量",evidence:"2025年某项业务量为1200亿元，同比增长15.2%。",
        }))};
      submittedResult=result;
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(result)});
  });
  await page.goto("/#/study/training");
  // The retired number-locating mode must not reappear in the workspace.
  await expect(page.getByTestId("training-mode-locating")).toHaveCount(0);
  await expect(page.getByRole("group",{name:"训练模式"}).getByRole("button")).toHaveCount(3);
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-setup")).toBeVisible();
  await expect(page.getByTestId("analysis-setup")).toContainText("20分钟");
  await expect(page.getByTestId("analysis-setup")).toContainText("mock-compatible-model");
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await page.getByTestId("analysis-skill-expand").click();
  await page.getByTestId("analysis-skill-options").getByRole("button",{name:"乘积增长率"}).click();
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await page.getByTestId("analysis-start").click();
  expect(manualSkillsSubmitted).toEqual(["乘积增长率"]);
  await expect(page.getByTestId("analysis-ready")).toBeVisible();
  await expect(page.getByTestId("analysis-clock")).toHaveCount(0);
  await page.getByTestId("analysis-enter").click();
  await expect(page.getByTestId("analysis-run")).toBeVisible();
  // Starting a comprehensive paper removes the parent mode navigation,
  // fixes the paper to the visible viewport and keeps document scroll locked.
  await expect(page.getByRole("group",{name:"训练模式"})).toHaveCount(0);
  await expect(page.getByTestId("analysis-exit")).toBeVisible();
  expect(await page.getByTestId("analysis-run").evaluate(el=>{
    const rect=el.getBoundingClientRect();
    return Math.max(Math.abs(rect.top),Math.abs(rect.left),Math.abs(rect.right-window.innerWidth),Math.abs(rect.bottom-window.innerHeight));
  })).toBeLessThanOrEqual(1);
  await expect(page.getByTestId("analysis-clock")).toContainText("19:");
  await page.getByTestId("analysis-clock").click();
  await expect(page.getByTestId("analysis-paused")).toBeVisible();
  await expect(page.getByTestId("analysis-material")).toHaveCount(0);
  await page.getByRole("button",{name:"继续计时"}).click();
  await expect(page.getByTestId("analysis-run")).toBeVisible();
  const material=await page.getByTestId("analysis-material").boundingBox();
  const question=await page.getByTestId("analysis-question").boundingBox();
  expect(material).not.toBeNull();
  expect(question).not.toBeNull();
  expect(material!.x).toBeLessThan(question!.x);
  expect(material!.width/(material!.width+question!.width)).toBeCloseTo(0.6,1);
  await expect(page.getByTestId("analysis-material")).toContainText("地区二为720亿元");
  await expect(page.getByTestId("analysis-material")).toContainText("模拟训练材料");
  await expect(page.getByTestId("analysis-material").getByRole("link")).toHaveCount(0);
  const passageText=await page.getByTestId("analysis-material").locator(".whitespace-pre-wrap").innerText();
  expect(passageText).toContain("\n\n");
  expect(passageText).not.toContain("模拟训练材料");

  // Long passages scroll only inside the material pane. The full-page card,
  // toolbar and question stay pinned in position.
  const passage=page.getByTestId("analysis-material-scroll");
  const scrollControls=page.getByRole("group",{name:"材料滚动"});
  await expect(scrollControls).toBeHidden();
  // Exactly ONE transparent canvas and ONE toolbar cover the whole two-column
  // workspace, so the same CSS-pixel pen width applies to both sides.
  const surface=page.getByTestId("analysis-run").getByTestId("calculation-scratchpad");
  const canvas=surface.getByTestId("calculation-scratchpad-canvas");
  const toggle=surface.getByTestId("calculation-scratchpad-toggle");
  await expect(canvas).toHaveCount(1);
  await expect(toggle).toHaveCount(1);
  await expect(toggle).toHaveAttribute("aria-label", "开启草稿");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-label", "关闭草稿");
  await expect(toggle).toHaveText("✕");
  const controls=surface.getByTestId("calculation-scratchpad-controls");
  await expect(surface.getByTestId("calculation-scratchpad-toolbar")).toBeVisible();
  await expect(surface.getByTestId("calculation-scratchpad-close")).toHaveCount(0);
  await expect(surface.getByLabel("铅笔已选中")).toHaveCount(0);
  const controlsBox=await controls.boundingBox();
  const surfaceBox=await surface.boundingBox();
  expect(controlsBox).not.toBeNull();
  expect(surfaceBox).not.toBeNull();
  expect(controlsBox!.y-surfaceBox!.y).toBeLessThan(20);
  expect(surfaceBox!.x+surfaceBox!.width-(controlsBox!.x+controlsBox!.width)).toBeLessThan(20);
  await expect(scrollControls).toBeVisible();
  const scrollUp=page.getByRole("button",{name:"材料向上滚动"});
  const scrollDown=page.getByRole("button",{name:"材料向下滚动"});
  await expect(scrollUp).toBeDisabled();
  await expect(scrollDown).toBeEnabled();
  const scrollInfo=await passage.evaluate(el=>({height:el.clientHeight,total:el.scrollHeight}));
  expect(scrollInfo.total).toBeGreaterThan(scrollInfo.height);
  const beforeFrame=await page.getByTestId("analysis-shared-workspace").boundingBox();
  // Each click moves one third of the material pane's actual visible height.
  await page.getByRole("button",{name:"材料向下滚动"}).click();
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeGreaterThan(scrollInfo.height*0.28);
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeLessThan(scrollInfo.height*0.38);
  await expect(scrollUp).toBeEnabled();
  await expect(scrollDown).toBeEnabled();
  await scrollUp.click();
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeLessThan(scrollInfo.height*0.04);
  await expect(scrollUp).toBeDisabled();
  await expect(scrollDown).toBeEnabled();
  await passage.evaluate(el=>{el.scrollTop=el.scrollHeight;});
  await expect(scrollDown).toBeDisabled();
  await expect(scrollUp).toBeEnabled();
  await passage.evaluate(el=>{el.scrollTop=0;});
  await expect(scrollUp).toBeDisabled();
  await expect(scrollDown).toBeEnabled();
  expect(await page.getByTestId("analysis-shared-workspace").boundingBox()).toEqual(beforeFrame);
  expect(await page.evaluate(()=>window.scrollY)).toBe(0);

  const workspace=await page.getByTestId("analysis-shared-workspace").boundingBox();
  const inkBox=await canvas.boundingBox();
  expect(inkBox).not.toBeNull();
  expect(workspace).not.toBeNull();
  expect(inkBox!.x).toBeCloseTo(workspace!.x,0);
  expect(inkBox!.width).toBeCloseTo(workspace!.width,0);
  const initial=await canvas.evaluate(el=>(el as HTMLCanvasElement).toDataURL());
  const draw=async (fromX:number,toX:number) => {
    await page.mouse.move(inkBox!.x+fromX, inkBox!.y+110);
    await page.mouse.down();
    await page.mouse.move(inkBox!.x+toX, inkBox!.y+150,{steps:6});
    await page.mouse.up();
  };
  await draw(60,160); // left material
  await draw(workspace!.width*.62,workspace!.width*.68); // right options
  const marked=await canvas.evaluate(el=>(el as HTMLCanvasElement).toDataURL());
  expect(marked).not.toEqual(initial);
  const canvasMetrics=await canvas.evaluate(el=>{
    const c=el as HTMLCanvasElement;
    return {width:c.width,clientWidth:c.clientWidth,pen:c.getContext("2d")?.lineWidth};
  });
  expect(canvasMetrics.width).toBeGreaterThanOrEqual(canvasMetrics.clientWidth);
  expect(canvasMetrics.pen).toBeCloseTo(2.6,4);
  await page.getByTestId("calculation-scratchpad-undo").click();
  const afterUndo=await canvas.evaluate(el=>(el as HTMLCanvasElement).toDataURL());
  expect(afterUndo).not.toEqual(marked);
  await page.getByTestId("calculation-scratchpad-redo").click();
  expect(await canvas.evaluate(el=>(el as HTMLCanvasElement).toDataURL())).toEqual(marked);
  await toggle.click();
  await expect(surface.getByTestId("calculation-scratchpad-toolbar")).toHaveCount(0);
  await expect(toggle).toHaveAttribute("aria-label", "开启草稿");
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(canvas).toBeHidden();
  await page.getByRole("button",{name:"上一题"}).click();
  await toggle.click();
  expect(await canvas.evaluate(el=>(el as HTMLCanvasElement).toDataURL())).toEqual(marked);
  await toggle.click();
  // Portrait stacks two internally scrollable panes without document scroll.
  await page.setViewportSize({width:820,height:430});
  const portraitMaterial=await page.getByTestId("analysis-material").boundingBox();
  const portraitQuestion=await page.getByTestId("analysis-question").boundingBox();
  expect(portraitMaterial).not.toBeNull();
  expect(portraitQuestion).not.toBeNull();
  expect(portraitMaterial!.y).toBeLessThan(portraitQuestion!.y);
  expect(portraitQuestion!.y+portraitQuestion!.height).toBeLessThanOrEqual(430);
  expect(await page.getByTestId("analysis-question").evaluate(el=>el.scrollHeight>el.clientHeight)).toBe(true);
  const shortPane=page.getByTestId("analysis-material-scroll");
  const shortHeight=await shortPane.evaluate(el=>el.clientHeight);
  await toggle.click();
  await expect(scrollControls).toBeVisible();
  await page.getByRole("button",{name:"材料向下滚动"}).click();
  await expect.poll(()=>shortPane.evaluate(el=>el.scrollTop)).toBeGreaterThan(shortHeight*0.28);
  await expect.poll(()=>shortPane.evaluate(el=>el.scrollTop)).toBeLessThan(shortHeight*0.38);
  await toggle.click();
  await expect(scrollControls).toBeHidden();
  await page.setViewportSize({width:390,height:844});
  await expectNoHorizontalOverflow(page);
  await toggle.click();
  await expect(scrollControls).toBeVisible();
  // On stacked phone layouts, material scroll controls must not sit below
  // the expanded toolbar at the top-right of the shared ink canvas.
  const phoneInkControls=await controls.boundingBox();
  const phoneScrollButtons=await scrollControls.boundingBox();
  expect(phoneInkControls).not.toBeNull();
  expect(phoneScrollButtons).not.toBeNull();
  expect(phoneScrollButtons!.x+phoneScrollButtons!.width).toBeLessThanOrEqual(phoneInkControls!.x);
  await shortPane.evaluate(el=>{el.scrollTop=0;});
  await expect(scrollUp).toBeDisabled();
  await expect(scrollDown).toBeEnabled();
  const phoneStart=await shortPane.evaluate(el=>el.scrollTop);
  await scrollDown.click();
  await expect.poll(()=>shortPane.evaluate(el=>el.scrollTop)).toBeGreaterThan(phoneStart);
  await toggle.click();
  await expect(scrollControls).toBeHidden();
  await page.getByTestId("analysis-question").evaluate(el=>{el.scrollTop=80;});
  expect(await page.getByTestId("analysis-question").evaluate(el=>el.scrollTop)).toBeGreaterThan(0);
  expect(await page.evaluate(()=>window.scrollY)).toBe(0);
  await page.setViewportSize({width:1180,height:820});
  await page.getByTestId("analysis-choice").first().click();
  await expect(page.getByTestId("analysis-choice").first()).toHaveClass(/btn-primary/);
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(page.getByTestId("analysis-question")).toContainText("第2题");
  await page.getByTestId("analysis-choice").nth(1).click();
  await expect(page.getByTestId("analysis-choice").nth(1)).toHaveClass(/btn-primary/);
  await page.getByRole("button",{name:"上一题"}).click();
  await expect(page.getByTestId("analysis-choice").first()).toHaveClass(/btn-primary/);
  await page.getByRole("button",{name:"交卷"}).click();
  await expect(page.getByTestId("analysis-result")).toBeVisible();
  await expect(page.getByTestId("analysis-result")).toContainText("1 / 20");
  await expect(page.getByTestId("analysis-result")).toContainText("第1题");
  // Review reuses the exact reading/answer card: right is readonly with
  // the correct answer green, wrong selection red, and explanations scrollable.
  await expect(page.getByTestId("analysis-shared-workspace")).toBeVisible();
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-state","correct");
  await expect(page.getByTestId("analysis-explanation")).toContainText("基期量");
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(page.getByTestId("analysis-review-choice").nth(1)).toHaveAttribute("data-state","wrong");
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-state","correct");
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(page.getByTestId("analysis-review-status")).toContainText("未答");
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-state","correct");
  await expect(page.getByTestId("analysis-review-choice").nth(1)).toHaveAttribute("data-state","neutral");
  await page.getByRole("button",{name:"查看练习历史"}).click();
  await expect(page.getByTestId("analysis-history")).toBeVisible();
  await page.getByRole("button",{name:"查看原题与解析"}).first().click();
  await expect(page.getByTestId("analysis-result")).toContainText("1 / 20");
  await expect(page.getByTestId("analysis-shared-workspace")).toBeVisible();
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-state","correct");
  await page.getByTestId("analysis-exit").click();
  await expect(page.getByTestId("analysis-history")).toBeVisible();
  await expect(page.getByTestId("analysis-history")).toContainText("查看原题与解析");
  await page.getByTestId("analysis-retry").first().click();
  await expect(page.getByTestId("analysis-run")).toBeVisible();
  expect(repeatCreated).toBe(1);
  expect(retryAnswers.every(x=>x===null)).toBe(true);
  await expect(page.getByTestId("analysis-question")).toContainText("第1题");
  await expect(page.getByTestId("analysis-question")).not.toContainText("基期量");
  await expect(page.getByTestId("analysis-choice").first()).not.toHaveClass(/btn-primary/);
  await page.getByTestId("analysis-choice").nth(1).click();
  await expect(page.getByTestId("analysis-choice").nth(1)).toHaveClass(/btn-primary/);
  await page.getByRole("button",{name:"交卷"}).click();
  await expect(page.getByTestId("analysis-result")).toContainText("0 / 20");
  await page.getByRole("button",{name:"查看练习历史"}).click();
  await expect(page.getByTestId("analysis-history")).toBeVisible();
  await page.getByRole("button",{name:"查看原题与解析"}).first().click();
  await expect(page.getByTestId("analysis-result")).toContainText("1 / 20");
  await page.getByTestId("analysis-exit").click();
  await page.getByRole("button",{name:"返回训练"}).click();
  await expect(page.getByRole("group",{name:"训练模式"})).toBeVisible();
});


test("AI job survives leaving training; ready paper is untimed until explicit entry",async({page})=>{
  let jobCreated=false;
  let generated=false;
  let starts=0;
  const begun=Date.now();
  const groups=Array.from({length:4},(_,index)=>({
    material:{

      topic:"资料分析",
      text:"2025年统计数据1200亿元，同比增长15.2%。",

    },
    questions:Array.from({length:5},()=>({
      question:"2025年统计数据是多少？",options:["1200","1300","1400","1500"],skill:"增长量"
    }))
  }));
  await page.route("**/api/v1/study/data-analysis/**",async(route)=>{
    const path=new URL(route.request().url()).pathname;
    let data:unknown={};
    if(path.endsWith("/status")){
      data={configured:true,model:"codex",cached_sources:3,verified_groups:4,
        available_skills:["乘积增长率"],preparing_job_id:jobCreated&&!generated?"same-task":null};
    }else if(path.endsWith("/unfinished")){
      data=[];
    }else if(path.endsWith("/history")){
      data=[];
    }else if(path.endsWith("/prepare")&&route.request().method()==="POST"){
      starts++;jobCreated=true;
      data={job_id:"same-task"};
    }else if(path.endsWith("/prepare/same-task")){
      data=generated
        ?{state:"ready",verified:20,message:"出题完成",run_id:"ready-run"}
        :{state:"generating",verified:12,message:"仍在生成",run_id:null};
    }else if(path.endsWith("/runs/ready-run/claim")){
      data={run:{id:"ready-run",status:"active",current_question:0,started_at_ms:begun,
        deadline_at_ms:Date.now()+1200000,paused_remaining_ms:null,
        duration_seconds:1200,groups,answers:Array(20).fill(null)},control_token:"ready-token"};
    }else if(path.endsWith("/runs/ready-run")){
      data={id:"ready-run",status:"active",started_at_ms:begun,
        deadline_at_ms:Date.now()+1200000,paused_remaining_ms:null,
        duration_seconds:1200,groups,answers:Array(20).fill(null)};
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(data)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await page.getByTestId("analysis-skill-expand").click();
  await page.getByTestId("analysis-skill-options").getByRole("button",{name:"乘积增长率"}).click();
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await page.getByTestId("analysis-start").click();
  await expect(page.getByTestId("analysis-start")).toBeDisabled();
  await expect(page.getByTestId("analysis-progress")).toContainText("仍在生成");
  await page.getByTestId("training-mode-calculation").click();
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-start")).toBeDisabled();
  await expect(page.getByTestId("analysis-progress")).toContainText("仍在生成");
  expect(starts).toBe(1);
  generated=true;
  await expect(page.getByTestId("analysis-ready")).toBeVisible({timeout:10000});
  await expect(page.getByTestId("analysis-clock")).toHaveCount(0);
  await page.reload();
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-ready")).toBeVisible();
  await expect(page.getByTestId("analysis-clock")).toHaveCount(0);
  await page.getByTestId("analysis-enter").click();
  await expect(page.getByTestId("analysis-run")).toBeVisible();
  await expect(page.getByTestId("analysis-clock")).toBeVisible();
  expect(starts).toBe(1);
});

test("iPad pauses and phone resumes server-held answers, position and single-editor lease", async ({ browser, baseURL }) => {
  const ipad=await browser.newContext({baseURL:baseURL!,viewport:{width:1180,height:820}});
  const phone=await browser.newContext({baseURL:baseURL!,viewport:{width:390,height:844}});
  const groups=Array.from({length:4},(_,g)=>({
    material:{topic:"工业能源",text:"2025年数据1200亿元，同比上升10%。"},
    questions:Array.from({length:5},(_,j)=>({question:`第${g*5+j+1}题？`,options:["A","B","C","D"]})),
  }));
  const answers:(number|null)[]=Array(20).fill(null);
  let currentQuestion=7,paused=false,controlToken="",serial=0,oldToken="";
  const respond=async (route:import("@playwright/test").Route) => {
    const url=new URL(route.request().url());
    const path=url.pathname,body=(route.request().method()==="POST" ? route.request().postDataJSON() : {}) as Record<string,unknown>;
    let data:unknown={};
    let status=200;
    const run=()=>({id:"server-run",status:"active",started_at_ms:Date.now()-120000,
      deadline_at_ms:paused?null:Date.now()+1000000,paused_remaining_ms:paused?980000:null,
      duration_seconds:1200,current_question:currentQuestion,groups,answers});
    if(path.endsWith("/status")) {
      data={configured:true,model:"codex",cached_sources:1,verified_groups:4,available_skills:["年均变化"],preparing_job_id:null};
    } else if(path.endsWith("/unfinished")) {
      data=[{id:"server-run",status:"active",answered:answers.filter(x=>x!==null).length,
        current_question:currentQuestion,paused_remaining_ms:paused?980000:null,
        deadline_at_ms:paused?null:Date.now()+1000000,created_at_ms:Date.now()-120000}];
    } else if(path.endsWith("/history")) {
      data=[];
    } else if(path.endsWith("/runs/server-run/claim")) {
      oldToken=controlToken;
      controlToken=`new-lease-${++serial}`;
      data={run:run(),control_token:controlToken};
    } else if(path.endsWith("/position")) {
      if(body.control_token!==controlToken){status=409;data={message:"other device has claimed"};}
      else {currentQuestion=body.question as number;data={saved:true};}
    } else if(path.includes("/answers/")) {
      if(body.control_token!==controlToken){status=409;data={message:"other device has claimed"};}
      else {answers[Number(path.split("/").pop())-1]=body.choice as number;data={saved:true};}
    } else if(path.endsWith("/pause")) {
      if(body.control_token!==controlToken){status=409;data={message:"other device has claimed"};}
      else {paused=!paused;data=run();}
    } else if(path.endsWith("/runs/server-run")) {
      data=run();
    }
    await route.fulfill({status,contentType:"application/json",body:JSON.stringify(data)});
  };
  await ipad.route("**/api/v1/study/data-analysis/**",respond);
  await phone.route("**/api/v1/study/data-analysis/**",respond);
  const ipadPage=await ipad.newPage();
  await ipadPage.goto("/#/study/training");
  await ipadPage.getByTestId("training-mode-comprehensive").click();
  await expect(ipadPage.getByTestId("analysis-unfinished")).toContainText("第8题");
  await ipadPage.getByTestId("analysis-resume").click();
  await expect(ipadPage.getByTestId("analysis-run")).toBeVisible();
  await expect(ipadPage.getByTestId("analysis-question")).toContainText("第8题");
  await ipadPage.getByTestId("analysis-choice").nth(2).click();
  await expect.poll(()=>answers[7]).toBe(2);
  await ipadPage.getByRole("button",{name:"下一题"}).click();
  await expect.poll(()=>currentQuestion).toBe(8);
  await ipadPage.getByTestId("analysis-exit").click();
  await expect(ipadPage.getByRole("group",{name:"训练模式"})).toBeVisible();
  expect(paused).toBe(true);

  const phonePage=await phone.newPage();
  await phonePage.goto("/#/study/training");
  await phonePage.getByTestId("training-mode-comprehensive").click();
  await expect(phonePage.getByTestId("analysis-unfinished")).toContainText("已作答 1/20");
  await phonePage.getByTestId("analysis-resume").click();
  await expect(phonePage.getByTestId("analysis-paused")).toBeVisible();
  await phonePage.getByRole("button",{name:"继续计时"}).click();
  await expect(phonePage.getByTestId("analysis-question")).toContainText("第9题");
  await phonePage.getByRole("button",{name:"上一题"}).click();
  await expect(phonePage.getByTestId("analysis-choice").nth(2)).toHaveClass(/btn-primary/);
  // The expired iPad lease can no longer alter the same SQLite record.
  const before=answers[7];
  const stale=await ipadPage.evaluate(async ({oldToken})=>{
    return fetch("/api/v1/study/data-analysis/runs/server-run/answers/8",{
      method:"POST",headers:{"Content-Type":"application/json"},
      body:JSON.stringify({choice:1,control_token:oldToken}),
    }).then(r=>r.status);
  },{oldToken});
  expect(stale).toBe(409);
  expect(answers[7]).toBe(before);
  await ipad.close();
  await phone.close();
});

test("iPad pause at 02:07 resumes at 02:07 even when Safari clock is four minutes slow", async ({ page }) => {
  // Simulate a real device whose wall clock does not match RK3568.
  await page.addInitScript(() => {
    const realNow=Date.now.bind(Date);
    Date.now=()=>realNow()-240_000;
  });
  const serverNow=Date.now();
  const baseMaterial={
    topic:"工业能源",
    text:"2025年能源消费1200万吨，比上年增长10%。",
  };
  const groups=Array.from({length:4},()=>({
    material:baseMaterial,
    questions:Array.from({length:5},()=>({question:"2024年消费量约是多少？",options:["1000","1100","1200","1300"]})),
  }));
  const answers:(number|null)[]=Array(20).fill(null);
  let paused=true;
  await page.route("**/api/v1/study/data-analysis/**",async route=>{
    const path=new URL(route.request().url()).pathname;
    let data:unknown={};
    const live=()=>({
      id:"skew-run",status:"active",started_at_ms:serverNow-1_073_000,
      paused_remaining_ms:paused?127_000:null,
      deadline_at_ms:paused?null:Date.now()+127_000,
      server_now_ms:Date.now(),
      duration_seconds:1200,current_question:0,groups,answers,
    });
    if(path.endsWith("/status")) {
      data={configured:true,model:"mock",cached_sources:1,verified_groups:4,available_skills:["年均变化"],preparing_job_id:null};
    } else if(path.endsWith("/history")) {
      data=[];
    } else if(path.endsWith("/unfinished")) {
      data=[{id:"skew-run",status:"active",answered:0,current_question:0,
        deadline_at_ms:null,paused_remaining_ms:127_000,created_at_ms:serverNow-1_073_000}];
    } else if(path.endsWith("/runs/skew-run/claim")) {
      data={run:live(),control_token:"mock-token"};
    } else if(path.endsWith("/runs/skew-run/pause")) {
      paused=false;
      data=live();
    } else if(path.endsWith("/runs/skew-run/position")) {
      data={saved:true};
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(data)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  await page.getByTestId("analysis-resume").click();
  await expect(page.getByTestId("analysis-paused")).toContainText("02:07");
  await page.getByRole("button",{name:"继续计时"}).click();
  await expect(page.getByTestId("analysis-clock")).toContainText(/02:0[5-7]/);
  await expect(page.getByTestId("analysis-clock")).not.toContainText("06:");
});

test("generation user prompt is collapsed by default, saved centrally, restored on reload and reset", async ({ page }) => {
  const defaultPrompt="默认：数据自洽，选项合理，解析采用 hyz 速算方法。";
  let savedPrompt:string|null=null;
  await page.route("**/api/v1/study/data-analysis/**", async route=>{
    const path=new URL(route.request().url()).pathname;
    let result:unknown={};
    if(path.endsWith("/user-prompt")) {
      if(route.request().method()==="POST") {
        savedPrompt=(route.request().postDataJSON() as {user_prompt:string|null}).user_prompt;
      }
      result={user_prompt:savedPrompt??defaultPrompt,customized:savedPrompt!==null};
    } else if(path.endsWith("/status")) {
      result={configured:true,model:"codex",cached_sources:1,verified_groups:4,
        available_skills:["年均变化"],preparing_job_id:null};
    } else if(path.endsWith("/history")||path.endsWith("/unfinished")) {
      result=[];
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(result)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  const settings=page.getByTestId("analysis-user-prompt");
  await expect(settings).toBeVisible();
  await expect(settings).not.toHaveAttribute("open","");
  await settings.getByTestId("analysis-user-prompt-toggle").click();
  await expect(settings.getByTestId("analysis-user-prompt-editor")).toHaveValue(defaultPrompt);
  const custom="仅使用资料分析知识点；模拟数据自然，防止题干给出方法提示。";
  await settings.getByTestId("analysis-user-prompt-editor").fill(custom);
  await settings.getByTestId("analysis-user-prompt-save").click();
  await expect.poll(()=>savedPrompt).toBe(custom);
  await expect(settings).toContainText("自定义版本");
  await page.reload();
  await page.getByTestId("training-mode-comprehensive").click();
  const reloaded=page.getByTestId("analysis-user-prompt");
  await expect(reloaded).not.toHaveAttribute("open","");
  await reloaded.getByTestId("analysis-user-prompt-toggle").click();
  await expect(reloaded.getByTestId("analysis-user-prompt-editor")).toHaveValue(custom);
  await reloaded.getByTestId("analysis-user-prompt-reset").click();
  await expect.poll(()=>savedPrompt).toBe(null);
  await expect(reloaded.getByTestId("analysis-user-prompt-editor")).toHaveValue(defaultPrompt);
  await expect(reloaded).toContainText("默认版本");
});
