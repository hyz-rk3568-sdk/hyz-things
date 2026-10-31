import { expect, test, type Page } from "@playwright/test";
import { expectDisabledScrollShield, expectNoHorizontalOverflow, expectUnselectable, resetHarness } from "./fixtures";

import { expectInlineInkControls, inkSnapshot as captureInk } from "./ink-fixtures";

test.beforeEach(async ({ request }) => { await resetHarness(request); });

test("ink snapshots preserve exact coverage and colors across edge-alpha rasterization", async ({page})=>{
  await page.route('**/ink-comparison-test',route=>route.fulfill({contentType:'text/html',body:'<canvas id="ink-comparison" width="4" height="4"></canvas>'}));
  await page.goto('/ink-comparison-test');
  const canvas=page.locator('#ink-comparison');
  await canvas.evaluate(element=>{
    const context=(element as HTMLCanvasElement).getContext('2d')!;
    const image=context.createImageData(4,4);
    image.data.set([225,29,72,255],0);
    image.data.set([225,29,72,64],4);
    context.putImageData(image,0,0);
  });
  const original=await captureInk(canvas);
  await canvas.evaluate(element=>{
    const context=(element as HTMLCanvasElement).getContext('2d')!;
    const image=context.getImageData(0,0,4,4); image.data[7]=96;
    context.putImageData(image,0,0);
  });
  expect(await captureInk(canvas)).toBe(original);
  await canvas.evaluate(element=>{
    const context=(element as HTMLCanvasElement).getContext('2d')!;
    const image=context.getImageData(0,0,4,4); image.data[7]=0;
    context.putImageData(image,0,0);
  });
  expect(await captureInk(canvas)).not.toBe(original);
  await canvas.evaluate(element=>{
    const context=(element as HTMLCanvasElement).getContext('2d')!;
    const image=context.getImageData(0,0,4,4); image.data.set([0,0,255,255],0); image.data.set([225,29,72,64],4);
    context.putImageData(image,0,0);
  });
  expect(await captureInk(canvas)).not.toBe(original);
});

async function openHeldScrollWorkspace(page: Page, openInk=true) {
  const started = Date.now();
  const groups = Array.from({length:4},(_,group)=>({
    material:{topic:`材料 ${group+1}`,text:"某项业务量同比增长15.2%。\n\n".repeat(100)},
    questions:Array.from({length:5},(_,index)=>({
      question:`第${group*5+index+1}题`,options:["甲","乙","丙","丁"],skill:"基期量",
    })),
  }));
  const run = {id:"held-scroll-run",status:"active",current_question:0,
    started_at_ms:started,deadline_at_ms:started+1200000,paused_remaining_ms:null,
    duration_seconds:1200,groups,answers:Array(20).fill(null)};
  await page.emulateMedia({reducedMotion:"no-preference"});
  await page.route("**/api/v1/study/data-analysis/**",async route=>{
    const path = new URL(route.request().url()).pathname;
    let result: unknown = {};
    if (path.endsWith("/status")) {
      result={configured:true,model:"mock-compatible-model",cached_sources:1,
        verified_groups:4,available_skills:[],preparing_job_id:null};
    } else if (path.endsWith("/unfinished") || path.endsWith("/history")) {
      result=[];
    } else if (path.endsWith("/prepare")) {
      result={job_id:"held-scroll-job"};
    } else if (path.endsWith("/prepare/held-scroll-job")) {
      result={state:"ready",verified:20,message:"完成",run_id:run.id};
    } else if (path.endsWith("/pause")) {
      result={...run,paused_remaining_ms:run.paused_remaining_ms===null?1100000:null};
      Object.assign(run,result);
    } else if (path.endsWith("/submit")) {
      result={id:run.id,correct:0,total:20,materials:groups.map(group=>group.material),
        questions:groups.flatMap((group,g)=>group.questions.map((question,q)=>({
          number:g*5+q+1,...question,chosen:null,correct_index:0,answer:"甲",
          explanation:"解析",topic:group.material.topic,evidence:"业务量同比增长15.2%",
        })))};
    } else if (path.endsWith("/claim")) {
      result={run,control_token:"held-scroll-token"};
    } else if (path.endsWith(`/runs/${run.id}`)) {
      result=run;
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(result)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  await page.getByTestId("analysis-start").click();
  await page.getByTestId("analysis-enter").click();
  if(openInk) await page.getByTestId("calculation-scratchpad-toggle").click();
}

async function inkSnapshot(page: Page) {
  return captureInk(page.getByTestId("calculation-scratchpad-canvas"));
}

async function drawWorkspaceStroke(page: Page, fromX=60, toX=160) {
  const box=await page.getByTestId("calculation-scratchpad-canvas").boundingBox();
  expect(box).not.toBeNull();
  await page.mouse.move(box!.x+fromX,box!.y+110);
  await page.mouse.down();
  await page.mouse.move(box!.x+toX,box!.y+150,{steps:6});
  await page.mouse.up();
}

async function reopenWorkspace(page: Page) {
  await page.reload();
  await page.getByTestId("training-mode-comprehensive").click();
  await page.getByTestId("calculation-scratchpad-toggle").click();
}

test("comprehensive scratchpad tools belong to the answer header and expand from the pen", async ({page})=>{
  await page.setViewportSize({width:1180,height:820});
  await openHeldScrollWorkspace(page,false);
  const answerPane=page.getByTestId("analysis-answer-pane");
  const toggle=answerPane.getByTestId("calculation-scratchpad-toggle");
  await expect(toggle).toHaveAttribute("aria-label","开启草稿");
  await expectInlineInkControls(page.getByTestId("analysis-answer-header"));
  await expect(answerPane.getByTestId("calculation-scratchpad-toolbar")).toHaveCount(0);
  await expect(page.getByTestId("analysis-material")).not.toContainText("模拟训练材料");
  const penBefore=await toggle.boundingBox();
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-label","关闭草稿");
  const penAfter=await toggle.boundingBox();
  expect(penBefore).not.toBeNull();
  expect(penAfter).not.toBeNull();
  expect(Math.abs(penAfter!.x-penBefore!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(penAfter!.y-penBefore!.y)).toBeLessThanOrEqual(1);
  const toolbar=answerPane.getByTestId("calculation-scratchpad-toolbar");
  await expect(toolbar).toBeVisible();
  await expect(toolbar.getByTestId("calculation-scratchpad-undo")).toBeVisible();
  await expect(toolbar.getByTestId("calculation-scratchpad-redo")).toBeVisible();
  await expect(toolbar.getByTestId("calculation-scratchpad-clear")).toHaveAttribute("aria-label","清空草稿");
  await toggle.click();
  await expect(answerPane.getByTestId("calculation-scratchpad-toolbar")).toHaveCount(0);
});

test("comprehensive answer header scrolls the question independently", async ({page})=>{
  await page.setViewportSize({width:820,height:430});
  await openHeldScrollWorkspace(page);
  const answerPane=page.getByTestId("analysis-answer-pane");
  const question=page.getByTestId("analysis-question");
  const down=answerPane.getByRole("button",{name:"题目向下滚动",exact:true});
  await expect(down).toBeEnabled();
  await down.click();
  expect(await question.evaluate(el=>el.scrollTop)).toBe(32);
  expect(await page.getByTestId("analysis-material-scroll").evaluate(el=>el.scrollTop)).toBe(0);
});

test("timed training navigation uses only safe area and footer spacing at the bottom", async ({page})=>{
  const viewport = {width:1180,height:820};
  const bottomSafeArea = 24;
  await page.setViewportSize(viewport);
  await openHeldScrollWorkspace(page,false);
  await page.evaluate(value=>document.documentElement.style.setProperty('--app-safe-bottom',`${value}px`),bottomSafeArea);
  const next=await page.getByRole('button',{name:'下一题',exact:true}).boundingBox();
  expect(next).not.toBeNull();
  const footerSpacing=viewport.height-bottomSafeArea-next!.y-next!.height;
  expect(footerSpacing).toBeLessThanOrEqual(9);
});

test("IndexedDB handwriting survives navigation and reload when localStorage is full", async ({page})=>{
  await page.addInitScript(()=>{
    const setItem=Storage.prototype.setItem;
    Storage.prototype.setItem=function(key,value){
      if(key.startsWith("hyz-analysis-ink-")) throw new DOMException("Storage full","QuotaExceededError");
      setItem.call(this,key,value);
    };
  });
  await page.setViewportSize({width:1180,height:820});
  await openHeldScrollWorkspace(page);
  const canvas=page.getByTestId("calculation-scratchpad-canvas");
  const toggle=page.getByTestId("calculation-scratchpad-toggle");
  const saveStatus=page.getByTestId("calculation-scratchpad-save-status");
  const blank=await inkSnapshot(page);
  await drawWorkspaceStroke(page);
  const first=await inkSnapshot(page);
  expect(first).not.toEqual(blank);
  await expect(saveStatus).toHaveText("草稿已保存到本机");
  await page.getByRole("button",{name:"下一题",exact:true}).click();
  await expect(canvas).toBeHidden();
  await toggle.click();
  expect(await inkSnapshot(page)).toEqual(blank);
  await drawWorkspaceStroke(page,180,260);
  const second=await inkSnapshot(page);
  expect(second).not.toEqual(first);
  await expect(saveStatus).toHaveText("草稿已保存到本机");
  await page.getByRole("button",{name:"上一题",exact:true}).click();
  await toggle.click();
  await expect.poll(()=>inkSnapshot(page)).toEqual(first);
  expect(await page.evaluate(()=>localStorage.getItem("hyz-analysis-ink-held-scroll-run-workspace-0"))).toBeNull();
  await reopenWorkspace(page);
  await expect.poll(()=>inkSnapshot(page)).toEqual(first);
  await page.getByRole("button",{name:"下一题",exact:true}).click();
  await toggle.click();
  await expect.poll(()=>inkSnapshot(page)).toEqual(second);
});

test("IndexedDB write failure retains question ink and retries without losing strokes", async ({page})=>{
  await page.addInitScript(()=>{
    (window as any).failInkWrites=true;
    const put=IDBObjectStore.prototype.put;
    IDBObjectStore.prototype.put=function(...args: Parameters<typeof put>){
      if(this.name==="drafts"&&(window as any).failInkWrites) throw new DOMException("Storage full","QuotaExceededError");
      return put.apply(this,args);
    };
  });
  await page.setViewportSize({width:1180,height:820});
  await openHeldScrollWorkspace(page);
  await drawWorkspaceStroke(page);
  const marked=await inkSnapshot(page);
  const status=page.getByTestId("calculation-scratchpad-save-status");
  await expect(status).toHaveText("保存失败，仅当前页面暂存");
  await page.getByRole("button",{name:"下一题",exact:true}).click();
  await page.getByRole("button",{name:"上一题",exact:true}).click();
  await page.getByTestId("calculation-scratchpad-toggle").click();
  await expect.poll(()=>inkSnapshot(page)).toEqual(marked);
  await expect(status).toHaveText("保存失败，仅当前页面暂存");
  await page.evaluate(()=>{(window as any).failInkWrites=false;});
  await page.getByRole("button",{name:"重试保存",exact:true}).click();
  await expect(status).toHaveText("草稿已保存到本机");
  await reopenWorkspace(page);
  await expect.poll(()=>inkSnapshot(page)).toEqual(marked);
});

test("PWA reload preserves failed IndexedDB drafts and resumes after saving", async ({page})=>{
  await page.addInitScript(()=>{
    (window as any).failInkWrites=true;
    const put=IDBObjectStore.prototype.put;
    IDBObjectStore.prototype.put=function(...args: Parameters<typeof put>){
      if(this.name==="drafts"&&(window as any).failInkWrites) throw new DOMException("Storage full","QuotaExceededError");
      return put.apply(this,args);
    };
  });
  await openHeldScrollWorkspace(page);
  await drawWorkspaceStroke(page);
  const marked=await inkSnapshot(page);
  await expect(page.getByTestId("calculation-scratchpad-save-status")).toHaveText("保存失败，仅当前页面暂存");
  await page.getByTestId("analysis-exit").click();
  await expect(page.getByTestId("analysis-run")).toBeHidden();
  await page.evaluate(()=>{(window as any).pagePreserved=true;});
  await page.getByRole("button",{name:"重新加载",exact:true}).click();
  await expect(page.locator("[data-pwa-status]")).toContainText("草稿保存失败，已停止刷新");
  expect(await page.evaluate(()=>(window as any).pagePreserved)).toBe(true);
  await page.evaluate(()=>{(window as any).failInkWrites=false;});
  await Promise.all([
    page.waitForEvent("load"),
    page.getByRole("button",{name:"重新加载",exact:true}).click(),
  ]);
  expect(await page.evaluate(()=>(window as any).pagePreserved)).toBeUndefined();
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-paused")).toBeVisible();
  await page.getByRole("button",{name:"继续计时",exact:true}).click();
  await page.getByTestId("calculation-scratchpad-toggle").click();
  await expect.poll(()=>inkSnapshot(page)).toEqual(marked);
});

test("IndexedDB read failures block blank overwrites and allow retry", async ({page})=>{
  await page.addInitScript(()=>{
    (window as any).failInkReads=true;
    const get=IDBObjectStore.prototype.get;
    IDBObjectStore.prototype.get=function(...args: Parameters<typeof get>){
      if(this.name==="drafts"&&(window as any).failInkReads) throw new DOMException("Read unavailable","UnknownError");
      return get.apply(this,args);
    };
  });
  await openHeldScrollWorkspace(page,false);
  const toggle=page.getByTestId("calculation-scratchpad-toggle");
  await expect(page.getByTestId("calculation-scratchpad-save-status")).toHaveText("草稿读取失败，请重试");
  await expect(toggle).toBeDisabled();
  await page.evaluate(()=>{(window as any).failInkReads=false;});
  await page.getByRole("button",{name:"重试保存",exact:true}).click();
  await toggle.click();
  await drawWorkspaceStroke(page);
  await expect(page.getByTestId("calculation-scratchpad-save-status")).toHaveText("草稿已保存到本机");
});

test("returning to a still-saving question waits for commit without reporting failure", async ({page})=>{
  await page.addInitScript(()=>{
    const transaction=IDBDatabase.prototype.transaction;
    const complete=Object.getOwnPropertyDescriptor(IDBTransaction.prototype,"oncomplete")!;
    (window as any).finishInkWrites=[];
    IDBDatabase.prototype.transaction=function(...args: Parameters<typeof transaction>){
      const tx=transaction.apply(this,args);
      if(args[1]==="readwrite") Object.defineProperty(tx,"oncomplete",{
        set(handler){complete.set!.call(tx,(event:Event)=>{
          (window as any).finishInkWrites.push(()=>handler.call(tx,event));
        });},
      });
      return tx;
    };
  });
  await openHeldScrollWorkspace(page);
  await drawWorkspaceStroke(page);
  await expect.poll(()=>page.evaluate(()=>(window as any).finishInkWrites.length)).toBe(1);
  await page.getByRole("button",{name:"下一题",exact:true}).click();
  await page.getByRole("button",{name:"上一题",exact:true}).click();
  await expect(page.getByTestId("calculation-scratchpad-save-status")).toHaveText("正在读取草稿…");
  await page.evaluate(()=>{for(const finish of (window as any).finishInkWrites) finish();});
  await expect(page.getByTestId("calculation-scratchpad-save-status")).toHaveText("草稿已保存到本机");
  await page.getByTestId("calculation-scratchpad-toggle").click();
  await expect(page.getByTestId("calculation-scratchpad-undo")).toBeEnabled();
});

test("comprehensive headers and navigation stay inside PWA safe areas at every viewport", async ({page})=>{
  await openHeldScrollWorkspace(page,false);
  await page.evaluate(()=>{
    const style=document.documentElement.style;
    style.setProperty("--app-safe-top","32px");
    style.setProperty("--app-safe-bottom","24px");
    style.setProperty("--app-safe-left","12px");
    style.setProperty("--app-safe-right","12px");
  });
  for(const viewport of [{width:1180,height:820},{width:820,height:1180},{width:360,height:800}]){
    await page.setViewportSize(viewport);
    const screen=page.getByTestId("analysis-run");
    const header=await screen.locator("header").boundingBox();
    expect(header!.y).toBeGreaterThanOrEqual(40);
    for(const name of ["analysis-exit","analysis-clock"]){
      const box=await page.getByTestId(name).boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(12);
      expect(box!.x+box!.width).toBeLessThanOrEqual(viewport.width-12);
    }
    const next=await page.getByRole("button",{name:"下一题",exact:true}).boundingBox();
    expect(next!.y+next!.height).toBeLessThanOrEqual(viewport.height-24);
    await expectNoHorizontalOverflow(page);
    await page.getByRole("button",{name:"下一题",exact:true}).click();
    await page.getByRole("button",{name:"上一题",exact:true}).click();
  }
  await page.getByTestId("analysis-clock").click();
  const paused=page.getByTestId("analysis-paused");
  await expect(paused).toBeVisible();
  const pauseCard=await paused.locator("div").first().boundingBox();
  expect(pauseCard!.y).toBeGreaterThanOrEqual(32);
  expect(pauseCard!.y+pauseCard!.height).toBeLessThanOrEqual(800-24);
  await page.getByRole("button",{name:"继续计时",exact:true}).click();
  await page.getByRole("button",{name:"交卷",exact:true}).click();
  await expect(page.getByTestId("analysis-result")).toBeVisible();
  for(const viewport of [{width:1180,height:820},{width:820,height:1180},{width:360,height:800}]){
    await page.setViewportSize(viewport);
    const header=page.getByTestId("analysis-result").locator("header");
    expect((await header.boundingBox())!.y).toBeGreaterThanOrEqual(40);
    for(const button of await header.getByRole("button").all()){
      const box=await button.boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(12);
      expect(box!.x+box!.width).toBeLessThanOrEqual(viewport.width-12);
    }
    await page.getByRole("button",{name:"下一题",exact:true}).click();
    await page.getByRole("button",{name:"上一题",exact:true}).click();
    await expectNoHorizontalOverflow(page);
  }
});

async function openFrozenScrollWorkspace(page: Page) {
  await page.clock.install();
  await openHeldScrollWorkspace(page);
  await page.clock.pauseAt((await page.evaluate(()=>Date.now()))+1000);
}

test("disabled material scroll buttons intercept long presses before they reach text", async ({page})=>{
  await openFrozenScrollWorkspace(page);
  for (const mode of ["practice","review"]) {
    if (mode==="review") {
      await page.getByRole("button",{name:"交卷",exact:true}).click();
      await expect(page.getByTestId("analysis-result")).toBeVisible();
    }
    const up=page.getByRole("button",{name:"材料向上滚动",exact:true});
    if (!(await up.isVisible())) await page.getByTestId("calculation-scratchpad-toggle").click();
    const passage=page.getByTestId("analysis-material-scroll");
    for (const viewport of [{width:1180,height:820},{width:834,height:1112},{width:390,height:844}]) {
      await page.setViewportSize(viewport);
      for (const direction of ["上","下"]) {
        await passage.evaluate((el,direction)=>{el.scrollTop=direction==="上"?0:el.scrollHeight;},direction);
        await page.clock.runFor(20);
        const before=await passage.evaluate(el=>el.scrollTop);
        await expectDisabledScrollShield(page,page.getByRole("button",{name:`材料向${direction}滚动`,exact:true}));
        expect(await passage.evaluate(el=>el.scrollTop)).toBe(before);
      }
      await expectNoHorizontalOverflow(page);
    }
  }
});

test("material scroll repeats after 300ms and stops without a release click step", async ({page})=>{
  await page.setViewportSize({width:1180,height:820});
  await openFrozenScrollWorkspace(page);
  const passage=page.getByTestId("analysis-material-scroll");
  for (const [name,start,direction] of [["材料向下滚动",0,1],["材料向上滚动",320,-1]] as const) {
    await passage.evaluate((element,top)=>{element.scrollTop=top;},start);
    await page.clock.runFor(20);
    const button=page.getByRole("button",{name,exact:true});
    await expect(button).toBeEnabled();
    await button.hover();
    await page.mouse.down();
    await page.clock.runFor(299);
    expect(await passage.evaluate(el=>el.scrollTop)).toBe(start);
    await page.clock.runFor(1);
    expect(await passage.evaluate(el=>el.scrollTop)).toBe(start+32*direction);
    await page.clock.runFor(240);
    const held=await passage.evaluate(el=>el.scrollTop);
    expect((held-start)*direction).toBeGreaterThanOrEqual(96);
    expect(Math.abs(held-start)%32).toBe(0);
    await page.mouse.up();
    await page.clock.runFor(500);
    expect(await passage.evaluate(el=>el.scrollTop)).toBe(held);
    await button.click();
    expect(await passage.evaluate(el=>el.scrollTop)).toBe(held+32*direction);
  }
});

test("Pencil and touch holds cancel safely without selection or a long-press menu",async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await openFrozenScrollWorkspace(page);
  const passage=page.getByTestId("analysis-material-scroll");
  const down=page.getByRole("button",{name:"材料向下滚动",exact:true});
  await expectUnselectable(down);
  await expect(down).toHaveCSS("touch-action","none");
  expect(await down.evaluate(el=>el.dispatchEvent(new MouseEvent("contextmenu",{bubbles:true,cancelable:true})))).toBe(false);
  for (const pointerType of ["pen","touch"]) {
    for (const duration of [100,380]) {
      for (const reason of ["pointercancel","lostpointercapture","blur","visibilitychange","window-pointerup"]) {
        await passage.evaluate(el=>{el.scrollTop=0;});
        await page.clock.runFor(20);
        const pointer={pointerId:41,pointerType,isPrimary:true,button:0,buttons:1};
        await down.dispatchEvent("pointerdown",pointer);
        await page.clock.runFor(duration);
        const before=await passage.evaluate(el=>el.scrollTop);
        expect(before).toBe(duration<300?0:64);
        if (reason==="blur") {
          await page.evaluate(()=>window.dispatchEvent(new Event("blur")));
        } else if (reason==="visibilitychange") {
          await page.evaluate(()=>{
            Object.defineProperty(document,"hidden",{configurable:true,value:true});
            document.dispatchEvent(new Event("visibilitychange"));
            Reflect.deleteProperty(document,"hidden");
          });
        } else if (reason==="window-pointerup") {
          await page.evaluate(pointer=>window.dispatchEvent(new PointerEvent("pointerup",{...pointer,buttons:0})),pointer);
        } else {
          await down.dispatchEvent(reason,pointer);
        }
        await page.clock.runFor(500);
        expect(await passage.evaluate(el=>el.scrollTop),`${pointerType} ${duration}ms ${reason}`).toBe(before);
      }
    }
  }
  await down.focus();
  const before=await passage.evaluate(el=>el.scrollTop);
  await page.keyboard.press("Enter");
  expect(await passage.evaluate(el=>el.scrollTop)).toBe(before+32);
  await page.keyboard.press("Space");
  expect(await passage.evaluate(el=>el.scrollTop)).toBe(before+64);
});

test("material scroll holds stop at edges, on closing ink, changing question and leaving training",async({page})=>{
  await page.setViewportSize({width:1180,height:820});
  await openFrozenScrollWorkspace(page);
  const passage=page.getByTestId("analysis-material-scroll");
  const down=page.getByRole("button",{name:"材料向下滚动",exact:true});
  const up=page.getByRole("button",{name:"材料向上滚动",exact:true});
  const pointer={pointerId:41,pointerType:"pen",isPrimary:true,button:0,buttons:1};
  for (const [button,edge] of [[down,"bottom"],[up,"top"]] as const) {
    await passage.evaluate((el,edge)=>{el.scrollTop=edge==="bottom"?el.scrollHeight-el.clientHeight-16:16;},edge);
    await page.clock.runFor(20);
    await button.dispatchEvent("pointerdown",pointer);
    await page.clock.runFor(1000);
    await expect(button).toBeDisabled();
    const atEdge=await passage.evaluate(el=>el.scrollTop);
    await passage.evaluate((el,edge)=>{el.scrollTop=edge==="bottom"?el.scrollHeight-el.clientHeight-96:96;},edge);
    await page.clock.runFor(20);
    await expect(button).toBeEnabled();
    const restored=await passage.evaluate(el=>el.scrollTop);
    expect(restored).not.toBe(atEdge);
    await page.clock.runFor(500);
    expect(await passage.evaluate(el=>el.scrollTop)).toBe(restored);
    await button.dispatchEvent("pointerup",{...pointer,buttons:0});
  }
  const toggle=page.getByTestId("calculation-scratchpad-toggle");
  for (const action of ["close","question","leave"]) {
    await passage.evaluate(el=>{el.scrollTop=0;});
    await page.clock.runFor(20);
    await down.dispatchEvent("pointerdown",pointer);
    await page.clock.runFor(380);
    expect(await passage.evaluate(el=>el.scrollTop)).toBe(64);
    const pane=await passage.elementHandle();
    if (!pane) throw new Error("material pane missing");
    if (action==="close") {
      await toggle.click();
      await expect(down).toHaveCount(0);
    } else if (action==="question") {
      await page.getByRole("button",{name:"下一题",exact:true}).click();
      await expect(page.getByTestId("analysis-question")).toContainText("第2题");
    } else {
      await page.evaluate(()=>{location.hash="#/study";});
      await expect(page.getByTestId("study-summary")).toBeVisible();
    }
    const stopped=await pane.evaluate(el=>el.scrollTop);
    await page.clock.runFor(500);
    expect(await pane.evaluate(el=>el.scrollTop)).toBe(stopped);
    if (action!=="leave") await toggle.click();
  }
});

test("all scheduled cards stay height-bounded and scroll under the portal CSP", async ({ page }) => {
  await page.route("**/api/v1/study/data-analysis/**", async (route) => {
    const isStatus = new URL(route.request().url()).pathname.endsWith("/status");
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(isStatus ? {
      configured:true,model:"mock-compatible-model",cached_sources:1,verified_groups:0,
      available_skills:[],preparing_job_id:null,
      answer_summary:{answered_count:0,correct_count:0},
      card_stats:Array.from({length:30},(_,i)=>({
        card_id:`scroll-card-${i}`,category:"增长量",source_file:"资料/增长量/cards.md",
        front_markdown:`卡片 ${i}：${"长标题验证".repeat(30)}`,back_markdown:"解题解析",
        answered_count:0,correct_count:0,
      })),
    } : [])});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  const allCards = page.getByTestId("analysis-all-cards");
  const summary = allCards.locator("summary");
  const list = page.getByTestId("analysis-all-card-list");
  const cards = page.getByTestId("analysis-all-card-item");
  await summary.click();
  await expect(cards).toHaveCount(30);
  for (const width of [1024,390]) {
    await page.setViewportSize({width,height:740});
    await expect.poll(() => list.evaluate((pane) => ({
      bounded: pane.clientHeight > 0 && pane.clientHeight <= 288,
      scrollable: pane.scrollHeight > pane.clientHeight,
      overflowY: getComputedStyle(pane).overflowY,
    }))).toEqual({bounded:true,scrollable:true,overflowY:"auto"});
    await expectNoHorizontalOverflow(page);
    await list.evaluate(pane=>pane.scrollBy(0,500));
    await expect.poll(() => list.evaluate((pane) => pane.scrollTop)).toBeGreaterThan(0);
    await cards.last().click();
    await expect(page.getByTestId("analysis-card-detail")).toContainText("累计作答 0 次 · 未作答");
    await page.getByTestId("analysis-card-back").click();
    await expect(list).toBeVisible();
  }
  await summary.click();
  await expect(list).toBeHidden();
  await summary.click();
  await expect(list).toBeVisible();
});

test("comprehensive schedules globally without manual type selection", async ({ page }) => {
  await page.setViewportSize({width:1024,height:740});
  let prepareBody: unknown = null;
  await page.route("**/api/v1/study/data-analysis/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    let result: unknown = {};
    if (path.endsWith("/status")) {
      result = {configured:true,model:"mock-compatible-model",cached_sources:1,verified_groups:0,
        available_skills:["乘积增长率"],preparing_job_id:null,
        answer_summary:{answered_count:16,correct_count:11},
        card_stats:[{card_id:"019a31b7-8c42-7c16-a6d1-0c1f0f34c821",category:"基期量",
                     source_file:"资料/基期量/cards.md",front_markdown:"如何快速计算基期量？",back_markdown:"基期量 = 现期量 / (1 + 增长率)",
                     answered_count:2,correct_count:1},
                    {card_id:"019a31b7-8c42-7c16-a6d1-0c1f0f34c822",category:"比重",
                     source_file:"资料/比重/cards.md",front_markdown:"如何判断比重变化？",back_markdown:"比较部分与整体增长率",
                     answered_count:4,correct_count:3},
                    ...Array.from({length:11},(_,i)=>({
                      card_id:`long-title-${i}`,category:"增长量",
                      source_file:"资料/增长量/cards.md",
                      front_markdown:"这是一张非常长的资料分析 Flashcard 标题，需要截断避免 Safari 中长文本撑破列表容器。".repeat(6),
                      back_markdown:"解题解析",answered_count:i===10?0:10,correct_count:i===10?0:9
                    }))]};
    } else if (path.endsWith("/unfinished") || path.endsWith("/history")) {
      result = [];
    } else if (path.endsWith("/prepare") && route.request().method() === "POST") {
      prepareBody = route.request().postDataJSON();
      result = {job_id:"zero-selection-task"};
    } else if (path.endsWith("/prepare/zero-selection-task")) {
      result = {state:"ready",verified:20,message:"出题完成",run_id:"zero-selection-run"};
    }
    await route.fulfill({status:200,contentType:"application/json",body:JSON.stringify(result)});
  });
  await page.goto("/#/study/training");
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-setup")).toContainText("智能选卡 · 综合训练");
  await expect(page.getByTestId("analysis-skill-expand")).toHaveCount(0);
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await expect(page.getByTestId("analysis-total-answered")).toHaveText("16");
  await expect(page.getByTestId("analysis-total-correct")).toHaveText("11");
  await expect(page.getByTestId("analysis-total-accuracy")).toHaveText("68.8%");
  const allCards = page.getByTestId("analysis-all-cards");
  await expect(allCards).not.toHaveAttribute("open", "");
  await expect(page.getByTestId("analysis-all-card-list")).toBeHidden();
  await allCards.locator("summary").click();
  await expect(page.getByTestId("analysis-all-card-item")).toHaveCount(13);
  await expect(page.getByTestId("analysis-all-card-item").first()).toContainText("50.0%");
  await expect(page.getByTestId("analysis-all-card-item").last()).toContainText("未作答");
  const scrollPane = page.getByTestId("analysis-all-card-list");
  const fits = await scrollPane.evaluate((pane) => {
    const parent = pane.closest("details")!;
    const cards = [...pane.querySelectorAll("button")];
    return {
      vertical: pane.clientHeight <= 288 && pane.scrollHeight > pane.clientHeight,
      horizontal: pane.scrollWidth <= pane.clientWidth + 1,
      parent: parent.scrollWidth <= parent.clientWidth + 1,
      cards: cards.every((card) => card.scrollWidth <= card.clientWidth + 1),
      document: document.documentElement.scrollWidth <= window.innerWidth + 1,
    };
  });
  expect(fits).toEqual({vertical:true,horizontal:true,parent:true,cards:true,document:true});
  await page.setViewportSize({width:390,height:740});
  await expect.poll(() => scrollPane.evaluate(
    (pane) => pane.scrollWidth <= pane.clientWidth + 1 &&
      document.documentElement.scrollWidth <= window.innerWidth + 1
  )).toBe(true);
  await expect(allCards).not.toContainText("vcnt");
  await page.getByTestId("analysis-all-card-item").first().click();
  await expect(page.getByTestId("analysis-card-detail")).toBeVisible();
  await expect(page.getByTestId("analysis-card-front")).toContainText("如何快速计算基期量？");
  await expect(page.getByTestId("analysis-card-back-content")).toContainText("基期量");
  await page.getByTestId("analysis-card-back").click();
  await expect(page.getByTestId("analysis-card-detail")).toHaveCount(0);
  await page.getByTestId("analysis-all-card-item").last().click();
  await expect(page.getByTestId("analysis-card-detail")).toContainText("累计作答 0 次 · 未作答");
  await page.getByTestId("analysis-card-back").click();
  await expect(page.getByTestId("analysis-card-detail")).toHaveCount(0);
  await page.getByTestId("analysis-start").click();
  await expect.poll(() => prepareBody).toEqual({});
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
  let prepareBody:unknown=null;
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
      prepareBody=route.request().postDataJSON();
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
  await expect(page.getByRole("group", { name: "训练模式" }).getByRole("button")).toHaveText([
    "速算训练", "错题复练", "资料分析", "判断推理", "数量关系",
  ]);
  await expect(page.getByTestId("training-mode-logic-judgment")).toBeVisible();
  await page.getByTestId("training-mode-comprehensive").click();
  await expect(page.getByTestId("analysis-setup")).toBeVisible();
  await expect(page.getByTestId("analysis-setup")).toContainText("20分钟");
  await expect(page.getByTestId("analysis-setup")).toContainText("mock-compatible-model");
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await expect(page.getByTestId("analysis-skill-options")).toHaveCount(0);
  await expect(page.getByTestId("analysis-start")).toBeEnabled();
  await page.getByTestId("analysis-start").click();
  expect(prepareBody).toEqual({});
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
  await expect(page.getByTestId("analysis-material")).not.toContainText("模拟训练材料");
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
  await expect(toggle).toHaveText("✎");
  const controls=surface.getByTestId("calculation-scratchpad-controls");
  await expect(surface.getByTestId("calculation-scratchpad-toolbar")).toBeVisible();
  await expect(surface.getByTestId("calculation-scratchpad-close")).toHaveCount(0);
  await expect(surface.getByLabel("铅笔已选中")).toHaveCount(0);
  await expectInlineInkControls(page.getByTestId("analysis-answer-header"));
  await expect(scrollControls).toBeVisible();
  const scrollUp=page.getByRole("button",{name:"材料向上滚动"});
  const scrollDown=page.getByRole("button",{name:"材料向下滚动"});
  await expect(scrollUp).toBeDisabled();
  await expect(scrollDown).toBeEnabled();
  const scrollInfo=await passage.evaluate(el=>({height:el.clientHeight,total:el.scrollHeight}));
  expect(scrollInfo.total).toBeGreaterThan(scrollInfo.height);
  const beforeFrame=await page.getByTestId("analysis-shared-workspace").boundingBox();
  // Each click advances one 32px rendered material line, regardless of pane height.
  await scrollDown.click();
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeGreaterThan(29);
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeLessThan(35);
  await expect(scrollUp).toBeEnabled();
  await expect(scrollDown).toBeEnabled();
  await scrollUp.click();
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeLessThan(3);
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
  const initial=await inkSnapshot(page);
  const draw=async (fromX:number,toX:number) => {
    await page.mouse.move(inkBox!.x+fromX, inkBox!.y+110);
    await page.mouse.down();
    await page.mouse.move(inkBox!.x+toX, inkBox!.y+150,{steps:6});
    await page.mouse.up();
  };
  await draw(60,160); // left material
  await draw(workspace!.width*.62,workspace!.width*.68); // right options
  const marked=await inkSnapshot(page);
  expect(marked).not.toEqual(initial);
  const canvasMetrics=await canvas.evaluate(el=>{
    const c=el as HTMLCanvasElement;
    return {width:c.width,clientWidth:c.clientWidth,pen:c.getContext("2d")?.lineWidth};
  });
  expect(canvasMetrics.width).toBeGreaterThanOrEqual(canvasMetrics.clientWidth);
  expect(canvasMetrics.pen).toBeCloseTo(2.6,4);
  await page.getByTestId("calculation-scratchpad-undo").click();
  const afterUndo=await inkSnapshot(page);
  expect(afterUndo).not.toEqual(marked);
  await page.getByTestId("calculation-scratchpad-redo").click();
  await expect.poll(()=>inkSnapshot(page)).toEqual(marked);
  // Material ink is anchored to text content: shifting its scroll position
  // changes the drawing, and scrolling back restores its original placement.
  await scrollDown.click();
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeGreaterThan(29);
  await expect.poll(()=>inkSnapshot(page)).not.toEqual(marked);
  await scrollUp.click();
  await expect.poll(()=>passage.evaluate(el=>el.scrollTop)).toBeLessThan(3);
  await expect.poll(()=>inkSnapshot(page)).toEqual(marked);
  await toggle.click();
  await expect(surface.getByTestId("calculation-scratchpad-toolbar")).toHaveCount(0);
  await expect(toggle).toHaveAttribute("aria-label", "开启草稿");
  await page.getByRole("button",{name:"下一题"}).click();
  await expect(canvas).toBeHidden();
  await page.getByRole("button",{name:"上一题"}).click();
  await toggle.click();
  await expect.poll(()=>inkSnapshot(page)).toEqual(marked);
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
  await toggle.click();
  await expect(scrollControls).toBeVisible();
  await page.getByRole("button",{name:"材料向下滚动"}).click();
  await expect.poll(()=>shortPane.evaluate(el=>el.scrollTop)).toBeGreaterThan(29);
  await expect.poll(()=>shortPane.evaluate(el=>el.scrollTop)).toBeLessThan(35);
  await toggle.click();
  await expect(scrollControls).toBeHidden();
  await page.setViewportSize({width:390,height:844});
  await expectNoHorizontalOverflow(page);
  await toggle.click();
  await expect(scrollControls).toBeVisible();
  // Each stacked pane keeps its controls in its own header.
  const phoneInkControls=await controls.boundingBox();
  const phoneScrollButtons=await scrollControls.boundingBox();
  expect(phoneInkControls).not.toBeNull();
  expect(phoneScrollButtons).not.toBeNull();
  expect(phoneScrollButtons!.y+phoneScrollButtons!.height).toBeLessThanOrEqual(phoneInkControls!.y);
  await expectInlineInkControls(page.getByTestId("analysis-answer-header"));
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
  // Answer review uses the same held-scroll controls as the active paper.
  const reviewToggle=page.getByTestId("analysis-result").getByTestId("calculation-scratchpad-toggle");
  await page.clock.install();
  await page.clock.pauseAt((await page.evaluate(()=>Date.now()))+1000);
  await reviewToggle.click();
  await passage.evaluate(el=>{el.scrollTop=0;});
  await page.clock.runFor(20);
  await scrollDown.hover();
  await page.mouse.down();
  await page.clock.runFor(380);
  expect(await passage.evaluate(el=>el.scrollTop)).toBe(64);
  await page.mouse.up();
  await page.clock.runFor(500);
  expect(await passage.evaluate(el=>el.scrollTop)).toBe(64);
  await reviewToggle.click();
  await page.clock.resume();
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
  await expect(page.getByTestId("analysis-skill-expand")).toHaveCount(0);
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
