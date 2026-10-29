import type { Locator } from '@playwright/test';

// WebKit composites incremental strokes and synchronous replay with different
// edge alpha. Compare every occupied pixel and opaque ink color exactly.
export async function inkSnapshot(canvas: Locator): Promise<string> {
  return canvas.evaluate(async element => {
    const surface = element as HTMLCanvasElement;
    const context = surface.getContext('2d');
    if (!context) throw new Error('ink canvas context missing');
    const pixels = context.getImageData(0, 0, surface.width, surface.height).data;
    const coverage = new Uint8Array(pixels.length / 4);
    const colors = new Set<string>();
    for (let offset = 0; offset < pixels.length; offset += 4) {
      coverage[offset / 4] = pixels[offset + 3] > 0 ? 1 : 0;
      if (pixels[offset + 3] === 255) {
        colors.add(`${pixels[offset]},${pixels[offset + 1]},${pixels[offset + 2]}`);
      }
    }
    const digest = await crypto.subtle.digest('SHA-256', coverage);
    const hash = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, '0')).join('');
    return JSON.stringify({ width: surface.width, height: surface.height, colors: [...colors].sort(), hash });
  });
}
