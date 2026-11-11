import { defineConfig } from 'vite';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const directory = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  build: {
    outDir: resolve(directory, '../ts-dist'),
    emptyOutDir: true,
    lib: { entry: resolve(directory, 'src/index.ts'), formats: ['es'], fileName: () => 'browser-bridge.js' },
  },
});
