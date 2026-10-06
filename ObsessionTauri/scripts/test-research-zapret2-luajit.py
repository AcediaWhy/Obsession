"""Execute the shipped research TLS recipes without a driver or network.

Uses the existing Lua fixture (including its regression suite), bundled Lua,
independent position oracle and an in-memory TCP receiver. Fake rejection is
an explicit model: bad ACK, MD5 on an unauthenticated connection, stale TS.
It does not establish real server rejection, exact native marker placement,
SYN/hostlist eligibility, DPI effect, QUIC/HTTP3 or video/voice functionality.
Install test-only lupa==2.6 and expose its directory through PYTHONPATH.
"""
from pathlib import Path
import json
import re
import runpy
import ssl
import struct
import subprocess
import tempfile
import shutil

ROOT = Path(__file__).resolve().parent.parent
fixture = runpy.run_path(str(ROOT / 'scripts/test-clienthello-profile-luajit.py'))
Harness, parse, hello = (fixture[name] for name in ('Harness', 'parse', 'hello'))
pack_dir = ROOT / 'src-tauri/resources/strategy-packs/builtin'
pack = json.loads((pack_dir / 'manifest.json').read_text(encoding='utf-8'))
profiles = [p for p in pack['presets'] if p.get('candidate') and 'tls' in p['filter_l7']]
quarantined = json.loads((ROOT / 'scripts/fixtures/zapret2-quarantined-recipes.json').read_text(encoding='utf-8'))['profiles']


class RecipeHarness(Harness):
    def __init__(self):
        super().__init__()
        g = self.runtime.globals()
        g[b'parse_hex'] = lambda value: bytes.fromhex(value.decode())
        g[b'_resolve_pos'], g[b'_resolve_range'] = self.position, self.position_range
        self.runtime.execute(b'''VERDICT_MODIFY=2; TCP_KIND_SCALE=3; TCP_KIND_MD5=19;
            resolve_pos=_resolve_pos; resolve_range=_resolve_range;
            instance_cutoff=function()end;
            brandom=function(n)local s={}; for i=1,n do s[i]=string.char(i%256)end; return table.concat(s)end;
            brandom_az=function(n)return string.rep('a',n)end;
            brandom_az09=brandom_az;''')
        for blob in pack['blobs']:
            g[blob['name'].encode()] = (pack_dir / blob['path']).read_bytes()

    def position(self, data, kind, marker):
        values = self.positions(data, kind, marker)
        return values[1]

    def position_range(self, data, kind, markers, _strict):
        values = self.positions(data, kind, markers)
        return values if len(values) == 2 else None

    def positions(self, data, kind, markers):
        assert kind == b'tls_client_hello'
        info = parse(data)
        start, end = info['sni_span']
        host = info['sni']
        labels = host.split(b'.')
        sld = labels[-2] if len(labels) > 1 else labels[-1]
        sld_start = start + len(host) - (len(labels[-1]) + 1 + len(sld)) if len(labels) > 1 else start
        anchors = {b'host': start, b'endhost': end, b'sniext': start - 9,
                   b'midsld': sld_start + len(sld) // 2}
        result = []
        for marker in markers.split(b','):
            if re.fullmatch(rb'-?\d+', marker):
                value = int(marker)
                offset = value if value >= 0 else len(data) + value
            else:
                match = re.fullmatch(rb'([a-z]+)([+-]\d+)?', marker)
                assert match and match[1] in anchors, marker
                offset = anchors[match[1]] + int(match[2] or b'0')
            if 0 <= offset < len(data):
                result.append(offset + 1)
        return self.runtime.table_from(sorted(set(result)))

    def run_recipe(self, profile, data, timestamps=True, base=100000, mss=560):
        d = self.desync(data, timestamps=timestamps, base=base, mss=mss)
        d[b'dis'][b'tcp'][b'th_win'] = 65535
        actions = profile.get('steps') or [{'action': a, 'payload': profile['payload']} for a in profile['desync']]
        verdict = 0
        for index, step in enumerate(actions):
            # CLI instance payload matching is modeled; all tested data are CH.
            assert 'all' in step['payload'] or 'tls_client_hello' in step['payload']
            fields = step['action'].split(':')
            args = {}
            for field in fields[1:]:
                key, _, value = field.partition('=')
                args[key.encode()] = value.encode()
            d[b'arg'] = self.runtime.table_from(args)
            d[b'func_instance'] = f'recipe_{index}'.encode()
            result = self.runtime.globals()[fields[0].encode()](None, d)
            verdict = result if result is not None else verdict
        if verdict != 1:
            self.send(d[b'dis'])  # Original survives pass/modify verdict.
        return d, verdict


def receive(harness, data, base):
    restored = [None] * len(data)
    originals, rejected = 0, 0
    for packet in harness.sends:
        assert packet['ok']
        options = dict(packet['options'])
        ts = options.get(8)
        if packet['ack'] != 900000 or 19 in options or (ts and int.from_bytes(ts[:4], 'big') < 400000):
            rejected += 1
            continue
        offset = (packet['seq'] - base) % (1 << 32)
        assert offset + len(packet['payload']) <= len(data), (offset, len(packet['payload']), len(data))
        for index, value in enumerate(packet['payload'], offset):
            assert restored[index] in (None, value), 'conflicting original TCP overlap'
            restored[index] = value
        originals += 1
    assert originals >= 2, 'recipe did not emit split original data'
    assert None not in restored, 'missing original TCP interval'
    assert bytes(restored) == data, 'ClientHello bytes changed'
    return bytes(restored), rejected


cases = 0
# Keep the rejected source recipe and its counterexample reproducible. It may
# succeed under the stale-TS model, but must fail integrity without timestamps.
for profile in quarantined:
    assert not any(p['id'] == profile['id'] for p in pack['presets'])
    for late in (False, True):
        data = hello(late=late)
        h = RecipeHarness()
        h.run_recipe(profile, data, timestamps=True, base=7)
        receive(h, data, 7)
        h = RecipeHarness()
        h.run_recipe(profile, data, timestamps=False, base=7)
        try:
            receive(h, data, 7)
        except AssertionError as error:
            assert 'conflicting original TCP overlap' in str(error)
        else:
            raise AssertionError('quarantined recipe unexpectedly passed without timestamps')
        cases += 1
for profile in profiles:
    for late in (False, True):
        for timestamps in (False, True):
            for base in (7, (1 << 32) - 10):
                data = hello(late=late)
                h = RecipeHarness()
                d, verdict = h.run_recipe(profile, data, timestamps=timestamps, base=base)
                try:
                    received, rejected = receive(h, data, base)
                except AssertionError as error:
                    raise AssertionError(f"{profile['id']}: timestamps={timestamps}, late={late}, base={base}: {error}") from error
                assert parse(received)['sni'] == b'gateway.discord.gg'
                assert verdict == 1
                if profile['candidate']['family'] == 'hostfake':
                    assert rejected >= 1
                if profile.get('steps'):
                    assert d[b'dis'][b'tcp'][b'th_win'] == 1
                cases += 1

# Actual authenticated TLS1.2/1.3 handshakes and HTTP requests, using the
# reconstructed original bytes and explicit fake-rejection model above.
openssl = shutil.which('openssl')
assert openssl, 'OpenSSL CLI needed for ephemeral fixture PKI'
with tempfile.TemporaryDirectory(prefix='obsession-research-fixture-') as folder:
    certificate, key = Path(folder) / 'cert.pem', Path(folder) / 'key.pem'
    subprocess.run([openssl, 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(key),
                    '-out', str(certificate), '-days', '1', '-subj', '/CN=recipe-fixture.local',
                    '-addext', 'subjectAltName=DNS:recipe-fixture.local'], capture_output=True, check=True, timeout=15)
    for profile in profiles:
        for version in (ssl.TLSVersion.TLSv1_2, ssl.TLSVersion.TLSv1_3):
            server_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            server_context.minimum_version = server_context.maximum_version = version
            server_context.load_cert_chain(certificate, key)
            client_context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            client_context.minimum_version = client_context.maximum_version = version
            client_context.load_verify_locations(str(certificate))
            cin, cout, sin, sout = (ssl.MemoryBIO() for _ in range(4))
            client = client_context.wrap_bio(cin, cout, server_hostname='recipe-fixture.local')
            server = server_context.wrap_bio(sin, sout, server_side=True)
            try:
                client.do_handshake()
            except ssl.SSLWantReadError:
                pass
            data = cout.read()
            h = RecipeHarness()
            h.run_recipe(profile, data, base=7)
            restored, _ = receive(h, data, 7)
            sin.write(restored)
            done = [False, False]
            for _ in range(20):
                for index, peer, incoming, outgoing in ((0, server, cin, sout), (1, client, sin, cout)):
                    if not done[index]:
                        try:
                            peer.do_handshake()
                            done[index] = True
                        except ssl.SSLWantReadError:
                            pass
                    if outgoing.pending:
                        incoming.write(outgoing.read())
                if all(done):
                    break
            assert all(done)
            request = b'GET /fixture HTTP/1.1\r\nHost: recipe-fixture.local\r\n\r\n'
            response = b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK'
            client.write(request)
            sin.write(cout.read())
            assert server.read(len(request)) == request
            server.write(response)
            cin.write(sout.read())
            assert client.read(len(response)) == response
            cases += 1
print(f'{cases} research TLS recipe cases passed, including {len(profiles) * 2} TLS/HTTP round trips and 4 quarantine counterexamples. No live DPI result claimed.')
