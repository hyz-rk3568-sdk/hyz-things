import { expect, test } from "@playwright/test";
import { resetHarness } from "./fixtures";

test.beforeEach(async ({ request }) => { await resetHarness(request); });

test("iPad comprehensive 20-question flow with answer save and submit", async ({ page }) => {
  await page.setViewportSize({width: 1180,height: 820});
  const started = Date.now();
  const answers: (number | null)[] = Array(20).fill(null);
  let paused=false;
  let submittedResult:unknown={};
  let manualSkillsSubmitted:string[]=[];
  const topics = ["宏观经济","农业","工业能源","交通物流与服务"];
  const groups = Array.from({length:4},(_,g)=>({
    material: {
      section: ["一、综合","二、农业","三、工业和建筑业","四、服务业"][g],
      topic: topics[g],
      subtopic: "文字统计材料",
      text: Array.from({length:30},()=> "2025年某项业务量为1200亿元，同比增长15.2%。\n\n其中地区一为480亿元，地区二为720亿元。").join("\n\n"),
      source_url: `practice-theme-${g}`,
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
    if (path.endsWith("/history")) {
      result=submittedResult && Object.keys(submittedResult as object).length>0 ? [
        {id:"fixture-run",started_at_ms:started,ended_at_ms:started+30000,elapsed_ms:30000,correct:1,total:20,topics:["经济","农业","工业","服务业"]}
      ] : [];
    } else if (path.endsWith("/status")) {
      result={configured:true,model:"mock-compatible-model",cached_sources:1,verified_groups:4,weak_skills:["基期量"],available_skills:["乘积增长率","混合增长率"],preparing_job_id:null};
    } else if (path.endsWith("/prepare") && route.request().method()==="POST") {
      manualSkillsSubmitted=(route.request().postDataJSON() as {manual_skills:string[]}).manual_skills;
      result={job_id:"fixture-task"};
    } else if (path.endsWith("/prepare/fixture-task")) {
      result={state:"ready",verified:20,message:"已完成20题校验",run_id:"fixture-run"};
    } else if (path.endsWith("/runs/fixture-run") && route.request().method()==="GET") {
      result={id:"fixture-run",status:"active",started_at_ms:started,deadline_at_ms:paused?null:started+1200000,paused_remaining_ms:paused?1100000:null,duration_seconds:1200,groups,answers};
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
  await page.getByTestId("analysis-skill-expand").click();
  await page.getByTestId("analysis-skill-options").getByRole("button",{name:"乘积增长率"}).click();
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
  const scrollInfo=await passage.evaluate(el=>({height:el.clientHeight,total:el.scrollHeight}));
  expect(scrollInfo.total).toBeGreaterThan(scrollInfo.height);
  const beforeFrame=await page.getByTestId("analysis-shared-workspace").boundingBox();
  await passage.evaluate(el=>{el.scrollTop=140;});
  expect(await passage.evaluate(el=>el.scrollTop)).toBeGreaterThan(0);
  expect(await page.getByTestId("analysis-shared-workspace").boundingBox()).toEqual(beforeFrame);
  expect(await page.evaluate(()=>window.scrollY)).toBe(0);

  // Exactly ONE transparent canvas and ONE toolbar cover the whole two-column
  // workspace, so the same CSS-pixel pen width applies to both sides.
  const surface=page.getByTestId("analysis-run").getByTestId("calculation-scratchpad");
  const canvas=surface.getByTestId("calculation-scratchpad-canvas");
  const toggle=surface.getByTestId("calculation-scratchpad-toggle");
  await expect(canvas).toHaveCount(1);
  await expect(toggle).toHaveCount(1);
  await toggle.click();
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
  await page.getByTestId("calculation-scratchpad-close").click();
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(canvas).toBeHidden();
  await page.getByRole("button",{name:"上一题"}).click();
  await toggle.click();
  expect(await canvas.evaluate(el=>(el as HTMLCanvasElement).toDataURL())).toEqual(marked);
  await page.getByTestId("calculation-scratchpad-close").click();
  // Portrait stacks two internally scrollable panes without document scroll.
  await page.setViewportSize({width:820,height:430});
  const portraitMaterial=await page.getByTestId("analysis-material").boundingBox();
  const portraitQuestion=await page.getByTestId("analysis-question").boundingBox();
  expect(portraitMaterial).not.toBeNull();
  expect(portraitQuestion).not.toBeNull();
  expect(portraitMaterial!.y).toBeLessThan(portraitQuestion!.y);
  expect(portraitQuestion!.y+portraitQuestion!.height).toBeLessThanOrEqual(430);
  expect(await page.getByTestId("analysis-question").evaluate(el=>el.scrollHeight>el.clientHeight)).toBe(true);
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
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-grade","correct");
  await expect(page.getByTestId("analysis-explanation")).toContainText("基期量");
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(page.getByTestId("analysis-review-choice").nth(1)).toHaveAttribute("data-grade","wrong");
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-grade","correct");
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(page.getByTestId("analysis-grade")).toContainText("未答");
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-grade","correct");
  await expect(page.getByTestId("analysis-review-choice").nth(1)).toHaveAttribute("data-grade","neutral");
  await page.getByRole("button",{name:"查看练习历史"}).click();
  await expect(page.getByTestId("analysis-history")).toBeVisible();
  await page.getByRole("button",{name:"查看原题与解析"}).first().click();
  await expect(page.getByTestId("analysis-result")).toContainText("1 / 20");
  await expect(page.getByTestId("analysis-shared-workspace")).toBeVisible();
  await expect(page.getByTestId("analysis-review-choice").nth(0)).toHaveAttribute("data-grade","correct");
  await page.getByTestId("analysis-exit").click();
  await expect(page.getByTestId("analysis-history")).toBeVisible();
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
      section:["一、综合","二、农业","三、工业和建筑业","四、服务业"][index],
      topic:"资料分析",subtopic:"统计资料",
      text:"2025年统计数据1200亿元，同比增长15.2%。",
      source_url:"https://www.stats.gov.cn/"
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
        weak_skills:[],available_skills:[],preparing_job_id:jobCreated&&!generated?"same-task":null};
    }else if(path.endsWith("/history")){
      data=[];
    }else if(path.endsWith("/prepare")&&route.request().method()==="POST"){
      starts++;jobCreated=true;
      data={job_id:"same-task"};
    }else if(path.endsWith("/prepare/same-task")){
      data=generated
        ?{state:"ready",verified:20,message:"出题完成",run_id:"ready-run"}
        :{state:"generating",verified:12,message:"仍在生成",run_id:null};
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
