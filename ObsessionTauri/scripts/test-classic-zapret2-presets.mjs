// Fixed public catalogue contracts and real native CLI grammar only.
// --dry-run exits before Lua initialization/WinDivert; no probes or capture.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
const packDir = path.join(root, 'src-tauri/resources/strategy-packs/builtin');
const pack = JSON.parse(fs.readFileSync(path.join(packDir, 'manifest.json'), 'utf8'));
const digest = value => crypto.createHash('sha256').update(JSON.stringify(value)).digest('hex');
assert.equal(pack.pack_version, '0.12.0');
assert.equal(pack.strategies.length, 19);
assert.equal(digest(pack.strategies), '87b5f5ec9223847b4544e9ee5bae6f218373e4d7dd88ed88445b240c3beb275e');
assert.equal(digest(pack.experiments), '2b372f9089e1942c3d7af924178842b88c84561822bf118968621dc59381599a');
const classicPresets = pack.presets.filter(profile => !profile.candidate);
assert.equal(classicPresets.length, 32);
assert.deepEqual(pack.presets.find(profile => profile.id === 'classic_youtube_twitch_tls_saved_disorder').desync,
  ['multidisorder:pos=1,midsld']);
const ids = new Set();
for (const category of pack.categories) {
  for (const [transport, minimum, maximum] of [['tls',6,10],['quic',2,4]]) {
    const entries = classicPresets.filter(profile => profile.category === category && profile.filter_l7.includes(transport));
    assert.ok(entries.length >= minimum && entries.length <= maximum);
    assert.equal(new Set(entries.map(profile => JSON.stringify([profile.desync, profile.out_range]))).size, entries.length);
  }
}
for (const profile of classicPresets) {
  assert.match(profile.id, /^classic_(discord|youtube_twitch|gaming)_(tls|quic)_[a-z0-9_]+$/);
  assert.equal(ids.has(profile.id), false);
  ids.add(profile.id);
  assert.equal(profile.ipset, undefined);
  assert.equal(profile.lua, 'lua/zapret-antidpi.lua');
  assert.ok(profile.desync.length >= 1 && profile.desync.length <= 2);
  assert.ok(profile.out_range === undefined || profile.out_range === '-d10');
  for (const action of profile.desync) {
    assert.match(action, /^(fake|multisplit|multidisorder|multidisorder_legacy):/);
    assert.doesNotMatch(action, /tcp_ts|(?:^|:)repeats=(?:[7-9]|\d\d)/);
    if (profile.filter_l7.includes('tls') && action.startsWith('fake:')) {
      assert.match(action, /:tcp_seq=-10000(?:$|:)/);
      assert.match(action, /:blob=(?:fake_default_tls|tls_google)(?:$|:)/);
    }
  }
  if (profile.category === 'gaming') {
    assert.equal(profile.hostlist, 'gaming-github.txt');
    assert.ok(profile.filter_tcp === '443' || profile.filter_udp === '443');
  }
  if (profile.category === 'discord' && profile.filter_l7.includes('tls')) {
    assert.equal(profile.filter_tcp, '443,2053,2083,2087,2096,8443');
  }
  const tcp = profile.filter_l7.includes('tls');
  const capture = tcp ? ['--wf-tcp-out=' + (profile.filter_tcp ?? '443')] : ['--wf-udp-out=' + (profile.filter_udp ?? '443')];
  const args = ['--dry-run', ...capture,
    '--lua-init=@' + path.join(packDir, 'lua/zapret-lib.lua'),
    '--lua-init=@' + path.join(packDir, 'lua/zapret-antidpi.lua'),
    ...pack.blobs.map(blob => '--blob=' + blob.name + ':@' + path.join(packDir, blob.path)),
    '--name=' + profile.id,
    tcp ? '--filter-tcp=' + (profile.filter_tcp ?? '443') : '--filter-udp=' + (profile.filter_udp ?? '443'),
    '--filter-l7=' + profile.filter_l7.join(','), '--payload=' + profile.payload.join(','),
    ...(profile.out_range ? ['--out-range=' + profile.out_range] : []),
    ...profile.desync.map(desync => '--lua-desync=' + desync),
  ];
  const options = {cwd:root, encoding:'utf8', timeout:10000, maxBuffer:512*1024, windowsHide:true,
    env:{...process.env, __COMPAT_LAYER:'RunAsInvoker'}};
  const executable = path.join(root, 'src-tauri/resources/bin/zapret2/winws2.exe');
  let result = spawnSync(executable, args, options);
  // Cygwin can fail DLL initialization before entering main. Retry only that
  // exact OS status once; never conceal a CLI/parser rejection or a timeout.
  if (result.status === 0xC0000142 && !result.stdout && !result.stderr) {
    console.log(profile.id + ': one retry after DLL initialization failure');
    result = spawnSync(executable, args, options);
  }
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, profile.id + ': ' + result.stdout + '\n' + result.stderr);
  assert.match(result.stdout, /command line parameters verified/);
}
console.log('32 fixed TLS/QUIC presets passed catalogue and native CLI contracts; 19 existing profiles and experiment unchanged. No Lua execution, probes or driver startup.');
