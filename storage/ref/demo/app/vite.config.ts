import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { boardIntoBuild } from '@ssccsorg/skene-frame/tools/vite-board.mjs';
import { SKENE_CHUNK_LIMIT, SKENE_DEDUPE, skeneChunks } from '@ssccsorg/skene-frame/tools/vite-chunks.mjs';

export default defineConfig({
  plugins: [react(), tailwindcss(), boardIntoBuild()],
  // One React for the frame and this application. The frame declares React as a peer, so a second copy is the failure
  // this prevents.
  resolve: { dedupe: SKENE_DEDUPE },
  // MapLibre's worker is resolved from its own package directory, so it is served as it ships rather than pre-bundled.
  optimizeDeps: { exclude: ['maplibre-gl'] },
  server: { port: 5292, strictPort: true },
  preview: { port: 5293, strictPort: true },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // The warning guards against an application that has grown, and this one is six kilobytes: the rest is two engines
    // and the frame drawn with them. The limit is where the largest engine sits, which is the frame's number to keep.
    chunkSizeWarningLimit: SKENE_CHUNK_LIMIT,
    // Keeping each engine in its own file lets a browser cache what a deploy does not change and fetch the rest beside
    // it instead of behind it. Which file is which is the frame's rule, because a second deployment cuts its build the
    // same way.
    rollupOptions: { output: { manualChunks: skeneChunks } },
  },
  test: { include: ['tests/**/*.test.ts'], environment: 'node' },
});
