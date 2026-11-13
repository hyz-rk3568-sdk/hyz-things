const DEFAULT_FIRST = 0.6;
const MIN_RATIO = 0.2;
const MIN_PANE_PX = 120;
const GAP_PX = 8;
const HOLD_MS = 220;
const CANCEL_DISTANCE = 8;
const STORAGE_PREFIX = 'hyz-things:question-bank-split:';

const clamp = (value, minimum, maximum) => Math.min(maximum, Math.max(minimum, value));

export function mountSplitResize(root, handle, breakpoint) {
  const query = window.matchMedia(`(min-width: ${breakpoint}px)`);
  const axisForViewport = () => query.matches ? 'horizontal' : 'vertical';
  const key = axis => `${STORAGE_PREFIX}${axis}`;
  const readRatio = axis => {
    try {
      const raw = localStorage.getItem(key(axis));
      if (raw === null || raw.trim() === '') return null;
      const value = Number(raw);
      return Number.isFinite(value) && value >= MIN_RATIO && value <= 1 - MIN_RATIO ? value : null;
    } catch { return null; }
  };
  const writeRatio = (axis, value) => {
    try {
      if (value === null) localStorage.removeItem(key(axis));
      else localStorage.setItem(key(axis), String(value));
    } catch { /* Resizing remains available when storage is disabled. */ }
  };

  let axis = axisForViewport();
  let preferred = readRatio(axis);
  let drag = null;
  let holdTimer = 0;
  let frame = 0;

  const size = () => Math.max(0, (axis === 'horizontal' ? root.clientWidth : root.clientHeight) - GAP_PX);
  const bounds = () => {
    const minimum = Math.min(0.45, Math.max(MIN_RATIO, size() > 0 ? MIN_PANE_PX / size() : MIN_RATIO));
    return [minimum, 1 - minimum];
  };
  const currentRatio = () => {
    const first = root.firstElementChild;
    if (!first || !size()) return preferred ?? DEFAULT_FIRST;
    const rectangle = first.getBoundingClientRect();
    return clamp((axis === 'horizontal' ? rectangle.width : rectangle.height) / size(), 0, 1);
  };
  const updateAccessibility = () => {
    const [minimum, maximum] = bounds();
    const percentage = Math.round(clamp(currentRatio(), minimum, maximum) * 100);
    handle.setAttribute('aria-orientation', axis === 'horizontal' ? 'vertical' : 'horizontal');
    handle.setAttribute('aria-valuemin', String(Math.round(minimum * 100)));
    handle.setAttribute('aria-valuemax', String(Math.round(maximum * 100)));
    handle.setAttribute('aria-valuenow', String(percentage));
    handle.setAttribute('aria-valuetext', `${axis === 'horizontal' ? '左侧' : '上方'} ${percentage}%`);
    handle.dataset.splitAxis = axis;
  };
  const scheduleAccessibility = () => {
    if (frame) cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => { frame = 0; updateAccessibility(); });
  };
  const apply = () => {
    if (preferred === null) {
      root.style.removeProperty('--training-split-user-first');
      root.style.removeProperty('--training-split-user-second');
    } else {
      const [minimum, maximum] = bounds();
      const actual = clamp(preferred, minimum, maximum);
      root.style.setProperty('--training-split-user-first', `${actual}fr`);
      root.style.setProperty('--training-split-user-second', `${1 - actual}fr`);
    }
    updateAccessibility();
    scheduleAccessibility();
    root.dispatchEvent(new Event('training-split-resized', { bubbles: true }));
  };
  const snapshotScroll = () => [...root.querySelectorAll('*')]
    .filter(node => node instanceof HTMLElement && (node.scrollHeight > node.clientHeight || node.scrollWidth > node.clientWidth))
    .filter(node => /auto|scroll/.test(`${getComputedStyle(node).overflowY} ${getComputedStyle(node).overflowX}`))
    .map(node => ({ node, top: node.scrollTop, left: node.scrollLeft }));
  const restoreScroll = positions => {
    for (const { node, top, left } of positions) {
      if (node.isConnected) { node.scrollTop = top; node.scrollLeft = left; }
    }
  };
  const clearHold = () => { if (holdTimer) clearTimeout(holdTimer); holdTimer = 0; };
  const endDrag = (revert, event) => {
    if (!drag || (event && event.pointerId !== drag.id)) return;
    const previous = drag;
    drag = null;
    clearHold();
    delete handle.dataset.dragging;
    if (revert && previous.active) {
      preferred = previous.preferred;
      apply();
      restoreScroll(previous.scrolls);
    } else if (!revert && previous.active) {
      writeRatio(previous.axis, preferred);
      // WebKit can clamp scrollTop while the grid is still settling on
      // pointerup. Restore the pre-drag offsets both now and after layout.
      restoreScroll(previous.scrolls);
      requestAnimationFrame(() => restoreScroll(previous.scrolls));
    }
    try { if (handle.hasPointerCapture(previous.id)) handle.releasePointerCapture(previous.id); } catch { /* Synthetic pointers have no capture. */ }
  };
  const down = event => {
    if (drag || !event.isPrimary || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    try { handle.setPointerCapture(event.pointerId); } catch { /* Synthetic pointers have no capture. */ }
    if (root.contains(document.activeElement) && document.activeElement instanceof HTMLTextAreaElement) document.activeElement.blur();
    const [minimum, maximum] = bounds();
    drag = {
      id: event.pointerId, axis, x: event.clientX, y: event.clientY,
      start: clamp(currentRatio(), minimum, maximum), preferred,
      active: event.pointerType === 'mouse', scrolls: snapshotScroll(),
    };
    if (drag.active) handle.dataset.dragging = 'true';
    else holdTimer = window.setTimeout(() => {
      holdTimer = 0;
      if (drag && drag.id === event.pointerId) { drag.active = true; handle.dataset.dragging = 'true'; }
    }, HOLD_MS);
  };
  const move = event => {
    if (!drag || event.pointerId !== drag.id) return;
    event.preventDefault();
    event.stopPropagation();
    const dx = event.clientX - drag.x, dy = event.clientY - drag.y;
    if (!drag.active) {
      if (Math.hypot(dx, dy) > CANCEL_DISTANCE) endDrag(true, event);
      return;
    }
    const [minimum, maximum] = bounds();
    preferred = clamp(drag.start + (axis === 'horizontal' ? dx : dy) / Math.max(1, size()), minimum, maximum);
    apply();
    restoreScroll(drag.scrolls);
  };
  const up = event => { if (drag && event.pointerId === drag.id) { event.preventDefault(); event.stopPropagation(); endDrag(false, event); } };
  const cancel = event => { if (drag && event.pointerId === drag.id) { event.stopPropagation(); endDrag(true, event); } };
  const reset = event => {
    event.preventDefault();
    event.stopPropagation();
    endDrag(true);
    preferred = null;
    writeRatio(axis, null);
    apply();
  };
  const keyboard = event => {
    if (event.key === 'Home') { reset(event); return; }
    const increase = axis === 'horizontal' ? 'ArrowRight' : 'ArrowDown';
    const decrease = axis === 'horizontal' ? 'ArrowLeft' : 'ArrowUp';
    if (event.key !== increase && event.key !== decrease) return;
    event.preventDefault();
    event.stopPropagation();
    const [minimum, maximum] = bounds();
    const positions = snapshotScroll();
    preferred = clamp(currentRatio() + (event.key === increase ? 0.025 : -0.025), minimum, maximum);
    apply();
    restoreScroll(positions);
    writeRatio(axis, preferred);
  };
  const resize = () => {
    const next = axisForViewport();
    if (next !== axis) {
      endDrag(true);
      axis = next;
      preferred = readRatio(axis);
    }
    apply();
  };
  const noMenu = event => event.preventDefault();
  handle.addEventListener('pointerdown', down);
  handle.addEventListener('pointermove', move);
  handle.addEventListener('pointerup', up);
  handle.addEventListener('pointercancel', cancel);
  handle.addEventListener('lostpointercapture', cancel);
  handle.addEventListener('keydown', keyboard);
  handle.addEventListener('dblclick', reset);
  handle.addEventListener('contextmenu', noMenu);
  window.addEventListener('resize', resize);
  const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(resize) : null;
  observer?.observe(root);
  if (root.firstElementChild) observer?.observe(root.firstElementChild);
  apply();
  return () => {
    endDrag(true);
    clearHold();
    if (frame) cancelAnimationFrame(frame);
    observer?.disconnect();
    window.removeEventListener('resize', resize);
    handle.removeEventListener('pointerdown', down);
    handle.removeEventListener('pointermove', move);
    handle.removeEventListener('pointerup', up);
    handle.removeEventListener('pointercancel', cancel);
    handle.removeEventListener('lostpointercapture', cancel);
    handle.removeEventListener('keydown', keyboard);
    handle.removeEventListener('dblclick', reset);
    handle.removeEventListener('contextmenu', noMenu);
    root.style.removeProperty('--training-split-user-first');
    root.style.removeProperty('--training-split-user-second');
  };
}
