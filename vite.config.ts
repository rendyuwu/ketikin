import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],

  // Prevent Vite from obscuring Rust errors.
  clearScreen: false,

  // Tauri expects a fixed port and fails if it is not available.
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Rust rebuilds are driven by cargo, not Vite.
      ignored: ["**/src-tauri/**"],
    },
  },

  // Only these prefixes are exposed to the frontend.
  envPrefix: ["VITE_", "TAURI_ENV_*"],

  build: {
    // Match the webview engine shipped on each platform.
    target: process.env.TAURI_ENV_PLATFORM == "windows" ? "chrome105" : "safari13",
    // Don't minify for debug builds; otherwise use Vite's default minifier.
    minify: !process.env.TAURI_ENV_DEBUG,
    // Produce sourcemaps for debug builds.
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
