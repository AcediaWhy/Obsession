#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const serviceDir = path.join(root, "runtime-service");
const resourcesRoot = path.join(root, "src-tauri", "resources");
const runtimeOutput = path.join(resourcesRoot, "runtime");
const serviceSource = path.join(
  serviceDir,
  "target",
  "release",
  "obsession-runtime-service.exe",
);
const serviceRelative = "runtime/Obsession.Runtime.exe";
const manifestRelative = "runtime/runtime-manifest.json";
const engineRelative = "bin/winws.exe";
const tgProxyRelative = "bin/obsession-tg-proxy.exe";
const zapret2EngineRelative = "bin/zapret2/winws2.exe";
const zapret2PackManifestRelative = "strategy-packs/builtin/manifest.json";

const MAX_CONFIG_BYTES = 512 * 1024;
const MAX_CONFIG_TOKENS = 65_536;
const MAX_TOKEN_BYTES = 8 * 1024;
const MAX_RESOURCE_BYTES = 256 * 1024 * 1024;
const MAX_RESOURCE_PATH_BYTES = 240;
const MAX_STRATEGY_DEPENDENCIES = 32;

const CATEGORY_DIRECTORIES = [
  ["atrisk", "atRisk"],
  ["discord", "discord"],
  ["gaming", "gaming"],
  ["universal", "universal"],
  ["youtube_twitch", "youtubeTwitch"],
];

const PATH_OPTIONS = new Set([
  "--hostlist",
  "--hostlist-auto",
  "--hostlist-exclude",
  "--ipset",
  "--ipset-exclude",
  "--dpi-desync-fake-tls",
  "--dpi-desync-fake-quic",
  "--dpi-desync-fake-unknown-tcp",
  "--dpi-desync-fake-unknown-udp",
  "--dpi-desync-split-seqovl-pattern",
  "--dpi-desync-multisplit-seqovl-pattern",
]);

const COMMON_RUNTIME_DEPENDENCIES = [
  "bin/WinDivert.dll",
  "bin/WinDivert64.sys",
  "bin/cygwin1.dll",
];

const ZAPRET2_RUNTIME_DEPENDENCIES = [
  "bin/zapret2/WinDivert.dll",
  "bin/zapret2/WinDivert64.sys",
  "bin/zapret2/cygwin1.dll",
];

const ZAPRET2_CATEGORY_NAMES = new Map([
  ["discord", "discord"],
  ["youtube_twitch", "youtubeTwitch"],
  ["gaming", "gaming"],
]);

function run(command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} завершился с кодом ${result.status ?? "unknown"}`);
  }
}

function assertExactGeneratedDirectory(candidate, expected) {
  if (path.resolve(candidate) !== path.resolve(expected)) {
    throw new Error(`Отказ от очистки неожиданного generated-каталога: ${candidate}`);
  }
}

function recreateDirectory(directory, expected) {
  assertExactGeneratedDirectory(directory, expected);
  fs.rmSync(directory, { recursive: true, force: true });
  fs.mkdirSync(directory, { recursive: true });
}

function toPosix(relative) {
  return relative.split(path.sep).join("/");
}

function normalizeReference(value) {
  if (!value || !/^[\x20-\x7e]+$/.test(value) || value.includes(":")) {
    throw new Error(`Недопустимая resource-ссылка в Legacy config: ${JSON.stringify(value)}`);
  }
  const normalized = value.replaceAll("\\", "/");
  if (normalized.startsWith("/") || normalized.endsWith("/")) {
    throw new Error(`Resource-ссылка должна быть относительной: ${value}`);
  }
  const components = normalized.split("/");
  if (components.some((component) => !component || component === "." || component === "..")) {
    throw new Error(`Resource-ссылка содержит небезопасный компонент: ${value}`);
  }
  return normalized.toLowerCase();
}

function looksLikePath(value) {
  return (
    value.includes("/") ||
    value.includes("\\") ||
    value.startsWith(".") ||
    (value.length > 1 && /^[A-Za-z]$/.test(value[0]) && value[1] === ":")
  );
}

function pushToken(tokens, token) {
  if (Buffer.byteLength(token, "utf8") > MAX_TOKEN_BYTES) {
    throw new Error("Legacy config содержит слишком длинный token");
  }
  if (tokens.length >= MAX_CONFIG_TOKENS) {
    throw new Error("Legacy config содержит слишком много tokens");
  }
  tokens.push(token);
}

function tokenizeConfig(source, relative) {
  if (Buffer.byteLength(source, "utf8") > MAX_CONFIG_BYTES) {
    throw new Error(`${relative}: config превышает ${MAX_CONFIG_BYTES} bytes`);
  }
  const tokens = [];
  let token = "";
  let quote = null;
  let comment = false;

  for (const character of source) {
    const code = character.codePointAt(0);
    if (character === "\0" || (code < 0x20 && !["\r", "\n", "\t"].includes(character))) {
      throw new Error(`${relative}: config содержит control character`);
    }
    if (comment) {
      if (character === "\n") comment = false;
      continue;
    }
    if (quote !== null) {
      if (character === quote) quote = null;
      else token += character;
    } else if (character === "#" && token.length === 0) {
      comment = true;
    } else if (character === "#") {
      token += character;
    } else if (character === '"' || character === "'") {
      quote = character;
    } else if (/\s/u.test(character)) {
      if (token) {
        pushToken(tokens, token);
        token = "";
      }
    } else {
      token += character;
    }
    if (Buffer.byteLength(token, "utf8") > MAX_TOKEN_BYTES) {
      throw new Error(`${relative}: config token превышает ${MAX_TOKEN_BYTES} bytes`);
    }
  }
  if (quote !== null) throw new Error(`${relative}: незакрытая кавычка`);
  if (token) pushToken(tokens, token);
  if (tokens.length === 0) throw new Error(`${relative}: config не содержит options`);
  return tokens;
}

function configDependencies(source, relative) {
  const tokens = tokenizeConfig(source, relative);
  const dependencies = new Set();
  for (let cursor = 0; cursor < tokens.length; cursor += 1) {
    const token = tokens[cursor];
    if (!token.startsWith("--")) {
      throw new Error(`${relative}: свободный argv token запрещён: ${token}`);
    }
    const separator = token.indexOf("=");
    const option = separator === -1 ? token : token.slice(0, separator);
    if (!/^--[a-z0-9-]{1,96}$/.test(option)) {
      throw new Error(`${relative}: недопустимое имя option: ${option}`);
    }
    let value = separator === -1 ? null : token.slice(separator + 1);
    if (value === null && tokens[cursor + 1] && !tokens[cursor + 1].startsWith("--")) {
      cursor += 1;
      value = tokens[cursor];
    }
    if (PATH_OPTIONS.has(option)) {
      if (!value) throw new Error(`${relative}: ${option} не содержит resource path`);
      dependencies.add(normalizeReference(value));
    } else if (value !== null && looksLikePath(value)) {
      throw new Error(`${relative}: необъявленная path-like option ${option}=${value}`);
    }
  }
  return dependencies;
}

function indexResourceTree(directory) {
  const index = new Map();
  const visit = (current) => {
    const entries = fs.readdirSync(current, { withFileTypes: true }).sort((a, b) =>
      a.name.localeCompare(b.name, "en"),
    );
    for (const entry of entries) {
      const absolute = path.join(current, entry.name);
      const metadata = fs.lstatSync(absolute);
      if (metadata.isSymbolicLink()) {
        throw new Error(`Runtime resource не может быть symlink/reparse: ${absolute}`);
      }
      if (metadata.isDirectory()) {
        visit(absolute);
      } else if (metadata.isFile()) {
        const relative = toPosix(path.relative(directory, absolute));
        const key = normalizeReference(relative);
        if (index.has(key)) throw new Error(`Дублирующий runtime resource: ${relative}`);
        index.set(key, relative);
      }
    }
  };
  visit(directory);
  return index;
}

function resolveResource(index, reference, context) {
  const normalized = normalizeReference(reference);
  const actual = index.get(normalized);
  if (!actual) throw new Error(`${context}: отсутствует runtime resource ${reference}`);
  return actual;
}

function resourceRecord(relative) {
  if (Buffer.byteLength(relative, "utf8") > MAX_RESOURCE_PATH_BYTES) {
    throw new Error(`Слишком длинный runtime resource path: ${relative}`);
  }
  const absolute = path.join(resourcesRoot, ...relative.split("/"));
  const metadata = fs.statSync(absolute);
  if (!metadata.isFile() || metadata.size > MAX_RESOURCE_BYTES) {
    throw new Error(`Недопустимый runtime resource: ${relative}`);
  }
  const sha256 = crypto.createHash("sha256").update(fs.readFileSync(absolute)).digest("hex");
  return { path: relative, size: metadata.size, sha256 };
}

function assertSingleManifestRecord(manifest, relative, label) {
  const resourceKey = relative.toLowerCase();
  const records = manifest.engines.flatMap((engine) =>
    engine.files.filter((file) => file.path.toLowerCase() === resourceKey),
  );
  if (records.length !== 1) {
    throw new Error(
      `Runtime manifest должен аутентифицировать ${label} ровно один раз, найдено: ${records.length}`,
    );
  }
}

function generateManifest() {
  const resourceIndex = indexResourceTree(resourcesRoot);
  const files = new Set([
    resolveResource(resourceIndex, serviceRelative, "service"),
    resolveResource(resourceIndex, engineRelative, "Legacy engine"),
    resolveResource(resourceIndex, tgProxyRelative, "Telegram proxy"),
  ]);
  const commonDependencies = COMMON_RUNTIME_DEPENDENCIES.map((reference) =>
    resolveResource(resourceIndex, reference, "Legacy runtime"),
  );
  commonDependencies.forEach((dependency) => files.add(dependency));

  const strategies = [];
  for (const [directory, category] of CATEGORY_DIRECTORIES) {
    const configRoot = path.join(resourcesRoot, "configs", directory);
    const configs = fs
      .readdirSync(configRoot, { withFileTypes: true })
      .filter((entry) => entry.isFile() && entry.name.endsWith(".conf"))
      .map((entry) => entry.name)
      .sort((a, b) => a.localeCompare(b, "en"));
    if (configs.length === 0) throw new Error(`Нет Legacy configs для ${directory}`);

    for (const name of configs) {
      const artifact = resolveResource(
        resourceIndex,
        `configs/${directory}/${name}`,
        `${directory}/${name}`,
      );
      const source = fs.readFileSync(path.join(configRoot, name), "utf8");
      const referenced = [...configDependencies(source, artifact)].map((reference) =>
        resolveResource(resourceIndex, reference, artifact),
      );
      const dependencies = [...new Set([...commonDependencies, ...referenced])].sort((a, b) =>
        a.localeCompare(b, "en"),
      );
      if (dependencies.length > MAX_STRATEGY_DEPENDENCIES) {
        throw new Error(`${artifact}: слишком много dependencies (${dependencies.length})`);
      }
      files.add(artifact);
      dependencies.forEach((dependency) => files.add(dependency));
      strategies.push({ id: name, category, artifact, dependencies });
    }
  }

  const manifest = {
    schema_version: 1,
    engines: [
      {
        engine: "legacy",
        executable: resolveResource(resourceIndex, engineRelative, "Legacy engine"),
        files: [...files]
          .sort((a, b) => a.localeCompare(b, "en"))
          .map(resourceRecord),
        strategies,
      },
    ],
  };

  const packManifest = JSON.parse(
    fs.readFileSync(
      path.join(resourcesRoot, ...zapret2PackManifestRelative.split("/")),
      "utf8",
    ),
  );
  if (
    packManifest.schema_version !== 1 ||
    !Array.isArray(packManifest.files) ||
    !Array.isArray(packManifest.strategies) ||
    packManifest.files.length === 0 ||
    packManifest.strategies.length === 0
  ) {
    throw new Error("Встроенный Zapret2 Strategy Pack имеет неверную схему");
  }
  const packRoot = "strategy-packs/builtin";
  const packResources = packManifest.files.map((file) =>
    resolveResource(resourceIndex, `${packRoot}/${file.path}`, "Zapret2 Strategy Pack"),
  );
  const zapret2Common = ZAPRET2_RUNTIME_DEPENDENCIES.map((reference) =>
    resolveResource(resourceIndex, reference, "Zapret2 runtime"),
  );
  const zapret2Strategies = [];
  const zapret2Files = new Set([
    resolveResource(resourceIndex, zapret2EngineRelative, "Zapret2 engine"),
    resolveResource(resourceIndex, zapret2PackManifestRelative, "Zapret2 Strategy Pack"),
    ...zapret2Common,
    ...packResources,
  ]);
  for (const [category, wireCategory] of ZAPRET2_CATEGORY_NAMES) {
    const profiles = packManifest.strategies.filter((strategy) => strategy.category === category);
    if (profiles.length === 0) {
      throw new Error(`Zapret2 Strategy Pack не содержит профили ${category}`);
    }
    const dependencies = new Set([...zapret2Common, ...packResources]);
    for (const profile of profiles) {
      const hostlist =
        profile.hostlist ?? (profile.ipset == null ? `${category}.txt` : null);
      if (hostlist != null) {
        dependencies.add(
          resolveResource(resourceIndex, `lists/${hostlist}`, `${category} hostlist`),
        );
      }
      if (profile.ipset != null) {
        dependencies.add(
          resolveResource(resourceIndex, `lists/${profile.ipset}`, `${category} ipset`),
        );
      }
    }
    const orderedDependencies = [...dependencies].sort((left, right) =>
      left.localeCompare(right, "en"),
    );
    if (orderedDependencies.length > MAX_STRATEGY_DEPENDENCIES) {
      throw new Error(`Zapret2 ${category}: слишком много dependencies`);
    }
    orderedDependencies.forEach((dependency) => zapret2Files.add(dependency));
    zapret2Strategies.push({
      id: `builtin-${category.replaceAll("_", "-")}`,
      category: wireCategory,
      artifact: zapret2PackManifestRelative,
      dependencies: orderedDependencies,
    });
  }
  manifest.engines.push({
    engine: "zapret2",
    executable: resolveResource(resourceIndex, zapret2EngineRelative, "Zapret2 engine"),
    files: [...zapret2Files]
      .sort((left, right) => left.localeCompare(right, "en"))
      .map(resourceRecord),
    strategies: zapret2Strategies,
  });
  assertSingleManifestRecord(manifest, serviceRelative, "service");
  assertSingleManifestRecord(manifest, tgProxyRelative, "Telegram proxy");
  fs.writeFileSync(
    path.join(resourcesRoot, ...manifestRelative.split("/")),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  );
  return manifest;
}

function verifyWithRuntimeService() {
  const verificationRoot = path.join(serviceDir, "target", "runtime-bundle-verification");
  const expected = path.join(serviceDir, "target", "runtime-bundle-verification");
  recreateDirectory(verificationRoot, expected);
  const programFiles = path.join(verificationRoot, "Program Files");
  const obsessionRoot = path.join(programFiles, "Obsession");
  fs.mkdirSync(programFiles, { recursive: true });
  try {
    fs.cpSync(resourcesRoot, obsessionRoot, { recursive: true, dereference: false });
    run(
      "cargo",
      [
        "run",
        "--manifest-path",
        path.join(serviceDir, "Cargo.toml"),
        "--release",
        "--locked",
        "--example",
        "verify_runtime_bundle",
        "--",
        obsessionRoot,
      ],
      root,
    );
  } finally {
    recreateDirectory(verificationRoot, expected);
    fs.rmSync(verificationRoot, { recursive: true, force: true });
  }
}

if (!process.argv.includes("--skip-build")) {
  run(
    "cargo",
    ["build", "--manifest-path", path.join(serviceDir, "Cargo.toml"), "--release", "--locked"],
    root,
  );
}
if (!fs.existsSync(serviceSource)) {
  throw new Error(`Не найден release service binary: ${serviceSource}`);
}

recreateDirectory(runtimeOutput, path.join(resourcesRoot, "runtime"));
fs.copyFileSync(serviceSource, path.join(runtimeOutput, "Obsession.Runtime.exe"));
const manifest = generateManifest();
verifyWithRuntimeService();

console.log(
  `Protected runtime bundle: ${manifest.engines.length} engines, ` +
    `${manifest.engines[0].strategies.length} Legacy strategies, ` +
    `${manifest.engines[1].strategies.length} Zapret2 category groups`,
);
