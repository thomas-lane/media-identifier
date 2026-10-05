/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` serves the UI for `tauri dev` on the fixed port tauri.conf.json expects.
// `npm run dev:mock` (mode "mock") serves the same UI in a normal browser against the mock
// backend in src/api/mock.ts.
export default defineConfig(({ mode }) => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: mode === "mock" ? 5173 : 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "es2022",
    sourcemap: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
  },
}));
