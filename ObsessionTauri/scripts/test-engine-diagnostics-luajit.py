"""Observer contract on native LuaJIT 2.1 (test-only lupa==2.6).

No driver or network primitives. Install lupa into a temporary test directory
and supply that directory through PYTHONPATH.
"""
from pathlib import Path
from lupa.luajit21 import LuaRuntime

root = Path(__file__).resolve().parent.parent
observer = (root / "src-tauri/resources/strategy-packs/builtin/lua/obsession-diagnostic.lua").read_text(encoding="utf-8-sig")
runtime = LuaRuntime(unpack_returned_tuples=True)
assert runtime.eval("_VERSION") == "Lua 5.1"
assert runtime.eval("type(table.pack)") == "nil"
assert runtime.eval("type(table.unpack)") == "nil"
assert runtime.eval("type(unpack)") == "function"
print("Native runtime:", runtime.eval("jit.version"))

for fixture in ["VERDICT_PASS=0",
                (root / "runtime-service/tests/fixtures/diagnostic-setup.lua").read_text(),
                observer,
                (root / "runtime-service/tests/fixtures/diagnostic-assertions.lua").read_text()]:
    runtime.execute(fixture)
print("Native LuaJIT observer contract passed; no network sends.")

# Also exercise the shipped library load order. Native helpers are inert
# stubs; only the PASS hook runs, so no packet data or sends are involved.
runtime = LuaRuntime(unpack_returned_tuples=True)
runtime.execute('''NFQWS2_COMPAT_VER=6; VERDICT_PASS=0;
local bit=require('bit'); bitnot=bit.bnot; bitxor=bit.bxor;
getpid=function()return 1 end; gettid=function()return 2 end;
clock_gettime=function()return 3,4 end; DLOG=function()end;
rawsend=function()error('unexpected network send')end;
rawsend_dissect=function()error('unexpected network send')end;''')
lua_root = root / "src-tauri/resources/strategy-packs/builtin/lua"
for name in ["zapret-lib.lua", "zapret-antidpi.lua", "obsession-diagnostic.lua"]:
    runtime.execute((lua_root / name).read_text(encoding="utf-8-sig"))
runtime.execute('''assert(obsession_trace({}, {arg={profile="adaptive-discord-tls-fixture"},
l7payload="tls_client_hello", outgoing=true}) == VERDICT_PASS); pass({}, {})''')
print("Shipped library load order and observed PASS hook passed.")
