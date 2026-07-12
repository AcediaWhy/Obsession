import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Tauri ожидает фиксированный порт и не должен падать при его занятости.
const host = process.env.TAURI_DEV_HOST;

// Версия приложения — единый источник: tauri.conf.json (та же, что в бандле и
// установщике). Инжектим как глобальную константу __APP_VERSION__, чтобы UI не
// хардкодил строку и не расходился с релизом при следующем бампе.
const appVersion = JSON.parse(
  readFileSync(fileURLToPath(new URL("./src-tauri/tauri.conf.json", import.meta.url)), "utf-8"),
).version as string;

export default defineConfig({
  plugins: [react()],
  // Предотвращаем очистку экрана vite, чтобы видеть логи Rust.
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
