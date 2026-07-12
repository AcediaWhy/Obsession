#!/usr/bin/env node
// Сборка фирменного инсталлера «Obsession Setup» (одинокий exe):
//   1) основное приложение → NSIS-движок (Obsession_<v>_x64-setup.exe);
//   2) движок вшивается в оболочку как payload (include_bytes!);
//   3) оболочка собирается БЕЗ бандла (фронт вшит в exe) → dist-release/.
// Флаг --skip-app: не пересобирать основное приложение, взять готовый setup.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const installerDir = path.join(root, "installer");
const args = new Set(process.argv.slice(2));
const mb = (bytes) => (bytes / 1024 / 1024).toFixed(1) + " МБ";

function run(cmd, cwd, extraEnv = {}) {
  console.log(`\n> ${cmd}   (cwd: ${path.relative(root, cwd) || "."})`);
  const r = spawnSync(cmd, {
    cwd,
    stdio: "inherit",
    shell: true,
    env: { ...process.env, ...extraEnv },
  });
  if (r.status !== 0) {
    console.error(`\n✖ Команда упала (код ${r.status}): ${cmd}`);
    process.exit(r.status ?? 1);
  }
}

// Версия — единственный источник: корневой tauri.conf.json.
const conf = JSON.parse(fs.readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const version = conf.version;
const setupSrc = path.join(
  root,
  "src-tauri", "target", "release", "bundle", "nsis",
  `Obsession_${version}_x64-setup.exe`
);

// 1. NSIS-движок основного приложения.
if (!args.has("--skip-app") || !fs.existsSync(setupSrc)) {
  run("npm run tauri build", root);
}
if (!fs.existsSync(setupSrc)) {
  console.error(`✖ Не найден NSIS-движок: ${setupSrc}`);
  process.exit(1);
}

// 2. Payload внутрь оболочки.
const payload = path.join(installerDir, "src-tauri", "payload", "payload.exe");
fs.mkdirSync(path.dirname(payload), { recursive: true });
fs.copyFileSync(setupSrc, payload);
console.log(`\npayload: ${path.relative(root, payload)} (${mb(fs.statSync(payload).size)})`);

// 3. Оболочка: зависимости, синхронизация версии (merge-конфигом, файлы в гите
//    не трогаем), сборка без бандла.
if (!fs.existsSync(path.join(installerDir, "node_modules"))) {
  run("npm install", installerDir);
}
const genConf = path.join(installerDir, "src-tauri", "gen-version.conf.json");
fs.writeFileSync(genConf, JSON.stringify({ version }) + "\n");
if (version !== JSON.parse(fs.readFileSync(path.join(installerDir, "src-tauri", "tauri.conf.json"), "utf8")).version) {
  console.warn(`⚠ Версия оболочки в installer/src-tauri/tauri.conf.json отстала от ${version} — на сборку не влияет (merge-конфиг), но стоит синхронизировать в гите.`);
}
run(
  "npx tauri build --no-bundle --config src-tauri/gen-version.conf.json",
  installerDir,
  { VITE_APP_VERSION: version }
);

// 4. Итоговый артефакт.
const built = path.join(installerDir, "src-tauri", "target", "release", "obsession-setup.exe");
if (!fs.existsSync(built)) {
  console.error(`✖ Не найден собранный бинарь оболочки: ${built}`);
  process.exit(1);
}
const outDir = path.join(root, "dist-release");
fs.mkdirSync(outDir, { recursive: true });
const out = path.join(outDir, `Obsession-Setup_${version}_x64.exe`);
fs.copyFileSync(built, out);
console.log(`\n✔ Готово: ${path.relative(root, out)} (${mb(fs.statSync(out).size)})`);
