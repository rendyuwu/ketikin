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
    // Match the webview engine shipped on each platform. Both values are the
    // floor the source is actually written against, not a guess: esbuild passes
    // CSS it cannot lower straight through rather than failing, so a target set
    // too low reads as a guarantee nothing is enforcing.
    //
    // `safari15.4` covers Linux and macOS, and comes from the stylesheet: it
    // already uses `:focus-visible` and unprefixed `appearance` (Safari 15.4)
    // on top of `inset` and `margin-inline` (14.1).
    //
    // Linux: WebKitGTK shipped that same WebKit with `:focus-visible` in 2.36.
    // The webkit2gtk-4.1 API Tauri v2 links against first appeared in 2.34, so
    // the dependency alone is two releases short of this line — but 4.1 only
    // became a distro's default WebKitGTK from 2.38 (GNOME 43) onward, so
    // anything actually shipping it is above 2.36.
    //
    // macOS: `minimumSystemVersion` in tauri.conf.json is 10.15, and Catalina's
    // last WebKit is Safari 15.6.1, so a patched floor machine clears this and
    // an unpatched one does not — as has been true since the focus work in #21
    // landed, which the old `safari13` here concealed rather than prevented.
    //
    // The floor this rules out is worth naming: `color-mix()` is Safari 16.2,
    // so it stays out of the stylesheet, and that is now a fact with a number
    // behind it rather than a hunch.
    target:
      process.env.TAURI_ENV_PLATFORM == "windows" ? "chrome105" : "safari15.4",
    // Don't minify for debug builds; otherwise use Vite's default minifier.
    minify: !process.env.TAURI_ENV_DEBUG,
    // Produce sourcemaps for debug builds.
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
