import { defineConfig } from "vite";

// Vite-конфиг под Tauri: фиксированный порт 1420 (совпадает с devUrl в
// tauri.conf.json), не очищать экран (чтобы видеть лог Rust-бекенда).
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "esnext", outDir: "dist" },
});
