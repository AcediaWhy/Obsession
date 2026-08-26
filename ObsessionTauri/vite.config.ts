/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Tauri ожидает фиксированный порт и не должен падать при его занятости.
const host = process.env.TAURI_DEV_HOST || "127.0.0.1";

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
    host,
    hmr: { protocol: "ws", host, port: 1421 },
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    rollupOptions: {
      input: {
        app: fileURLToPath(new URL("./index.html", import.meta.url)),
        yaniDev: fileURLToPath(new URL("./yani-dev.html", import.meta.url)),
        earLab: fileURLToPath(new URL("./ear-lab.html", import.meta.url)),
      },
    },
  },
  // Юнит-тесты редьюсеров сторов. Окружение node: тесты чистые (мокают ../lib/tauri),
  // DOM не нужен. Rust-тесты живут отдельно в src-tauri (cargo test).
  test: {
    environment: "node",
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
