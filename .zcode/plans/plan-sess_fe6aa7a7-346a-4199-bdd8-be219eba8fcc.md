# Диагноз: утечка памяти при переключении тем

## Вердикт (TL;DR)

**Rust-сторона чистая**: переключение темы — чисто фронтовая операция (`themeStore.ts:61-72` — только localStorage, ни одной Rust-команды; `grep -ri theme src-tauri/src` — ноль попаданий). Но измеряемое число (диспетчер задач / `perf-sample.ps1` / `memdiag.rs`) — это **сумма всего дерева процессов** (obsession.exe + все msedgewebview2.exe: renderer + GPU), поэтому рост сидит в WebView2-рендерере и GPU-процессе.

Рост 50 → 150-200 МБ за ~18 переключений = три слагаемых:

---

## 1. Постоянная прибавка после первых посещений тем (ограниченная, ~40-60 МБ, разово)

- **videoPool — главный документированный виновник**: `src/design/videoPool.ts:29` — модульный Map, один медиаконвейер на видео-тему, никогда не высвобождается. В комментарии самого файла (строки 4-17) ваш замер: «каждый заход на Catnap/Midnight добавлял ~0.03 ядра и ~15 МБ, и они не отдавались даже в трее». Пул ограничивает рост: 2 темы → **~30 МБ навсегда** после первых посещений (декодер GPU-процесса + буферизованный MP4, `preload="auto"`).
- Ленивые чанки при первом заходе: three.js (`HeroField.tsx:18-20`) и RainHybridScene (`HeroField.tsx:17`).
- `THREE.Cache.enabled = true` (`YaniCharacterScene.tsx:107`) — байты GLB, 2 МБ в куче навсегда.
- Модульный кэш картинок Rain (`RainHybridScene.tsx:24-48`) — 5 изображений, ~2-3 МБ.

## 2. Мусор на КАЖДОЕ переключение, который GC отдаёт не сразу (главный множитель)

Каждое переключение = полный ремоант сцены: `App.tsx:251-260` — `<ThemeScene key={theme}>` внутри `AnimatePresence mode="sync"` + кроссфейд (dur.slow); то же для ядер — `HeroCore.tsx:62-77`. При разборке старой сцены:

- **2D-поля НЕ обнуляют canvas при unmount**: `AuroraField.tsx:288-292`, `OphanimField.tsx:439-443`, `FallenField.tsx:141-145` + ядра `FallenCore.tsx:223-226`, `CatnapCore.tsx:264-267`, `MidnightCore.tsx:260-263`, `AuroraCore.tsx:318-320`, `OphanimCore.tsx:332-335`, `RainLanternCore.tsx:244-246`, `RainFallback.tsx:176-180`. Fullscreen backing store при backingScale 0.75 (`AuroraField.tsx:61`) — **~5-8 МБ на каждое переключение**, висят до сборки мусора. Сравните: WebGL-компоненты обнуляют canvas сами и документируют зачем (`YaniCharacterScene.tsx:125-128`: «обнуление размера канваса освобождает его сразу», `ObsessionChoirField.tsx:171-172`).
- **WebGL-контексты не убиваются явно**: `ObsessionChoirPipeline.destroy()` (`obsessionChoir/pipeline.ts:232-245`), `YaniNekoPipeline.destroy()` (`yanineko/pipeline.ts:250-262`), three.js `releaseRenderer` (`YaniCharacterScene.tsx:140-144`) — без `loseContext()`/`forceContextLoss()`. Единственный, кто делает — `rain/pipeline.ts:316`. Осиротевший контекст держит командные буферы и программы в GPU-процессе до GC (и занимает слоты лимита ~16 контекстов Chromium). Замечу: в комментарии `YaniCharacterScene.tsx:130-139` ваш замер «6 циклов сворачивания» показал ровный commit — но сценарий «смена темы 18 раз подряд» — другой, стоит перепроверить.
- **Кроссфейд стекует сцены**: `mode="sync"` держит уходящую сцену dur.slow; при быстром переборе тем несколько уходящих копий живы одновременно (каждая с полным канвасом/контекстом).
- **GLB парсится заново на каждый монтаж** (`YaniCharacterScene.tsx:101-108`) — всплеск GC-мусора на каждое переключение на yanineko.

## 3. Настоящие (классические) утечки — точечные

- **PMREM-цель утекает при уходе во время загрузки GLB**: `environmentTarget` создаётся на `YaniCharacterScene.tsx:242-244` **до** `await sharedLoader.loadAsync` (строка 250). Ветка отмены (251-254) диспозит только `gltf.scene`, cleanup-фолбэк (428-436) — только renderer. Если сменить тему, пока GLB ещё грузится — cubemap render target (несколько МБ GPU) утекает до смерти контекста. Реальная утечка на каждый «рассинхронный» уход из yanineko.
- **EyeLogo** (`EyeLogo.tsx:47-77`): на каждый цикл «спрятать/показать окно» создаётся НОВЫЙ `<video>` — ровно тот паттерн, который до videoPool замеряли как +15 МБ/заход («конвейер переживает откреплённый элемент», `videoPool.ts:8-12`). С темами не связан, но в живой сессии копит декодеры. **Вероятная причина того, что синтетический прогон «N переключений темы» у вас не воспроизводит рост** (`memoryDiagnostics.ts:8-14`), а живая сессия — да.
- `YaniEarScene.tsx:86-94, 392-405` (только дев-лабы): retry может создать второй renderer, не disposив первый.

## Что чистое (проверено агентами построчно)

frameScheduler (tasks Map, dispose удаляет, телеметрия со splice), pointerBus, все rAF-циклы диспозятся, все addEventListener/Tauri-listen парные, logStore ограничен 200 записями, Rust: memdiag — один слот 4 КБ, логи на диск с ротацией, каналы bounded, пайпы читают в transient Vec.

## Почему число не падает обратно

В трее `webmem.rs` делает EmptyWorkingSet (working set падает), но commit и куча рендерера не возвращаются; таймеры в трее Chromium душит → GC откладывается → мусор от 18 переключений копится, пока окно снова активно не «продавит» GC.

## Как проверить руками (10 минут)

1. `%APPDATA%\Obsession\logs\memory.jsonl` — per-process разбивка: рост в `--type=gpu-process` → декодеры/контексты/канвасы; в renderer → heap/DOM.
2. DevTools (F12 в окне): после 18 переключений `document.querySelectorAll('canvas').length` — если >2, стекуются уходящие сцены; Heap snapshot → фильтр Detached — покажет удержания detached canvas/video.
3. Контрольный прогон: 18 переключений при открытом окне → свернуть → развернуть → если память просела, это GC-ленивость, а не удержания.

---

# План правок (по приоритету)

1. **Обнулить canvas во всех 2D-полях/ядрах при unmount** (как уже сделано в WebGL-компонентах): добавить `canvas.width = 0; canvas.height = 0;` в cleanup `AuroraField`, `OphanimField`, `FallenField`, `FallenCore`, `CatnapCore`, `MidnightCore`, `AuroraCore`, `OphanimCore`, `RainLanternCore`, `RainFallback`. Убирает главный per-switch кусок (~5-8 МБ/переключение) детерминированно.
2. **Добавить `loseContext()` в destroy() всех WebGL-пайплайнов** (choir, yanineko) по образцу `rain/pipeline.ts:316`; для three.js — `forceContextLoss()` в `releaseRenderer` (перепроверить комментарий-замер 130-139 на сценарии смены тем).
3. **Закрыть PMREM-утечку** в `YaniCharacterScene`: переносить создание `environmentTarget` после `loadAsync` и/или диспозить его в ветке отмены (251-254) и в catch.
4. **EyeLogo → на videoPool** (тот же механизм, что спас catnap/midnight).
5. (Опционально) Кроссфейд: дедупликация быстрых переключений / `mode="wait"`, чтобы не стековать тяжёлые сцены.
6. (Опционально) Эвикция videoPool по давности (выгружать конвейер, к которому не обращались N минут).

# Верификация после правок

- Сборка, прогон `scripts/perf-sample.ps1` до/после: сценарий «18 переключений тем» — сравнить privWS/commit/vram и форму кривой.
- `document.querySelectorAll('canvas').length` и `frameScheduler.taskCount()` из вашей же диагностики (`collectFrontSnapshot`) до/после.