# Traceability модернизации производительности и надёжности Obsession

**Дата закрытия code phase:** 2026-07-18
**Ветка:** `codex/zapret2-gate-d-recovery`
**Статус:** P0-P4 реализованы в локальных коммитах; автоматический gate зелёный;
Windows live acceptance и измерения на реальном WebView2/WinDivert остаются.
`git push` в ходе модернизации не выполнялся.

## 1. Граница отчёта

Отчёт связывает утверждённый дизайн
`2026-07-17-obsession-performance-and-reliability-modernization-design.md`,
два implementation plan и фактические локальные коммиты. Он не объявляет
пройденными сценарии, которые требуют реального Windows-трафика, UAC,
WinDivert, WebView2, монитора заданной частоты или длительного memory profile.

Модернизация не меняла Safe Strategy DSL, набор core/optional targets, quorum,
Eyes veto, проверку сертификатов или зафиксированный security scope.

## 2. Фазы и commit boundaries

| Фаза | Реализация | Основной результат |
| --- | --- | --- |
| P0 | `8cca42b` | Зафиксирован исходный dirty-worktree baseline и attribution. |
| P1 | `86df70b`, `823d665`, `4ca508b`, `baecc48`, `0022b14` | Stale completions отбрасываются до side effects; session tasks имеют владельца; network identity single-flight; candidate/rollback side effects не блокируют coordinator. |
| P2 | `c1eed18`, `5ad96ef`, `79977ae` | Один абсолютный budget на target; математически безопасное раннее завершение серий; bounded DPI supervisor с readiness и PID identity. |
| P3 | `0586c5b`, `29ed8cf`, `fdf73d9`, `12dbc52` | Refresh-aware scheduler, quality tiers, единый Rain pipeline, bounded transition/pointer/log churn. |
| P4 | `bca6c8d`, `1f302bc` | Versioned backend snapshot и listener-first frontend hydration с независимыми revisions. |
| Final lint gate | `6dc21cc` | `cargo clippy --all-targets -- -D warnings` очищен без изменения runtime semantics. |

Все перечисленные коммиты локальные. Изменения `package.json`,
`package-lock.json`, `vite.config.ts`, удаление `SECURITY_SCOPE.md`, удаление
`AdaptiveStrategyPanel.tsx` и `install.ps1` не включались в эти commit
boundaries и остаются отдельным owner-worktree.

## 3. Требования, код, тесты и метрики

| Требование | Коммиты / код | Автоматическое доказательство | Текущий статус метрики |
| --- | --- | --- | --- |
| Не ослаблять качество подбора | `c1eed18`, `5ad96ef`; `adaptive_strategy/probe.rs`, `evidence.rs`, `runtime.rs` | `evaluator_tolerates_one_transient_round_and_optional_failure`, `repeated_eyes_evidence_vetoes_http_success`, `quic_requires_quic_evidence_not_tls_flags`, `series_verdict_is_quorum_and_transport_aware` | Quorum, transport evidence и Eyes veto сохранены. Windows verdict comparison ещё нужен. |
| Ограничить target latency | `c1eed18`; `ProbeBudget`, bounded IPv4/IPv6 attempts | `target_budget_caps_sequential_operations`, `dns_cache_keeps_at_most_one_address_per_family` | Абсолютный deadline доказан paused-time/unit тестом. Реальный wall-clock target timeout + 250 мс ещё не измерен. |
| Завершать серию без потери качества | `5ad96ef`; verdict-state и bounded negative-evidence window | `series_verdict_is_quorum_and_transport_aware`, `evidence_quiet_window_stops_at_hard_deadline` | Решение завершается только когда итог уже неизменяем. Live search duration ещё не снята. |
| Не принимать stale completion | `823d665`, `4ca508b`, `0022b14` | `probe_completion_guard_rejects_stale_session_attempt_candidate_and_phase`, `stale_candidate_events_are_ignored`, `crash_signal_is_generation_scoped` | В unit-модели — 0 stale commits. Live cancel race ещё нужен. |
| Cancel/Shutdown во всех фазах | `4ca508b`, `0022b14`; `SessionTaskSet`, `CandidateRuntime`, `RollbackCoordinator` | `abort_all_drops_preparation_and_probe_tasks`, `interruptible_cancel_keeps_candidate_and_rollback_workers_owned`, `cancel_and_shutdown_during_rollback_do_not_schedule_second_restore`, `shutdown_waits_for_owned_side_effect_completion` | Ownership и single rollback доказаны. Observable status `<= 150 мс` требует Windows live-log measurement. |
| Single-flight network identity | `baecc48`; tracked preparation join | Runtime/model tests и полный Rust suite | Дублирующий nested resolve удалён. Реальная холодная/тёплая DNS/netid latency ещё не профилировалась. |
| Bounded DPI lifecycle | `86df70b`, `79977ae`; `dpi_supervisor.rs` | `startup_marker_finishes_readiness_early`, `missing_marker_uses_bounded_fallback`, `early_exit_fails_readiness`, `owned_processes_are_stopped_in_parallel`, `pid_reuse_is_not_killed`, `process_stop_has_hard_deadline` | Deadlines и PID identity покрыты. 20 respawn и stop/respawn p95 `<= 2 с` требуют живого winws/winws2. |
| Плавность 60-240 Гц | `0586c5b`, `29ed8cf`; `frameScheduler.ts` и theme cores | 9 scheduler tests: refresh classification, compatible cadence, hidden stop, reduced-motion frame, quality hysteresis/cooldown, bounded telemetry | Scheduler contract зелёный. WebView2 p95 `<= 1.15 x interval`, missed frames `< 1%` ещё не измерены. |
| Один Rain frame pipeline | `fdf73d9`; `rainFramePipeline.ts`, renderer и raindrops | `keeps parallax smoothing stable across refresh rates`, `normalizes pointer input against the CSS client rect`, ordered phase test | Порядок и dt semantics доказаны. Screenshot/GPU profile всех тем ещё нужен. |
| Ограничить React/pointer/log churn | `12dbc52`; semantic morph, pointer bus, cached rect, batched logs | pointer normalization/coalescing tests; log burst/limit/clear tests; production build | Pure contracts зелёные. React Profiler и 50 theme/tab memory cycles ещё не выполнены. |
| Исключить bootstrap race | `bca6c8d`, `1f302bc`; `BootstrapSnapshot`, `launcherBootstrap.ts`, version-aware stores | Rust revision/serialization tests; TypeScript contract tests; listener-before-snapshot, stale overlap, snapshot failure, StrictMode remount, unrelated resume event и listener failure tests | В детерминированных race tests — 0 потерянных updates. Реальный tray/sleep/resume smoke ещё нужен. |
| Не дублировать startup IPC | `1f302bc`; shared settings/brain/hosts/DPI/proxy/adaptive stores | `39/39` frontend tests и production TypeScript build | Settings/status startup hydration объединена; hosts startup остаётся local-only. |

## 4. Последний автоматический gate

Проверено 2026-07-18 на текущем worktree:

- `cargo test --manifest-path ObsessionTauri/src-tauri/Cargo.toml --all-targets`:
  `282 passed`, `0 failed`;
- `cargo fmt --manifest-path ObsessionTauri/src-tauri/Cargo.toml --check`:
  успешно;
- `cargo clippy --manifest-path ObsessionTauri/src-tauri/Cargo.toml --all-targets -- -D warnings`:
  успешно;
- `npm test -- --run` в `ObsessionTauri`: `39 passed`, `0 failed`;
- `npm run build` в `ObsessionTauri`: успешно, `515 modules transformed`;
- `git diff --check`: успешно для modernization changes; остаются только
  предупреждения о будущей нормализации CRLF в пользовательском worktree.

Frontend tests запускались в текущем worktree, где Vitest metadata находится в
незакоммиченных `package.json`, `package-lock.json` и `vite.config.ts`. Решение о
самостоятельном test-infrastructure commit остаётся за владельцем; эти файлы не
были молча включены в modernization commits.

## 5. Оставшийся Windows live acceptance

### 5.1 Adaptive search

1. YouTube TLS, YouTube QUIC и Discord: полный search, Cancel во время
   preparation/probe/spawn/rollback и ручное Confirm/Reject.
2. Подтвердить exact rollback после timeout, reject, cancel, candidate crash и
   shutdown; поздние события не меняют UI/cache.
3. Снять время от Cancel до observable status; целевое значение `<= 150 мс`.
4. Снять target и end-to-end search latency без уменьшения targets/quorum.
5. Gaming + GitHub: проверить control endpoints и выбранную игру; builtin data
   plane остаётся помеченным как не проверенный активным probe.

### 5.2 DPI process lifecycle

1. Не менее 20 start/stop/respawn циклов Legacy и Zapret2.
2. Проверить ранний exit, missing readiness marker, reader failure и
   intentional stop без ложного crash fallback.
3. Снять normal stop/respawn p95; цель `<= 2 с`.
4. Проверить отсутствие orphan `winws.exe`/`winws2.exe` после shutdown,
   cancel и закрытия в первые 500 мс запуска.

### 5.3 Launcher и bootstrap

1. Реальные tray hide/show, hotkey, sleep/resume и смена DPI/proxy во время
   скрытого окна.
2. Убедиться, что settings, hosts, DPI, proxy, brain и adaptive догоняются
   независимо и UI не мигает старым snapshot.
3. Проверить StrictMode/dev remount без дублирующихся listeners по живому логу.

### 5.4 Frame, GPU и память

1. 60-секундные profiles на доступных 60/120/144/165/240 Гц.
2. Для активных сцен подтвердить p95 `<= 1.15 x target interval` и missed frames
   `< 1%`.
3. Сравнить screenshots всех тем, reduced motion, resize и theme morph.
4. Выполнить 50 theme/tab cycles и tray cycles без устойчивого роста RAM,
   WebGL contexts или video decoders.

## 6. Статус старых документов

- `2026-07-14-zapret2-gate-d-recovery-implementation-plan.md` помечен
  `superseded`: его историческая реализация и последующая стабилизация теперь
  отслеживаются актуальными adaptive/modernization документами и этим отчётом.
- `2026-07-16-adaptive-zapret2-strategy-confidence-design.md` по-прежнему
  содержит статус «ожидает review владельца». Этот отчёт не трактует
  утверждение modernization design как отдельное подтверждение confidence
  product direction, поэтому статус не изменён.
- Планы Gaming + GitHub сохраняют явный незакрытый Windows live acceptance и
  не помечаются завершёнными автоматически.
- Удаление `SECURITY_SCOPE.md` и `AdaptiveStrategyPanel.tsx`, а также
  `install.ps1` остаются owner decisions вне commit boundaries модернизации.

## 7. Критерий полного закрытия

Code phase завершён. Полное release-closure наступает после выполнения раздела
5, фиксации фактических latency/frame/memory значений и отдельного решения по
frontend test-infrastructure metadata. До этого branch готов к Windows live
acceptance, но не объявляется полностью release-validated.
