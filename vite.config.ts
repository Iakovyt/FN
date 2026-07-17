import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// @tauri-apps/cli sets TAURI_DEV_HOST when running on a device/emulator.
const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [svelte()],

  // Tauri expects a fixed port, fail if that port is not available.
  clearScreen: false,
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
      // Don't watch the Rust backend from the Vite dev server.
      ignored: ["**/src-tauri/**"],
    },
  },

  // Produce a smaller bundle; Tauri targets modern webviews (Edge WebView2).
  build: {
    target: "esnext",
    minify: "esbuild",
    sourcemap: false,
  },
});
