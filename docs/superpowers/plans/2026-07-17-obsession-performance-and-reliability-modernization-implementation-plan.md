# Obsession Performance and Reliability Modernization Implementation Plan

**Дата:** 2026-07-17
**Статус:** готов к исполнению
**Design:** `docs/superpowers/specs/2026-07-17-obsession-performance-and-reliability-modernization-design.md`
**База:** текущий незакоммиченный worktree ветки `codex/zapret2-gate-d-recovery`

## Правила исполнения

- Не выполнять `git push`; commit boundaries остаются локальными.
- Не откатывать изменения, существовавшие до задачи.
- Перед каждым commit проверять `git diff --cached --name-status`.
- Не добавлять `install.ps1` и другие несвязанные файлы.
- `SECURITY_SCOPE.md` и `AdaptiveStrategyPanel.tsx` не восстанавливать без
  отдельного решения владельца.
- Каждая поведенческая правка начинается с failing/characterization test.
- После task запускать focused tests; после phase - полный gate.
- Не ослаблять targets, quorum, Eyes veto или certificate verification.
- Не менять HeroCore, композицию тем и визуальную идентичность.

## P0. Baseline и attribution

### Task 0.1. Карта текущего worktree

**Files:**

- Create: `docs/superpowers/reports/2026-07-17-modernization-worktree-baseline.md`

**Steps:**

1. Зафиксировать branch, HEAD, staged/unstaged/untracked list и diff stat.
2. Для каждого файла отметить: already implemented, partial, unrelated или
   owner decision.
3. Отдельно отметить два удаления и untracked `install.ps1`.
4. Перечислить уже готовые PID/write-queue/uptime/render/store-test изменения.
5. Проверить `git diff --check`.
6. Local commit: `docs: record modernization worktree baseline`.

### Task 0.2. Baseline telemetry

**Files:**

- Create: `ObsessionTauri/src/design/frameTelemetry.ts`
- Create: `ObsessionTauri/src/design/frameTelemetry.test.ts`
- Modify: `ObsessionTauri/src/design/render.ts`
- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`

**Steps:**

1. Написать Vitest для bounded sample buffer, p50/p95/p99 и reset.
2. Подтвердить падение focused test.
3. Реализовать pure telemetry collector без UI.
4. Подключить opt-in recording к `createRenderLoop`, не меняя cadence.
5. Добавить structured Rust timing fields без изменения model verdict.
6. Запустить focused tests и build.
7. Local commit: `perf: add modernization baseline telemetry`.

## P1. Runtime safety и cancellation

### Task 1.1. Stale probe characterization

**Files:**

- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`
- Reference: `ObsessionTauri/src-tauri/src/adaptive_strategy/model.rs`

**Steps:**

1. Создать session A, затем B и поздний `ProbeFinished` от A.
2. Зафиксировать: stale result не меняет `last_probe`, не эмитит UI probe и не
   участвует в cache confirmation.
3. Подтвердить падение на текущем mutation/emit order.
4. Вынести completion guard по session/attempt/generation до side effects.
5. Запустить adaptive model/runtime tests.
6. Local commit: `fix: reject stale adaptive probe completions`.

### Task 1.2. Session-owned tasks

**Files:**

- Create: `ObsessionTauri/src-tauri/src/adaptive_strategy/tasks.rs`
- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/mod.rs`
- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`

**Steps:**

1. Написать paused-time tests для cooperative cancel, repeated cancel и late
   result после closed generation.
2. Реализовать `SessionTaskSet` на существующих Tokio primitives.
3. Отслеживать preparation и candidate probe handles.
4. Abort применять только после bounded cooperative cancellation.
5. Удалить unowned `RunProbes` spawn.
6. Local commit: `refactor: own adaptive session tasks`.

### Task 1.3. Single-flight network identity

**Files:**

- Modify/Test: `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`

**Steps:**

1. Добавить test раннего завершения preparation до netid resolve.
2. Проверить, что consumers используют один resolve.
3. Заменить nested direct `netid::resolve` существующим single-flight helper.
4. Сохранить параллельность netid и QUIC discovery через tracked join.
5. Local commit: `fix: reuse single-flight network identity`.

### Task 1.4. Responsive spawn и rollback

**Files:**

- Create: `ObsessionTauri/src-tauri/src/adaptive_strategy/candidate_runtime.rs`
- Create: `ObsessionTauri/src-tauri/src/adaptive_strategy/rollback.rs`
- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/mod.rs`
- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`

**Steps:**

1. Написать tests: Cancel принимается во время spawn, rollback и base recheck;
   rollback выполняется ровно один раз.
2. Ввести typed worker completion с session/attempt/generation.
3. Перенести process side effect в `CandidateRuntime`.
4. Перенести exact restore/base recheck в `RollbackCoordinator`.
5. Оставить coordinator свободным для control queue.
6. Local commit: `refactor: keep adaptive coordinator responsive`.

### P1 gate

```powershell
cargo test --manifest-path ObsessionTauri/src-tauri/Cargo.toml --all-targets
cargo fmt --manifest-path ObsessionTauri/src-tauri/Cargo.toml --check
cargo clippy --manifest-path ObsessionTauri/src-tauri/Cargo.toml --all-targets -- -D warnings
npm --prefix ObsessionTauri test
npm --prefix ObsessionTauri run build
```
