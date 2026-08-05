import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    // CodeMirror breaks hard if two copies of these are ever loaded —
    // `instanceof` checks fail with "Unrecognized extension value". Force a
    // single instance no matter how the dependency graph resolves.
    dedupe: ["@codemirror/state", "@codemirror/view", "@codemirror/language"],
  },
  // Tauri expects a fixed port and fails rather than silently moving.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Rebuilding the frontend on Rust changes just costs time.
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
