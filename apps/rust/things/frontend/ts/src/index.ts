import { mountFloatingMenu } from './floating-menu';

declare global {
  interface Window {
    hyzThingsUi?: { version: 1; mountFloatingMenu: typeof mountFloatingMenu };
  }
}

window.hyzThingsUi = { version: 1, mountFloatingMenu };
window.dispatchEvent(new Event('hyz-things-ui-ready'));
