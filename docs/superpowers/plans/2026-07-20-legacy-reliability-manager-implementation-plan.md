# Legacy Reliability Manager Implementation Plan

**Design:** `docs/superpowers/specs/2026-07-20-legacy-reliability-manager-design.md`

**Goal:** migrate Legacy from direct `Eyes -> Brain -> global start_many` to an
observe-only, generation-fenced Manager foundation, then add assessment and
scoped replacement without changing Zapret2.

## Safety Rules

1. Phase 1 is observe-only. New events cannot invoke the current Brain executor.
2. Zapret2 keeps its existing Eyes/adaptive path until a separate approved phase.
3. Existing `.conf` files are parsed independently and never rewritten or merged.
4. Every side effect in later phases requires session, lane, sensor, registry and
   network fences.
5. Dirty Rain/adaptive files are not edited, formatted, staged or reverted.

## Task 1: Contracts

**Files:**

- `ObsessionTauri/src-tauri/src/legacy_reliability/mod.rs`
- `ObsessionTauri/src-tauri/src/legacy_reliability/contracts.rs`
- `ObsessionTauri/src-tauri/src/lib.rs`

Add strong IDs/generations, `LegacySessionContext`, network stability,
`EventEnvelope`, typed Flow/Health/Gap events and owned evidence. Verify stable
serialization and the category/lane-generation invariant.

## Task 2: TargetRegistry

**File:** `ObsessionTauri/src-tauri/src/legacy_reliability/target_registry.rs`

Parse independent config/hostlist records into one immutable snapshot. Implement
longest label-boundary suffix matching, explicit ambiguity, exclusions,
selection-aware deterministic version and compact per-config TCP port plans.
Runtime capture uses only active selections and matches remote ports by packet
direction; inactive candidates remain snapshot metadata. TCP/80 is captured
only for packet/parse/drop health counters and never creates a policy Flow or
evidence in Phase 1; UDP/QUIC do not feed Legacy automatic recovery.

## Task 3: Sensor Health and Gap

**File:** `ObsessionTauri/src-tauri/src/legacy_reliability/health.rs`

Implement a pure tracker for Ready/Degraded/Blind/Stopped, monotonic counters and
coalesced Gap generation. A clean 10-second window is required after a gap before
the sensor becomes reliable again.

## Task 4: Bounded Legacy Ingress

**Files:**

- `ObsessionTauri/src-tauri/src/legacy_reliability/ingress.rs`
- `ObsessionTauri/src-tauri/src/state.rs`

Use separate bounded data/control channels. Flow producers use `try_send`; an
atomic dirty counter guarantees that overflow is observed before a later Flow.
Fence every event against the active session/sensor/registry/lane snapshot.
Expose only an observe-only status in Phase 1.

## Task 5: Legacy-Only Eyes Adapter

**Files:**

- `ObsessionTauri/src-tauri/src/eyes/signal.rs`
- `ObsessionTauri/src-tauri/src/eyes/flow.rs`
- `ObsessionTauri/src-tauri/src/eyes/capture.rs`
- `ObsessionTauri/src-tauri/src/dpi.rs`

Add a new Legacy entry point that emits typed events and uses TargetRegistry.
Keep the existing `eyes::start` callback intact for Zapret2. Remove disk/UI/Brain
work from the Legacy capture callback. Legacy requires bounded TCP reassembly
and structurally valid TLS records; worker failure is terminally Blind for its
sensor generation. Attribute runtime flows only through active owners. Keep
`syn_no_synack` unattributed and
diagnostic-only until reliable DNS/socket correlation exists. Do not yet route
accepted events to the old Brain executor.

## Task 6: Lifecycle Fencing

Close the Legacy session before teardown, create a new session only after a
successful manual Legacy start and reject late events. An empty/invalid registry
means degraded sensor, never an implicit watch-all filter. Engine switch to
Zapret2 closes the Legacy context first.

## Task 7: Verification and Commits

Run focused contract/registry/health/ingress tests, Eyes replay tests, the full
Rust suite, `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`.
Commit Phase 1 separately after inspecting the exact staged paths. Phase 2 begins
only after the observe-only foundation is green.
