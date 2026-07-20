# Legacy Observe-only Status UI Implementation Plan

**Design:** `docs/superpowers/specs/2026-07-20-legacy-observe-only-status-ui-design.md`

**Goal:** replace the disconnected Legacy Brain indicator with an accurate,
read-only projection of the active observe-only Manager without enabling any
recovery side effects.

## Safety rules

1. Status publication is read-only and cannot start, stop, switch or rewrite a
   Legacy configuration.
2. A live `winws` process alone never produces an `observing` status.
3. Session and sensor identity fence every asynchronous status update.
4. Zapret2 adaptive behavior and UI remain unchanged.
5. Existing Rain/adaptive/package changes are not edited, formatted, staged or
   committed.

## Task 1: Pure public status projection

**Files:**

- `ObsessionTauri/src-tauri/src/legacy_reliability/status.rs`
- `ObsessionTauri/src-tauri/src/legacy_reliability/mod.rs`

Add serializable `LegacyReliabilityPhase` and `LegacyReliabilityStatus` types.
Implement pure constructors for inactive, starting, blind and projection from
`ObserveOnlySnapshot`. Map Ready to observing, Degraded to degraded and
Blind/Stopped to blind. Test exact JSON shape and mapping without Tauri.

## Task 2: App-owned status and versioned publication

**Files:**

- `ObsessionTauri/src-tauri/src/state.rs`
- `ObsessionTauri/src-tauri/src/legacy_reliability/status.rs`
- `ObsessionTauri/src-tauri/src/legacy_reliability/runtime.rs`

Store the current public status and a monotonic revision in `AppState`. Add a
single publisher for `legacy-reliability://status`. It updates only when the
public projection changes. Manager health forwarding uses a cloned watch
receiver and rejects snapshots whose session/sensor no longer own the public
status.

## Task 3: Legacy lifecycle integration

**Files:**

- `ObsessionTauri/src-tauri/src/dpi.rs`
- `ObsessionTauri/src-tauri/src/commands.rs`

Publish starting after an exact Legacy runtime is established. Publish the
Manager snapshot only after Manager/Eyes installation succeeds. Registry,
spawn or Eyes failures publish blind while leaving bypass processes alone.
Stop, engine switch and emergency teardown publish inactive before awaiting
Manager shutdown so late Stopped snapshots are fenced out. Replace the English
Brain-suppression debug line with the explicit observe-only startup message.

## Task 4: Bootstrap and TypeScript contract

**Files:**

- `ObsessionTauri/src-tauri/src/commands.rs`
- `ObsessionTauri/src/lib/tauri.ts`
- `ObsessionTauri/src/store/legacyReliabilityStore.ts`
- `ObsessionTauri/src/store/legacyReliabilityStore.test.ts`
- `ObsessionTauri/src/store/launcherBootstrap.ts`
- `ObsessionTauri/src/store/launcherBootstrap.test.ts`
- `ObsessionTauri/src/lib/tauri.contract.test.ts`

Add a mandatory versioned `legacyReliability` bootstrap section, bump schema to
2 and register its listener before requesting the snapshot. The store applies
strictly newer revisions, matching the existing DPI/Brain/Adaptive pattern.
Extend listener-first overlap and cleanup tests.

## Task 5: Read-only Legacy panel

**Files:**

- `ObsessionTauri/src/design/components/LegacyReliabilityPanel.tsx`
- `ObsessionTauri/src/design/components/LegacyReliabilityPanel.test.tsx` or a
  pure display-model test if the current Vitest environment has no DOM
- `ObsessionTauri/src/screens/Dpi.tsx`

Render `Контроль надёжности`, `Режим — Только наблюдение`, the phase label and
the fixed capability explanation. Do not render the old Brain toggle for
Legacy. Keep the Zapret2 branch unchanged. Test all five phases, tones and the
absence of automatic-recovery wording/actions.

## Task 6: Verification and commit

Run focused Rust status/lifecycle tests and focused Vitest store/bootstrap/UI
tests, then `cargo test --all-targets`, `cargo clippy --all-targets -- -D
warnings`, `cargo fmt --all -- --check`, `npm test`, `npm run build` and
`git diff --check`. Inspect and stage only the status/UI implementation and this
plan. Verify the running dev build shows `Наблюдение` for the active healthy
Legacy Manager and `Ожидание запуска` after stop.
