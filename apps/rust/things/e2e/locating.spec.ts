import { expect, test } from "@playwright/test";
import { resetHarness } from "./fixtures";

test.beforeEach(async ({ request }) => { await resetHarness(request); });

test("fresh locating passage preserves topical paragraphs and uses its own five-answer API", async ({ page }) => {
  await page.setViewportSize({width:1180,height:820});
  const body = "【模拟训练数据，非官方发布数值】\n2024年某地区工业增加值1200亿元，同比增长8%。\n\n其中规模以上国有企业增加值450亿元，民营企业增加值750亿元。";
  const answered:({correct:boolean,elapsed_ms:number,selected_start:number,expected_start:number}|null)[]=Array(5).fill(null);
  const questions=Array.from({length:5},(_,i)=>({question:`第${i+1}题：2024年某指标是多少？`}));
  const material={section:"三、工业和建筑业",topic:"工业能源",subtopic:"工业增加值",
    text:body,source_url:"https://www.stats.gov.cn/"};
  let starts=0;
  const view=()=>({id:"fresh-loc-run",status:answered.every(Boolean)?"complete":"active",
    material,questions,answers:answered});
  await page.route("**/api/v1/study/data-analysis/**", async route => {
    const path=new URL(route.request().url()).pathname;
    let result:unknown={};
    if(path.endsWith("/locating/topics")){
      result=[{topic:"工业能源",subtopics:["工业增加值"]}];
    } else if(path.endsWith("/locating/prepare")&&route.request().method()==="POST"){
      starts++;result={job_id:"loc-job"};
    } else if(path.endsWith("/prepare/loc-job")){
      result={state:"ready",verified:5,message:"5道题已生成",run_id:"fresh-loc-run"};
    } else if(path.endsWith("/locating/runs/fresh-loc-run")){
      result=view();
    } else if(path.includes("/locating/runs/fresh-loc-run/answers/")){
      const idx=Number(path.split("/").pop())-1;
      const submitted=route.request().postDataJSON() as {start:number,end:number,elapsed_ms:number};
      answered[idx]={correct:true,elapsed_ms:submitted.elapsed_ms,
        selected_start:submitted.start,expected_start:submitted.start};
      result=view();
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(result)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-locating").click();
  await expect(page.getByTestId("locating-setup")).toBeVisible();
  await expect(page.getByTestId("locating-setup")).not.toContainText("复练已见过的材料");
  await page.getByRole("button",{name:"开始5题找数训练"}).click();
  await expect(page.getByTestId("locating-run")).toBeVisible();
  expect(starts).toBe(1);
  await expect(page.getByTestId("locating-material")).toContainText("国有企业增加值450亿元");
  const passage=page.getByTestId("locating-material").locator(".whitespace-pre-wrap");
  await expect(passage).toHaveCSS("white-space","pre-wrap");
  expect(await passage.textContent()).toContain("\n\n");
  for(let i=0;i<5;i++){
    await expect(page.getByTestId("locating-run")).toContainText(`第 ${i+1}/5 题`);
    await page.getByTestId("locating-number").nth(i).click();
    if(i<4){await expect(page.getByTestId("locating-run")).toContainText(`第 ${i+2}/5 题`);}
  }
  await expect(page.getByTestId("locating-result")).toContainText("5/5 题定位正确");
  await page.getByRole("button",{name:"再练一篇"}).click();
  await expect(page.getByTestId("locating-setup")).toBeVisible();
});
