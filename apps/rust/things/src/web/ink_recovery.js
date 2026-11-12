// Safari can discard a Canvas backing buffer or change visual-viewport
// geometry while the app is suspended, without emitting window.resize.
// Repaint saved strokes only after the visible layout has settled.
export function mountInkRecovery(canvas, redraw) {
  let frame = 0;
  const schedule = () => {
    if (document.hidden || !canvas.isConnected || !canvas.clientWidth || !canvas.clientHeight || frame) return;
    frame = window.requestAnimationFrame(() => {
      frame = 0;
      if (!document.hidden && canvas.isConnected && canvas.clientWidth && canvas.clientHeight) redraw();
    });
  };

  const surface = canvas.closest('[data-testid="calculation-scratchpad"]');
  surface?.addEventListener('training-split-resized', schedule);
  const questionSelector = [
    "[data-testid='question-bank-practice-question-scroll']",
    "[data-testid='collection-review-question-scroll']",
    "[data-testid='question-bank-ai-messages']",
  ].join(', ');
  let questionPane = surface?.querySelector(questionSelector) ?? null;
  const paneObserver = surface && typeof MutationObserver === 'function' ? new MutationObserver(() => {
    const next = surface.querySelector(questionSelector);
    if (next !== questionPane) { questionPane = next; schedule(); }
  }) : null;
  paneObserver?.observe(surface, { childList: true, subtree: true });
  const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(schedule) : null;
  observer?.observe(canvas);
  window.addEventListener('resize', schedule);
  window.visualViewport?.addEventListener('resize', schedule);
  document.addEventListener('visibilitychange', schedule);
  window.addEventListener('pageshow', schedule);
  window.addEventListener('focus', schedule);

  return () => {
    observer?.disconnect();
    paneObserver?.disconnect();
    surface?.removeEventListener('training-split-resized', schedule);
    window.removeEventListener('resize', schedule);
    window.visualViewport?.removeEventListener('resize', schedule);
    document.removeEventListener('visibilitychange', schedule);
    window.removeEventListener('pageshow', schedule);
    window.removeEventListener('focus', schedule);
    if (frame) window.cancelAnimationFrame(frame);
  };
}
