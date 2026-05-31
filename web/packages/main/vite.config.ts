import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { resolve } from "node:path";

// Vite multi-entry:main 与 floating 都从这个 project build,产物到 web/dist/。
// Tauri 的 frontendDist 指向 web/dist/。
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    outDir: resolve(__dirname, "../../dist"),
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        floating: resolve(__dirname, "floating.html"),
      },
    },
  },
});
