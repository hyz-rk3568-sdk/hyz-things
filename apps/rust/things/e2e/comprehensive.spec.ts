import { expect, test } from "@playwright/test";
import { resetHarness } from "./fixtures";

test.beforeEach(async ({ request }) => { await resetHarness(request); });

test("iPad comprehensive 20-question flow with answer save and submit", async ({ page }) => {
  await page.setViewportSize({width: 1180,height: 820});
  const started = Date.now();
  const answers: (number | null)[] = Array(20).fill(null);
  const topics = ["宏观经济","农业","工业能源","交通物流与服务"];
  const groups = Array.from({length:4},(_,g)=>({
    material: {
      section: ["一、综合","二、农业","三、工业和建筑业","四、服务业"][g],
      topic: topics[g],
      subtopic: "文字统计材料",
      text: "2025年某项业务量为1200亿元，同比增长15.2%。其中地区一为480亿元。".repeat(4),
      source_url: "https://www.stats.gov.cn/sj/zxfb/202602/t20260228_1962662.html",
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
    if (path.endsWith("/status")) {
      result={configured:true,model:"mock-compatible-model",cached_sources:1,verified_groups:4,weak_skills:["基期量"]};
    } else if (path.endsWith("/prepare") && route.request().method()==="POST") {
      result={job_id:"fixture-task"};
    } else if (path.endsWith("/prepare/fixture-task")) {
      result={state:"ready",verified:20,message:"已完成20题校验",run_id:"fixture-run"};
    } else if (path.endsWith("/runs/fixture-run") && route.request().method()==="GET") {
      result={id:"fixture-run",status:"active",started_at_ms:started,deadline_at_ms:started+1200000,duration_seconds:1200,groups,answers};
    } else if (path.includes("/answers/")) {
      const number=Number(path.split("/").pop()!);
      const data=route.request().postDataJSON() as {choice:number};
      answers[number-1]=data.choice;
      result={saved:true};
    } else if (path.endsWith("/submit")) {
      result={id:"fixture-run",correct:1,total:20,questions:Array.from({length:20},(_,i)=>({
        number:i+1,chosen:answers[i],correct_index:0,answer:"1000亿元",
        explanation:"基期量为现期量除以增长因子",topic:topics[Math.floor(i/5)],
        skill:"基期量",evidence:"2025年某项业务量为1200亿元，同比增长15.2%。",
      }))};
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(result)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-setup")).toBeVisible();
  await expect(page.getByTestId("analysis-setup")).toContainText("20分钟");
  await expect(page.getByTestId("analysis-setup")).toContainText("mock-compatible-model");
  await page.getByTestId("analysis-start").click();
  await expect(page.getByTestId("analysis-run")).toBeVisible();
  await expect(page.getByTestId("analysis-clock")).toContainText("19:");
  const material=await page.getByTestId("analysis-material").boundingBox();
  const question=await page.getByTestId("analysis-question").boundingBox();
  expect(material).not.toBeNull();
  expect(question).not.toBeNull();
  expect(material!.x).toBeLessThan(question!.x);
  const materialCanvas=page.getByTestId("analysis-material").getByTestId("calculation-scratchpad-canvas");
  const materialToggle=page.getByTestId("analysis-material").getByTestId("calculation-scratchpad-toggle");
  const inkSnapshot=()=>materialCanvas.evaluate(el=>(el as HTMLCanvasElement).toDataURL());
  await materialToggle.click();
  const initialInk=await inkSnapshot();
  const inkBox=await materialCanvas.boundingBox();
  if(!inkBox)throw new Error("missing material canvas");
  await page.mouse.move(inkBox.x+25,inkBox.y+70);
  await page.mouse.down();
  await page.mouse.move(inkBox.x+120,inkBox.y+110,{steps:7});
  await page.mouse.up();
  const marked=await inkSnapshot();
  expect(marked).not.toEqual(initialInk);
  await page.getByRole("button",{name:"下一题"}).click();
  expect(await inkSnapshot()).toEqual(marked);
  await page.getByRole("button",{name:"上一题"}).click();
  expect(await inkSnapshot()).toEqual(marked);
  await page.getByTestId("analysis-material").getByTestId("calculation-scratchpad-close").click();
  await page.getByTestId("analysis-choice").first().click();
  await expect(page.getByTestId("analysis-choice").first()).toHaveClass(/btn-primary/);
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(page.getByTestId("analysis-question")).toContainText("第2题");
  await page.getByRole("button",{name:"上一题"}).click();
  await expect(page.getByTestId("analysis-choice").first()).toHaveClass(/btn-primary/);
  await page.getByRole("button",{name:"交卷"}).click();
  await expect(page.getByTestId("analysis-result")).toBeVisible();
  await expect(page.getByTestId("analysis-result")).toContainText("1 / 20");
});
