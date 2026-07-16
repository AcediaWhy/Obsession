# Runtime Phase 0 traceability

**Дата проверки:** 14 июля 2026 года

**Спецификация:** `docs/superpowers/specs/2026-07-13-tray-memory-and-runtime-reliability-design.md`

## Автоматический baseline

- `cargo fmt -- --check` — проходит после форматирования.
- `cargo test` — baseline 73 passed; после добавления generation regression-тестов 75 passed, 0 failed.
- `npm run build` — проходит.
- `npm run tauri build` — проходит; собраны release `obsession.exe` и NSIS installer 1.1.0.
- `git diff --check` — проходит до форматирования; требуется повторить перед коммитом.
- `cargo clippy --all-targets -- -D warnings` — strict gate пока не проходит из-за пяти существовавших до Phase 0 замечаний: два doc formatting и один collapsible-if в `brain/model.rs`, collapsible-if в `eyes/fake_filter.rs`, too-many-arguments в тестовом helper `eyes/parse.rs`. Новое замечание в `settings.rs` исправлено.

## Матрица требований

| Требование | Реализация | Проверка | Статус |
|---|---|---|---|
| Half-close завершает обе стороны proxy-forwarding | `src-tauri/src/proxy.rs`: `forward_connection` | `proxy::tests::half_close_finishes_both_forwarding_directions` | Автоматически покрыто |
| Abort listener завершает дочерние connection tasks | `src-tauri/src/proxy.rs`: `run_forwarder` + `JoinSet` | `proxy::tests::aborting_listener_aborts_active_connections` | Автоматически покрыто |
| Старый proxy generation не очищает новый runtime | `src-tauri/src/proxy.rs`: `begin_generation_state`, `is_current_state`, `detach_if_current_state` | `proxy::tests::stale_generation_cannot_detach_new_runtime`, `proxy_generation_never_uses_zero_after_wrap` | Автоматически покрыто |
| Параллельные proxy start/stop сериализованы | `AppState::proxy_gate`, `proxy::start/toggle/stop` | Отдельного integration-теста нет | Проверить тестом состояния или release stress |
| Eyes packet queue bounded | `eyes/capture.rs`: `sync_channel`, `try_send`, drop counter | Косвенно кодом; отдельного saturation-теста capture нет | Частично покрыто |
| Brain observations bounded, control events доступны | `brain/runtime.rs`: отдельные observation/control каналы | `brain::runtime::tests::observation_channel_is_bounded_but_control_remains_available` | Автоматически покрыто |
| Tracker ticks зависят от времени, не числа пакетов | `eyes/capture.rs`: deadline-driven tick | `eyes::capture::tests::tick_deadline_is_time_driven_and_skips_missed_intervals` | Автоматически покрыто |
| Shutdown во время первых 500 мс старта не оставляет winws | `commands.rs`, `dpi.rs`: `shutting_down`, gate и post-spawn guard | Unit/integration-теста нет | Нужен release smoke |
| DPI UI/tray/hotkey/shutdown используют единый coordinator | `AppState::dpi_gate`, locked helpers в `commands.rs`/`lib.rs` | Проверяется инспекцией; runtime integration-теста нет | Нужен release stress |
| Настройки patch не теряют соседние поля | `settings.rs`: `SettingsPatch`, atomic save | `settings::tests::patch_changes_only_present_fields`, `atomic_save_never_leaves_partial_json_under_concurrency` | Автоматически покрыто |
| HTTPS probe не принимает plain TCP как успех | `net.rs`: HTTPS-запрос через reqwest | `net::tests::plain_tcp_listener_is_not_a_successful_https_probe` | Автоматически покрыто |
| Runtime snapshot возвращает DPI/proxy/brain | `commands.rs`: `runtime_get_snapshot`, `src/lib/tauri.ts` | Frontend test framework отсутствует | Нужен manual smoke |
| Stale snapshot не перезаписывает новое frontend-state | `App.tsx`: resume epoch | Frontend test framework отсутствует | Нужен manual stress |
| Скрытие окна не уничтожает WebView | `lib.rs`, `App.tsx`, `design/render.ts` | Только manual | Нужен hide/show stress |
| В suspended нет активных тяжёлых сцен/video/rAF | `design/render.ts` и theme components | Только manual | Нужен resource smoke |
| WebView2 получает LOW/NORMAL memory target | `webmem.rs`, вызовы из `lib.rs` | Только Windows release | Нужен A/B measurement |
| Нет persistent CLOSE_WAIT после proxy-клиентов | Новый half-close/JoinSet lifecycle | Unit-тест проверяет закрытие socket, но не OS-state | Нужен release stress |

## Ближайшие пробелы

1. Решить, нужен ли отдельный saturation-тест capture queue, или достаточно brain queue + явного drop counter.
2. Выполнить короткий Windows release smoke для shutdown во время старта, proxy start/stop, DPI start/stop и hide/show.
3. После smoke выполнить длинные resource tests из спецификации.

## Выполненный release smoke

- release-приложение запускается и отвечает;
- при штатном закрытии основного окна процесс остаётся жить в трее;
- single-instance повторный запуск показывает существующее окно с тем же PID;
- один автоматизированный цикл hide/show прошёл без создания второго процесса;
- во время этого цикла DPI и TgWsProxy не запускались;
- после скрытия working set Rust host уменьшился примерно с 32 MiB до 1.5 MiB, после возврата восстановился примерно до 8 MiB; это только smoke-наблюдение, не итоговый A/B-замер.
