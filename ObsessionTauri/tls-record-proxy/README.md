# TLS record fragmentation prototype

This is an isolated fixed-target raw TCP adapter for Obsession experiments. It reconstructs the **first plaintext ClientHello**, checks its visible SNI against the configured name, and places a TLS-record boundary in the middle of that hostname. Additional records keep each fragment below the configured byte cap. All original handshake bytes, including random values, extensions and key shares, remain identical. The adapter then relays the remaining encrypted stream in both directions.

TLS stays between the original client and target server: the adapter holds no certificate/key, does not decrypt application traffic and does not intercept authentication. The operating system manages independent TCP streams on its two sides, so this implementation does not rewrite TCP sequence/ACK numbers. It accepts only `127.0.0.1` or `::1`, one numeric target IP:port, and one expected DNS name. It supports neither HTTP CONNECT nor arbitrary target selection.

```powershell
cargo build --offline --release --manifest-path tls-record-proxy/Cargo.toml
.\tls-record-proxy\target\release\obsession-tls-record-proxy.exe --listen 127.0.0.1:18443 --target 203.0.113.1:443 --server-name example.org
```

The address above is a documentation placeholder. Supply a currently correct target IP explicitly; the adapter does not resolve or modify DNS. A client that can preserve the original TLS hostname while connecting to the local listener can use it, for example:

```powershell
curl.exe --noproxy "*" --connect-to "example.org:443:127.0.0.1:18443" --http1.1 --max-time 15 https://example.org/
```

That command retains ordinary certificate and hostname verification. Do not use `--insecure`. Normal certificate trust is needed for a real server. The tests use a fresh, explicitly trusted local test certificate and never modify the machine trust store.

This binary is a development prototype, **not transparent Discord routing**. It does not register a proxy, change hosts/routes/DNS, or capture other applications. A real Discord client cannot be assumed to use it automatically. QUIC/UDP, voice and subsequent ClientHello after HelloRetryRequest are not reframed. ECH traffic whose visible outer SNI differs from the expected name is rejected before opening the target connection. Compatibility with local TLS servers is established; effectiveness against any ISP's TSPU has not been tested.

Default limits: 16 simultaneous connections; 5 seconds to collect/send ClientHello and 5 seconds to connect to the target; 60 seconds idle; **300 seconds total connection lifetime**. This deliberately bounded lifetime also closes a still-active WSS connection after five minutes. Maximum original hello is 65,536 bytes; maximum input record is 16,384 bytes and 256 input records. Fragment cap defaults to 512 bytes and can be set with `--fragment-bytes 32..16384`. A full record coalesced after ClientHello is preserved, as are all later records. FIN on one side shuts down only the other side's write half and leaves the reverse direction available. Ctrl+C closes the listener and its active relays.

CLI status reports connection numbers, input/output record counts, prepared byte counts, elapsed time, and static error kinds. It never logs ClientHello contents, URLs, cookies, tokens or application payloads. It does not log native packet receipt or prove what an external DPI reconstructed.

## Verification

```powershell
cargo test --offline --manifest-path tls-record-proxy/Cargo.toml
cargo clippy --offline --manifest-path tls-record-proxy/Cargo.toml --all-targets -- -D warnings
```

Eleven integration tests on Windows cover:

- Two fresh, certificate-verified TLS 1.3 sessions and two fresh TLS 1.2 sessions; verified Finished, negotiated version and ALPN; 200 KB HTTP uploads and 400 KB responses after the client closes its write half.
- Byte-identical handshake reconstruction and a TLS-record boundary inside the actual client SNI.
- Already fragmented ClientHello and transport writes one byte at a time; complete TLS 1.3 and HTTP thereafter.
- TLS 1.3 WebSocket upgrade followed by three masked ping/pong exchanges.
- Native Windows curl/Schannel HTTPS with `--connect-to`, original-name certificate validation, an explicitly trusted temporary public certificate, and no external connections. This test needs Schannel credential APIs; the Codex filesystem sandbox blocks those APIs, so the recorded Windows run used approved unsandboxed execution.
- Invalid/mismatched/oversized/truncated input opens no upstream connection; connection cap, hello deadline, idle deadline and total lifetime; original-record trailing data preservation; non-loopback or unbounded configuration rejection.

All tests use the computer's own loopback TLS server and ephemeral certificates. No real Discord account, user journal or remote test endpoint is involved.
