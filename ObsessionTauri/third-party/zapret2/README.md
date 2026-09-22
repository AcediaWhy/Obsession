# Obsession zapret2 patch stack

Obsession does not vendor a mutable fork of zapret2. `upstream.lock.json` pins an
exact upstream tag/commit, the LuaJIT input, the signed Cygwin bootstrap and the
toolchain package versions. The byte-exact official Windows release archive is
also pinned as the source of the v1.0.5.2 `WinDivert.dll`. `patches/` is an ordered
`git format-patch` series.

The current patchset (`v1.0.5.2-h2`, upstream `6b6c63e`) contains:

1. nested IPv4/IPv6 length validation for the packet dissector;
2. CSPRNG-backed randomness for wire-visible fake data;
3. LRU and byte budgets for conntrack, reassembly and delayed packets;
4. a token-bucket limit for unauthenticated QUIC Initial decryption;
5. compiler/linker mitigations, explicit source lists and an ASan target.

The h2 port keeps all five protections. Upstream now checks IPv4 total length
against header length and uses `size_t` in the dissector, so patch 1 retains only
the missing short-buffer guards. Upstream removed `ReasmResize`; patch 3 no
longer modifies that dead function. Patches 2, 4 and 5 are unchanged. Lua files
are taken together from the v1.0.5.2 release; its compatibility API remains 6.
The Windows DLL and compiler versions are unchanged from h1.

Build from the repository root:

```powershell
./ObsessionTauri/scripts/build-zapret2.ps1 -OutputDirectory ./artifacts/zapret2
```

The script verifies every pinned input and patch before compiling. It refuses a
changed Cygwin bootstrap, package drift, a patch hash mismatch, a wrong upstream
commit, a `winws2.exe` without ASLR, high-entropy VA, NX and stack-protector
imports, or a `WinDivert.dll` that differs from the pinned official release. The
DLL must retain `DYNAMIC_BASE` and must not restore the `HIGH_ENTROPY_VA` flag
removed by upstream in v1.0.4. The script also compares every output with
`artifact.lock.json`; use
`-AllowArtifactHashChange` only while deliberately reviewing an upgrade. The
output includes `build-provenance.json`, the matching `cygwin1.dll`, and the
pinned `WinDivert.dll`.

## Updating upstream safely

1. Update the upstream tag, commit, source timestamp and official archive hash
   in `upstream.lock.json` after checking the release against its published digest.
2. Apply the existing patches with `git am` to a clean checkout of that commit.
3. Resolve conflicts patch-by-patch; never squash the series before review.
4. Re-run the builder and the Obsession runtime tests.
5. Review the upstream diff for the packet paths touched by patches 1, 3 and 4.
6. Bump `WINWS2_VERSION`, the strategy-pack `winws2_min`, resource provenance,
   binary sizes/hashes and `runtime-manifest.json` in the same Obsession commit.

If a patch applies cleanly, that proves only textual compatibility. A successful
clean build, PE checks and runtime regression tests remain mandatory before the
new artifact is shipped.

## Verification of v1.0.5.2-h2

Two clean builds with the pinned Cygwin toolchain produced the same executable:
`d2a3dc3f6bb11f343e6ff74fa083a03e973ddb36d1901632d5ccfc07ccc2912e`.
The full application and service unit suites passed. Run
`npm run verify:dpi-parsers` from `ObsessionTauri` on Windows to build the service
and check 68 materialized invocations with the shipped engines' `--dry-run`.
This exercises argument parsing and resource loading, not packet processing.
It does not install a service or start network capture. The test child uses
`RunAsInvoker`; production elevation behavior is unchanged.
