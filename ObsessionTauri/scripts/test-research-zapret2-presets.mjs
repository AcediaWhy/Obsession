// Resource contracts and the shipped native parser; no driver or connections.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {auditPack} from './audit-dpi-configs.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const packDir = path.join(root, 'src-tauri/resources/strategy-packs/builtin');
const pack = JSON.parse(fs.readFileSync(path.join(packDir, 'manifest.json'), 'utf8'));
assert.deepEqual(auditPack(pack), []);
assert.equal(pack.schema_version, 2);
const research = pack.presets.filter(profile => profile.candidate);
assert.equal(research.length, 11);
assert.equal(new Set(research.map(p => p.candidate.family)).size, 4);
assert.equal(pack.presets.some(p => p.id.endsWith('_fake_ts_seq_split')), false);
for (const profile of research) {
  assert.equal(profile.ipset, undefined);
  assert.ok(['discord', 'youtube_twitch'].includes(profile.category));
  const tcp = profile.filter_l7.includes('tls');
  assert.deepEqual(profile.payload, [tcp ? 'tls_client_hello' : 'quic_initial']);
  assert.equal(profile.lua, 'lua/zapret-antidpi.lua');
  const scope = profile.steps?.length ? profile.steps.flatMap(step => [
    '--payload=' + step.payload.join(','), '--out-range=' + (step.out_range ?? profile.out_range ?? 'a0'),
    '--in-range=' + (step.in_range ?? profile.in_range ?? 'a0'), '--lua-desync=' + step.action,
  ]) : ['--payload=' + profile.payload.join(','), ...profile.desync.map(action => '--lua-desync=' + action)];
  const args = ['--dry-run', '--wf-' + (tcp ? 'tcp-out=' + profile.filter_tcp : 'udp-out=' + profile.filter_udp),
    '--lua-init=@' + path.join(packDir, 'lua/zapret-lib.lua'),
    '--lua-init=@' + path.join(packDir, 'lua/zapret-antidpi.lua'),
    ...pack.blobs.map(blob => '--blob=' + blob.name + ':@' + path.join(packDir, blob.path)),
    '--name=' + profile.id, '--filter-' + (tcp ? 'tcp=' + profile.filter_tcp : 'udp=' + profile.filter_udp),
    '--filter-l7=' + profile.filter_l7.join(','),
    ...(profile.out_range ? ['--out-range=' + profile.out_range] : []),
    ...scope,
  ];
  const options = {cwd: root, encoding: 'utf8', timeout: 10000, maxBuffer: 512 * 1024,
    windowsHide: true, env: {...process.env, __COMPAT_LAYER: 'RunAsInvoker'}};
  const executable = path.join(root, 'src-tauri/resources/bin/zapret2/winws2.exe');
  let result = spawnSync(executable, args, options);
  if (result.status === 0xC0000142 && !result.stdout && !result.stderr) result = spawnSync(executable, args, options);
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, profile.id + ': ' + result.stdout + '\n' + result.stderr);
  assert.match(result.stdout, /command line parameters verified/);
}
console.log('11 research profiles passed resource and native CLI checks. Source reports are not local network results.');
