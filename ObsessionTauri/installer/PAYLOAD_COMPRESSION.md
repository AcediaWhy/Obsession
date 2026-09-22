# Installer payload compression

The setup builder embeds a gzip-compressed machine payload. No runtime download
or external unpacker is required; no UPX packing is used.

## Format

The outer envelope consists of `OBSGZIP1` (8 bytes), the uncompressed length
(little-endian u64), SHA-256 of the uncompressed payload (32 bytes), and exactly
one gzip member. Its content is the existing `OBSMACH1` package, unchanged.
The worker continues to accept legacy uncompressed `OBSMACH1` packages.

Before materialization, the decoder checks the size bound, gzip integrity,
exact expanded length, SHA-256, and absence of trailing members/data. Nested
compression is rejected. The existing inner manifest, per-file hashes, version,
and safe-path checks still apply. Hashes provide integrity checking, not a
publisher signature.

Both input and expanded payloads are limited to 512 MiB. Decoding reads at most
the declared expanded length plus one byte. File ownership currently requires
copying the decoded file contents, so transient memory can approach twice the
raw payload size, in addition to the embedded archive and application overhead.

## Verification

From the application root:

```powershell
node --test scripts/payload-compression.test.mjs
cargo test --manifest-path installer/src-tauri/Cargo.toml --lib --offline payload -- --test-threads=1
cargo test --manifest-path installer/src-tauri/Cargo.toml --lib --offline machine_worker::tests::machine_update -- --test-threads=1
node scripts/build-setup.mjs
cargo test --manifest-path installer/src-tauri/Cargo.toml --lib --offline built_compressed_machine_payload_is_authenticated -- --ignored --nocapture
```

Tests cover malformed/truncated archives, false lengths, excessive expansion,
hash/CRC failures, extra gzip members, unsafe inner paths, file materialization,
and update/rollback using temporary fixtures and a fake service. The ignored
test validates the real generated package after a build. These checks do not
replace an end-to-end elevated installation/update test on a disposable Windows
machine; that test is intentionally not performed on the development machine.

Installer UI copy and styling are independent of this change.

## Build measurement (2026-09-21, version 1.1.0)

- Previous saved setup: 46,359,552 bytes (44.21 MiB).
- Newly rebuilt setup: 32,815,104 bytes (31.29 MiB), 29.22% smaller.
- Current payload: 44,235,681 bytes raw, 28,710,333 bytes compressed.
- All 89 packaged files passed the real-package parser test.
- The full compressed payload was found byte-for-byte in the final EXE, and
  its generated `.sha256` file matched the EXE.

The previous EXE was preserved as
`dist-release/Obsession-Setup_1.1.0_x64-before-compression.exe`. The two installers
also differ in application build age: the new artifact rebuilds current source,
so the payload-only figures are the same-content compression comparison.
