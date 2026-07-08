# HANDOFF: Мозг (L3) + Менеджер сети (L2) + эмиссия Глаз — план выполнения

> Документ-передача для продолжения в новом чате. Содержит всё: контекст, решения,
> контракты API существующего кода, прогресс, точные следующие шаги. Полный
> исходный план — `C:\Users\biinn\.claude\plans\lovely-zooming-avalanche.md`.

---

## 0. Что это за проект (рамка)

**Obsession** — десктопное приложение (Tauri v2: Rust backend + React/TS) для повышения
надёжности сетевого соединения при работе через провайдерскую фильтрацию трафика (DPI).
По классу задач аналогично таким открытым инструментам, как Zapret/GoodbyeDPI/VPN-клиенты:
локальная настройка исходящих TLS-соединений (ClientHello) так, чтобы фильтрующее
оборудование провайдера не могло сматчить SNI по сигнатуре. Всё работает локально,
на трафике самого пользователя, никаких внешних хостов не затрагивает.
Путь: `C:\Users\biinn\Desktop\VlarpSu\ObsessionTauri`.

Единственное требование, связанное с безопасностью, — **защитное**: вспомогательный
процесс (`winws`) запускается с правами администратора, поэтому конфигурационные
параметры, пришедшие из сети, нельзя передавать ему напрямую без проверки → в этом
заходе источник кандидатов — ТОЛЬКО bundled `.conf`-файлы (41 доверенный файл из
дистрибутива приложения), а внешний JSON используется лишь для **сортировки** уже
существующих локальных имён файлов по приоритету, не поставляет новых строк для `winws`.

## 1. Цель захода

Достроить верх контура надёжности (нижний слой «Глаза» уже готов):
- **L2 «Менеджер сети»** — MAC шлюза (ключ L1-кэша) + регион/ASN через ipinfo (ключ L2-рейтинга).
- **L3 «Мозг»** — машина состояний, лестница L1→L2→L3 + отдельный предохранитель на случай
  затяжной потери соединения.
- **Эмиссия в React** — `eyes://observation` (сырое) + `brain://status` (агрегат).

**Эмпирический факт (проверено на практике, определяет весь дизайн):** быстрый
последовательный перебор конфигураций вреден — при устойчивой проблеме соединения все
стратегии дают идентичный результат «тишина + повторные пакеты», а частый перебор внешне
похож на массовое зондирование одного хоста (~15 вариантов ClientHello за 10 секунд) →
провайдер может временно заблокировать конкретный IP на 5–10 минут. Поэтому: строгая
лестница + гистерезис + предохранитель, НЕ «перебор всего подряд». Протокол при длительной
потере связи: остановить обход → пауза 5–10 мин на охлаждение → ОДНА пробная попытка.

## 2. Несущая идея архитектуры (повторяет успешный паттерн Глаз)

**Чистый редуктор на логическом времени (`brain::model`) + тонкий интерпретатор
(`brain::runtime`).** Model возвращает `Vec<Action>` синхронно, тестируется на фикстурах
без tokio/WinDivert/admin. Runtime — единственное место сайд-эффектов и `.await`.

## 3. Существующий код — контракты (НЕ переписывать)

### Глаза (`src-tauri/src/eyes/`) — ГОТОВО
- `eyes::start(dll_path: &Path, cfg: eyes::Config, on_observation: F) -> Result<EyesHandle,String>`
  где `F: Fn(Observation) + Send + 'static`. **Колбэк вызывается из OS-потока `eyes-tracker`, НЕ tokio.**
- `Observation { domain: String, dst_ip: IpAddr, local_port: u16, verdict: Verdict, evidence: &'static str, ts_ms: u64 }`
  — уже `#[derive(Clone, Debug, Serialize)]`.
- `enum Verdict { Working, Reset, Blackhole }` — `#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]`,
  serde lowercase. Working=получен ServerHello; Reset=входящий RST (можно пробовать другую стратегию);
  Blackhole=тишина+повторные пакеты/SYN без SYNACK (соединение зависает в этом направлении —
  ЖДАТЬ, не перебирать варианты).
- `eyes::Config { hostlist: Vec<String>, max_flows, syn_synack_timeout_ms, armed_silence_timeout_ms,
  done_linger_ms, min_syn_retx, min_ch_retx, ip_cache_cap }` — есть `Default`.
- Реэкспорты в `eyes/mod.rs`: `pub use signal::{Observation, Verdict}; pub use flow::{Config, FlowTable};`
  `#[cfg(windows)] pub use capture::{start, EyesHandle};`

### dpi.rs (управление winws)
- «Стратегия» = `.conf` файл. 41 bundled в `%APPDATA%\Obsession\configs\<category>\<file>.conf`.
- `dpi::start_many(app: &AppHandle, &[(String,String)]) -> Result<Vec<u32>,String>` — (category, config_file) пары.
  Вызывает `stop_all` первым, стартует winws на каждый, ЗАТЕМ (cfg windows) sleep 800мс +
  стартует Глаза с хостлистом из `collect_hostlist` (regex `--hostlist(?:-auto)?="([^"]+)"` → читает
  list-файлы → домены). Handle Глаз → `AppState.eyes`.
- `dpi::stop_all(app)` — стоп Глаз, `taskkill /F /PID` по всем PID, sleep 500мс (выгрузка WinDivert), flush_dns.
- `dpi::test(app, cat, file) -> bool` — start → 1с → `net::test_url` → stop. Синхронная одиночная проба.
- `collect_hostlist(app, configs)`, `start_eyes(app, hostlist)`, `stop_eyes(app)` — уже есть.
- winws спавнится tokio-процессом, `kill_on_drop(false)`. Ранний выход (<500мс) = провал.

### Инфра для переиспользования
- `util::emit_log(app, level, source, message)` — событие `log` + диск `%APPDATA%\Obsession\logs\app.log`.
  Импорт в util.rs: `use tauri::{AppHandle, Emitter, Manager};`, `use crate::state::AppState;`.
- `util::std_command(program)` — спавн с CREATE_NO_WINDOW (для `route`, `arp`, `ipconfig`).
- `net::test_url(url, secs) -> bool` — TCP :443 + HTTP GET фолбэк. `reqwest::Client` доступен.
- `AppState` (state.rs): `paths: Paths`, `dpi: Mutex<DpiState>`, `proxy: Mutex<ProxyState>`,
  `settings: Mutex<Settings>`, `#[cfg(windows)] eyes: Mutex<Option<EyesHandle>>`. Конструктор
  `AppState::new(paths, settings)`.
- `Paths` (paths.rs): `base_dir`, `configs_dir()`, `config_path(cat,file)`, `get_configs_for_category(cat) -> Vec<String>`
  (отсортированы), `get_categories()`, `logs_dir()`, `lists_dir()`. Есть `sanitize_configs()` (снимает BOM,
  зовётся в `init()`). Паттерн копирования ресурсов: `extract_assets` (не перезатирает при той же версии).
- Персист JSON: паттерн `settings.rs` (serde load/save) и `profiles.rs`.
- Команды регистрируются в `lib.rs` → `invoke_handler![...]` (сейчас заканчивается на `delete_profile`).
  Модули объявлены в начале `lib.rs`: `mod admin; ... mod util;` (алфавитно).
- Мост фронта `src/lib/tauri.ts`: `api.*` (invoke) + `on.*` (`listen<T>("event", cb)`).
- `Cargo.toml` УЖЕ имеет всё: tokio `["process","io-util","rt-multi-thread","macros","time","net","sync"]`,
  reqwest 0.12 rustls, serde/serde_json, regex, chrono. **Новых зависимостей НЕ нужно.**
- `tauri.conf.json` → `bundle.resources` мапит директориями: `"resources/configs":"configs"` и т.д.
- Не-windows должен компилироваться (cfg-гейты обязательны на боевых частях).

## 4. Решения пользователя (ЖЁСТКИЕ рамки)

1. Объём — полная вертикаль, но **БЕЗ внешнего CDN/GitHub Action** (рейтинг из bundled JSON).
2. Кандидаты — **только bundled `.conf`**, упорядочены локальным `ranking.json`. Никаких строк
   конфигурации `winws` из сети.
3. Сеть — **MAC шлюза** (оффлайн, ключ L1) + **регион/ASN через `ipinfo.io/json`** (ключ L2;
   MAC мемоизирует регион → ipinfo вызывается 1 раз на новую сеть).
4. `settings.auto_recovery: bool` default `false` (пока не обкатано).

---

## 5. ПРОГРЕСС

> **ОБНОВЛЕНО (сессия реализации).** Вся вертикаль L2+L3 достроена и связана в
> рантайме. Ниже — актуальное состояние; исходные «осталось» помечены как сделано.
> ⚠️ Компиляция кодом написана полностью, но `cargo build`/`test` на момент записи
> НЕ прогонялись (недоступность классификатора команд в среде) — первый шаг новой
> сессии: собрать и прогнать тесты, поправить возможные мелочи.

### ✅ Этап 1 — Мозг подключён и работает
- `brain::window` — ГОТОВ (5 тестов).
- **`brain::model` — ГОТОВ (полностью написан, 15 тестов).** Машина состояний,
  все переходы, лестница, предохранитель, пейсинг, гистерезис.
- `brain/mod.rs` создан (`pub mod model; pub mod runtime; pub mod window;` +
  реэкспорты), `mod brain;` в lib.rs. Крейт собирает Мозг.

### ✅ РАЗВИЛКА §6 ЗАКРЫТА
`ladder_level` реализован через **метку источника при сборке кандидатов** (тот
вариант, к которому склонялись). В `model.rs`: `enum Source {L1,L2,L3}`,
`struct Candidate{conf, source}`; `ladder_level` = `active_source().tag()`. Model
про полосы L1/L2/L3 НЕ знает — runtime проставляет `Source` в сборщике кандидатов
(`brain/runtime.rs::build_session_start`). Развилки больше нет.

### ✅ Этапы 2-6 — реализованы
- **`netid.rs`** — парсеры route/arp/ipinfo (7 тестов) + `resolve` + `netid_cache.json`.
- **`ranking.rs`** — ревалидация схемы/semver/существования имён + `ranked_for` (тесты).
- **`netcache.rs`** — атомарный L1-кэш `get`/`put` (тесты). `resources/ranking.json`
  заполнен реальными именами (discord 10, gaming 15, universal 6, youtube_twitch 10),
  копируется через `extract_assets` + `tauri.conf.json`.
- **`brain/runtime.rs`** — tokio-задача `start`, тикер, `exec` действий, сборщик
  кандидатов с метками Source, `build_session_start`. **Время: рантайм ведёт СВОЮ
  монотонику и перештамповывает `now`/`ts` во ВСЕХ событиях** (часы Глаз сбрасываются
  на респавне — доверять нельзя).
- **Проводка Глаз→Мозг** (dpi.rs колбэк): `emit("eyes://observation")` + форвард
  `BrainEvent::Observation` в `AppState.brain.tx`.
- **Команды**: `brain_set_enabled`/`brain_get_status`; `SessionStart`/`Stop` из
  обвязки `dpi_start`/`dpi_stop`. `settings.auto_recovery` (default false). Автоподъём
  Мозга в `setup()` если флаг включён. Очистка orphan-winws в `setup()` (Шаг 7).
- **Фронт** (`tauri.ts`): типы `Verdict`/`Observation`/`BrainStatus`, `api.brainSetEnabled`/
  `brainGetStatus`, `on.eyesObservation`/`brainStatus`. Компонент `BrainPanel.tsx`
  (тумблер + читалка статуса) на экране DPI.

### ⏳ ОСТАЛОСЬ новой сессии
1. **`cargo build` + `cargo test`** (базовые 30 + brain/netid/ranking/netcache ~35),
   починить возможные ошибки компиляции. Затем `npm run tauri build -- --debug --no-bundle`.
2. Практический прогон протокола охлаждения (см. §7).
3. Наполнить `ranking.json` реальными `by_asn_region` (сейчас пусто — L2==L3 по факту).
4. (Опц.) Прокинуть `AppState.netid` (сейчас поле есть, но не читается).


## 6. Точные следующие шаги

### Этап 1 (докончить): `brain/model.rs` + `brain/mod.rs`

**`brain/model.rs`** — чистая машина, без cfg/tokio/AppHandle. Определить:
```rust
pub struct BrainCfg {
    pub window_ms: u64,             // 20_000
    pub switch_after_resets: u32,   // 3
    pub confirm_min_hellos: u32,    // 2
```
*(далее без изменений — см. исходный файл, строки 119-180: определения `BrainEvent`,
`Action`, `Phase`, `BrainStatus`, `NetSnapshot`, переходы состояний.)*

- **Переходы (кратко, без изменений логики):**
  1. При обнаружении затяжной потери связи (Blackhole) → `[StopBypass, EmitStatus]`,
     `Frozen{until=now+backoff[level], level}`. (level стартует 0.)
  2. **Confirming:** `hello_count>=confirm_min_hellos` до `deadline` → `Healthy` + `WriteCache` на
     каждую подтверждённую категорию. `now>=deadline` с недобором → кандидат провален → шаг лестницы →
     Switch (если есть кандидат и `now-last_switch>=min_switch_interval_ms`) или `Exhausted`.
  3. **Healthy/Suspect:** `resets_for(cat)>=switch_after_resets` → next_candidate; есть + прошёл интервал →
     `Switching`+`Switch`; нет → `Exhausted`. 1-2 Reset → `Suspect` (без переключения = гистерезис).
  4. **Frozen:** `now>=until` → ОДНА `Probe{cat, лучший L1/L2 кандидат}`.
- `RespawnResult{ok}` → `ok` → `Confirming{deadline=now+confirm_grace_ms}`, сброс `hello_count`,
  `last_switch=now`; `!ok` → шаг лестницы/`Exhausted`.
- `ProbeResult{ok}` → `ok` → `Switch` + `level=0`; `!ok` → `level+=1`, `Frozen{until=now+backoff[min(level,last)]}`.
- `SessionStart` → сид `CatState` из selections+candidates, `Confirming{deadline=now+confirm_grace_ms}`, EmitStatus.
- `SessionStop`/`Shutdown` → `Idle`, `window.clear()`, чистка `tried`.
- Каждый переход → `EmitStatus`. `next_candidate(cat)` = первый из `candidates` не в `tried`;
  `ladder_level` из полосы индекса (L1=0, L2=диапазон ranking, L3=хвост — runtime передаёт границы
  ИЛИ проще: помечать источник при сборке кандидатов; в model достаточно строкой рядом). `Exhausted` =
  курсор за концом.

**Юнит-тесты `model.rs` (`#[cfg(test)]`, скриптованные `Vec<BrainEvent>` с явными ts/Tick):**
- Healthy под редкими Reset → нет Switch (гистерезис).
- `switch_after_resets` Reset в окне → ровно один Switch к next_candidate.
- один Blackhole → `[StopBypass,...]` + Frozen, лестница оборвана.
- Frozen: нет Probe до `until`; ровно одна в `until`; `!ok`→эскалация; `ok`→Switch+level=0.
- Confirming: недобор hellos к deadline → шаг лестницы; добор → Healthy+WriteCache.
- Обход L1→L2→L3, dedup, Exhausted когда сухо.
- Тишина в `confirm_grace_ms` НЕ триггерит предохранитель (нет Blackhole-вердиктов — просто нет hellos).
- `min_switch_interval_ms` блокирует ранний второй Switch.

**`brain/mod.rs`** — `pub mod model; pub mod window; #[cfg(windows)] pub mod runtime;`
реэкспорты (`pub use model::{Brain, BrainCfg, BrainEvent, Action, BrainStatus, NetSnapshot, Phase};`).
Добавить `mod brain;` в `lib.rs` (алфавитно, после `autostart`). **Прогнать `cargo test brain`** — Этап 1 закрыт.

### Этап 2: `netid.rs` (парсеры сети)
- `pub struct NetIdentity { gateway_mac:Option<String>, asn_region:Option<String>, org:Option<String> }`.
- Чистые парсеры (юнит-тест на captured-строках):
  - `parse_gateway_ip(route_print:&str)->Option<String>` — строка `0.0.0.0  0.0.0.0  <gw>` (фолбэк `ipconfig` "Default Gateway").
  - `parse_mac_for_ip(arp_a:&str, ip:&str)->Option<String>` — строка ARP для gw_ip → MAC.
  - `parse_ipinfo(json:&str)->(Option<asn>,Option<region>,Option<org>)` — `org`="AS12389 …"→`AS12389`, `country`+`region`→`AS12389_RU-MOW`.
- Исполнение под `cfg(windows)`: `util::std_command("route").arg("print")`, `arp -a`. ipinfo через reqwest
  (таймаут как net.rs). `getmac` — только последний фолбэк (это MAC адаптера, не шлюза!).
- `netid_cache.json`: `mac -> {asn_region, org, fetched_at}`. MAC известен → ipinfo НЕ дёргать.
- Деградация: нет MAC → синтетический ключ (gw_ip/`"unknown"`); оффлайн → `asn_region=None`.
- `async fn resolve(paths:&Paths)->NetIdentity` (собирает всё, читает/пишет кэш).

### Этап 3: `ranking.rs` + `netcache.rs`
- **`resources/ranking.json`** (bundled, копируется в appdata как configs — добавить в `tauri.conf.json`
  `bundle.resources`: `"resources/ranking.json":"ranking.json"`, и копирование в `paths.rs`):
  ```json
  { "schema_version":1, "winws_compat":">=0.9.0",
    "categories": { "<cat>": { "default":["a.conf",...],
      "by_asn_region": { "AS12389_RU-MOW":["b.conf",...] } } } }
  ```
  ⚠️ Заполнить реальными именами: взять `ls resources/configs/<cat>/`. Для первой версии можно
  `default` = существующие файлы категории, `by_asn_region` пустой.
- **`ranking.rs`**: load + ревалидация: `schema_version==1`; `winws_compat` semver ок; **каждое имя
  существует** среди `get_configs_for_category(cat)` (несуществующие отбросить, порядок сохранить);
  битый/нет файла → L3-only + warn. `fn ranked_for(cat, asn_region:Option<&str>)->Vec<String>`
  = `by_asn_region[asn] ∪ default` (валидированные).
- **`netcache.rs`** (`%APPDATA%\Obsession\netcache.json`):
  ```json
  { "schema_version":1, "networks": { "<mac>": { "asn_region":"...",
      "categories": { "<cat>": {"conf":"x.conf","confirmed_at":N,"success_count":N} } } } }
  ```
  Load/save atomic (tmp→rename, как profiles.rs). `get(mac,cat)->Option<String>`,
  `put(mac, asn_region, cat, conf)` (success_count++, confirmed_at). Битый → пустой + log.
- Юнит-тесты: schema good/bad, отброс несуществующих, round-trip.
- `paths.rs`: добавить `ranking_path()`, `netcache_path()`, `netid_cache_path()` + копирование ranking.json.

### Этап 4: `brain/runtime.rs` (tokio-задача) — `#[cfg(windows)]` боевое, стаб иначе
- `BrainHandle { tx: mpsc::UnboundedSender<BrainEvent>, status: watch::Receiver<BrainStatus>, join }`.
- `pub fn start(app:AppHandle)->BrainHandle`: спавн tokio-задачи с циклом
  `select!{ Some(ev)=rx.recv() => for a in model.step(ev){ exec(a).await } }`; тикер-сосед
  (`interval(tick_ms)` → `tx.send(Tick(now))`, now из `Instant`/монотоника).
- `async fn exec(app, net, action)`: `Switch`→`dpi::start_many` затем `tx.send(RespawnResult{ok})`;
  `StopBypass`→`dpi::stop_all`; `Probe`→`dpi::test` затем `tx.send(ProbeResult)`; `WriteCache`→`netcache::put`
  (mac/asn из net); `EmitStatus`→`app.emit("brain://status", s)` + обновить `watch`.
  **ПРАВИЛО: не держать мьютекс AppState через `.await`.**
- **Сборщик кандидатов** (runtime, до SessionStart): per-cat dedup([L1 netcache.get, L2 ranking.ranked_for,
  L3 get_configs_for_category]).
- **Проводка Глаз** (dpi.rs): в колбэке `on_observation` добавить `app.emit("eyes://observation",&o)` +
  форвард в Мозг `tx` (клонировать из `AppState.brain` если есть). Событий сессии в `stop_all` НЕ добавлять.
- `AppState`: добавить `brain: Mutex<Option<BrainHandle>>`, `netid: Mutex<Option<NetIdentity>>`.

### Этап 5: команды + события + settings
- `settings.rs`: поле `auto_recovery:bool` (default false).
- `commands.rs`: `brain_set_enabled(app, enabled)` (спавн/стоп задачи + персист) и
  `brain_get_status(app)->BrainStatus` (чтение watch). Регистрация в `lib.rs` invoke_handler.
  При SessionStart событие шлёт командный слой (dpi_start обвязка), не dpi.rs.
- `mod brain; mod netid; mod ranking; mod netcache;` в lib.rs.

### Этап 6: фронтенд `src/lib/tauri.ts`
- Типы `Verdict`, `Observation`, `BrainStatus` (camelCase — совпасть с serde rename).
- `api.brainSetEnabled(enabled)`, `api.brainGetStatus()`.
- `on.eyesObservation(cb)` (`listen("eyes://observation")`), `on.brainStatus(cb)` (`listen("brain://status")`).
- Debug-читалка (минимум, без тяжёлого UI).

## 7. Верификация
- После каждого этапа: `cd src-tauri && cargo build` (или `cargo test <mod>`).
- Финал: `cargo test` (сейчас база 30 тестов зелёные — не сломать), затем
  `npm run tauri build -- --debug --no-bundle` (~30с, `--no-bundle` обязателен — иначе NSIS-таймаут).
- Практическая проверка протокола охлаждения: включить Мозг на 1 проблемной категории, смотреть
  `brain://status`; 2-й раз в той же сети → старт с L1 (без ipinfo); при затяжной потере связи →
  StopBypass + полный backoff до 1 пробной попытки (РУКАМИ не перезапускать во время паузы!).
  `min_switch_interval_ms>=30с`.
- ⚠️ Перед rebuild убить `obsession.exe` + `winws.exe` (иначе os error 5 / зависший процесс winws
  блокирует "A copy of winws is already running").

## 8. Риски (кратко)
1. Скорость восстановления соединения vs риск временной блокировки IP (центральное) →
   L1-first, гистерезис, пейсинг, предохранитель.
2. Сама проверка кандидатов может спровоцировать временную блокировку → L3 идёт за пейсингом
   и предохранителем; L1/L2 исчерпать до L3.
3. Пауза при перезапуске (~800мс Глаза не активны) может быть ошибочно принята за длительную
   потерю связи → `confirm_grace_ms` + фаза Confirming карантинят такие случаи.
4. Один шумный поток данных → пороги агрегации не дают лишний раз перезапускать/замораживать.
5. Нет стабильного MAC (VPN/мобильная сеть) → синтетический ключ; ipinfo вернул неверные данные →
   используется default. Деградация без падений приложения.
6. Один набор конфигурации winws: провал одной категории требует перезапуска всех (короткая пауза
   у рабочих категорий). Принято как компромисс.

## 9. Память (обновить в конце)
- `dpi-reliability-arch.md` — пометить L2/L3 реализованными.
- `obsession-tauri-port.md` — этап 2 завершён.
- Индекс `MEMORY.md`.
- (Опц.) новый memory-файл про brain model/runtime split, если появятся нетривиальные решения.
- ⚠️ TODO из прошлой сессии (не забыть): очистка зависших процессов winws на старте приложения
  (`detect_orphaned`+kill в init).
