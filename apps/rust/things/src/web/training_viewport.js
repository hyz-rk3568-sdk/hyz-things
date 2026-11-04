// Keep the setup shell above a software keyboard without reflowing pinch zoom.
export function mountTrainingViewport(root) {
  const viewport = window.visualViewport;
  if (!viewport) return () => {};
  let frame = 0;
  let settle = 0;
  const update = () => {
    frame = 0;
    const layoutWidth = window.innerWidth || document.documentElement?.clientWidth || 0;
    const layoutHeight = window.innerHeight || document.documentElement?.clientHeight || 0;
    if (!Number.isFinite(layoutWidth) || layoutWidth <= 0 ||
        !Number.isFinite(layoutHeight) || layoutHeight <= 0) return;
    const scale = Number.isFinite(viewport.scale) ? viewport.scale : 1;
    const visualWidth = Number.isFinite(viewport.width) ? viewport.width : 0;
    const visualHeight = Number.isFinite(viewport.height) ? viewport.height : 0;
    const zoomed = Math.abs(scale - 1) > 0.01;
    const visualUsable = !zoomed &&
      visualWidth > 0 && visualHeight > 0 && visualHeight <= layoutHeight + 1;
    const measured = (name) => {
      const value = Number.parseFloat(root.style.getPropertyValue(`--training-viewport-${name}`));
      return Number.isFinite(value) ? value : 0;
    };
    // Width always follows the layout viewport. iPad Safari can keep a stale
    // reduced visualViewport.width after returning from another app, which
    // otherwise leaves the whole training shell at partial width.
    //
    // Pinch zoom reports a visual viewport that covers only part of the
    // layout. Keep the last keyboard-adjusted box there so the fixed shell
    // does not jump to full height in the middle of the gesture.
    const geometry = {
      height: zoomed ? measured('height') || layoutHeight : (visualUsable ? visualHeight : layoutHeight),
      width: layoutWidth,
      top: zoomed ? measured('top') : (visualUsable ? Math.max(0, viewport.offsetTop) : 0),
      left: visualUsable ? Math.max(0, viewport.offsetLeft) : 0,
    };
    for (const [dimension, value] of Object.entries(geometry)) {
      root.style.setProperty(`--training-viewport-${dimension}`, `${value}px`);
    }
  };
  const schedule = () => {
    if (frame) return;
    frame = window.requestAnimationFrame(() => {
      update();
      // iPad can report transitional geometry right after app switching.
      clearTimeout(settle);
      settle = window.setTimeout(update, 250);
    });
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
    if (settle) clearTimeout(settle);
  };
}
