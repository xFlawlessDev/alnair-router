import { fileURLToPath, URL } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vite";

import { resolveRouterTarget } from "./src/lib/routerTarget.ts";

// Where the alnair-router server listens during development. `pnpm dev`
// proxies `/api` and `/v1` there so the browser stays same-origin. Besides an
// explicit ALNAIR_ROUTER_URL, the target follows the address the running router
// recorded, so `--port` and `server.port` changes are picked up automatically.
const routerTarget = resolveRouterTarget();
console.log(`[alnair-router] dev proxy -> ${routerTarget}`);

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      // Brand artwork shared with the tray icon and the installers.
      "@assets": fileURLToPath(new URL("../../assets", import.meta.url)),
    },
  },
  server: {
    // The artwork lives outside this package; let the dev server read it.
    fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] },
    proxy: {
      "/api": { target: routerTarget, changeOrigin: true },
      "/v1": { target: routerTarget, changeOrigin: true },
    },
  },
});
