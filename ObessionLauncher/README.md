# Obsession

> **Obsession** — продвинутый лаунчер для обхода DPI-блокировок и разблокировки ИИ-сервисов на Windows. Космический неоновый UI с GLSL-шейдерами, 8 визуальных тем и гибкой системой профилей.

![Obsession Screenshot](assets/icons/screenshot.png)

---

## Возможности

### DPI-обход (Zapret / GoodbyeDPI)
- Управление категориями: Discord, YouTube/Twitch, Gaming, Universal
- Множество предустановленных конфигураций для каждой категории
- Автоподбор работающей конфигурации с тестированием
- Визуальный лог процессов winws в реальном времени

### Telegram MTProto-прокси
- Запуск `TgWsProxy` в один клик
- Генерация `tg://proxy` ссылок
- Копирование в буфер / открытие в Telegram
- Настройка порта и Fake TLS домена

### ИИ-разблокировка (hosts-based)
- Два провайдера: Malw (dns.malw.link) и GeoHide (dns.geohide.ru)
- Дополнительные hosts из Goida-AI-Unlocker
- Установка / удаление / проверка обновлений в один клик
- Атомарная запись системного hosts-файла с бэкапом

### Профили
- Сохранение/восстановление полного состояния: DPI-конфиги, hosts-провайдер, прокси, настройки
- Переключение профилей в один клик
- Создание/удаление с подтверждением

### Визуальные темы (8 пресетов)
| Тёмные (Black Hole stack) | Светлые (LightBackground stack) |
|---------------------------|----------------------------------|
| **Obsession** — индиго-фиолет, баланс | **Aurora Mist** — молочный премиум |
| **Obsidian** — матовый, минималистичный | **Candy Terminal** — пастельный грид |
| **Terminal** — зелёный хакер + CRT | **Seraphim** — перламутр + iridescence |
| **Eclipse** — агрессивный оранж-красный | |

Каждая тема: свой акцент, glow, физика частиц, шрифты, border-radius. Анимированные переходы между темами через `ThemeExtension.lerp`.

---

## Скриншоты

*Добавьте скриншоты в `assets/icons/`*

---

## Требования

- **Windows 10/11** (x64)
- **Права администратора** (для работы winws и записи в hosts)
- .NET 6+ Runtime (для некоторых системных операций)

---

## Установка

1. Скачайте последний релиз из [Releases](https://github.com/VlarpSu/Obsession/releases)
ession/releases)
2. Запустите `Obsession-<version>-setup.exe`
3. При первом запуске приложение запросит права администратора через UAC

---

## Сборка из исходников

```bash
# Клонирование
git clone https://github.com/VlarpSu/Obs.git
cd Obs/ObessionLauncher

# Зависимости
flutter pub get

# Генерация локализаций
flutter gen-l10n

# Debug run
flutter run -d windows

# Release build (single exe + installer)
.\scripts\build_single_exe.ps1
# или
flutter build windows --release
.\installer\build_installer.ps1
```

### Зависимости
- Flutter 3.19+ (SDK ^3.12.0)
- `flutter_riverpod` — state management
- `window_manager` + `system_tray` — нативное окно и трей
- `flutter_animate` — анимации уровня Framer Motion
- `google_fonts` — Nunito / Plus Jakarta Sans / JetBrains Mono / Inter
- GLSL шейдеры: `shaders/black_hole.frag`, `shaders/iridescence.frag`

---

## Архитектура

```
lib/
├── main.dart                    # Entry point, UAC, lifecycle
├── app.dart                     # MaterialApp,テーマ, routing
├── core/
│   ├── constants/               # AppConstants
│   ├── errors/                  # Result<T>, Failures
│   ├── lifecycle/               # AppShutdownCoordinator
│   └── usecases/                # UseCase base
├── data/
│   ├── datasources/             # Local/remote sources (winws, proxy, hosts, paths, etc.)
│   ├── models/                  # Data models
│   ├── network/                 # NetworkTester
│   └── repositories/            # Repository implementations
├── domain/
│   ├── entities/                # Pure domain entities
│   ├── repositories/            # Repository interfaces
│   └── usecases/                # Business logic
├── l10n/                        # RU/EN локализация (ARB)
└── presentation/
    ├── providers/               # Riverpod StateNotifiers
    ├── screens/                 # 7 экранов (Home, DPI, AI, Telegram, Lists, Profiles, Settings, Onboarding)
    ├── theme/                   # AppTheme, DesignTokens, 8 пресетов
    └── widgets/                 # GlassPanel, NeonPowerButton, шейдеры, частицы, CRT, etc.
```

### Clean Architecture
- **Domain** не зависит от внешних фреймворков
- **Data** реализует интерфейсы Domain
- **Presentation** использует Riverpod для DI и состояния

---

## Горячие клавиши / UX

- **Свернуть в трей** — кнопка закрытия окна (если включено в настройках)
- **Автозапуск** — создаёт задачу в планировщике с highest privileges
- **Reduce Motion** — поддерживается через `MediaQuery.disableAnimations`

---

## Безопасность

- ✅ **Верификация SHA-256** обновлений перед запуском установщика
- ✅ **Atomic write** системного `hosts` (temp + rename, UTF-8)
- ✅ **TLS validation** не отключена (нет `badCertificateCallback`)
- ✅ **PID-based process management** — убивает только свои процессы
- ✅ **Argument injection protection** — валидация хостов перед ping

---

## Тестирование

```bash
flutter test --no-pub
# 35 unit/widget tests covering:
# - DPI provider state machine
# - List editor validation (by stable entry ID)
# - Profile CRUD + switching
# - Settings persistence
# - Ping monitor parallelism + dispose guard
```

---

## Лицензия

MIT License — см. [LICENSE](LICENSE).

---

## Автор

**VlarpSu** — made with obsession.