import { fileURLToPath, URL } from "node:url";

import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      // Keep in sync with vite.config.ts: shared brand artwork.
      "@assets": fileURLToPath(new URL("../../assets", import.meta.url)),
    },
  },
  server: {
    // The artwork lives outside this package.
    fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    setupFiles: ["./src/test/setup.ts"],
  },
});
