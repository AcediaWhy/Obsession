# Adaptive Zapret2 Strategy Confidence Implementation Plan

**Design:** `docs/superpowers/specs/2026-07-16-adaptive-zapret2-strategy-confidence-design.md`

**Goal:** различать технически подготовленные, рекомендованные и реально
подтверждённые Zapret2-профили; перестать запускать бессмысленный Gaming probe
по доступным сайтам; сделать YouTube QUIC recheck устойчивым к одиночному DNS
timeout.

## Task 1: Trust schema and cache migration

**File:** `ObsessionTauri/src-tauri/src/adaptive_strategy/cache.rs`

1. Добавить `StrategyTrust::{Prepared, Recommended, Confirmed}` и увеличить
   версию cache schema.
2. Мигрировать валидные Discord/YouTube записи старой схемы в `Confirmed`;
   legacy Gaming entries удалить как недоказанные.
3. Хранить source candidate, transport, network и scope metadata для
   `Recommended`.
4. Ограничить `confirmed_candidate_*` только trust `Confirmed`.
5. Разделить reset производной рекомендации и подтверждённого источника.
6. Покрыть миграцию, фильтрацию trust, scope invalidation и reset unit tests.

## Task 2: Deterministic recommendation engine

**Files:** `cache.rs`, `generator.rs`, новый `recommendation.rs`, `mod.rs`

1. Найти same-network confirmed evidence того же transport.
2. Для Gaming TLS разрешить источники Discord/YouTube TLS, для Gaming QUIC —
   только YouTube QUIC.
3. Ранжировать по exact network, success count, отсутствию invalidation и
   свежести; candidate id использовать только как стабильный tie-breaker.
4. Клонировать только Safe Strategy steps/transport, заменить category и list
   scope, затем повторно прогнать validator/compiler.
5. Никогда не переносить `Confirmed` на новую category/transport.
6. Добавить unit tests допустимых и запрещённых переносов.

## Task 3: Session DNS reliability

**Files:** `probe.rs`, `runtime.rs`, `model.rs`

1. Поднять bounded DNS cache до search session и передавать его discovery,
   baseline, candidate rounds и rollback recheck.
2. Хранить максимум один IPv4 и один IPv6 адрес на host.
3. После успешного rollback повторить base recheck один раз только при чистом
   DNS failure.
4. Двойной DNS failure завершать как `probe_unreliable`, не `base_unhealthy`.
5. Не ослаблять Balanced quorum `2/3` и существующие rollback invariants.
6. Добавить deterministic tests cache reuse и обеих recheck веток.

## Task 4: Backend descriptors and commands

**Files:** `runtime.rs`, `commands.rs`, `lib.rs`, соответствующие DPI descriptor
types.

1. Добавить в Zapret2 descriptors поля trust, evidence source, source
   candidate/transport и recommendation reason.
2. Возвращать builtin Gaming IPSet как `Prepared` после технической проверки.
3. Добавить локальный Gaming recommendation lookup без сетевого candidate loop.
4. Добавить явное применение Recommended profile без повышения trust.
5. Оставить Confirmed только за существующим recovery + manual confirmation.
6. Не включать adaptive Zapret2 descriptors в Legacy profile list.

## Task 5: Frontend contract and store

**Files:** `ObsessionTauri/src/lib/tauri.ts`,
`ObsessionTauri/src/store/adaptiveStrategyStore.ts`

1. Типизировать trust/evidence/recommendation descriptor fields.
2. Добавить команды `recommend` и `apply recommendation` для Gaming.
3. Разделить reset рекомендации и reset подтверждённой стратегии.
4. Сохранить cancellable live recovery для Discord/YouTube и реального Gaming
   failure evidence.

## Task 6: Zapret2-only UI

**File:** `ObsessionTauri/src/design/components/Zapret2StrategyPanel.tsx`

1. Показывать compact badges `Подготовлен`, `Рекомендован`, `Подтверждён`.
2. Для Gaming заменить обычный долгий search на мгновенный локальный подбор
   рекомендации.
3. Показывать источник рекомендации одной строкой и явное действие применения.
4. Оставить ids, paths, fingerprints и raw probe только в технических деталях.
5. Не менять HeroCore, темы, Legacy UI, Diagnostics, Brain, shared components и
   общую DPI-компоновку.

## Task 7: Regression verification

1. `cargo fmt --check`.
2. Целевые cache/recommendation/model/runtime/probe tests.
3. Полные `cargo test` и `cargo check`.
4. `npx tsc --noEmit` и `npm run build`.
5. Gaming `winws2 --dry-run` для Prepared и Recommended profiles.
6. Запустить `npm run tauri dev` и проверить: Gaming без evidence завершается
   мгновенно как Prepared, same-transport evidence даёт Recommended, YouTube
   QUIC не падает от одиночного DNS recheck timeout.

## Completion boundary

Community/provider telemetry и protocol-specific game oracle не входят в эту
реализацию. Без реальной проверяемой блокировки Gaming остаётся Prepared или
