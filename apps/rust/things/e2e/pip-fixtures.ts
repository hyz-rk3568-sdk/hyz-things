import { type Page } from "@playwright/test";

// Opt in only for Document PiP DOM/lifecycle tests. A native same-origin popup
// does not cover OS PiP presentation, always-on-top behavior, or video PiP.
// Browsers with Document PiP keep exercising their real requestWindow API.
export async function installDocumentPipWindowFallback(page: Page) {
  await page.addInitScript(() => {
    if (Reflect.get(window, "documentPictureInPicture") != null) {
      return;
    }
    Object.defineProperty(window, "documentPictureInPicture", {
      configurable: true,
      value: {
        requestWindow({ width, height }: { width: number; height: number }) {
          // Open synchronously while the product's user activation is still live.
          const pipWindow = window.open("", "_blank", `popup,width=${width},height=${height}`);
          return pipWindow
            ? Promise.resolve(pipWindow)
            : Promise.reject(new DOMException("Popup blocked", "NotAllowedError"));
        },
      },
    });
  });
}
