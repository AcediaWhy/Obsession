import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { auditBundle, auditConfig, auditPack, resourcesRoot } from "./audit-dpi-configs.mjs";

test("all bundled configs and required transport profiles are consistent", () => {
  const report = auditBundle();
  // atrisk was retired; keep an independent inventory so a missing supported
  // profile cannot be hidden by another file with the same total count.
  const numberedConfigs = (category, count, first = 1, suffix = "") =>
    Array.from({ length: count }, (_, index) =>
      `configs/${category}/${category}_${first + index}${suffix}.conf`);
  const expectedConfigs = [
    ...numberedConfigs("discord", 16),
    ...numberedConfigs("gaming", 8),
    ...numberedConfigs("gaming", 7, 2, "_ultimate"),
    ...numberedConfigs("universal", 6),
    ...numberedConfigs("youtube_twitch", 11),
  ];
  assert.deepEqual(report.configs.map(config => config.path).sort(), expectedConfigs.sort());
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

test("explicit presets are audited without extending the ordinary ladder", () => {
  const pack = JSON.parse(fs.readFileSync(path.join(resourcesRoot, "strategy-packs/builtin/manifest.json"), "utf8"));
  assert.equal(pack.strategies.length, 19);
  assert.equal(pack.presets.filter(profile => !profile.candidate).length, 32);
  assert.deepEqual(auditPack(pack), []);
  pack.presets[0].id = pack.strategies[0].id;
  assert.match(auditPack(pack).join("\n"), /Duplicate profile/);
  pack.presets[0].id = "../untrusted.lua";
  assert.match(auditPack(pack).join("\n"), /Unsafe profile ID/);
  pack.presets[0].lua = "lua/missing.lua";
  assert.match(auditPack(pack).join("\n"), /Missing profile Lua/);
});

test("scoped recipes reject silent scope errors and missing staged blobs", () => {
  const pack = JSON.parse(fs.readFileSync(path.join(resourcesRoot, "strategy-packs/builtin/manifest.json"), "utf8"));
  const profile = pack.presets.find(p => p.steps?.length);
  assert.ok(profile);
  pack.schema_version = 1;
  assert.match(auditPack(pack).join("\n"), /Schema 2 required/);
  pack.schema_version = 2;
  profile.desync = ["fake:blob=tls_google"];
  assert.match(auditPack(pack).join("\n"), /Invalid recipe representation/);
  profile.desync = [];
  profile.steps[0].payload = ["all", "tls_client_hello"];
  assert.match(auditPack(pack).join("\n"), /Invalid recipe step/);
  profile.steps[0].payload = ["all"];
  profile.steps[0].action = "fake:blob=undeclared";
  assert.match(auditPack(pack).join("\n"), /Missing action blob/);
  profile.candidate.urls = ["https://github.com.evil.example/bol-van/recipe"];
  assert.match(auditPack(pack).join("\n"), /Invalid candidate source/);
});
