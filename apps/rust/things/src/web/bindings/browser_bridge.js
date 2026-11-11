export async function mountFloatingMenu(reference, floating) {
  if (!window.hyzThingsUi) {
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        window.removeEventListener('hyz-things-ui-ready', ready);
        reject(new Error('Browser bridge initialization timed out'));
      }, 10000);
      function ready() {
        clearTimeout(timeout);
        resolve();
      }
      window.addEventListener('hyz-things-ui-ready', ready, { once: true });
      if (window.hyzThingsUi) {
        window.removeEventListener('hyz-things-ui-ready', ready);
        ready();
      }
    });
  }
  const bridge = window.hyzThingsUi;
  if (!bridge || bridge.version !== 1 || typeof bridge.mountFloatingMenu !== 'function') {
    throw new Error('Unsupported browser bridge version');
  }
  return bridge.mountFloatingMenu(reference, floating);
}
