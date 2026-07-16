# Adaptive Zapret2 Strategy Brain — Implementation Plan

> **Назначение:** единая точка продолжения работ. Документ фиксирует текущий
> checkpoint, уже реализованные части Zapret2/Gate D и пошаговый план локального
> адаптивного подбора стратегий для YouTube и Discord.

**Design:**
`docs/superpowers/specs/2026-07-15-adaptive-zapret2-strategy-brain-design.md`

**Рабочая ветка:** `codex/zapret2-gate-d-recovery`

## 1. Цель следующего этапа

Добавить локальный детерминированный автотюнер Zapret2:

1. Наблюдатель обнаруживает устойчивую проблему YouTube или Discord.
2. UI предлагает запустить поиск, но не начинает его автоматически.
3. Мозг перебирает не более 12 безопасных DSL-кандидатов по одному.
4. Найденный кандидат проходит автоматические probes.
5. Кандидат включается на 60 секунд для ручной проверки.
6. Пользователь сохраняет его или продолжает поиск.
7. Подтверждённая стратегия автоматически используется на той же сети.

MVP полностью локальный: без LLM, облачного API и произвольного Lua.

---

## 2. Текущий checkpoint

### 2.1 Уже реализовано и проверено

- [x] Стабилизирован lifecycle DPI/proxy/tray и operation gates.
- [x] Добавлены generation-safe DPI state и защита от stale runtime events.
- [x] Существующий Мозг разделён на чистую state machine и tokio runtime.
- [x] Наблюдатель выдаёт `working/reset/blackhole` с evidence.
- [x] L1 `netcache.json` хранит подтверждённые Legacy-конфиги по сети.
- [x] Есть миграция старой схемы netcache вместо безусловного сброса.
- [x] Подключены два DPI-движка: Zapret Legacy и ручной Zapret2 Beta.
- [x] Мозг не выбирает Zapret2 автоматически.
- [x] Перед стартом Zapret2 останавливается принадлежащий приложению DPI runtime.
- [x] Неожиданный crash Zapret2 возвращает последний подтверждённый Legacy-набор.
- [x] Intentional/stale exit не запускает ошибочный fallback.
- [x] Исправлена сериализация нескольких профилей Zapret2: `--new` находится
  только между полными профилями, пустого первого профиля нет.
- [x] Zapret2 изолирован в `resources/bin/zapret2/`.
- [x] В runtime-каталоге находятся `winws2.exe`, `cygwin1.dll`, собственная
  `WinDivert.dll` и byte-identical `WinDivert64.sys`.
- [x] Бинарные ресурсы проверяются по размеру и SHA-256.
- [x] Strategy Pack проверяет engine version, Lua API, paths и SHA-256 Lua/blob.
- [x] Dev extraction доставляет свежий Strategy Pack и resource manifest в
  AppData даже при устаревшем Tauri staging.
- [x] Ранний Windows exit `0xC0000135` диагностируется как missing DLL, а не
  общая ошибка Lua.
- [x] Встроенный pack `builtin.base 0.2.3` содержит три профиля:
  `discord_tls_text`, `youtube_tls`, `youtube_quic`.
- [x] Рабочий YouTube TLS:
  `multidisorder_legacy:pos=1,midsld`.
- [x] Рабочий YouTube QUIC:
  `fake:blob=fake_default_quic:repeats=6`.
- [x] Discord открывается и отправляет текстовые сообщения.
- [x] YouTube открывается, 4K загружается и быстро перематывается.
- [x] Финальный Rust-набор: `159 passed, 0 failed`.
- [x] `npm run build` проходит.
- [x] `cargo fmt -- --check` и `git diff --check` проходят.
- [x] Dev Tauri/WebView успешно запускается с pack `0.2.3`.

### 2.2 Документация и коммиты checkpoint

- `c06da3e` — Gate D recovery design.
- `1013015` — Gate D implementation plan.
- `14ac162` — первоначальный YouTube legacy-equivalence design.
- `4e7872a` — as-built рабочая стратегия `0.2.3`.
- `c96845f` — adaptive Zapret2 Strategy Brain design.

### 2.3 Важное состояние Git

Рабочее дерево содержит крупный набор модернизационных изменений, часть которых
существовала до текущего этапа. Реализация Gate D также находится в dirty tree.

**Запрещено:** выполнять общий `git add .` или коммитить всё дерево без
последовательного review. Каждый новый этап должен стадироваться только
явно перечисленными файлами.

Перед первым кодовым коммитом адаптивного Мозга:

```powershell
git status --short
git diff --check
git diff -- <явно перечисленные файлы>
```

### 2.4 Прогресс адаптивного Мозга после создания плана

- [x] Task 0: создан изолированный модуль `adaptive_strategy` без runtime side
  effects.
- [x] Task 1: реализован Safe Strategy DSL со schema version, стабильным
  candidate id, нормализованным JSON и запретом unknown fields.
- [x] Task 2: реализован backend validator с allowlist функций, blobs, payload,
  positions, ranges и bounded числовыми параметрами.
- [x] Task 3: реализован compiler DSL -> `Zapret2Profile`; проверен инвариант
  bare `--new` только между профилями.
- [x] Task 4: реализован deterministic generator для YouTube/Discord с dedup,
  исключением current/tried, приоритетом network-confirmed и лимитом 12.
- [x] Task 5: реализован отдельный Adaptive Strategy Cache с atomic write,
  изоляцией по сети/category/engine/schema и блокировкой после повторных сбоев.
- [x] Task 6: реализована чистая Recovery State Machine с session/attempt id,
  ручным подтверждением и обязательным rollback на reject/timeout/cancel/crash.
- [x] Task 7: реализован Probe Evaluator для YouTube/Discord с DNS, TCP, TLS,
  HTTPS и разделением core/optional целей.
- [ ] Task 8+: runtime, settings, frontend и live acceptance.

Новые файлы:

- `ObsessionTauri/src-tauri/src/adaptive_strategy/mod.rs`;
- `ObsessionTauri/src-tauri/src/adaptive_strategy/dsl.rs`;
- `ObsessionTauri/src-tauri/src/adaptive_strategy/validator.rs`;
- `ObsessionTauri/src-tauri/src/adaptive_strategy/compiler.rs`;
- `ObsessionTauri/src-tauri/src/adaptive_strategy/generator.rs`;
- `ObsessionTauri/src-tauri/src/adaptive_strategy/cache.rs`;
- `ObsessionTauri/src-tauri/src/adaptive_strategy/model.rs`;
- `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`.

Проверки checkpoint:

- adaptive unit tests: `46 passed`;
- полный Rust-набор: `205 passed, 0 failed`;
- `cargo check`: успешно;
- `cargo fmt -- --check`: успешно;
- `git diff --check`: успешно.

Код первой части пока не закоммичен: `lib.rs` уже содержит более ранние
модернизационные изменения, поэтому перед staging требуется отдельный scoped
review/partial staging, а не коммит всего файла.

---

## 3. Принятые архитектурные решения

- [x] Гибридный режим: автообнаружение, ручной запуск поиска.
- [x] Постоянное применение — только после подтверждения пользователя.
- [x] Временная ручная проверка кандидата — 60 секунд.
- [x] Timeout подтверждения возвращает last-known-good.
- [x] MVP поддерживает только `youtube_twitch` и `discord`.
- [x] Генератор полностью локальный и детерминированный.
- [x] Кандидаты описываются Safe Strategy DSL, а не Lua-кодом.
- [x] Максимум 12 кандидатов, 10–15 секунд на кандидат.
- [x] Кандидаты проверяются последовательно.
- [x] Одновременно восстанавливается одна категория.
- [x] Подтверждённая стратегия привязывается к отпечатку сети.
- [x] На той же сети подтверждённый результат применяется автоматически.
- [x] Legacy и Zapret2 не работают одновременно на пересекающемся трафике.
- [x] Существующий Legacy netcache не используется для хранения DSL.

---

## 4. План реализации

### Task 0 — Зафиксировать безопасную границу нового модуля

**Цель:** не смешивать адаптивный поиск с уже работающей Legacy state machine.

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/mod.rs`

**Modify:**

- `ObsessionTauri/src-tauri/src/lib.rs`

**Шаги:**

1. Создать отдельный модуль `adaptive_strategy`.
2. Не менять поведение существующих `brain::model` и `brain::runtime` на этом
   этапе.
3. Добавить module declaration без startup side effects.
4. Зафиксировать внутренние подмодули: `dsl`, `validator`, `generator`, `model`,
   `runtime`, `probe`, `cache`.

**Проверка:**

```powershell
cargo check
```

**Коммит:** `feat: scaffold adaptive strategy module`

---

### Task 1 — Safe Strategy DSL

**Цель:** представить кандидата типизированными безопасными данными.

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/dsl.rs`

**Основные типы:**

```rust
pub enum AdaptiveCategory {
    Discord,
    YoutubeTwitch,
}

pub enum StrategyTransport {
    Tls,
    Quic,
}

pub enum StrategyFunction {
    Fake,
    MultiSplit,
    MultiDisorder,
    MultiDisorderLegacy,
    FakeDSplit,
    FakeDDisorder,
}

pub struct StrategyStep {
    pub function: StrategyFunction,
    pub args: BTreeMap<String, StrategyValue>,
}

pub struct StrategyCandidate {
    pub id: String,
    pub category: AdaptiveCategory,
    pub transport: StrategyTransport,
    pub steps: Vec<StrategyStep>,
    pub payload: Vec<AllowedPayload>,
    pub out_range: Option<AllowedRange>,
}
```

Точные enum names могут корректироваться, но public data model должен:

- сериализоваться в нормализованный JSON;
- иметь стабильный schema version;
- выдавать стабильный candidate hash/id;
- не содержать raw Lua, raw CLI или произвольных путей;
- поддерживать только YouTube/Discord и TLS/QUIC.

**Тесты:**

- JSON roundtrip;
- стабильная нормализация порядка args;
- стабильный candidate id;
- неизвестные enum values отклоняются;
- отсутствует поле для raw Lua/path/command.

**Коммит:** `feat: add safe adaptive strategy DSL`

---

### Task 2 — Backend Strategy Validator

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/validator.rs`

**Allowlist MVP:**

- функции только из bundled `zapret-antidpi.lua`;
- payload: `tls_client_hello`, `quic_initial`;
- L7: `tls`, `quic`;
- positions: ограниченный набор `1`, `2`, `midsld`, `sniext`, `host`,
  `endhost`, а также разрешённые bounded offsets;
- repeats в небольшом фиксированном диапазоне;
- разрешённые fake blobs: стандартные встроенные и объявленные pack blobs;
- bounded `tcp_seq`, `tcp_ts`, TTL и ranges;
- максимум функций в одном кандидате;
- hostlist и capture ports задаются приложением.

**API:**

```rust
pub fn validate(candidate: &StrategyCandidate) -> ValidationReport;
```

**Негативные тесты:**

- неизвестная категория/функция/аргумент;
- path traversal;
- raw `@file`, shell fragments и separators;
- слишком большие repeats/offsets;
- несовместимый transport/payload;
- пустая стратегия;
- слишком длинная цепочка.

**Коммит:** `feat: validate adaptive strategy candidates`

---

### Task 3 — Candidate Compiler

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/compiler.rs`

**Modify при необходимости:**

- `ObsessionTauri/src-tauri/src/dpi_engine/zapret2.rs`
- `ObsessionTauri/src-tauri/src/dpi_engine/manifest.rs`

**Цель:** компилировать проверенный DSL в существующий `Zapret2Profile`, не
создавая Lua-файлы.

**API:**

```rust
pub fn compile(
    candidate: &StrategyCandidate,
    hostlist: PathBuf,
) -> Result<Zapret2Profile, CompileError>;
```

**Инварианты:**

- `validate()` вызывается на backend boundary;
- имя профиля включает session/candidate id, но не пользовательский ввод;
- hostlist берётся из `Paths` по категории;
- filter ports и payload выводятся из enum;
- CLI escaping не требуется, потому что процесс получает отдельные argv;
- output builder не может вставить дополнительный `--new`.

**Тесты:** exact argv для нескольких TLS/QUIC-кандидатов и rejection invalid DSL.

**Коммит:** `feat: compile safe candidates to Zapret2 profiles`

---

### Task 4 — Deterministic Candidate Generator

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/generator.rs`

**Вход:**

- категория;
- diagnosis/evidence;
- текущая стратегия;
- подтверждённая стратегия текущей сети;
- built-in seed library;
- already tried candidate ids.

**Порядок:**

1. network-confirmed DSL;
2. текущий bundled known-good;
3. близкие мутации одной функции/позиции;
4. allowlisted fake + split/disorder chains;
5. более агрессивные bounded варианты.

**Ограничения:**

- максимум 12 уникальных кандидатов;
- deterministic output при одинаковом input;
- сначала меняется один параметр;
- YouTube и Discord имеют отдельные seed sets;
- TLS и QUIC ранжируются отдельно;
- кандидат текущей активной стратегии не тестируется повторно.

**Тесты:**

- deterministic order;
- dedup;
- budget 12;
- diagnosis-sensitive ranking;
- known-good прежде мутаций;
- неизвестная категория не даёт кандидатов.

**Коммит:** `feat: generate bounded adaptive strategy candidates`

---

### Task 5 — Отдельный Adaptive Strategy Cache

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/cache.rs`

**Modify:**

- `ObsessionTauri/src-tauri/src/paths.rs`

**Файл:** `%APPDATA%/Obsession/adaptive-strategies.json`

Не расширять существующий `netcache.json`: он хранит Legacy `.conf` и уже
используется работающим L1-контуром. Новый cache получает собственную схему.

**Entry:**

- network key/fingerprint;
- category;
- engine version;
- strategy schema version;
- normalized DSL;
- candidate id;
- confirmed timestamp;
- success/failure counters;
- last probe summary;
- disabled/invalidated reason.

**Поведение:**

- atomic tmp -> rename;
- миграция старых совместимых схем;
- future schema не применяется;
- другая сеть не видит entry;
- engine/schema mismatch инвалидирует применение, но сохраняет файл для
  диагностики;
- reset удаляет только выбранную category/network entry.

**Тесты:** roundtrip, migration, isolation, corrupt JSON, atomic semantics.

**Коммит:** `feat: persist confirmed adaptive strategies per network`

---

### Task 6 — Чистая Recovery State Machine

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/model.rs`

Следовать существующему паттерну `brain::model`: никакого Tokio/AppHandle/FS.

**Phases:**

- `Idle`;
- `Suggested`;
- `Searching`;
- `CandidateProbe`;
- `TemporaryVerification`;
- `AwaitingApproval`;
- `Applying`;
- `RollingBack`;
- `Applied`;
- `Exhausted`;
- `Cancelled`.

**Events:**

- confirmed diagnosis;
- user start/cancel;
- candidate started/start failed;
- probe result;
- observation;
- verification timeout;
- user confirm/reject;
- runtime crash;
- session stop/shutdown;
- tick.

**Actions:**

- emit suggestion/status;
- start candidate;
- run probes;
- start 60-second verification;
- rollback exact snapshot;
- persist confirmed candidate;
- continue/finish search.

**Обязательные тесты:**

- поиск не начинается от одной ошибки;
- поиск не начинается без user start;
- stale session/candidate event игнорируется;
- timeout verification вызывает rollback;
- reject продолжает поиск;
- confirm относится только к текущему candidate id;
- cancel/shutdown всегда rollback;
- budget exhaustion;
- no candidate after last-known-good loss;
- generation never equals zero.

**Коммит:** `feat: model adaptive recovery workflow`

---

### Task 7 — Probe Evaluator

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/probe.rs`

**Reuse:**

- `ObsessionTauri/src-tauri/src/net.rs`
- `ObsessionTauri/src-tauri/src/eyes/`
- bounded channels/pattern из `brain::runtime`.

**YouTube automatic probes:**

- DNS;
- TCP/443;
- TLS ServerHello;
- HTTPS response `youtube.com`/`www.youtube.com`;
- отсутствие reset/blackhole в окне;
- положительные `googlevideo.com` observations при ручной проверке.

**Discord automatic probes:**

- DNS/TCP/TLS для `discord.com`;
- HTTPS response;
- TLS/gateway availability для `gateway.discord.gg`;
- WebSocket handshake без пользовательского token, если это возможно без
  нестабильной зависимости; иначе TLS probe + ручное подтверждение;
- отсутствие reset/blackhole.

**Scoring:** результат структурированный, а не один bool. Постоянное применение
требует нескольких независимых сигналов.

**Коммит:** `feat: evaluate adaptive YouTube and Discord candidates`

---

### Task 8 — Runtime Coordinator и DPI integration

**Create:**

- `ObsessionTauri/src-tauri/src/adaptive_strategy/runtime.rs`

**Modify:**

- `ObsessionTauri/src-tauri/src/state.rs`
- `ObsessionTauri/src-tauri/src/dpi.rs`
- `ObsessionTauri/src-tauri/src/commands.rs`
- `ObsessionTauri/src-tauri/src/lib.rs`
- при необходимости `ObsessionTauri/src-tauri/src/eyes/mod.rs`

**AppState:** добавить отдельный handle/runtime slot, не смешивая его с
существующим `brain`.

**DPI refactor:** вынести общий старт Zapret2 так, чтобы runtime мог заменить
один профиль кандидатом, сохранив подтверждённые профили остальных категорий.
Обычный `dpi_start` должен использовать прежний путь без изменения поведения.

**Ворота:** все start/stop/candidate/rollback операции используют существующий
`dpi_gate`. Нельзя держать std lock через `.await`.

**Rollback snapshot:**

- выбранный engine;
- активные категории;
- точные Legacy configs;
- точный bundled/confirmed Zapret2 DSL;
- generation/session id.

**События UI:**

- `adaptive://suggestion`;
- `adaptive://status`;
- `adaptive://verification`;
- `adaptive://applied`.

**Команды:**

- `adaptive_get_status`;
- `adaptive_start_search(category)`;
- `adaptive_cancel_search()`;
- `adaptive_confirm_candidate(session_id, candidate_id)`;
- `adaptive_reject_candidate(session_id, candidate_id)`;
- `adaptive_reset_saved(category)`.

**Коммит:** `feat: run adaptive Zapret2 recovery sessions`

---

### Task 9 — Settings и feature flag

**Modify:**

- `ObsessionTauri/src-tauri/src/settings.rs`
- `ObsessionTauri/src/lib/tauri.ts`
- `ObsessionTauri/src/store/settingsStore.ts`

**Настройка:** `adaptive_strategy_enabled`, default `false` до live acceptance.

Не делать пользовательскими в MVP budget/timers: 12 кандидатов, 10–15 секунд и
60-секундная проверка остаются внутренними безопасными константами.

Существующий `auto_recovery` не переименовывать и не менять его Legacy-семантику.

**Коммит:** `feat: gate adaptive strategy recovery behind setting`

---

### Task 10 — Frontend API и отдельный Zustand store

**Create:**

- `ObsessionTauri/src/store/adaptiveStrategyStore.ts`

**Modify:**

- `ObsessionTauri/src/lib/tauri.ts`
- `ObsessionTauri/src/App.tsx` или текущая точка lifecycle subscriptions.

Не раздувать `dpiStore.ts`: новый store отвечает только за adaptive status,
suggestion, progress, verification countdown и команды пользователя.

**State:**

- phase;
- suggested category/reason;
- session/candidate ids;
- index/total;
- probe summary;
- verification deadline;
- error;
- saved strategy summary.

**Коммит:** `feat: add adaptive strategy frontend store`

---

### Task 11 — DPI UI

**Modify:**

- `ObsessionTauri/src/screens/Dpi.tsx`
- при необходимости атомы/компоненты в `ObsessionTauri/src/design/components/`.

**UI:**

- ненавязчивая карточка suggestion;
- `Найти стратегию`;
- modal/panel предупреждения о кратких переподключениях;
- прогресс `N / 12`;
- текущий probe этап;
- cancel;
- 60-секундная verification card;
- `Работает — сохранить`;
- `Не работает — продолжить поиск`;
- `Отмена и возврат`;
- saved strategy и reset для текущей сети.

UI не показывает raw Lua/CLI. Допустима безопасная человекочитаемая сводка:
`TLS · legacy disorder · split 1/midsld`.

**Коммит:** `feat: add adaptive strategy recovery UI`

---

### Task 12 — Integration, resilience и resource budget

**Проверить:**

- только один winws/winws2 владеет трафиком;
- cancel не оставляет процессы/timers/cache drafts;
- crash кандидата вызывает rollback;
- shutdown при любой phase корректен;
- ручной DPI Stop имеет приоритет;
- tray hide/show восстанавливает свежий snapshot;
- observation flood не вытесняет control events;
- поиск не идёт параллельно `dpi_test/autoConfigure`;
- высокая системная нагрузка может отложить candidate start;
- app log не содержит packet payload и не разрастается debug-выводом.

**Коммит:** `fix: harden adaptive strategy lifecycle`

---

### Task 13 — Полная автоматическая проверка

```powershell
cd C:\Users\biinn\Desktop\VlarpSu\ObsessionTauri\src-tauri
cargo fmt -- --check
cargo test

cd C:\Users\biinn\Desktop\VlarpSu\ObsessionTauri
npm run build

cd C:\Users\biinn\Desktop\VlarpSu
git diff --check
```

Ожидание: все существующие 159 тестов остаются зелёными плюс новые DSL,
validator, generator, model, cache и runtime tests.

---

### Task 14 — Live acceptance

#### YouTube

1. Включить feature flag.
2. Подставить контролируемый нерабочий кандидат.
3. Убедиться, что Мозг только предлагает поиск.
4. Нажать `Найти стратегию`.
5. Проверить последовательный перебор и cancel.
6. Дождаться рабочего кандидата.
7. Проверить YouTube/4K в 60-секундном окне.
8. Сохранить.
9. Перезапустить приложение: стратегия применяется на той же сети.
10. Сменить network fingerprint: стратегия не применяется.

#### Discord

1. Повторить сценарий для Discord.
2. Проверить открытие и gateway.
3. Отправить текстовое сообщение вручную.
4. Сохранить кандидат.

#### Rollback

1. Reject -> следующий кандидат.
2. Verification timeout -> exact last-known-good.
3. Cancel -> exact last-known-good.
4. Candidate crash -> exact last-known-good.
5. App shutdown -> нет orphaned winws/winws2.

После live acceptance включить настройку в UI, но оставить default `false` ещё
на один release cycle. Автоматический запуск поиска без клика остаётся вне MVP.

---

## 5. Рекомендуемый порядок ближайшей работы

Следующая сессия должна продолжить строго отсюда:

1. Task 0 — scaffold отдельного модуля.
2. Task 1 — DSL.
3. Task 2 — validator.
4. Task 3 — compiler.
5. Task 4 — generator.

Первые пять задач не должны переключать реальный DPI runtime. Это создаёт
безопасный, полностью unit-tested фундамент до работы с WinDivert и UI.

После Task 4 остановиться на code review и только затем переходить к cache,
state machine и side effects.

## 6. Критерий завершения функции

Функция считается готовой, когда одновременно выполнены условия:

- пользователь сам запускает предложенный recovery;
- генератор не выходит за Safe DSL;
- поиск bounded и отменяемый;
- найденная стратегия не сохраняется без подтверждения;
- timeout/reject/cancel/crash возвращают exact last-known-good;
- подтверждённая стратегия изолирована по сети;
- YouTube и Discord проходят live acceptance;
- Legacy и обычный ручной Zapret2 не регрессировали;
- полный test/build pipeline зелёный.
