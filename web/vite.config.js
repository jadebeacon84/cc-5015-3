import { defineConfig } from 'vite';

// The UI talks to the Rust kernel on the same origin when served by the
// server binary, and via this proxy during `npm run dev`.
export default defineConfig({
  server: {
    port: 5173,
    proxy: {
      '/api': { target: 'http://127.0.0.1:8787', changeOrigin: true },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
