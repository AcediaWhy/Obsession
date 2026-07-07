# Obsession (Tauri)

Порт Flutter-лаунчера **Obsession** на **Tauri** (Rust-бэкенд + React-фронтенд)
с премиум-дизайном **Aurora Glass** (тёмный glassmorphism).

## Стек

- **Backend:** Rust + Tauri v2 (`tokio`, `reqwest`, `windows`)
- **Frontend:** React 18 + TypeScript + Vite + Tailwind + Framer Motion + Zustand

## Возможности (MVP, этап 1)

- **DPI-обход** (Zapret/winws): категории Discord/YouTube-Twitch/Gaming/Universal,
  выбор конфигов, тест/авто-подбор, лог реального времени, отлов orphan-процессов
- **ИИ-разблокировка** (hosts): провайдеры Malw/GeoHide, install/uninstall/check,
  атомарная запись hosts + бэкап + flushdns
- **Telegram-прокси** (TgWsProxy): старт/стоп, `tg://proxy` ссылка (copy/open)
- Системный трей, кастомный титлбар, UAC-элевация, персист настроек

Этап 2 (в планах): профили, редактор списков, темы, автозапуск, автообновление,
локализация RU/EN, инсталлятор.

## Разработка

```bash
npm install
npm run tauri dev      # dev-режим (UAC-релонч отключён, работает hot-reload)
```

> ⚠️ В **dev**-сборке приложение НЕ запрашивает права администратора, поэтому
> реальный обход winws и запись в hosts не работают (нужны права). Для проверки
> обхода собери и запусти release-сборку.

## Сборка

```bash
npm run tauri build    # release + NSIS-инсталлятор; при запуске запросит UAC
```

## Архитектура

```
src-tauri/src/
  paths.rs      appdata-папки + распаковка ресурсов
  dpi.rs        winws: spawn/kill, стрим лога, orphan, тест
  proxy.rs      TgWsProxy + парсинг tg://
  hosts.rs      atomic hosts write + бэкап + провайдеры
  net.rs        TCP/HTTP тест доступности
  admin.rs      is_elevated + UAC-релонч
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
`src-tauri/resources/` и при первом запуске распаковываются в
`%APPDATA%\Obsession`.
