// Keep the setup shell above a software keyboard without reflowing pinch zoom.
export function mountTrainingViewport(root) {
  const viewport = window.visualViewport;
  if (!viewport) return () => {};
  let frame = 0;
  const update = () => {
    frame = 0;
    if (Math.abs(viewport.scale - 1) > 0.01 ||
        !Number.isFinite(viewport.height) || viewport.height <= 0 ||
        !Number.isFinite(viewport.width) || viewport.width <= 0) return;
    const geometry = {
      height: viewport.height,
      width: viewport.width,
      top: Math.max(0, viewport.offsetTop),
      left: Math.max(0, viewport.offsetLeft),
    };
    for (const [dimension, value] of Object.entries(geometry)) {
      root.style.setProperty(`--training-viewport-${dimension}`, `${value}px`);
    }
  };
  const schedule = () => {
    if (!frame) frame = window.requestAnimationFrame(update);
  };
  // A restored iPad visual viewport may change without window.resize.
  const listeners = [
    [viewport, 'resize'], [viewport, 'scroll'], [window, 'resize'],
    [window, 'pageshow'], [window, 'focus'], [document, 'visibilitychange'],
  ];
  for (const [target, event] of listeners) target.addEventListener(event, schedule);
  update();
  return () => {
    for (const [target, event] of listeners) target.removeEventListener(event, schedule);
    if (frame) window.cancelAnimationFrame(frame);
  };
}
