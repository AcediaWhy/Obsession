# Adaptive QUIC Recovery and Search Latency Implementation Plan

**Design:** `docs/superpowers/specs/2026-07-16-adaptive-quic-recovery-and-search-latency-design.md`

**Goal:** запускать bounded QUIC candidates при неработающем baseline,
проверять только HTTP/3-capable endpoints, ограничить target probe одним
deadline и сразу показывать cancellable preparation progress.

## Task 1: Bounded probe primitives

**File:** `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`

1. Общий target deadline покрывает DNS, address attempts и handshake.
2. Выбирать не более одного IPv4 и одного IPv6.
3. Запускать семейства конкурентно со stagger и отменять после первого успеха.
4. Досрочно завершать QUIC series, когда required successes недостижимы.
5. Добавить deterministic Tokio tests для deadline и address race.

## Task 2: HTTP/3 target discovery

**Files:** `probe.rs`, `runtime.rs`

1. Выполнить параллельный TLS preflight allowlisted category endpoints.
2. Разобрать `Alt-Svc` helper-ом и выбрать endpoints с `h3`.
3. Передавать discovered targets в QUIC baseline/candidate series.
4. Вернуть `quic_targets_unavailable`, если eligible core пуст.
5. Покрыть parser, eligibility и empty-result tests.

## Task 3: Preparation state machine

**Files:** `model.rs`, `runtime.rs`

1. Добавить phases `discovering_quic` и `calibrating`.
2. Добавить `SearchSessionMode::{Comparison, Recovery}` и status fields:
   transport, session mode, current/total round.
3. Заменить inline `prepare_search` событиями Begin/Progress/Finished с session
   id и runtime generation.
4. Emit status до первого network await; preparation выполнять в отдельной
   cancellable task, stale results игнорировать.
5. Повторный Start Search возвращает `search_already_running`.
6. Cancel/Shutdown во время preparation отменяют task без rollback.

## Task 4: Recovery semantics

**Files:** `model.rs`, `runtime.rs`

1. Успешный baseline создает Comparison session.
2. Reliable QUIC failure создает Recovery session вместо `ProbeUnreliable`.
3. DNS/discovery/runtime failures остаются unreliable outcomes.
4. Comparison после rollback выполняет base recheck.
5. Recovery проверяет exact spawn/generation и продолжает без сетевого recheck.
6. Exhausted recovery сообщает, что исходный QUIC мог остаться нерабочим.
7. Добавить tests обеих веток и rollback failure.

## Task 5: Frontend progress

**Files:** `src/lib/tauri.ts`, `src/store/adaptiveStrategyStore.ts`,
`src/design/components/AdaptiveStrategyPanel.tsx`

1. Расширить AdaptivePhase/AdaptiveStatus.
2. Считать discovery/calibration активными backend phases.
3. Показывать HTTP/3 discovery и `Калибровка QUIC N/M` сразу после клика.
4. Явно показывать Recovery mode и не рисовать candidate bar до candidates.
5. Сохранить видимые command errors и рабочий Cancel.

## Task 6: Verification

1. `cargo fmt --check`.
2. Целевые `probe`, `model`, `runtime` tests.
3. Полные `cargo test` и `cargo check`.
4. `npm run build`.
5. Gaming `winws2 --dry-run`.
6. `npm run tauri dev` и live-log acceptance: immediate phase, bounded target,
   failed baseline -> Recovery candidate, cancel без зависания.

## Completion boundary

Если category pool не объявляет HTTP/3 в текущей сессии, поиск завершается
`quic_targets_unavailable`. Слепой перебор в этот план не входит.
