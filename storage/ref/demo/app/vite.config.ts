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
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // The warning guards against an application that has grown, and this one is six kilobytes: the rest is two engines
    // and the frame drawn with them. The limit sits above the largest of those so a build that starts to carry the
    // application's own weight still says so.
    chunkSizeWarningLimit: 1200,
    // Keeping each engine in its own file lets a browser cache what a deploy does not change and fetch the rest beside
    // it instead of behind it.
    rollupOptions: {
      output: {
        manualChunks: (id: string) => {
          if (id.includes('node_modules/maplibre-gl')) return 'maplibre';
          // The renderer sits in its own file. Its React bindings travel with the frame, because they are what the
          // frame's shell reading is drawn with; separating those two files from each other produced a chunk cycle.
          if (id.includes('node_modules/three/')) return 'three';
          if (id.includes('node_modules/@react-three')) return 'skene';
          if (/node_modules\/(react|react-dom|scheduler)\//.test(id)) return 'react';
          if (id.includes('node_modules/@ssccsorg/')) return 'skene';
          return undefined;
        },
      },
    },
  },
  test: { include: ['tests/**/*.test.ts'], environment: 'node' },
});
