<div align="center">

<img src="ObsessionTauri/icon-source.png" width="116" alt="Obsession" />

# Obsession

**DPI-обход, ИИ-разблокировка и Telegram-прокси — в одном окне.**

Десктоп-лаунчер на [Tauri](https://tauri.app/): нативный Rust-бэкенд и React-фронтенд
в дизайне _Aurora Glass_ (тёмный glassmorphism, живые Three.js-фоны).

![Platform](https://img.shields.io/badge/platform-Windows-00b3b3?style=flat-square)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-stable-CE4A2F?style=flat-square&logo=rust&logoColor=white)
![React](https://img.shields.io/badge/React-18-61DAFB?style=flat-square&logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?style=flat-square&logo=typescript&logoColor=white)
![Version](https://img.shields.io/badge/version-1.0-8a63d2?style=flat-square)

</div>

---

## ✨ Что умеет

| | Возможность | Детали |
|:--:|---|---|
| 🛡️ | **DPI-обход** | winws / Zapret. Категории Discord · YouTube/Twitch · Gaming · Universal. Выбор конфигов, тест и авто-подбор, рейтинг надёжности стратегий, лог в реальном времени. |
| 🧠 | **Авто-восстановление обхода** | Контур **Глаза → Менеджер сети → Мозг**: наблюдает за трафиком, распознаёт вмешательство ТСПУ и сам переподбирает рабочую стратегию под текущую сеть. |
| 🤖 | **ИИ-разблокировка** | Доступ к ChatGPT, Claude, Gemini, Perplexity, Poe, HuggingFace, Midjourney через системный `hosts`. Провайдеры Malw / GeoHide, атомарная запись + бэкап + flushdns, авто-проверка обновлений. |
| ✈️ | **Telegram-прокси** | MTProto-через-WebSocket в один клик (headless TgWsProxy). Ссылка `tg://proxy`, **QR для телефона** по LAN, пресеты Fake-TLS, диск-кэш CF-доменов. |
| 📊 | **Обзор** | Сетевая идентичность и сводный статус обхода / прокси / ИИ на одном экране. |
| 🗂️ | **Профили и списки** | Наборы настроек и встроенный редактор доменных списков. |
| 🎨 | **Темы и атмосфера** | Aurora Glass, дождь (Three.js), «Russia» (фото-глубина), скрытые темы, оверлеи (снег / сакура), режим «меньше анимаций». |
| ⚙️ | **Система** | Динамическая иконка трея, нативные уведомления, тосты, кастомный титлбар, UAC-элевация, автозапуск, онбординг, персист настроек. |

---

## 🧰 Стек

| Слой | Технологии |
|---|---|
| **Backend** | Rust · Tauri 2 · `tokio` · `reqwest` · `windows` · WinDivert |
| **Frontend** | React 18 · TypeScript · Vite · Tailwind CSS · Framer Motion · Zustand |
| **Графика** | Three.js (`@react-three/fiber`, `drei`, `postprocessing`) |

---

## 🚀 Быстрый старт

**Требования:** [Node.js](https://nodejs.org/) 18+ · [Rust](https://www.rust-lang.org/tools/install) (stable) · [зависимости Tauri](https://tauri.app/start/prerequisites/) для вашей ОС.

```bash
cd ObsessionTauri
npm install
npm run tauri dev      # dev-режим с hot-reload
```

> [!WARNING]
> В **dev**-сборке приложение не запрашивает права администратора, поэтому реальный
> обход winws и запись в `hosts` не работают. Для полной проверки собери release.

### Сборка

```bash
cd ObsessionTauri
npm run tauri build    # release + NSIS-инсталлятор (при запуске запросит UAC)
```

Готовый установщик появится в `ObsessionTauri/src-tauri/target/release/bundle/`.

---

## 🗺️ Архитектура

```
ObsessionTauri/
├── src/                     React-фронтенд (Aurora Glass)
│   ├── screens/             Overview · Dpi · Ai · Telegram · Lists · Profiles · Settings
│   ├── store/               Zustand: dpi · hosts · proxy · lists · profile · log · theme · …
│   ├── design/              дизайн-токены, компоненты, Three.js-сцены
│   └── lib/tauri.ts         типизированный мост invoke + события
│
└── src-tauri/src/           Rust-бэкенд
    ├── dpi.rs               winws: spawn/kill, стрим лога, orphan, тест
    ├── eyes/                «Глаза» — наблюдатель трафика (WinDivert)
    ├── brain/               «Мозг» — авто-восстановление стратегии обхода
    ├── netcache.rs          рейтинг надёжности конфигов по сети
    ├── netid.rs             идентификация сети (MAC шлюза → ASN/регион)
    ├── hosts.rs             атомарная запись hosts + бэкап + провайдеры
    ├── proxy.rs             TgWsProxy + tg://proxy + LAN-форвардер + кэш доменов
    ├── profiles.rs          профили настроек
    ├── lists.rs             доменные списки
    ├── admin.rs             is_elevated + UAC-релонч
    ├── settings.rs          JSON-персист настроек
    ├── commands.rs          поверхность #[tauri::command]
    └── lib.rs               окно, трей, уведомления, shutdown-хук
```

Ресурсы (winws, WinDivert, TgWsProxy, конфиги, списки, иконки) лежат в
`ObsessionTauri/src-tauri/resources/` и при первом запуске распаковываются в
`%APPDATA%\Obsession`.

---

## 📦 Репозиторий

Под контролем версий только `ObsessionTauri/`. Другие лаунчеры на диске
(`LarpingLauncher`, `ObessionLauncher`, `Goida-AI-Unlocker`, `Smart-Zapret-Launcher`,
`tg-ws-proxy`) намеренно исключены через `.gitignore`.

<div align="center">
<sub>Сделано с одержимостью 👁️</sub>
</div>
