// Real bundled engine CLI grammar check only: --dry-run exits before Lua init
// and WinDivert initialization. Behavioral tests are in the paired Python file.
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
const root = fileURLToPath(new URL('../', import.meta.url));
const resource = file => path.join(root, 'src-tauri/resources/strategy-packs/builtin/lua', file);
const result = spawnSync(path.join(root, 'src-tauri/resources/bin/zapret2/winws2.exe'), [
  '--dry-run', '--wf-tcp-out=443', '--filter-tcp=443', '--filter-l7=tls',
  '--payload=tls_client_hello', '--out-range=-d8',
  `--lua-init=@${resource('zapret-lib.lua')}`,
  `--lua-init=@${resource('obsession-clienthello.lua')}`,
  `--lua-init=@${resource('zapret-antidpi.lua')}`,
  '--lua-desync=obsession_clienthello:payload=tls_client_hello',
], {cwd:root, encoding:'utf8', timeout:10000, maxBuffer:512 * 1024,
  windowsHide:true, env:{...process.env, __COMPAT_LAYER:'RunAsInvoker'}});
assert.equal(result.error, undefined);
assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
assert.ok(result.stdout.includes('command line parameters verified'), `${result.stdout}\n${result.stderr}`);
console.log('ClientHello profile CLI parser passed; no Lua behavior, driver or capture startup.');
