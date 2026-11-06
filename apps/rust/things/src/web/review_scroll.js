// A saved offset may exceed the current height until asynchronous content loads.
export function restoreReviewScroll(element, offset) {
  let pending = offset > 0;
  let observer;
  const cancel = () => {
    pending = false;
    element.removeAttribute('data-review-scroll-restoring');
    observer?.disconnect();
    element.removeEventListener('load', retry, true);
    for (const name of ['pointerdown', 'wheel', 'touchstart', 'keydown']) {
      element.removeEventListener(name, cancel, true);
    }
  };
  const retry = () => {
    if (!pending || !element.isConnected) return;
    element.scrollTop = offset;
    if (element.scrollTop >= offset) cancel();
  };
  if (pending) {
    element.setAttribute('data-review-scroll-restoring', '');
    observer = new MutationObserver(retry);
    observer.observe(element, { childList: true, subtree: true, characterData: true });
    element.addEventListener('load', retry, true);
    for (const name of ['pointerdown', 'wheel', 'touchstart', 'keydown']) {
      element.addEventListener(name, cancel, true);
    }
    retry();
  } else {
    element.scrollTop = 0;
  }
  return cancel;
}
