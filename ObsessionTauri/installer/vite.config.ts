import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// Мини-фронт инсталлера «Obsession Setup». Ассеты бренда (глаз, шрифты)
// импортируются напрямую из ../src основного приложения — единый источник,
// без копий; поэтому dev-серверу разрешаем выход на уровень ObsessionTauri/.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
    fs: { allow: [fileURLToPath(new URL("..", import.meta.url))] },
  },
  build: {
    target: "chrome105",
    outDir: "dist",
  },
});
