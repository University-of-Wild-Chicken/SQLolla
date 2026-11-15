import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

/**
 * Vite development/build configuration plus the jsdom test environment.
 *
 * The Graphviz worker is emitted as a first-party asset by Vite; the app never
 * fetches a renderer from a CDN.
 */
export default defineConfig({
  plugins: [react()],
  worker: { format: "es" },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        workbench: resolve(import.meta.dirname, "index.html"),
        docs: resolve(import.meta.dirname, "docs/index.html"),
      },
    },
  },
  server: {
    watch: {
      ignored: ["**/.tools/**", "**/e2e/screenshots/**", "**/.*.tmpdir/**", "**/*.tmpdir/**", "**/*.tmp"],
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    restoreMocks: true,
    css: false,
  },
});
