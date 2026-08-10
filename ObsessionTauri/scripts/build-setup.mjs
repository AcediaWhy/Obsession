#!/usr/bin/env node
// Сборка фирменного инсталлера «Obsession Setup» (одинокий exe):
//   1) основное приложение собирается без current-user bundle;
//   2) exact release-layout упаковывается в authenticated machine payload;
//   3) medium UI + native UAC worker собираются в один exe → dist-release/.
// Флаг --skip-app: не пересобирать основное приложение, взять готовый layout.

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const installerDir = path.join(root, "installer");
const args = new Set(process.argv.slice(2));
const mb = (bytes) => (bytes / 1024 / 1024).toFixed(1) + " МБ";
const MACHINE_PAYLOAD_MAGIC = Buffer.from("OBSMACH1", "ascii");
const MAX_MACHINE_PAYLOAD_FILES = 4096;
const MAX_MACHINE_PAYLOAD_FILE_BYTES = 256 * 1024 * 1024;
const MAX_MACHINE_PAYLOAD_MANIFEST_BYTES = 1024 * 1024;

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

function safeMachinePath(value) {
  const normalized = value.split(path.sep).join("/");
  if (
    !normalized ||
    Buffer.byteLength(normalized, "utf8") > 240 ||
    !/^[\x20-\x7e]+$/.test(normalized) ||
    normalized.includes("\\") ||
    normalized.includes(":") ||
    normalized.startsWith("/") ||
    normalized.endsWith("/")
  ) {
    throw new Error(`Недопустимый machine payload path: ${JSON.stringify(value)}`);
  }
  const components = normalized.split("/");
  if (components.some((component) => !component || component === "." || component === "..")) {
    throw new Error(`Небезопасный machine payload path: ${normalized}`);
  }
  for (const component of components) {
    if (component.endsWith(" ") || component.endsWith(".")) {
      throw new Error(`Недопустимое Windows-имя в machine payload: ${normalized}`);
    }
    const stem = component.split(".", 1)[0].toUpperCase();
    if (
      ["CON", "PRN", "AUX", "NUL"].includes(stem) ||
      /^(COM|LPT)[1-9]$/.test(stem)
    ) {
      throw new Error(`Зарезервированное Windows-имя в machine payload: ${normalized}`);
    }
  }
  return normalized;
}

function collectMachineFiles(source, destination, output) {
  const metadata = fs.lstatSync(source);
  if (metadata.isSymbolicLink()) {
    throw new Error(`Machine payload не может содержать symlink/junction: ${source}`);
  }
  if (metadata.isDirectory()) {
    for (const entry of fs.readdirSync(source, { withFileTypes: true }).sort((left, right) =>
      left.name.localeCompare(right.name, "en"),
    )) {
      collectMachineFiles(
        path.join(source, entry.name),
        destination ? `${destination}/${entry.name}` : entry.name,
        output,
      );
    }
    return;
  }
  if (!metadata.isFile() || metadata.size > MAX_MACHINE_PAYLOAD_FILE_BYTES) {
    throw new Error(`Недопустимый machine payload file: ${source}`);
  }
  output.push({ source, path: safeMachinePath(destination), size: metadata.size });
}

function prepareMachinePayload(conf, version) {
  const releaseDir = path.join(root, "src-tauri", "target", "release");
  const files = [];
  collectMachineFiles(path.join(releaseDir, "obsession.exe"), "obsession.exe", files);
  const destinations = Object.values(conf.bundle?.resources ?? {});
  if (destinations.length === 0) {
    throw new Error("Tauri bundle не содержит machine resources");
  }
  for (const destination of destinations) {
    const relative = safeMachinePath(destination);
    collectMachineFiles(path.join(releaseDir, ...relative.split("/")), relative, files);
  }
  files.sort((left, right) => left.path.localeCompare(right.path, "en"));
  if (files.length === 0 || files.length > MAX_MACHINE_PAYLOAD_FILES) {
    throw new Error(`Недопустимое число machine payload files: ${files.length}`);
  }
  const seen = new Set();
  const payloadBuffers = [];
  const records = files.map((file) => {
    const key = file.path.toLowerCase();
    if (seen.has(key)) throw new Error(`Дублирующий machine payload path: ${file.path}`);
    seen.add(key);
    const bytes = fs.readFileSync(file.source);
    if (bytes.length !== file.size) {
      throw new Error(`Machine payload file изменился во время чтения: ${file.source}`);
    }
    payloadBuffers.push(bytes);
    return {
      path: file.path,
      size: bytes.length,
      sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
    };
  });
  for (const required of [
    "obsession.exe",
    "runtime/Obsession.Runtime.exe",
    "runtime/runtime-manifest.json",
  ]) {
    if (!seen.has(required.toLowerCase())) {
      throw new Error(`Machine payload не содержит обязательный файл: ${required}`);
    }
  }
  const manifest = Buffer.from(
    `${JSON.stringify({
      schema_version: 1,
      product_id: "com.vlarpsu.obsession",
      version,
      files: records,
    })}\n`,
    "utf8",
  );
  if (manifest.length === 0 || manifest.length > MAX_MACHINE_PAYLOAD_MANIFEST_BYTES) {
    throw new Error(`Machine payload manifest слишком велик: ${manifest.length}`);
  }
  const header = Buffer.alloc(16);
  MACHINE_PAYLOAD_MAGIC.copy(header, 0);
  header.writeUInt32LE(manifest.length, 8);
  header.writeUInt32LE(records.length, 12);
  const output = path.join(installerDir, "src-tauri", "payload", "machine-payload.bin");
  fs.writeFileSync(output, Buffer.concat([header, manifest, ...payloadBuffers]));
  console.log(
    `machine payload: ${path.relative(root, output)} ` +
      `(${records.length} файлов, ${mb(fs.statSync(output).size)})`,
  );
}

// Версия — единственный источник: корневой tauri.conf.json.
const conf = JSON.parse(fs.readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const version = conf.version;
const appExe = path.join(root, "src-tauri", "target", "release", "obsession.exe");

// 1. Main application and its exact release resource layout, without a
// current-user NSIS bundle.
if (!args.has("--skip-app") || !fs.existsSync(appExe)) {
  run("npm run tauri build -- --no-bundle", root);
}
if (!fs.existsSync(appExe)) {
  console.error(`✖ Не найден release binary: ${appExe}`);
  process.exit(1);
}

// 2. Authenticated per-machine payload embedded directly into the wrapper.
fs.mkdirSync(path.join(installerDir, "src-tauri", "payload"), { recursive: true });
prepareMachinePayload(conf, version);

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
  {
    VITE_APP_VERSION: version,
    OBSESSION_MACHINE_PAYLOAD_VERSION: version,
  }
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
