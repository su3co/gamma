import { defineConfig } from "vite";

// The interface is loaded by the Tauri shell (from dist/) and served by the
// hub to browsers; relative asset paths work in both.
export default defineConfig({
  base: "./",
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true, sourcemap: false, chunkSizeWarningLimit: 900 },
});
