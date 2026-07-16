# Obsession Modernization Implementation Plan — P2–P4

**Дата:** 2026-07-17
**Статус:** продолжение основного implementation plan
**Основной план:**
`docs/superpowers/plans/2026-07-17-obsession-performance-and-reliability-modernization-implementation-plan.md`

Правила исполнения, запрет `git push` и safety constraints наследуются из
основного плана.

## P2. Probe deadlines и DPI lifecycle

### Task 2.1. Абсолютный ProbeBudget

**Files:**

- Modify/Test: `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`

**Steps:**

1. Добавить paused-time tests: DNS retry и sleep входят в общий target timeout.
2. Добавить test исчерпанного budget до transport connect.
3. Реализовать `ProbeBudget { deadline }` и `remaining()`.
4. Передавать remaining budget в DNS, connect и request.
5. Сохранить `TargetProbeResult` contract.
6. Local commit: `perf: bound adaptive target probe deadline`.

### Task 2.2. TLS Happy Eyeballs без двойного connect

**Files:**

- Modify/Test: `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`

**Steps:**

1. Покрыть slow IPv6/fast IPv4, fast IPv6, оба failure и expired budget.
2. Удалить последовательный preliminary `TcpStream::connect` loop.
3. Ограничить набор одним IPv6 и одним IPv4.
4. Сохранить stagger и отменять проигравшую попытку после успеха.
5. На HTTPS success выставлять DNS/TCP/TLS/HTTPS evidence; failure оставлять
   типизированным.
6. Проверить certificate validation и redirect policy.
7. Local commit: `perf: race adaptive TLS address families`.

### Task 2.3. Quorum-aware early termination

**Files:**

- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`
- Modify: `ObsessionTauri/src-tauri/src/adaptive_strategy/evidence.rs`

**Steps:**

1. Добавить table tests для final success, impossible success и undecided в
   TLS/QUIC.
2. Выделить transport-neutral series verdict state.
3. Завершать только когда verdict и confidence fields неизменяемы.
4. Добавить bounded Eyes quiet window на coordinator level.
5. Проверить Fast/Balanced/Deep fixtures.
6. Local commit: `perf: finish probe series on final quorum`.

### Task 2.4. DpiProcessSupervisor

**Files:**

- Create: `ObsessionTauri/src-tauri/src/dpi_supervisor.rs`
- Modify: `ObsessionTauri/src-tauri/src/lib.rs`
- Modify: `ObsessionTauri/src-tauri/src/dpi.rs`
- Modify: `ObsessionTauri/src-tauri/src/eyes/capture.rs`

**Steps:**

1. Fake-process tests: early exit, readiness, fallback, hard timeout и kill
   failure.
2. Перенести wait/kill/readiness orchestration без изменения launch arguments.
3. Сохранить раннюю PID registration.
4. Добавить bounded Eyes stop и typed teardown outcome.
5. Завершать owned PIDs параллельно с exit verification.
6. Оставить 500 мс WinDivert sleep только fallback path.
7. Выполнять DNS flush условно.
8. Local commit: `refactor: supervise DPI process lifecycle`.

### P2 gate

- Полный Rust/frontend gate из основного плана.
- Deterministic target wall time `<= timeout + 250ms`.
- Windows TLS/QUIC search в Fast/Balanced/Deep.
- Cancel во время probe/rollback и 20 respawn без orphan PID.

## P3. Frame scheduling и UI performance

### Task 3.1. Refresh-aware FrameScheduler

**Files:**

- Create: `ObsessionTauri/src/design/frameScheduler.ts`
- Create: `ObsessionTauri/src/design/frameScheduler.test.ts`
- Modify: `ObsessionTauri/src/design/render.ts`

**Steps:**

1. Tests: refresh classification, compatible cadence, hysteresis, hidden state
   и reduced motion.
2. Реализовать scheduler как external store/service.
3. Сохранить `createRenderLoop` compatibility adapter.
4. Подключить telemetry и quality tiers с cooldown.
5. Local commit: `perf: add refresh-aware frame scheduler`.

### Task 3.2. Перевести fields и cores на scheduler

**Files:**

- Modify full-screen field components and all `*Core.tsx` theme engines.

**Steps:**

1. Заменить статические FPS constants на scheduler subscriptions.
2. Передавать quality tier в backing resolution без React render каждый frame.
3. Сохранить stop frame и hidden teardown.
4. Сравнить screenshots каждой темы.
5. Local commit: `perf: schedule theme canvas workloads`.

### Task 3.3. Один Rain frame pipeline

**Files:**

- Modify: `ObsessionTauri/src/design/components/RainScene3D.tsx`
- Modify: `ObsessionTauri/src/design/components/rain/rainRenderer.ts`
- Modify: `ObsessionTauri/src/design/components/rain/raindrops.ts`

**Steps:**

1. Unit tests для dt smoothing и ordered phases.
2. Убрать внутренние loops; предоставить `step/render` methods.
3. Один callback: input, smoothing, simulation, texture, draw.
4. Нормализовать pointer по client rect/CSS size.
5. Проверить resize, cleanup, tray resume и reduced motion.
6. Local commit: `perf: unify Rain simulation and render frame`.

### Task 3.4. Theme morph, pointer и React churn

**Files:**

- Modify: `ObsessionTauri/src/App.tsx`
- Modify: `ObsessionTauri/src/styles/globals.css`
- Modify: `ObsessionTauri/src/design/components/HeroField.tsx`
- Modify: `ObsessionTauri/src/design/parallax.tsx`
- Modify: `ObsessionTauri/src/design/components/GlassPanel.tsx`
- Modify: `ObsessionTauri/src/store/logStore.ts`
- Modify: `ObsessionTauri/src/design/components/LogStream.tsx`
- Modify relevant screen selectors.

**Steps:**

1. Заменить universal morph semantic layers.
2. Замораживать outgoing scene, сохраняя crossfade.
3. Расширить passive pointer bus и кешировать GlassPanel rect.
4. Batch logs один раз за frame; stable IDs и viewport window.
5. Перевести screens на narrow selectors и отделить frequent fragments.
6. Profile React commits и проверить screenshots/backdrop.
7. Local commit: `perf: bound launcher transition and update work`.

### P3 gate

- `npm test` и `npm run build`.
- Screenshots всех тем и основных экранов.
- 60 секунд profiles на 60/120/144/165/240 Гц.
- p95 `<= 1.15 x target interval`, missed frames `< 1%`.
- 50 theme/tab cycles без устойчивого memory growth.

## P4. Versioned bootstrap и завершение архитектуры

### Task 4.1. Versioned BootstrapSnapshot backend contract

**Files:**

- Modify: `ObsessionTauri/src-tauri/src/commands.rs`
- Modify: `ObsessionTauri/src-tauri/src/state.rs`
- Modify: `ObsessionTauri/src-tauri/src/lib.rs`
- Modify: `ObsessionTauri/src/lib/tauri.ts`

**Steps:**

1. Rust serialization tests для snapshot sections/revisions.
2. Monotonic revision отдельно для DPI, proxy, brain и adaptive.
3. Вернуть settings/hosts/runtime sections одним command.
4. Каждая section сравнивается отдельно, без общего lock/epoch.
5. Обновить TypeScript contract fixtures.
6. Local commit: `feat: add versioned launcher bootstrap snapshot`.

### Task 4.2. Listener-first hydration

**Files:**

- Modify: `ObsessionTauri/src/App.tsx`
- Modify settings, DPI, proxy, hosts и adaptive stores/tests.

**Steps:**

1. Deferred-promise tests: event between listen/snapshot, snapshot error,
   StrictMode remount и unrelated event during resume.
2. Регистрировать listeners до snapshot invoke.
3. Применять sections независимо по revision.
4. Удалить общий cross-subsystem epoch и повторные settings/status IPC.
5. Сохранить write queues и operation IDs.
6. Local commit: `fix: hydrate launcher stores by revision`.

### Task 4.3. Traceability и stale docs

**Files:**

- Create/Modify:
  `docs/superpowers/reports/2026-07-17-modernization-traceability.md`
- Modify только подтвержденные stale specs/plans.

**Steps:**

1. Связать requirement, implementation commit, test и metric.
2. Пометить замененные планы `superseded`, не удаляя историю.
3. Исправлять owner-review status только после фактического подтверждения.
4. Записать оставшиеся Windows live acceptance items.
5. Local commit: `docs: close modernization traceability`.

## Финальный gate

```powershell
git diff --check
cargo test --manifest-path ObsessionTauri/src-tauri/Cargo.toml --all-targets
cargo fmt --manifest-path ObsessionTauri/src-tauri/Cargo.toml --check
cargo clippy --manifest-path ObsessionTauri/src-tauri/Cargo.toml --all-targets -- -D warnings
npm --prefix ObsessionTauri test
npm --prefix ObsessionTauri run build
```

Дополнительно обязательны Windows live search/cancel/rollback, 20 respawn,
tray/sleep/resume, frame/memory profiles и проверка, что ни один commit не был
отправлен через `git push`.
