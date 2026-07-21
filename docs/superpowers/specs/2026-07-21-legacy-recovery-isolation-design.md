# Legacy Recovery Isolation and Blackhole Recovery

Date: 2026-07-21

Status: approved direction, pending written-spec review

## Context

Legacy currently runs one independent `winws.exe` process per selected category. Every process may learn failed domains through `--hostlist-auto`. Because all processes observe overlapping TCP/UDP ports, a category can learn domains owned by another category. The current production data demonstrates this: `autohosts/discord.txt` contains YouTube and `googlevideo.com` hosts.

This produces two coupled failures:

1. A neighboring `winws` process can partially bypass another category, so a deliberately broken YouTube configuration appears intermittently functional.
2. `TargetRegistry` treats learned entries as authoritative targets. A more-specific learned Discord host therefore steals a YouTube flow through longest-suffix matching, causing false Discord assessments.

The assessment state also clears an applied negative Gate result as soon as a small Working quorum appears. Intermittent services consequently oscillate between `Working` and `DpiBlocked`. Finally, the current policy freezes a blackhole incident instead of offering a candidate, so common timeout-style DPI blocking can never exercise automatic recovery.

## Goals

- Preserve independent category lanes without merging `.conf` files.
- Keep every user-owned source `.conf` byte-for-byte unchanged.
- Back up and safely migrate existing cross-category learned domains.
- Prevent known foreign domains from being learned again during the same Legacy session.
- Make static category lists authoritative for reliability attribution.
- Keep configuration trust stable while dynamic auto-hostlists grow.
- Add recovery hysteresis so a few alternating flows cannot flap the UI or policy.
- Allow a freshly confirmed blackhole incident to use the existing scoped replacement, confirmation, rollback, pacing, and cache machinery.
- Preserve neighboring processes throughout a recovery attempt.

## Non-goals

- Combining category configs into one `winws` process.
- Redesigning the Legacy reliability UI.
- Assigning semantic categories to learned domains that match no static list.
- Wiring regional `ranking.json` into candidate ordering.
- Changing Zapret2/adaptive strategy behavior.

## 1. Static ownership model

Static `--hostlist` inputs define semantic category ownership. Dynamic `--hostlist-auto` inputs remain valid runtime learning stores but do not define actionable reliability targets.

The registry will retain auto-hostlist references for validation and launch preparation, but it will not:

- insert auto-hostlist contents into `TargetRegistry.targets`;
- use auto-hostlist contents for active attribution or Environment Gate targets;
- include mutable auto-hostlist bytes in the registry content identity or per-config trust fingerprint.

The config text and auto-hostlist reference remain part of the config identity. Static include and exclusion contents remain fingerprinted. Hash domains will be versioned so old and new fingerprint semantics cannot collide. Existing cache entries will simply stop matching; no unsafe promotion is migrated.

Result: `www.youtube.com` learned by Discord cannot outrank the authoritative `youtube.com` suffix, and ordinary auto-learning no longer invalidates trust on the next session.

## 2. Existing auto-hostlist migration

A bounded `autohost_isolation` component will build a pure migration plan before any Legacy process starts. It scans known static lists and auto-hostlists and applies these rules:

- A learned host matching its source category's static ownership stays in place.
- A learned host matching exactly one foreign category, and not its source category, moves to that category's auto-hostlist.
- A host matching multiple static categories is removed from category-specific auto ownership and retained in the backup; static shared ownership continues to cover it.
- A host matching no static category stays in its source auto-hostlist. It remains available to `winws` but is ignored by reliability attribution.

Migration is idempotent, deduplicates and sorts normalized hostnames, and runs only when no Legacy process owns the files. Before the first write, it creates a timestamped snapshot under the Obsession data directory. Writes use same-directory temporary files and Windows-safe atomic replacement. If any replacement fails, the component restores the snapshot and aborts Legacy startup instead of launching against a partially migrated set. Backups are bounded to the three newest successful migration snapshots.

Only privacy-safe counts enter application logs: files changed, hosts moved, ambiguous hosts quarantined, and whether rollback occurred. Raw learned hosts never enter the reliability JSONL or UI.

## 3. Runtime isolation overlay

One-time cleanup is insufficient because running `winws` processes can relearn foreign hosts. Every Legacy start path—initial start, scoped candidate start, rollback, and crash retry—will therefore prepare an effective launch config.

For each selected category, the preparer:

1. Builds a foreign-only static exclusion set from the other selected categories. Shared static ownership is not excluded.
2. Writes a generated exclusion list beneath a bounded runtime directory in the Obsession data directory.
3. Tokenizes the selected source `.conf` and injects that exclusion reference into every desync profile containing `--hostlist-auto`.
4. Writes a generated effective `.conf` and launches `winws` with that file.

The source config, selected config name, candidate identity, and UI value remain unchanged. `DpiProc` continues to own the original category/config selection and strategy fingerprint. The generated overlay is a constant safety boundary fenced by the surrounding registry/session/neighbor identities, not a user strategy candidate.

If no foreign exclusion is required, the original config may be launched directly. If tokenization or generated-file persistence fails, startup fails closed with an actionable log message. The application must never silently fall back to an unisolated launch.

## 4. Assessment hysteresis

Negative and positive evidence will no longer cancel each other symmetrically.

- Initial detection thresholds remain unchanged: reset quorum is 3 flows across 2 targets; TLS-blackhole quorum is 2 flows across 2 targets.
- Working still requires 2 flows across 2 targets, or 2 flows for a genuinely single-target category.
- After an Environment Gate applies `DpiSuspected` or `DpiBlocked`, a Working quorum begins a recovery-confirmation interval instead of immediately deleting the incident.
- Recovery requires at least 10 seconds of retained Working quorum with no new adverse flow. Any reset or TLS blackhole restarts the recovery interval.
- Only after that interval may the applied diagnosis and adverse evidence be cleared. Silence alone never clears or confirms recovery.
- A policy cooldown cannot be bypassed by a different adverse trigger. A stable recovery confirmation may clear the displayed incident, while attempt pacing/candidate cooldown remains owned by the recovery coordinator/cache.

This separates three concepts that were previously conflated: the current access diagnosis, suppression of duplicate Gate work, and pacing of replacement attempts.

## 5. Blackhole recovery policy

A fresh `DpiBlocked` Gate result is actionable when an eligible candidate exists. It will produce `SwitchLane` just like `DpiSuspected`, while retaining its higher confidence in diagnostics.

- With an eligible candidate: propose or automatically execute the existing scoped recovery according to mode.
- With no eligible candidate, or when all candidates are cooling down/exhausted: freeze the lane without touching the current process.
- The executor's fresh preflight may authorize `DpiBlocked` only for the exact current session, sensor, registry, lane generation, stable network identity, and still-valid passive quorum.
- Offline, DNS failure, upstream degradation, partial target availability, stale fences, or unreliable sensor state continue to reject mutation.
- Candidate readiness, access confirmation, exact rollback, neighbor preservation, global 30-second pacing, and persisted candidate cooldown remain unchanged.

Successful candidate confirmation starts a new lane generation, naturally fencing the previous incident. Failed confirmation rolls back exactly as Phase 4 already specifies.

## 6. UI behavior

No new primary controls are required.

- Assisted mode shows `Сменить конфигурацию` for an actionable confirmed blackhole instead of `Приостановить восстановление`.
- Automatic mode runs the same action without approval when not globally paused or manually frozen.
- During the recovery-confirmation interval, the last applied negative assessment remains stable instead of flickering to `Работает`.
- After stable recovery, the row returns to `Работает` or the existing sticky `Доступ подтверждён` state.

## 7. Failure handling and data safety

- Migration and overlay preparation occur before process mutation.
- Existing source configs are never rewritten.
- Existing auto-hostlists are recoverable from bounded backups.
- Ambiguous or malformed learned entries never authorize category actions.
- Isolation preparation failure prevents the affected Legacy start and leaves neighboring running lanes untouched during scoped recovery.
- Generated paths remain within the Obsession data directory and use validated category/config components.
- Reliability diagnostics continue to omit raw SNI and hostlist contents.

## 8. Verification

### Pure/unit tests

- Migration: foreign move, same-owner retention, ambiguity quarantine, unknown retention, deduplication, idempotence, bounded backups, simulated rollback.
- Overlay: multiple profiles, inline/separate option values, quoted paths, existing excludes, comments, configs without auto-learning, invalid syntax, path confinement.
- Registry: a foreign auto entry cannot steal static ownership; auto content changes do not change registry/config fingerprints; static/config changes still do.
- Assessment: the captured production timeline no longer flaps; recovery requires 10 clean seconds; new adverse evidence restarts it; cross-trigger cooldown bypass is rejected.
- Policy: `DpiBlocked` switches with candidates and freezes without them.
- Executor: fresh exact `DpiBlocked` is accepted; environment, target, sensor, and stale-fence failures remain rejected.

### Integration/regression tests

- Seed Discord auto-hosts with exact YouTube/CDN hosts and run a pass-through YouTube config.
- Verify YouTube flows remain attributed to YouTube, Discord does not change assessment because of those flows, and learned foreign entries cannot reappear in Discord during the session.
- Verify automatic recovery changes only the YouTube config/PID, leaves the Discord PID intact, confirms access, and writes trust/cooldown state correctly.
- Verify failed candidates roll back the exact previous process/config and preserve neighboring lanes.
- Run the complete Rust, frontend, formatting, clippy, and production-build suites.
- Perform a Windows dev-build acceptance test with the real `winws.exe`, followed by an offline safety test.

## Acceptance criteria

The change is complete when the original contaminated-auto-host scenario produces all of the following:

1. Discord remains correctly assessed and its PID is unchanged.
2. The broken YouTube lane is diagnosed without category theft or status flapping.
3. Assisted mode offers a YouTube candidate; Automatic mode performs the scoped attempt.
4. A working candidate restores YouTube and becomes provisional trust; a failing candidate rolls back exactly.
5. Disconnecting the network prevents every config mutation.
6. Source `.conf` files remain unchanged, and migrated auto-host data has a restorable backup.
