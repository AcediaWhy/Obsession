# Obsession (Tauri)

Порт Flutter-лаунчера **Obsession** на **Tauri** (Rust-бэкенд + React-фронтенд)
с флагманской темой **Obsession: The Fixation** на чёрном оптическом стекле и пятью открытыми темами: Obsession, Aurora, Ophanim, Rain и Midnight.

## Стек

- **Backend:** Rust + Tauri v2 (`tokio`, `reqwest`, `windows`)
- **Frontend:** React 18 + TypeScript + Vite + Tailwind + Framer Motion + Zustand

## Возможности (MVP, этап 1)

- **DPI-обход** (Zapret/winws): категории Discord/YouTube-Twitch/Gaming/Universal,
  выбор конфигов, тест/авто-подбор, лог реального времени, отлов orphan-процессов
- **ИИ-разблокировка** (hosts): провайдеры Malw/GeoHide, install/uninstall/check,
  атомарная запись hosts + бэкап + flushdns
- **Telegram-прокси** (TgWsProxy): проверенный локальный старт/стоп, `tg://proxy` ссылка и защищённая LAN-публикация по service-owned firewall lease
- Системный трей, кастомный титлбар, функциональный Onboarding V2 и защищённая
  служба для привилегированных операций без постоянного UAC

Этап 2 (в планах): профили, редактор списков, темы, автозапуск, автообновление,
локализация RU/EN, инсталлятор.

## Разработка

```bash
npm install
npm run tauri dev      # dev-режим без self-elevation, работает hot-reload
```

> UI никогда не повышает себя. Привилегированные DPI/hosts-действия доступны
> только через совместимую службу ObsessionRuntime, установленную setup в
> Program Files.

## Сборка

```bash
npm run tauri build    # release приложения без current-user setup
npm run build:setup    # transactional setup + соседний SHA-256 checksum
```

## Архитектура

```
src-tauri/src/
  paths.rs      appdata-папки + распаковка ресурсов
  dpi.rs        winws: spawn/kill, стрим лога, orphan, тест
  proxy.rs      TgWsProxy + manifest verification + tg:// + LAN lease client
  hosts.rs      atomic hosts write + бэкап + провайдеры
  net.rs        TCP/HTTP тест доступности
  onboarding.rs durable plan/apply/verify/rollback через защищённую службу
  settings.rs   JSON-персист настроек
  commands.rs   поверхность #[tauri::command]
  lib.rs        окно, трей, shutdown-хук
src/
  lib/tauri.ts       типизированный мост invoke + события
  store/             Zustand: dpi, proxy, hosts, log
  design/            Aurora Glass токены + компоненты
  screens/           Dpi, Ai, Telegram, Soon
```

Ресурсы (winws, WinDivert, TgWsProxy, конфиги, списки, иконки) лежат в
`src-tauri/resources/`, хешируются в runtime manifest и устанавливаются в
`%ProgramFiles%\Obsession`.
