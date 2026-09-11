// expected-tests: 37
// expected-runnable-tests: 36
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
  fromX: number,
  fromY: number,
  toX: number,
  toY: number,
) {
  const surface = page.locator("#portal-swipe-surface");
  const event = {
    bubbles: true,
    button: 0,
    clientX: fromX,
    clientY: fromY,
    isPrimary: true,
    pointerId: 1,
    pointerType: "touch",
  };
  await surface.dispatchEvent("pointerdown", event);
  await surface.dispatchEvent("pointerup", {
    ...event,
    clientX: toX,
    clientY: toY,
  });
}

export async function realTouchSwipe(page: Page, direction: "left" | "right") {
  const surface = page.locator("#portal-swipe-surface");
  const box = await surface.boundingBox();
  if (!box) throw new Error("portal swipe surface has no bounding box");
  const client = await page.context().newCDPSession(page);
  await client.send("Emulation.setTouchEmulationEnabled", {
    enabled: true,
    configuration: "mobile",
  });

  const startX = Math.round(
    box.x + box.width * (direction === "left" ? 0.72 : 0.18),
  );
  const endX = Math.round(
    box.x + box.width * (direction === "left" ? 0.18 : 0.72),
  );
  const y = Math.round(box.y + 180);
  const touchPoint = (x: number, y: number) => ({
    x,
    y,
    radiusX: 8,
    radiusY: 8,
    force: 1,
    id: 1,
  });

  await client.send("Input.dispatchTouchEvent", {
    type: "touchStart",
    touchPoints: [touchPoint(startX, y)],
    modifiers: 0,
  });
  for (const x of [
    startX + (endX - startX) * 0.35,
    startX + (endX - startX) * 0.7,
    endX,
  ]) {
    await client.send("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [touchPoint(Math.round(x), y + 4)],
      modifiers: 0,
    });
  }
  await client.send("Input.dispatchTouchEvent", {
    type: "touchEnd",
    touchPoints: [],
    modifiers: 0,
  });
}

export async function readCountdownTotal(target: Page, cardId?: string): Promise<number> {
  return target.evaluate((id) => {
    const selector = id
      ? `[data-exam-id="${id}"] [data-countdown-values]`
      : "[data-countdown-values]";
    const counter = document.querySelector(selector);
    if (!counter) {
      return 0;
    }
    const values = Array.from(
      counter.querySelectorAll("strong[data-value]"),
      (element) => Number(element.textContent),
    );
    const weights = [86_400, 3_600, 60, 1];
    return values.reduce(
      (total, value, index) => total + value * (weights[index] ?? 0),
      0,
    );
  }, cardId);
}

export async function readCustomCountdownTotal(
  target: Page,
  cardId?: string,
): Promise<number> {
  return target.evaluate((id) => {
    const card = id
      ? document.querySelector(`[data-exam-id="${id}"]`)
      : document.querySelector("[data-remaining-seconds]");
    return Number(card?.getAttribute("data-remaining-seconds") ?? 0);
  }, cardId);
}
