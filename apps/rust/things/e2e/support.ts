// expected-tests: 47
// expected-runnable-tests: 46
import type { Page } from "@playwright/test";

export async function installCountdownVideoPipMock(page: Page) {
  await page.addInitScript(() => {
    Object.defineProperty(window, "documentPictureInPicture", {
      configurable: true,
      value: undefined,
    });
    const calls: string[] = [];
    let inPip = false;
    (window as any).__hyzCountdownVideoPipCalls = calls;
    Object.defineProperty(HTMLVideoElement.prototype, "requestPictureInPicture", {
      configurable: true,
      value: function () {
        calls.push("request");
        inPip = true;
        return Promise.resolve();
      },
    });
    Object.defineProperty(Document.prototype, "pictureInPictureElement", {
      configurable: true,
      get() {
        return inPip
          ? document.querySelector("video[data-countdown-pip-active]") || null
          : null;
      },
    });
  });
}

export async function swipePortal(
  page: Page,
  startX: number,
  startY: number,
  endX: number,
  endY: number,
) {
  const surface = page.locator("#portal-swipe-surface");
  await surface.dispatchEvent("pointerdown", {
    pointerId: 1,
    pointerType: "touch",
    isPrimary: true,
    clientX: startX,
    clientY: startY,
  });
  await surface.dispatchEvent("pointerup", {
    pointerId: 1,
    pointerType: "touch",
    isPrimary: true,
    clientX: endX,
    clientY: endY,
  });
}

export async function realTouchSwipe(page: Page, direction: "left" | "right") {
  const box = await page.locator("#portal-swipe-surface").boundingBox();
  if (!box) throw new Error("portal swipe surface is not visible");
  const y = Math.round(box.y + Math.min(box.height / 2, 220));
  const left = Math.round(box.x + Math.min(56, box.width / 4));
  const right = Math.round(box.x + box.width - Math.min(56, box.width / 4));
  const client = await page.context().newCDPSession(page);
  const startX = direction === "left" ? right : left;
  const endX = direction === "left" ? left : right;
  await client.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [{ x: startX, y }],
  });
  await client.send("Input.dispatchTouchEvent", {
    type: "touchMove",
    touchPoints: [{ x: endX, y: y + 4 }],
  });
  await client.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
  });
}

export async function readCountdownTotal(page: Page) {
  return Number(
    await page
      .locator('[data-exam-id="guangdong-exam"]')
      .getAttribute("data-countdown-total-seconds"),
  );
}

export async function readCustomCountdownTotal(page: Page) {
  return Number(
    await page
      .locator('[data-custom-countdown="true"]')
      .getAttribute("data-countdown-total-seconds"),
  );
}
