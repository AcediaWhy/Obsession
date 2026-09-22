import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { auditBundle, auditConfig, auditPack, resourcesRoot } from "./audit-dpi-configs.mjs";

test("all bundled configs and required transport profiles are consistent", () => {
  const report = auditBundle();
  assert.equal(report.configs.length, 51);
  assert.deepEqual(report.errors, []);
});

test("a scoped profile must also be captured by WinDivert", () => {
  const config = '--wf-udp=443 --filter-udp=19294-19344 --filter-l7=discord,stun';
  assert.match(auditConfig(config, "discord").errors.join("\n"), /outside capture/);
});

test("broad game UDP cannot silently lose its destination restriction", () => {
  const config = '--wf-udp=1024-65535 --filter-udp=1024-65535 --dpi-desync=fake';
  assert.match(auditConfig(config, "gaming").errors.join("\n"), /without ipset/);
  assert.deepEqual(auditConfig(`${config} --ipset="lists\\games.txt"`, "gaming").errors, []);
});

test("missing fake payload files prevent publishing", () => {
  const config = '--wf-tcp=443 --filter-tcp=443 --dpi-desync-fake-tls="bin\\missing.bin"';
  assert.match(auditConfig(config, "discord", () => false).errors.join("\n"), /missing resource/);
});

test("removing voice or QUIC from one level is rejected", () => {
  const pack = JSON.parse(fs.readFileSync(path.join(resourcesRoot, "strategy-packs/builtin/manifest.json"), "utf8"));
  pack.strategies = pack.strategies.filter(s => s.id !== "discord_voice_level_2" && s.id !== "discord_quic_level_3");
  assert.match(auditPack(pack).join("\n"), /discord\/2: missing hostname-free voice/);
  assert.match(auditPack(pack).join("\n"), /discord\/3: missing quic/);
});
