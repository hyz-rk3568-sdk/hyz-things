import { autoUpdate, computePosition, flip, shift } from '@floating-ui/dom';

export function mountFloatingMenu(reference: HTMLElement, floating: HTMLElement): () => void {
  let disposed = false;
  let generation = 0;
  const update = () => {
    const current = ++generation;
    void computePosition(reference, floating, {
      placement: 'bottom-end',
      middleware: [flip(), shift({ padding: 8 })],
    }).then(({ x, y, strategy }) => {
      if (disposed || current !== generation) return;
      Object.assign(floating.style, { position: strategy, left: `${x}px`, top: `${y}px`, right: 'auto', bottom: 'auto' });
    }).catch((error: unknown) => console.error('Floating menu positioning failed', error));
  };
  const cleanup = autoUpdate(reference, floating, update);
  return () => {
    if (disposed) return;
    disposed = true;
    cleanup();
  };
}
