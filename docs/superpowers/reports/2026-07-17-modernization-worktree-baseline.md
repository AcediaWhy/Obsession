# Modernization Worktree Baseline

**Дата:** 2026-07-17
**Ветка:** `codex/zapret2-gate-d-recovery`
**HEAD:** `1845467`
**Origin baseline:** `4235693`
**Push policy:** локальные commits, без `git push`

## 1. Назначение

Отчет фиксирует исходный незакоммиченный worktree перед реализацией
модернизации. Существующие изменения считаются пользовательской работой и не
могут быть откатаны, перезаписаны или автоматически включены в новый commit.

Локально поверх origin находятся только два documentation commits:

- `7bf7462` — утвержденный design;
- `1845467` — implementation plan.

## 2. Проверенный baseline

На текущем dirty worktree ранее успешно выполнены:

- `cargo test --all-targets`: 256/256;
- `npm test`: 11/11;
- `npm run build`: успешно, 508 modules;
- `cargo fmt --check`: успешно.

`cargo clippy --all-targets -- -D warnings` останавливается на 14 существующих
замечаниях. Clean clippy остается phase gate, но не считается доказательством
функционального отказа текущего worktree.

## 3. Текущие tracked изменения

### 3.1 Backend — уже реализовано или начато

| Файл | Состояние | Содержание и правило продолжения |
| --- | --- | --- |
| `adaptive_strategy/runtime.rs` | partial | Добавлены cache + `netid_gate` single-flight и удалены дублирующие adaptive events. P1 должен сохранить эти изменения, но устранить оставшийся direct nested `netid::resolve`, detached probes и stale mutation. |
| `commands.rs` | partial | Single-flight `current_netid`, backend `started_at` в runtime snapshot, удаление legacy full-replace `save_settings`. Не возвращать удаленную команду без отдельного compatibility evidence. |
| `dpi.rs` | partial | Backend uptime payload, точечный `kill_orphans`, явная ошибка чтения stdout/stderr. P2 накладывает supervisor поверх этого поведения. |
| `lib.rs` | implemented slice | Startup убивает только обнаруженные orphan PID; poisoned settings lock восстанавливается через `lock_recover`; `save_settings` исключен из invoke list. |
| `state.rs` | implemented slice | `started_at_unix`, `netid_gate` и state tests. Сохранить при выделении supervisor/bootstrap revisions. |
| `util.rs` | implemented slice | `unix_secs` и `started_at` в DPI status payload. |

### 3.2 Frontend — уже реализовано или начато

| Файл/группа | Состояние | Содержание и правило продолжения |
| --- | --- | --- |
| `App.tsx` | partial | Resume применяет adaptive snapshot через store logic; bootstrap cleanup и visibility lifecycle доработаны. P3/P4 должны сохранять эти guards. |
| `RainScene3D.tsx`, `rainRenderer.ts`, `raindrops.ts` | partial | Render-loop visibility/cleanup и resize lifecycle улучшены, но три независимых loop остаются. P3 объединяет их, не откатывая cleanup. |
| `Uptime.tsx`, `Dpi.tsx`, `Overview.tsx`, `tauri.ts` | implemented slice | UI использует backend-owned DPI start time. |
| `adaptiveStrategyStore.ts` | partial | Status application и subscription contract улучшены; строгий versioned listener-first bootstrap еще не реализован. |
| `dpiStore.ts` | partial | Listener-first registration, backend uptime, error-safe test loops и serialized writes. P4 не должен возвращать старые parallel writes. |
| `proxyStore.ts` | partial | Listener-first registration и сохранение `transitioning` ownership. |
| `hostsStore.ts`, `profileStore.ts` | partial | Settings store становится единым writer для provider, bootstrap errors перехватываются. |
| `package.json`, `package-lock.json`, `vite.config.ts` | implemented slice | Добавлен Vitest и test scripts/config. |

## 4. Новые tests в worktree

Следующие untracked-файлы принадлежат текущему frontend stabilization slice:

- `ObsessionTauri/src/store/adaptiveStrategyStore.test.ts`;
- `ObsessionTauri/src/store/dpiStore.test.ts`;
- `ObsessionTauri/src/store/proxyStore.test.ts`.

Они проходят в составе 11 Vitest tests. Их нельзя потерять при P4. До отдельного
commit они остаются unstaged вместе с соответствующими store changes.

## 5. Удаления и несвязанные файлы

### `AdaptiveStrategyPanel.tsx`

Tracked-файл удален, при этом проект использует
`Zapret2StrategyPanel.tsx`. Удаление похоже на migration cleanup, но не
считается подтвержденным автоматически. Модернизация не восстанавливает и не
коммитит это удаление до owner decision.

### `SECURITY_SCOPE.md`

Tracked-документ удален в worktree, хотя опубликованный HEAD содержит явно
утвержденный security scope. Это конфликтует с предыдущим решением владельца.
Файл не восстанавливается автоматически, но удаление исключается из любых
modernization commits до явного решения.

### `install.ps1`

Untracked `install.ps1` не относится к модернизации и всегда исключается из
staging.

## 6. Конфликтные зоны

- `runtime.rs` — высокий overlap с P1; перед каждым patch требуется просмотр
  текущего diff и focused tests.
- `dpi.rs`, `lib.rs`, `state.rs` — overlap с P2 supervisor; переносить behavior
  маленькими characterization-backed шагами.
- `App.tsx`, Rain и stores — overlap с P3/P4; не начинать broad mechanical
  rewrite до завершения backend safety phases.
- `probe.rs` и `eyes/capture.rs` не изменены в baseline. `probe.rs` является
  наиболее изолированной зоной для P2 deadline tests.

## 7. Attribution policy

Перед каждым local commit:

1. stage только файлы текущего task;
2. проверить `git diff --cached --name-status`;
3. проверить `git diff --cached --check`;
4. убедиться, что staged diff не захватил baseline hunks без прямой связи;
5. не выполнять push.

Если task должен изменить уже dirty-файл, commit допускается только после
разделения либо явного учета существующих hunks; нельзя приписывать весь файл
modernization commit.
