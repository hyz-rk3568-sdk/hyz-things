// Keep watching while asynchronous content changes size; user input cancels restoration.
export function restoreReviewScroll(element, offset) {
  let pending = offset > 0;
  let observer;
  const pane = element.closest('[data-testid="question-bank-practice-material-pane"], [data-testid="question-bank-practice-question-pane"]') || element;
  const surface = element.closest('[data-testid="training-swipe-surface"]') || pane;
  const intent = event => {
    if (event.target === surface || pane.contains(event.target)) cancel(true);
  };
  const cancel = (userIntent = false) => {
    pending = false;
    element.removeAttribute('data-review-scroll-restoring');
    observer?.disconnect();
    element.removeEventListener('load', retry, true);
    for (const name of ['pointerdown', 'wheel', 'touchstart', 'keydown']) {
      surface.removeEventListener(name, intent, true);
    }
    // A user interrupting restoration also replaces the saved offset. Merely
    // unmounting/rebinding a pane must not reset the previously saved offset.
    if (userIntent && element.isConnected) {
      element.dispatchEvent(new Event('scroll'));
    }
  };
  const retry = () => {
    if (!pending) return;
    if (!element.isConnected) { cancel(); return; }
    if (element.scrollTop !== offset) element.scrollTop = offset;
  };
  if (pending) {
    element.setAttribute('data-review-scroll-restoring', '');
    observer = new MutationObserver(retry);
    observer.observe(element, { childList: true, subtree: true, characterData: true });
    element.addEventListener('load', retry, true);
    for (const name of ['pointerdown', 'wheel', 'touchstart', 'keydown']) {
      surface.addEventListener(name, intent, true);
    }
    retry();
  } else {
    element.scrollTop = 0;
  }
  return cancel;
}
