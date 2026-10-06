// Executes only Lua fixture assertions and the native parser. No driver starts.
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import fengari from 'fengari';
const root = fileURLToPath(new URL('../', import.meta.url));
const engine = path.join(root, 'src-tauri/resources/bin/zapret2/winws2.exe');
const files = ['runtime-service/tests/fixtures/diagnostic-setup.lua',
  'src-tauri/resources/strategy-packs/builtin/lua/obsession-diagnostic.lua',
  'runtime-service/tests/fixtures/diagnostic-assertions.lua'];
// Native --dry-run validates only CLI grammar; it exits before Lua init. Run
// behavioral fixtures with both the Lua 5.1/LuaJIT and Lua 5.3 table API surfaces.
// This emulates missing APIs; it is not a native LuaJIT execution test.
const {lua, lauxlib, lualib, to_luastring} = fengari;
for (const surface of ['luajit51', 'lua53']) {
  const vm = lauxlib.luaL_newstate();
  lualib.luaL_openlibs(vm);
  const compatibility = surface === 'luajit51'
    ? 'unpack=table.unpack; table.pack=nil; table.unpack=nil; assert(unpack and not table.pack and not table.unpack);\n'
    : 'unpack=nil;\n';
  const fixture = compatibility + `VERDICT_PASS=0; io={stdout={write=function() end,flush=function() end}}\n`
    + files.map(file => fs.readFileSync(path.join(root, file), 'utf8')).join('\n');
  const status = lauxlib.luaL_dostring(vm, to_luastring(fixture));
  assert.equal(status, lua.LUA_OK, `${surface}: ${status === lua.LUA_OK ? '' : lua.lua_tojsstring(vm, -1)}`);
  lua.lua_close(vm);
}
const result = spawnSync(engine, ['--dry-run', '--wf-tcp-out=443', `--lua-init=@${path.join(root, files[1])}`], {
  encoding:'utf8', timeout:10000, maxBuffer:512*1024, windowsHide:true,
  env:{...process.env, __COMPAT_LAYER:'RunAsInvoker'},
});
assert.equal(result.error, undefined);
assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
assert.ok(result.stdout.includes('command line parameters verified'), `${result.stdout}\n${result.stderr}`);
console.log('Observer contracts passed with LuaJIT 5.1 and Lua 5.3 API surfaces; native CLI parser passed. No packet capture.');
