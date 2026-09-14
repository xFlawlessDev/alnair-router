import { fileURLToPath, URL } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vite';

// Where the alnair-router server listens during development. `npm run dev`
// proxies `/api` and `/v1` there so the browser stays same-origin.
const routerTarget = process.env.ALNAIR_ROUTER_URL ?? 'http://127.0.0.1:7878';

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    proxy: {
      '/api': { target: routerTarget, changeOrigin: true },
      '/v1': { target: routerTarget, changeOrigin: true },
    },
  },
});
