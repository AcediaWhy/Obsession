# Adaptive Zapret2 Gaming + GitHub IP-set Implementation Plan

**Design:**
`docs/superpowers/specs/2026-07-15-adaptive-zapret2-gaming-github-ipset-design.md`

**Goal:** добавить в Zapret2 категорию Gaming + GitHub с нативными ipset-scoped
профилями, сохранив adaptive search только для проверяемого TLS/QUIC control
plane и показывая в UI реальные effective Zapret2 profiles.

## Task 1: Typed profile and manifest schema

**Files:**

- `ObsessionTauri/src-tauri/src/dpi_engine/zapret2.rs`
- `ObsessionTauri/src-tauri/src/dpi_engine/manifest.rs`

**Steps:**

1. Добавить `ipset` в `Zapret2Profile` и сериализацию `--ipset`.
2. Добавить optional `hostlist`, `ipset`, `filter_tcp`, `filter_udp` в
   `StrategyDef`.
3. Валидировать безопасные basename `.txt` и port filters.
4. Запретить `1024-65535` без ipset.
5. Сохранить обратную совместимость старых manifest entries.

## Task 2: Runtime list resolution and effective profiles

**Files:**

- `ObsessionTauri/src-tauri/src/dpi.rs`
- `ObsessionTauri/src-tauri/src/dpi_engine/zapret2.rs`
- `ObsessionTauri/src-tauri/src/lists_validate.rs`

**Steps:**

1. Разрешать explicit hostlist/ipset только внутри `Paths::lists_dir()`.
2. Проверять наличие и тип содержимого списка до spawn.
3. Передавать resolved hostlist/ipset в чистый profile builder.
4. Проверять high-port/ipset invariant повторно перед сборкой argv.
5. Сохранять порядок control profiles перед ipset fallbacks.

## Task 3: Gaming + GitHub resources and builtin pack

**Files:**

- `ObsessionTauri/src-tauri/resources/lists/gaming-github.txt`
- `ObsessionTauri/src-tauri/resources/strategy-packs/builtin/manifest.json`
- `ObsessionTauri/src-tauri/src/dpi_engine/mod.rs`
- `ObsessionTauri/src-tauri/src/paths.rs`

**Steps:**

1. Собрать versioned `gaming-github.txt` из активных Gaming и GitHub domains.
2. Добавить Gaming category и пять profiles в builtin pack.
3. Зафиксировать control-before-ipset ordering тестами.
4. Обновить pack version/resource extraction anchor.
5. Проверить manifest integrity и `winws2 --dry-run`, если бинарник доступен.

## Task 4: Adaptive Gaming control plane

**Files:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/dsl.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/generator.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`
- `ObsessionTauri/src-tauri/src/adaptive_strategy/cache.rs`
- `ObsessionTauri/src-tauri/src/commands.rs`

**Steps:**

1. Добавить `AdaptiveCategory::Gaming` и probe targets GitHub/Gaming.
2. Генерировать только allowlisted TLS/QUIC control candidates.
3. Подменять только `gaming_control_tls` или `gaming_control_quic`.
4. Не менять builtin ipset profiles при candidate/rollback.
5. Добавить cache dimension transport и миграцию schema v1.
6. Связать Gaming cache с fingerprints hostlist/ipset.

## Task 5: Effective Zapret2 profile visibility

**Files:**

- `ObsessionTauri/src-tauri/src/commands.rs`
- `ObsessionTauri/src/lib/tauri.ts`
- `ObsessionTauri/src/store/dpiStore.ts`
- `ObsessionTauri/src/screens/Dpi.tsx`
- `ObsessionTauri/src/design/components/AdaptiveStrategyPanel.tsx`

**Steps:**

1. Вернуть descriptors: profile id, source, transport, ports, list scope и
   candidate id.
2. Не использовать Legacy `.conf` selector как описание Zapret2 runtime.
3. Показать отдельные Gaming control/data rows.
4. Пометить data plane как builtin/not actively verified.
5. Показать предупреждение о широком `ipset-gaming.txt` без утверждения о
   подмене региона/IP.

## Task 6: Verification

1. `cargo fmt --check`.
2. `cargo test` для полного Rust workspace.
3. `npm run build`.
4. `winws2 --dry-run` на effective Gaming invocation, если ресурс запускаем.
5. Windows live acceptance: GitHub, два Gaming control endpoint и выбранная
   пользователем игра.

## Commit sequence

1. `docs: plan gaming github ipset profiles`
2. `feat: add typed zapret2 ipset profiles`
3. `feat: add gaming github builtin profiles`
4. `feat: adapt gaming control strategies`
5. `feat: show effective zapret2 profiles`
6. `test: verify gaming github zapret2 integration`
