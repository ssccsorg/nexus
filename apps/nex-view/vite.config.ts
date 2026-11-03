import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { boardIntoBuild } from '@ssccsorg/skene-frame/tools/vite-board.mjs';

export default defineConfig({
  plugins: [react(), tailwindcss(), boardIntoBuild()],
  // One React for the frame and this application. The frame declares React as a peer, so a second copy is the failure
  // this prevents.
  resolve: { dedupe: ['react', 'react-dom'] },
  // MapLibre's worker is resolved from its own package directory, so it is served as it ships rather than pre-bundled.
  optimizeDeps: { exclude: ['maplibre-gl'] },
  server: { port: 5292, strictPort: true },
  preview: { port: 5293, strictPort: true },
  build: { outDir: 'dist', emptyOutDir: true },
  test: { include: ['tests/**/*.test.ts'], environment: 'node' },
});
