# Obsession Telegram Proxy

Headless Rust MTProto-to-WebSocket bridge used by Obsession. The implementation
is an independent rewrite derived from Flowseal's `tg-ws-proxy`; attribution and
license terms are in `NOTICE` and `LICENSE`.

## Privacy defaults

- Direct Telegram DC connections are attempted first.
- Public Cloudflare relay domains are disabled by default. They are used only
  when the process is started with `--cfproxy`.
- `--no-cfproxy` remains available as an explicit compatibility-safe disable.
- The relay cache can only reorder the built-in allowlist; it cannot add hosts.
- The example Cloudflare Worker is intentionally fail-closed because Telegram
  currently rejects Cloudflare Workers egress.
- Proxy secrets and full `tg://` links must not be written to application logs.

Run `obsession-tg-proxy --help` for the complete CLI contract.

## Build and test

```powershell
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
./scripts/build-for-obsession.ps1
```

The final script builds the locked release binary, copies it into Obsession,
synchronizes the license files, and regenerates the protected runtime manifest.
