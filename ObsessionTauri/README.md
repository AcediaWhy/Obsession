# Obsession — руководство разработчика

Этот каталог содержит desktop-приложение Obsession, защищённую Windows-службу, общий IPC-протокол и фирменный transactional setup.

Пользовательское описание и ссылка на опубликованный установщик находятся в [корневом README](../README.md).

## Workspace

| Каталог | Назначение |
|---|---|
| `src/` | React-интерфейс, Zustand stores, onboarding и визуальные сцены тем. |
| `src-tauri/` | Tauri host: окно, трей, команды frontend-моста и непривилегированная orchestration-логика. |
| `runtime-protocol/` | Версионированные request/response/event типы для IPC. |
| `runtime-client/` | Клиент named pipe с проверкой совместимости и ожиданием занятой службы. |
| `runtime-service/` | Machine-wide Windows service, выполняющая allowlisted привилегированные операции. |
| `runtime-reliability/` | Общая логика проверки и восстановления runtime-состояния. |
| `installer/` | UI и Rust backend фирменного setup размером `720×500`. |
| `scripts/` | Подготовка manifest/resources и сборка setup. |
| `docs/` | Спецификации runtime, onboarding и темы Obsession. |

## Стек

- **Frontend:** React 18, TypeScript, Vite 6, Tailwind CSS, Framer Motion, Zustand.
- **Desktop host:** Tauri 2 и Rust stable.
- **Runtime:** Tokio, Windows API, named pipe IPC, WinDivert/winws и Obsession Telegram Proxy.
- **Графика:** собственные WebGL2 и Canvas 2D pipelines с SVG/CSS fallback.
- **Тесты:** Vitest и Rust unit/integration tests для каждого crate.

## Требования

- Windows 10/11 x64.
- Node.js 18+ и pnpm 11+.
- Rust stable с MSVC toolchain.
- Системные зависимости из [Tauri prerequisites](https://tauri.app/start/prerequisites/).

Установите workspace-зависимости из корня репозитория:

```powershell
pnpm install --frozen-lockfile
```

## Запуск

Полный Tauri dev build:

```powershell
pnpm --dir ObsessionTauri tauri dev
```

Только Vite поднимает frontend, но основной `App` ожидает Tauri API. Для изолированной работы над сценами используйте специальные harness-страницы, например:

```text
http://127.0.0.1:1420/obsession-choir-dev.html
```

> [!IMPORTANT]
> Dev-приложение не повышает себя до администратора. DPI, `hosts` и firewall-команды доступны только через совместимую установленную службу ObsessionRuntime. Отсутствующая capability должна оставаться fail-closed.

## Проверки

Frontend:

```powershell
pnpm --dir ObsessionTauri test
pnpm --dir ObsessionTauri build
```

Rust formatting и основные crates:

```powershell
cargo fmt --manifest-path ObsessionTauri/src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path ObsessionTauri/src-tauri/Cargo.toml
cargo test --manifest-path ObsessionTauri/runtime-protocol/Cargo.toml
cargo test --manifest-path ObsessionTauri/runtime-client/Cargo.toml
cargo test --manifest-path ObsessionTauri/runtime-service/Cargo.toml
cargo test --manifest-path ObsessionTauri/runtime-reliability/Cargo.toml
cargo test --manifest-path ObsessionTauri/installer/src-tauri/Cargo.toml
```

## Сборка

Production frontend и Tauri binary:

```powershell
pnpm --dir ObsessionTauri build
pnpm --dir ObsessionTauri tauri build
```

Фирменный setup:

```powershell
pnpm --dir ObsessionTauri build:setup
```

Результат появляется в `ObsessionTauri/dist-release/`:

```text
Obsession-Setup_<version>_x64.exe
Obsession-Setup_<version>_x64.exe.sha256
```

Setup собирает приложение, службу и проверенные runtime-ресурсы в единый payload. Большая часть размера установщика приходится именно на этот payload, а не на React-интерфейс.

## Архитектурные границы

```mermaid
flowchart TB
    FE["React frontend"] -->|"Tauri commands"| HOST["Tauri host"]
    HOST -->|"runtime-client"| PIPE["versioned named pipe"]
    PIPE --> SERVICE["ObsessionRuntime service"]
    SERVICE --> DPI["DPI supervisor"]
    SERVICE --> HOSTS["hosts transaction"]
    SERVICE --> PROXY["Telegram / firewall lease"]
    INSTALLER["Transactional setup"] --> SERVICE
```

Основные правила:

- frontend не передаёт службе произвольные executable paths, URL, домены или команды оболочки;
- protocol version и capabilities проверяются до privileged mutation;
- длительные проверки могут занимать единственный pipe, поэтому runtime-client отличает занятую службу от недоступной;
- progress events ускоряют UI, но snapshot остаётся источником истины;
- фоновые health checks являются read-only;
- установка `hosts` использует pre-operation snapshot, post-write verification и полный rollback при неожиданном отказе;
- setup выполняет install/update/repair транзакционно и не продолжает обычный retry после неполного rollback.

Подробнее см. [SECURE_RUNTIME_ARCHITECTURE.md](docs/SECURE_RUNTIME_ARCHITECTURE.md) и [ONBOARDING_OVERHAUL_SPEC.md](docs/ONBOARDING_OVERHAUL_SPEC.md).

## Функциональные области

- `src-tauri/src/dpi.rs` — управление Legacy/Zapret2, конфигурациями и логами.
- `src-tauri/src/legacy_reliability/` — наблюдение, environment gate и подтверждение Legacy-стратегий.
- `src-tauri/src/adaptive_strategy/` — генерация, проверка и кэш Zapret2-кандидатов.
- `src-tauri/src/hosts.rs` — frontend-facing façade для protected hosts runtime.
- `src-tauri/src/onboarding.rs` — durable plan/apply/verify/rollback flow.
- `src-tauri/src/proxy.rs` — Obsession Telegram Proxy и LAN lease client.
- `src/design/components/obsessionChoir/` — Black Choir geometry, motion и WebGL pipeline.
- `src/store/` — состояние экранов и синхронизация snapshot с UI.

## Ресурсы и безопасность сборки

winws, WinDivert, Obsession Telegram Proxy, конфигурации и списки находятся в `src-tauri/resources/`. Скрипт подготовки создаёт manifest с хешами, а setup устанавливает payload в `%ProgramFiles%\Obsession`.

Не добавляйте в frontend обходные пути для прямой записи системных файлов или запуска произвольных процессов. Если требуется новая привилегированная возможность, она должна получить отдельный тип протокола, backend-валидацию, capability и тесты отказа.

## Документация

- [Protected runtime architecture](docs/SECURE_RUNTIME_ARCHITECTURE.md)
- [Onboarding V2 specification](docs/ONBOARDING_OVERHAUL_SPEC.md)
- [Obsession theme specification](docs/OBSESSION_THEME_SPEC.md)

Автор интерфейса и проекта: **AcediaWhy**.
