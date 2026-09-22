# Discord Legacy and tray memory

The user's memory complaint concerns the hidden tray state, not the visible window. The target is 30–50 MiB if achievable without weakening protection or losing UI state. Total installed-app memory after this change has not yet been established.

## Discord strategies

Replaced the ten Legacy Discord profiles with adaptations of Flowseal's current strategies, pinned to commit `6cec828910d0809863205702182a3557d9d0e8c3` from https://github.com/Flowseal/zapret-discord-youtube. Exact source-to-profile mapping and fake-payload hashes are in `ObsessionTauri/third-party/flowseal/discord.lock.json`; its license is shipped in the custom installer.

The set covers multisplit with sequence overlap, fake/fakedsplit, fake/multisplit, hostfakesplit, badseq and timestamp families. Discord host filtering, QUIC and the protocol-filtered voice/STUN ranges are preserved. HTTP, voice and STUN fake-file options are now resolved through the protected manifest like TLS fake files. Saved profile filenames remain valid. The bundled winws binary is unchanged.

All 68 real engine parser invocations passed (`--dry-run`, no packet capture); nine protected materializer tests passed. This proves packaging and syntax, not effectiveness on Rostelecom. The user reports Zapret2 works on that connection; the new Legacy candidates still need a live installed-app check.

## Application-wide tray lifecycle

- Graphics/video pools release after one hidden second instead of three minutes. Reduced-motion on a visible window no longer triggers release. Startup already hidden and timer cancellation are handled.
- Hidden screens use `display:none` to release compositor surfaces while retaining forms and React state.
- Pending UI logs are capped at 200 before the paused frame scheduler can flush them.
- No unconditional Rain scene warmup; WebGL capability contexts are created only when a WebGL theme is selected.
- After three seconds with no pending backend requests, the frontend signals that WebView2 can sleep. Every command through the common Tauri bridge is counted until success or failure. Sequential manual config searches and concurrent requests postpone sleep.
- The native side rechecks actual Win32 visibility on the UI thread, sets controller visibility only on transitions, and uses `ICoreWebView2_3::TrySuspend/Resume`. Restore resumes before the visibility signal and the existing bootstrap refresh reconciles backend state. Rust/service protection keeps running.
- Removed `EmptyWorkingSet` and the previous mixed assumptions around `MemoryUsageTargetLevel`. No forced GC or repeated working-set trimming.

Microsoft API reference: https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_3 and https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19 . Suspension is best effort; a failed request is logged.

Alchemist also decodes its eight layers sequentially into display-sized, explicitly closed bitmaps. At 240px/DPR1 the retained layer pixels are 1,843,200 bytes instead of 50,320,512 bytes. This is a buffer calculation, not whole-app RAM savings. Canvas2D nearest-neighbour scaling preserves the existing sampler; a same-size animation frame no longer reallocates its canvas.

## Validation

- 453 frontend tests passed. Two pre-existing failures remain: `Onboarding.test.tsx` expects the older SVG/Ophanim core while HEAD already renders Alchemist canvases; `obsessionAxolotlLab.test.tsx` expects `obsession` while HEAD uses `quietpond`. These unrelated assertions were not changed.
- New tests cover hidden/reduced-motion distinction, cancelling delayed cleanup, startup hidden, concurrent/chained/failed backend work, bitmap failure and abort cleanup, and same-size canvas reuse.
- GitNexus reports critical aggregate impact because the worktree also includes the earlier DPI/recovery changes. The command bridge alone has 52 direct callers; its argument and result contract tests passed. New modules are not in the existing index.
- `scripts/measure-process-tree.ps1` measures private resident bytes by PID ancestry, excluding unrelated WebView apps. Commit bytes are recorded separately.
- `src-tauri/examples/tray_memory_probe.rs` is an isolated native WebView2 lifecycle test using the real render components. It has no DPI service access and does not represent total installed Obsession memory.

## Native diagnostic reproduction

Run from `ObsessionTauri`:

1. `npx vite build --config artifacts/theme-memory/vite.config.mjs`
2. `python -m http.server 1420 --bind 127.0.0.1 --directory src-tauri/target/theme-memory-static`
3. `cargo build --release --manifest-path src-tauri/Cargo.toml --example tray_memory_probe`
4. Embed `artifacts/theme-memory/tray-probe.manifest` in the example with the Windows SDK `mt.exe`: `mt.exe -nologo -manifest artifacts/theme-memory/tray-probe.manifest -outputresource:src-tauri/target/release/examples/tray_memory_probe.exe;#1` (quote the final argument in PowerShell).
5. Run `src-tauri/target/release/examples/tray_memory_probe.exe`; concurrently run `scripts/measure-process-tree.ps1 -ProcessName tray_memory_probe -Samples 21 -IntervalSeconds 3`.

The Common Controls manifest is needed only for this standalone diagnostic; the production application already receives its manifest from Tauri. A static server is required: Vite HMR reconnects can reload a suspended page and invalidate state-persistence checks. Earlier blank-page and HMR measurements are not treated as installed-app memory results.

### Observed results

The static production diagnostic passed both native suspend/resume cycles. Canvas counts were `5 → 0 → 2` in each Golden Meadow → tray → Alchemist cycle. The page counter continued `55 → 67 → 88 → 112 → 124 → 145`, confirming that restore preserved the document instead of reloading it. `IsSuspended` was true in both tray samples and false after both restores. A 250ms counter advanced by 21 ticks over each five-second restore interval, consistent with its timer being paused while suspended.

The Alchemist comparison found zero differing pixels at 104, 240 and 480 pixels. All eight retained layer bitmaps closed on every one of five unmounts; the last count was 128 created, 128 closed. Machine-readable evidence is in `ObsessionTauri/artifacts/theme-memory/verified-result.json`.

Memory did not settle identically in both cycles. First-cycle hidden samples still exceeded 200 MiB private resident memory; second-cycle samples fell to 7.64–10.92 MiB for the test host plus its WebView processes. Private committed memory remained about 255 MiB. These are distinct metrics: this is not evidence that all allocations were freed or that installed Obsession reliably meets 30–50 MiB. No whole-app before/after claim is made. The protected service and normal application stores were absent from this isolated diagnostic.

## Release artifact

Custom installer: `ObsessionTauri/dist-release/Obsession-Setup_1.1.0_x64-discord-tray-fix.exe` (46,552,576 bytes).

SHA-256: `3c52bec4fcfd1da0987c335e31585426e4a820719e700db6d00799edcc74d2d4`.

All 89 files in the installer machine payload were checked against their manifest hashes and current build/resources. The complete payload was verified byte-for-byte inside the final installer. This includes the production executable, ten Discord profiles, four new fake payload files and the Flowseal license. Installation and real Discord startup on Rostelecom remain unverified.
