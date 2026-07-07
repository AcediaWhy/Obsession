# VlarpSu — Obsession

Десктоп-лаунчер **Obsession** — порт на [Tauri](https://tauri.app/) (React + Rust).

## Стек

- **Frontend:** React 18 + TypeScript + Vite
- **UI:** Tailwind CSS, Framer Motion, Three.js (`@react-three/fiber`, `drei`, `postprocessing`)
- **State:** Zustand
- **Backend:** Rust (Tauri 2)

## Требования

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- Зависимости Tauri для вашей ОС — см. [prerequisites](https://tauri.app/start/prerequisites/)

## Запуск (разработка)

```bash
cd ObsessionTauri
npm install
npm run tauri dev
```

## Сборка

```bash
cd ObsessionTauri
npm run tauri build
```

Готовый установщик появится в `ObsessionTauri/src-tauri/target/release/bundle/`.

## Структура

```
ObsessionTauri/
├── src/           # React-фронтенд (UI, дизайн-компоненты)
└── src-tauri/     # Rust-бэкенд (команды, прокси, hosts, автозапуск)
```

> Примечание: другие лаунчеры на диске (`LarpingLauncher`, `ObessionLauncher`,
> `Goida-AI-Unlocker`, `Smart-Zapret-Launcher`, `tg-ws-proxy`) намеренно
> исключены из репозитория через `.gitignore`.
