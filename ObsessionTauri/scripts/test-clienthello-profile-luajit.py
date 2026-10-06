"""Offline behavior tests: shipped LuaJIT + real bundled Zapret Lua helpers.

The real bundled TLS parser, clone/reconstruction, fake, MSS segmentation and
replay helpers run unmodified. Binary integer helpers are byte-exact shims;
the C SNI-position API uses an independent strict Python TLS fixture oracle;
rawsend_dissect is a recording sink. These tests do not prove physical
WinDivert delivery, server rejection, exact C marker positions or DPI effect.
Use a Python matching the test-only lupa installation (lupa.luajit21).
"""
from pathlib import Path
import ssl
import struct
import shutil
import subprocess
import tempfile
from lupa.luajit21 import LuaRuntime

ROOT = Path(__file__).resolve().parent.parent
LUA_ROOT = ROOT / "src-tauri/resources/strategy-packs/builtin/lua"
U32 = 1 << 32


def u16(data, pos):
    return int.from_bytes(data[pos:pos + 2], "big")


def extension(kind, data):
    return struct.pack("!HH", kind, len(data)) + data


def hello(late=False, host=b"gateway.discord.gg", pad=400):
    sni = extension(0, struct.pack("!HBH", len(host) + 3, 0, len(host)) + host)
    keyshare = extension(51, struct.pack("!HHH", 1220, 4588, 1216) + bytes(range(256)) * 4 + b"K" * 192)
    before = [extension(43, b"\x02\x03\x04"), keyshare, extension(65037, b"G" * 70)]
    exts = b"".join(before + [sni] if late else [sni] + before) + extension(21, b"\0" * pad)
    body = b"\x03\x03" + b"R" * 32 + b"\x20" + b"S" * 32 + b"\x00\x04\x13\x01\x13\x02\x01\x00" + struct.pack("!H", len(exts)) + exts
    handshake = b"\x01" + len(body).to_bytes(3, "big") + body
    return b"\x16\x03\x01" + struct.pack("!H", len(handshake)) + handshake


def parse(data):
    """Strict independent single-record ClientHello parser, with byte spans."""
    if len(data) < 44 or data[:2] != b"\x16\x03" or data[5] != 1:
        raise ValueError("not ClientHello")
    if u16(data, 3) != len(data) - 5 or int.from_bytes(data[6:9], "big") != len(data) - 9:
        raise ValueError("truncated or multiple record")
    p = 43
    p += 1 + data[p]
    p += 2 + u16(data, p)
    p += 1 + data[p]
    start = p
    if u16(data, p) != len(data) - p - 2:
        raise ValueError("bad extensions length")
    p += 2
    entries, sni_span, sni = [], None, None
    while p < len(data):
        if p + 4 > len(data):
            raise ValueError("truncated extension header")
        kind, size = u16(data, p), u16(data, p + 2)
        end = p + 4 + size
        if end > len(data):
            raise ValueError("truncated extension")
        value = data[p + 4:end]
        entries.append((kind, value))
        if kind == 0:
            if len(value) < 5 or u16(value, 0) != len(value) - 2 or value[2] != 0 or u16(value, 3) != len(value) - 5:
                raise ValueError("invalid SNI")
            sni, sni_span = value[5:], (p + 9, end)
        p = end
    return {"prefix": data[:start], "entries": entries, "sni": sni, "sni_span": sni_span}


def ssl_hello():
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    context.set_alpn_protocols(["http/1.1"])
    incoming, outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
    client = context.wrap_bio(incoming, outgoing, server_hostname="discord.com")
    try:
        client.do_handshake()
    except ssl.SSLWantReadError:
        pass
    return outgoing.read()


class Harness:
    def __init__(self, custom_first=False):
        self.runtime = LuaRuntime(encoding=None, unpack_returned_tuples=True)
        self.sends, self.attempts, self.fail_at, self.clone_calls = [], 0, None, 0
        g = self.runtime.globals()
        g[b"_count_clone"] = self.count_clone
        g[b"_position_oracle"] = self.positions
        g[b"_send_sink"] = self.send
        self.runtime.execute(b'''NFQWS2_COMPAT_VER=6; VERDICT_PASS=0; VERDICT_DROP=1;
            local bit=require('bit'); bitnot=bit.bnot; bitxor=bit.bxor; bitand=bit.band; bitor=bit.bor;
            getpid=function()return 1 end; gettid=function()return 2 end;
            clock_gettime=function()return 3,4 end; DLOG=function()end; b_debug=false;
            IP_BASE_LEN=20; IP6_BASE_LEN=40; TCP_BASE_LEN=20; UDP_BASE_LEN=8;
            TCP_KIND_TS=8; TCP_KIND_NOOP=1; TCP_KIND_END=0;
            TH_FIN=1; TH_SYN=2; TH_RST=4; TH_URG=32; TH_ACK=16;
            u32add=function(a,b)return (a+tonumber(b)) % 4294967296 end;
            u8=function(s,p)return s:byte(p or 1)end;
            u16=function(s,p)p=p or 1; return s:byte(p)*256+s:byte(p+1)end;
            u24=function(s,p)p=p or 1; return s:byte(p)*65536+s:byte(p+1)*256+s:byte(p+2)end;
            u32=function(s,p)p=p or 1; return s:byte(p)*16777216+s:byte(p+1)*65536+s:byte(p+2)*256+s:byte(p+3)end;
            bu8=function(v)return string.char(v % 256)end;
            bu16=function(v)return string.char(math.floor(v/256)%256, v%256)end;
            bu24=function(v)return string.char(math.floor(v/65536)%256, math.floor(v/256)%256, v%256)end;
            bu32=function(v)return string.char(math.floor(v/16777216)%256, math.floor(v/65536)%256, math.floor(v/256)%256, v%256)end;
            barray=function(a,f)local out={}; for i,v in ipairs(a)do out[i]=f(v)end; return table.concat(out)end;
            resolve_multi_pos=_position_oracle;
            find_tcp_option=function(options, kind)
                for i,v in ipairs(options or {})do if v.kind==kind then return i end end end;
            rawsend_dissect=_send_sink;
            rawsend=function()error('unexpected non-sink rawsend')end;''')
        self.runtime.execute((LUA_ROOT / "zapret-lib.lua").read_bytes())
        self.runtime.execute(b'''local original=tls_client_hello_mod;
            tls_client_hello_mod=function(...)_count_clone(); return original(...)end;''')
        files = ["obsession-clienthello.lua", "zapret-antidpi.lua"] if custom_first else ["zapret-antidpi.lua", "obsession-clienthello.lua"]
        for name in files:
            self.runtime.execute((LUA_ROOT / name).read_bytes())

    def count_clone(self):
        self.clone_calls += 1

    def positions(self, data, kind, markers):
        assert kind == b"tls_client_hello"
        try:
            span = parse(data)["sni_span"]
        except (ValueError, IndexError):
            span = None
        resolved = []
        for marker in markers.split(b","):
            if marker == b"2":
                # Native numeric markers are zero-based byte offsets; the
                # ordinary C-to-Lua resolver adds one for Lua string indices.
                resolved.append(3)
            elif marker == b"midsld" and span:
                # A structural oracle for an interior SNI boundary. Exact C
                # marker placement remains a native parser integration check.
                resolved.append((span[0] + span[1]) // 2 + 1)
        return self.runtime.table_from(sorted(set(resolved)))

    def send(self, dis, options=None, reconstruct=None):
        self.attempts += 1
        packet = {"seq": dis[b"tcp"][b"th_seq"] % U32, "payload": dis[b"payload"],
                  "flags": dis[b"tcp"][b"th_flags"], "ack": dis[b"tcp"][b"th_ack"],
                  "options": [(o[b"kind"], o[b"data"]) for _, o in dis[b"tcp"][b"options"].items()],
                  "ok": self.attempts != self.fail_at}
        self.sends.append(packet)
        return packet["ok"]

    def desync(self, data, timestamps=True, base=100000, mss=560, replay=False, piece=1, track=None):
        t = self.runtime.table_from
        options = [t({b"kind": 8, b"data": struct.pack("!II", 400000, 300000)})] if timestamps else []
        dis = t({b"payload": data, b"ip": t({b"ip_id": 1, b"ip_ttl": 64}),
                 b"tcp": t({b"th_seq": base, b"th_ack": 900000, b"th_flags": 24,
                            b"th_urp": 0, b"options": t(options)})})
        return t({b"outgoing": True, b"l7payload": b"tls_client_hello", b"arg": t({}),
                  b"func_instance": b"test_instance", b"dis": dis, b"tcp_mss": mss,
                  b"track": track if track is not None else t({b"lua_state": t({})}),
                  b"replay": replay, b"replay_piece": piece, b"replay_piece_last": False})

    def run(self, desync):
        return self.runtime.globals()[b"obsession_clienthello"](None, desync)


def relative(seq, base):
    return ((seq - base + (1 << 31)) % U32) - (1 << 31)


def verify_emission(h, data, base, timestamps):
    real, fake_packets = [], []
    for packet in h.sends:
        off = relative(packet["seq"], base)
        (fake_packets if off < 0 else real).append((off, packet))
        assert packet["flags"] == 24 and packet["ack"] == 900000
        assert packet["options"] == ([(8, struct.pack("!II", 400000, 300000))] if timestamps else [])
    assert fake_packets and real
    cursor, restored = 0, b""
    for off, packet in real:
        assert off == cursor and packet["ok"]
        restored += packet["payload"]
        cursor += len(packet["payload"])
    assert restored == data
    cursor, fake_bytes = -16384, b""
    for off, packet in fake_packets:
        assert off == cursor and off + len(packet["payload"]) < 0 and packet["ok"]
        fake_bytes += packet["payload"]
        cursor += len(packet["payload"])
    original, clone = parse(data), parse(fake_bytes)
    assert clone["sni"] == b"www.google.com" and clone["sni"] != original["sni"]
    assert clone["prefix"][9:] == original["prefix"][9:]
    assert [(k, v) for k, v in clone["entries"] if k != 0] == [(k, v) for k, v in original["entries"] if k != 0]
    for _, packet in real + fake_packets:
        assert len(packet["payload"]) <= 560 - (12 if timestamps else 0)
    # Three independent boundaries: TLS header, interior SNI, negotiated MSS.
    assert real[0][1]["payload"] == data[:2]
    assert any(original["sni_span"][0] < off < original["sni_span"][1] for off, _ in real)


cases = 0
for custom_first in (False, True):
    for data in (hello(), hello(late=True), hello(late=True, pad=5000), ssl_hello()):
        for timestamps in (False, True):
            for base in (100000, 7, U32 - 40):
                h = Harness(custom_first)
                d = h.desync(data, timestamps, base)
                assert h.run(d) == 1
                assert d[b"dis"][b"payload"] == data and d[b"dis"][b"tcp"][b"th_seq"] == base
                verify_emission(h, data, base, timestamps)
                # Same tracked connection retransmits original bytes, with no
                # second fake or persisted ClientHello packet contents.
                h.sends.clear()
                assert h.run(d) == 1
                assert h.clone_calls == 1
                assert all(relative(p["seq"], base) >= 0 for p in h.sends)
                assert b"".join(p["payload"] for p in h.sends) == data
                assert all(isinstance(v, bool) for k, v in d[b"track"][b"lua_state"].items() if k != b"cutoff_shim")
                cases += 1

# Replay of a three-piece >MSS PQ hello: one whole clone, one whole native
# emission, then DROP only the already emitted original replay pieces.
h = Harness()
data, track = hello(late=True), h.runtime.table_from({b"lua_state": h.runtime.table_from({})})
d = h.desync(data[:560], replay=True, track=track)
d[b"reasm_data"] = data
assert h.run(d) == 1
count = len(h.sends)
for piece in (2, 3, 4):
    d = h.desync(data[(piece - 1) * 560:piece * 560], base=100000 + (piece - 1) * 560,
                 replay=True, piece=piece, track=track)
    d[b"reasm_data"] = data
    d[b"replay_piece_last"] = piece == 4
    assert h.run(d) == 1 and len(h.sends) == count
assert track[b"lua_state"][b"test_instance_replay_drop"] is None
cases += 1

# Fail every fake or real send in turn. Successful partial real sends never
# authorize dropping subsequent replay pieces; the original source is intact.
h = Harness()
d = h.desync(data[:560], replay=True)
d[b"reasm_data"] = data
assert h.run(d) == 1
total_sends = len(h.sends)
for fail_at in range(1, total_sends + 1):
    h = Harness()
    h.fail_at = fail_at
    d = h.desync(data[:560], replay=True)
    d[b"reasm_data"] = data
    verdict = h.run(d)
    real_failed = any(not p["ok"] and relative(p["seq"], 100000) >= 0 for p in h.sends)
    assert verdict == (0 if real_failed else 1)
    later = h.desync(data[560:1120], base=100560, replay=True, piece=2, track=d[b"track"])
    later[b"reasm_data"] = data
    assert h.run(later) == (0 if real_failed else 1)
    assert d[b"dis"][b"payload"] == data[:560]
    cases += 1

for mutate in (
    lambda d: d.__setitem__(b"track", None),
    lambda d: d.__setitem__(b"outgoing", False),
    lambda d: d.__setitem__(b"l7payload", b"unknown"),
    lambda d: d.__setitem__(b"tcp_mss", 20),
    lambda d: d[b"dis"].__setitem__(b"tcp", None),
    lambda d: d[b"dis"][b"tcp"].__setitem__(b"th_flags", 2),
    lambda d: d[b"dis"][b"tcp"].__setitem__(b"th_flags", 20),
    lambda d: d[b"dis"][b"tcp"].__setitem__(b"th_flags", 25),
    lambda d: d[b"dis"][b"tcp"].__setitem__(b"th_flags", 56),
):
    h = Harness()
    d = h.desync(data)
    mutate(d)
    assert h.run(d) == 0 and not h.sends
    cases += 1

for bad in (data[:560], data + b"\x14\x03\x03\x00\x01\x01", hello(pad=9000), b"GET / HTTP/1.1\r\n\r\n"):
    h = Harness()
    d = h.desync(bad)
    assert h.run(d) == 0 and not h.sends
    cases += 1

h = Harness()
no_sni = parse(data)
exts = b"".join(extension(k, v) for k, v in no_sni["entries"] if k != 0)
body = no_sni["prefix"][9:] + struct.pack("!H", len(exts)) + exts
handshake = b"\x01" + len(body).to_bytes(3, "big") + body
without_sni = data[:3] + struct.pack("!H", len(handshake)) + handshake
assert h.run(h.desync(without_sni)) == 0 and not h.sends
cases += 1
# Broken extensions keep record/handshake lengths intact. A position oracle
# that still recognizes SNI cannot make an upstream parser exception fatal.
h = Harness()
h.runtime.execute(b'''local original=tls_client_hello_mod;
    tls_client_hello_mod=function()error('fixture reconstruction failure')end;''')
assert h.run(h.desync(data)) == 0 and not h.sends
cases += 1
h = Harness()
h.runtime.execute(b'''rawsend_dissect=function()error('fixture rawsend failure')end;''')
d = h.desync(data)
assert h.run(d) == 0 and not h.sends and d[b"dis"][b"payload"] == data
cases += 1
# Full authenticated TLS + HTTP round trips over an in-memory receive-window
# model. The certificate is ephemeral and trusted only by this fixture client.
# Nothing binds, connects or invokes WinDivert; this models rejection of old
# sequence intervals and does not claim on-wire rejection by a real server.
openssl = shutil.which("openssl")
assert openssl, "OpenSSL CLI is needed only to generate temporary fixture PKI"
with tempfile.TemporaryDirectory(prefix="obsession-clienthello-fixture-") as fixture_dir:
    fixture_path = Path(fixture_dir)
    certificate, key = fixture_path / "cert.pem", fixture_path / "key.pem"
    generated = subprocess.run([openssl, "req", "-x509", "-newkey", "rsa:2048", "-nodes",
                                "-keyout", str(key), "-out", str(certificate), "-days", "1",
                                "-subj", "/CN=clienthello-fixture.local", "-addext",
                                "subjectAltName=DNS:clienthello-fixture.local"],
                               capture_output=True, timeout=15, check=False)
    assert generated.returncode == 0, "Could not generate ephemeral test certificate"
    for version in (ssl.TLSVersion.TLSv1_2, ssl.TLSVersion.TLSv1_3):
        for timestamps in (False, True):
            server_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            server_context.minimum_version = server_context.maximum_version = version
            server_context.load_cert_chain(certificate, key)
            server_context.set_alpn_protocols(["http/1.1"])
            client_context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            client_context.minimum_version = client_context.maximum_version = version
            client_context.load_verify_locations(cafile=str(certificate))
            client_context.set_alpn_protocols(["http/1.1"])
            cin, cout, sin, sout = (ssl.MemoryBIO() for _ in range(4))
            client = client_context.wrap_bio(cin, cout, server_hostname="clienthello-fixture.local")
            server = server_context.wrap_bio(sin, sout, server_side=True)
            try:
                client.do_handshake()
            except ssl.SSLWantReadError:
                pass
            fresh_hello = cout.read()
            h = Harness()
            d = h.desync(fresh_hello, timestamps=timestamps, base=7)
            assert h.run(d) == 1
            verify_emission(h, fresh_hello, 7, timestamps)
            next_expected, restored = 0, b""
            for packet in h.sends:
                offset = relative(packet["seq"], 7)
                if offset + len(packet["payload"]) <= next_expected:
                    continue  # Entire stale fake interval is before RCV.NXT.
                assert offset == next_expected
                restored += packet["payload"]
                next_expected += len(packet["payload"])
                sin.write(packet["payload"])
            assert restored == fresh_hello
            client_done = server_done = False
            for _ in range(20):
                if not server_done:
                    try:
                        server.do_handshake()
                        server_done = True
                    except ssl.SSLWantReadError:
                        pass
                if sout.pending:
                    cin.write(sout.read())
                if not client_done:
                    try:
                        client.do_handshake()
                        client_done = True
                    except ssl.SSLWantReadError:
                        pass
                if cout.pending:
                    sin.write(cout.read())
                if client_done and server_done:
                    break
            assert client_done and server_done
            assert client.getpeercert()["subjectAltName"] == (("DNS", "clienthello-fixture.local"),)
            assert client.selected_alpn_protocol() == server.selected_alpn_protocol() == "http/1.1"
            request = b"GET /fixture HTTP/1.1\r\nHost: clienthello-fixture.local\r\n\r\n"
            response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK"
            client.write(request)
            sin.write(cout.read())
            assert server.read(len(request)) == request
            server.write(response)
            cin.write(sout.read())
            assert client.read(len(response)) == response
            cases += 1
print(f"{cases} offline ClientHello profile cases passed on {h.runtime.eval(b'jit.version').decode()}; no driver or network sends.")
