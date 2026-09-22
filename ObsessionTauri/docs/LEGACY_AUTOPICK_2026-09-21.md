# Legacy auto-picker repair — 2026-09-21

## Cause and repair

The DPI screen gated Legacy tests on `protectedRuntimeAvailable`. That retired
capability intentionally returns false: the old AppData executable launch path
must remain disabled. The screen now uses the protected service DPI capability,
and `dpi_test` launches through the authenticated service instead of the retired
local launcher. Access-control monitoring is not the cause of that disabled gate.

The test holds the DPI operation gate, rejects an existing service session,
disables recovery for its temporary session, and stops only the generation it
started. Cancellation interrupts probes; cleanup failure aborts the picker.
The service also rejects concurrent starts while a runtime is active.

All category endpoints must pass, with successful HTTP responses and body reads
(Discord CDN root permits its expected 404). Twitch alone no longer qualifies a
YouTube strategy. Exhaustion reports failure without marking an old choice as
verified. The UI explains why controls are disabled.

## Configuration provenance

Ten Discord and ten YouTube/Twitch profiles are scoped adaptations of Flowseal
release 1.10.3. Source file SHA-256 values and mappings are recorded in
`third-party/flowseal/legacy-1.10.3.lock.json`. The older Discord lock remains as
historical asset provenance. No upstream BAT is executed. Broad IP-set/game
profiles are omitted; category host filters are retained. Google-specific TLS
profiles precede the generic YouTube/Twitch profile. Existing executables remain
unchanged by this repair.

## Verification and limits

- Frontend regression tests: 11 passed; production frontend build passed.
- New Rust protected-test module: 3 tests passed.
- Static config audit: 44 Legacy configs and 19 Zapret2 profiles passed.
- Config audit regression tests: 5 passed.
- Protected bundle verification: 44 Legacy strategies and 3 Zapret2 groups passed.
- Engine parser dry-run passed the complete Legacy loop, then failed on the
  Zapret2 executable with Windows status `0xc0000142`; the combined dry-run is
  therefore not a full success. No packet capture is enabled by parser checks.

No live provider test or installation was performed. Web access checks do not
prove Discord voice connectivity or YouTube video throughput. The picker still
selects the first passing strategy, not the fastest one. Results depend on the
provider; these profiles are not guaranteed to work on every network.

The running native app and previously built installer do not automatically gain
this Rust change. Rebuild/restart the native app, and distribute the updated
protected configuration bundle through the normal installer workflow. Updating
only the browser preview cannot validate service-backed tests.
