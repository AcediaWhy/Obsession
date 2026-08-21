# Obsession zapret2 patch stack

Obsession does not vendor a mutable fork of zapret2. `upstream.lock.json` pins an
exact upstream tag/commit, the LuaJIT input, the signed Cygwin bootstrap and the
toolchain package versions. `patches/` is an ordered `git format-patch` series.

The current patchset (`v1.0.4-h1`) contains:

1. nested IPv4/IPv6 length validation for the packet dissector;
2. CSPRNG-backed randomness for wire-visible fake data;
3. LRU and byte budgets for conntrack, reassembly and delayed packets;
4. a token-bucket limit for unauthenticated QUIC Initial decryption;
5. compiler/linker mitigations, explicit source lists and an ASan target.

Build from the repository root:

```powershell
./ObsessionTauri/scripts/build-zapret2.ps1 -OutputDirectory ./artifacts/zapret2
```

The script verifies every pinned input and patch before compiling. It refuses a
changed Cygwin bootstrap, package drift, a patch hash mismatch, a wrong upstream
commit, or a PE file without ASLR, high-entropy VA, NX and stack-protector
imports. It also compares the result with `artifact.lock.json`; use
`-AllowArtifactHashChange` only while deliberately reviewing an upgrade. The
output includes `build-provenance.json` and the matching `cygwin1.dll`.

## Updating upstream safely

1. Change only the upstream tag and commit in `upstream.lock.json`.
2. Apply the existing patches with `git am` to a clean checkout of that commit.
3. Resolve conflicts patch-by-patch; never squash the series before review.
4. Re-run the builder and the Obsession runtime tests.
5. Review the upstream diff for the packet paths touched by patches 1, 3 and 4.
6. Bump `WINWS2_VERSION`, the strategy-pack `winws2_min`, resource provenance,
   binary sizes/hashes and `runtime-manifest.json` in the same Obsession commit.

If a patch applies cleanly, that proves only textual compatibility. A successful
clean build, PE checks and runtime regression tests remain mandatory before the
new artifact is shipped.
