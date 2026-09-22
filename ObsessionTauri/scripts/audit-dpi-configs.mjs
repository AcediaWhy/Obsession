#!/usr/bin/env node
// Static bundle audit; does not start a driver, service or network connection.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";

export const resourcesRoot = fileURLToPath(new URL("../src-tauri/resources/", import.meta.url));

export function parseConfig(source) {
  const tokens = source.split(/\r?\n/).filter(line => !line.trimStart().startsWith("#"))
    .join(" ").match(/(?:[^\s"']+|"[^"]*"|'[^']*')+/g) ?? [];
  const globals = new Map();
  const profiles = [];
  let profile = new Map();
  for (const token of tokens) {
    if (token === "--new") {
      if (profile.size) profiles.push(profile);
      profile = new Map();
      continue;
    }
    if (!token.startsWith("--")) throw new Error(`Invalid config token: ${token}`);
    const equal = token.indexOf("=");
    const key = equal < 0 ? token : token.slice(0, equal);
    const value = equal < 0 ? "" : token.slice(equal + 1).replace(/^(["'])(.*)\1$/, "$2");
    const destination = key.startsWith("--wf-") ? globals : profile;
    destination.set(key, [...(destination.get(key) ?? []), value]);
  }
  if (profile.size) profiles.push(profile);
  return { globals, profiles };
}

function ports(value) {
  return value.split(",").map(part => {
    if (!/^\d+(?:-\d+)?$/.test(part)) throw new Error(`Invalid ports: ${value}`);
    const [start, end = start] = part.split("-").map(Number);
    if (start < 1 || end > 65535 || start > end) throw new Error(`Invalid ports: ${value}`);
    return [start, end];
  });
}

export function auditConfig(source, category, resourceExists = () => true) {
  const { globals, profiles } = parseConfig(source);
  const errors = [];
  const dependencies = new Set();
  if (!profiles.length) errors.push("No profiles");
  for (const [index, profile] of profiles.entries()) {
    const fail = message => errors.push(`profile ${index + 1}: ${message}`);
    for (const transport of ["tcp", "udp"]) {
      for (const filter of profile.get(`--filter-${transport}`) ?? []) {
        const selected = ports(filter);
        const captured = (globals.get(`--wf-${transport}`) ?? []).flatMap(ports);
        for (const [first, last] of selected) {
          // Check the union, including adjacent capture ranges.
          for (let port = first; port <= last; port++) {
            if (!captured.some(([a, b]) => a <= port && port <= b)) {
              fail(`${transport} port ${port} is outside capture`);
              break;
            }
          }
        }
        const highCount = selected.reduce((n, [a, b]) => n + Math.max(0, b - Math.max(a, 1024) + 1), 0);
        if (highCount > 256 && !profile.has("--ipset")) fail(`broad ${transport} filter without ipset`);
      }
    }
    for (const values of profile.values()) {
      for (const value of values) {
        if (!/^(lists|autohosts|bin)[\\/]/.test(value)) continue;
        const reference = value.replaceAll("\\", "/");
        if (reference.split("/").some(part => part === ".." || !part)) fail(`unsafe path ${reference}`);
        else if (!resourceExists(reference)) fail(`missing resource ${reference}`);
        dependencies.add(reference);
      }
    }
  }
  if (category === "discord") {
    const voice = profiles.filter(p => (p.get("--filter-l7") ?? []).some(v => v.split(",").includes("discord")));
    if (!voice.some(p => (p.get("--filter-udp") ?? []).flatMap(ports)
      .some(([a, b]) => a <= 19294 && b >= 19344))) errors.push("Discord voice range 19294-19344 is missing");
  }
  return { errors, profiles: profiles.length, dependencies: [...dependencies].sort() };
}

export function auditPack(pack, root = resourcesRoot) {
  const errors = [];
  const packRoot = path.join(root, "strategy-packs/builtin");
  for (const file of pack.files) {
    const resolved = path.resolve(packRoot, file.path);
    if (!resolved.startsWith(`${path.resolve(packRoot)}${path.sep}`)) {
      errors.push(`Unsafe pack path: ${file.path}`);
      continue;
    }
    if (!fs.existsSync(resolved) || crypto.createHash("sha256").update(fs.readFileSync(resolved)).digest("hex") !== file.sha256)
      errors.push(`Pack file/hash mismatch: ${file.path}`);
  }
  const ids = new Set();
  for (const s of pack.strategies) {
    if (ids.has(s.id)) errors.push(`Duplicate profile: ${s.id}`);
    ids.add(s.id);
    for (const list of [s.hostlist, s.ipset].filter(Boolean)) {
      if (!/^[\w.-]+\.txt$/.test(list) || list.includes("..") || !fs.existsSync(path.join(root, "lists", list)))
        errors.push(`Missing/unsafe list: ${s.id}/${list}`);
    }
  }
  for (const category of pack.categories) {
    const all = pack.strategies.filter(s => s.category === category);
    for (const level of new Set(all.map(s => s.aggressiveness))) {
      const profiles = all.filter(s => s.aggressiveness === level);
      for (const transport of ["tls", "quic"]) {
        if (!profiles.some(s => s.transports.includes(transport))) errors.push(`${category}/${level}: missing ${transport}`);
      }
      if (category === "discord" && !profiles.some(s =>
        s.filter_l7?.includes("discord") && s.filter_l7?.includes("stun") &&
        s.payload?.includes("discord_ip_discovery") && s.payload?.includes("stun") &&
        !s.hostlist && !s.ipset && s.filter_udp && ports(s.filter_udp).some(([a, b]) => a <= 19294 && b >= 19344)))
        errors.push(`discord/${level}: missing hostname-free voice scope`);
    }
  }
  return errors;
}

export function auditBundle(root = resourcesRoot) {
  const configs = [];
  const errors = [];
  for (const category of fs.readdirSync(path.join(root, "configs")).sort()) {
    for (const file of fs.readdirSync(path.join(root, "configs", category)).filter(n => n.endsWith(".conf")).sort()) {
      const relative = `configs/${category}/${file}`;
      const report = auditConfig(fs.readFileSync(path.join(root, relative), "utf8"), category,
        ref => fs.existsSync(path.join(root, ref)));
      errors.push(...report.errors.map(error => `${relative}: ${error}`));
      configs.push({ path: relative, ...report });
    }
  }
  const pack = JSON.parse(fs.readFileSync(path.join(root, "strategy-packs/builtin/manifest.json"), "utf8"));
  errors.push(...auditPack(pack, root));
  const activeDomains = fs.readFileSync(path.join(root, "lists/youtube_twitch.txt"), "utf8")
    .split(/\r?\n/).map(line => line.trim()).filter(line => line && !line.startsWith("#"));
  for (const domain of ["twitch.tv", "ttvnw.net", "jtvnw.net", "googlevideo.com"])
    if (!activeDomains.includes(domain)) errors.push(`Missing media domain: ${domain}`);
  return { configs, packVersion: pack.pack_version, packProfiles: pack.strategies.length, errors };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const report = auditBundle();
  if (process.argv.includes("--json")) console.log(JSON.stringify(report, null, 2));
  else {
    console.log(`${report.configs.length} Legacy configs; Zapret2 ${report.packVersion}: ${report.packProfiles} profiles`);
    console.log(report.errors.length ? report.errors.join("\n") : "Static config audit passed (live network behavior not tested).");
  }
  if (report.errors.length) process.exitCode = 1;
}
