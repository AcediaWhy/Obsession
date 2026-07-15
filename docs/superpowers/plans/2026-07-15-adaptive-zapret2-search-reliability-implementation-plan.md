# Adaptive Zapret2 Search Reliability Implementation Plan

**Design:**
`docs/superpowers/specs/2026-07-15-adaptive-zapret2-search-reliability-design.md`

**Goal:** устранить false-negative оценку кандидатов и заменить статический
перебор на детерминированный поиск, который использует evidence предыдущей
попытки и никогда не выдаёт рабочую базу за новый результат.

## Task 1: Typed probe evidence and deterministic evaluator

**Files:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/model.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`

**Steps:**

1. Ввести transport/failure-stage, timings, round и Eyes counters.
2. Разделить raw target evidence и итоговое evaluator decision.
3. Реализовать правило 2/3 и Eyes veto чистой тестируемой функцией.
4. Писать каждый probe batch и decision в `app.log`.
5. Добавить regression tests для transient timeout и false-negative базы.

## Task 2: Calibration, stabilization and exact base recheck

**Files:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/model.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`
- `ObsessionTauri/src-tauri/src/dpi.rs`

**Steps:**

1. Захватывать неизменяемый baseline до генерации.
2. Выполнять три calibration rounds до первого кандидата.
3. Добавить 1000 ms stabilization и три candidate rounds.
4. После каждого rollback выполнять два base-check rounds.
5. Останавливать поиск как `probe_unreliable` или `base_unhealthy`, не как
   найденную стратегию.

## Task 3: Eyes lifecycle for Zapret2

**Files:**

- `ObsessionTauri/src-tauri/src/commands.rs`
- `ObsessionTauri/src-tauri/src/dpi.rs`
- `ObsessionTauri/src-tauri/src/state.rs`
- `ObsessionTauri/src-tauri/src/eyes/*`

**Steps:**

1. Вынести общее владение Eyes для Legacy и Zapret2.
2. Запускать Eyes после успешного spawn Zapret2.
3. Привязывать evidence к DPI generation/session window.
4. Очищать окно между baseline, candidate и base recheck.
5. Проверить stop/crash/cancel/shutdown без stale evidence.

## Task 4: Transport-specific probes

**Files:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`
- `ObsessionTauri/src-tauri/src/eyes/quic.rs`
- `ObsessionTauri/src-tauri/Cargo.toml`

**Steps:**

1. Оставить TLS evaluator на DNS/TCP/TLS/HTTPS.
2. Добавить настоящий QUIC Initial или HTTP/3 probe.
3. Запретить TCP/HTTPS evidence подтверждать QUIC candidate.
4. Добавить scripted tests для независимости транспортов.

## Task 5: Effective fingerprint and evidence-driven generator

**Files:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/dsl.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/compiler.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/generator.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`

**Steps:**

1. Получать canonical fingerprint effective transport profile.
2. Исключать baseline, tried и equivalent argv до увеличения attempt.
3. Генерировать одну безопасную мутацию относительно baseline.
4. Выбирать тип мутации по failure stage последней попытки.
5. Сохранить общий лимит 12 реально запущенных уникальных кандидатов.

## Task 6: Status, UI and regression verification

**Files:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/model.rs`
- `ObsessionTauri/src/lib/tauri.ts`
- `ObsessionTauri/src/store/adaptiveStrategyStore.ts`
- `ObsessionTauri/src/design/components/AdaptiveStrategyPanel.tsx`

**Steps:**

1. Передавать stage, transport, round progress, failure stage и terminal result.
2. Не отображать rollback к базе как найденную стратегию.
3. Показать `probe_unreliable`, `base_unhealthy` и
   `no_distinct_strategy_found`.
4. Прогнать `cargo test`, frontend build и live smoke под feature flag.

## Commit sequence

1. `docs: plan reliable adaptive Zapret2 search`
2. `fix: make adaptive probe evaluation evidence based`
3. `fix: calibrate and recheck adaptive Zapret2 baseline`
4. `fix: observe Zapret2 with Eyes`
5. `fix: separate TLS and QUIC adaptive probes`
6. `fix: generate distinct adaptive Zapret2 candidates`
7. `fix: expose adaptive search failure evidence`
