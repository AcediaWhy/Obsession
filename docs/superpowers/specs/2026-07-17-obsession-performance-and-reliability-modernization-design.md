# Модернизация производительности и надежности Obsession

**Дата:** 2026-07-17
**Статус:** утверждено владельцем 2026-07-17
**База:** текущий незакоммиченный worktree ветки
`codex/zapret2-gate-d-recovery`, HEAD `4235693`

## 1. Контекст

Проект уже имеет рабочий Zapret2 runtime, адаптивный подбор стратегий,
наблюдение Eyes, exact rollback, Tauri/WebView2 launcher и несколько тяжелых
анимированных тем. Текущая задача не состоит в замене этих систем. Требуется
довести существующую архитектуру до предсказуемой задержки, строгого lifecycle
и максимально плавного интерфейса без неконтролируемой нагрузки на ПК.

Аудит выполнялся по текущему незакоммиченному worktree. В нем уже начаты
улучшения, которых нет в опубликованном HEAD:

- ранняя регистрация PID и более точная очистка orphan-процессов;
- single-flight разрешение network identity в части runtime;
- очереди frontend-записей и last-write-wins в части stores;
- backend-owned uptime и сокращение дублирующих status events;
- общий render visibility gate и доработки Rain lifecycle;
- первые Vitest-тесты adaptive, DPI и proxy stores.

Эти изменения являются исходной точкой. Реализация не должна повторять их,
откатывать либо смешивать с несвязанными правками.

### 1.1 Проверенный baseline

На момент аудита текущий worktree проходил:

- `cargo test --all-targets`: 256 из 256 тестов;
- `npm test`: 11 из 11 тестов;
- `npm run build`: успешно, 508 modules;
- `cargo fmt --check`: успешно.

`cargo clippy --all-targets -- -D warnings` находил 14 замечаний. Они связаны в
основном со сложностью orchestration API, условными выражениями и тестовыми
инициализаторами. Это не функциональный отказ, но clean clippy является
обязательным release gate модернизации.

## 2. Цели

1. Сделать время target probe и полного поиска ограниченным и объяснимым.
2. Ускорить подбор без уменьшения количества доказательств и без ослабления
   проверки сертификатов.
3. Исключить detached-задачи, stale events и зависание coordinator во время
   side effects.
4. Сделать start, stop, respawn и rollback DPI-процесса наблюдаемыми и
   ограниченными deadline.
5. Обеспечить плавную анимацию на 60-240 Гц при управляемых CPU, GPU и RAM.
6. Исключить bootstrap, resume и store write races.
7. Создать детерминированное тестовое покрытие отмены, сбоев и переходов
   lifecycle.
8. Снизить сложность самых крупных модулей постепенно, без rewrite.

## 3. Не входит в работу

- изменение логики обхода DPI, встроенных стратегий или security boundary;
- сокращение probe targets, кворума или certificate verification ради скорости;
- скрытое признание кандидата рабочим по неполному набору доказательств;
- редизайн launcher, замена HeroCore, тем, стекла или визуальной идентичности;
- переход с Tauri, React, Zustand, Framer Motion или WebView2;
- массовая переработка всех Rust/React-модулей одним изменением;
- автоматическое восстановление удаленных файлов или включение посторонних
  untracked-файлов без подтверждения владельца.

## 4. Инварианты

### 4.1 Качество поиска

- Набор проверяемых сервисов и роль core/optional targets сохраняются.
- Требуемый кворум и число доказательных раундов не уменьшаются.
- TLS и QUIC certificate validation остаются включенными.
- Раннее завершение разрешено только тогда, когда итог уже математически не
  может измениться.
- Успешный сетевой кворум не отменяет короткое ограниченное окно для
  отрицательного сигнала Eyes.
- Любая неопределенная среда измерения остается отдельным результатом, а не
  маскируется под отсутствие рабочей стратегии.

### 4.2 Lifecycle

- У каждой асинхронной операции есть один владелец.
- Session, attempt и DPI generation проверяются до изменения context, emit и
  записи cache.
- Cancel/Shutdown принимаются во всех фазах, включая spawn, rollback и base
  recheck.
- Исходный runtime snapshot восстанавливается не более одного раза.
- После отмены никакое позднее событие старой сессии не влияет на UI или cache.

### 4.3 Визуальная идентичность

- Сцены, цвета, стекло, HeroCore и характер движения сохраняются.
- Качество регулируется cadence, backing resolution и стоимостью вторичных
  вычислений, а не удалением узнаваемых элементов.
- Reduced motion сохраняет корректный статичный кадр.
- Скрытое окно не продолжает rAF, WebGL simulation или video decoding.

## 5. Рассмотренные подходы

### 5.1 Поэтапная модернизация с измерениями — выбран

Сначала фиксируется baseline и telemetry. Затем отдельными фазами исправляются
lifecycle, probe deadlines, DPI supervision, renderer scheduling и bootstrap.
Каждая фаза имеет свои тесты и acceptance gates.

Преимущества: минимальный regression radius, измеримый эффект и возможность
остановиться после любой законченной фазы. Недостаток: архитектурное разделение
занимает несколько последовательных изменений.

### 5.2 Глубокая одновременная переработка

Полная замена adaptive runtime и render orchestration дала бы более чистый
результат быстрее на бумаге, но одновременно изменила бы state machine,
process lifecycle, evidence flow и каденс сцен. Риск скрытых регрессий слишком
высок для runtime, управляющего пользовательским трафиком.

### 5.3 Только точечные патчи

Локальные таймауты и несколько React memo могли бы быстро убрать отдельные
симптомы, но оставили бы detached tasks, блокирующий rollback и раздробленные
render loops. Этот вариант не закрывает требования по стабильности.

## 6. Целевая архитектура

Новые границы выделяются из существующих модулей постепенно:

- `SearchCoordinator` владеет state machine и принимает control events;
- `SessionTaskSet` владеет preparation, probe, spawn и rollback tasks;
- `ProbeExecutor` отвечает за deadline, DNS, transport и quorum;
- `CandidateRuntime` запускает конкретного кандидата и возвращает typed result;
- `RollbackCoordinator` выполняет exact restore без блокировки event loop;
- `DpiProcessSupervisor` владеет readiness, exit и teardown процессов;
- `BootstrapSnapshot` согласованно гидратирует frontend stores;
- `FrameScheduler` распределяет общий кадровый бюджет сцен.

`adaptive_strategy/runtime.rs` остается точкой композиции на время миграции, но
сетевые операции, process side effects и rollback перестают выполняться inline
в управляющем цикле.

## 7. Measurement-first baseline

До изменения поведения добавляется измерение:

- полная длительность search session и каждого candidate attempt;
- DNS, connect, TLS/QUIC, HTTPS, readiness, teardown и rollback;
- причина раннего завершения серии;
- время от Cancel до смены observable status;
- число отброшенных stale events;
- p50/p95/p99 frame interval и frame cost;
- доля пропущенных целевых кадров и long tasks;
- выбранная refresh rate, target FPS и quality tier;
- память до и после циклов theme/tab/tray.

Production telemetry остается локальной и ограниченной. Экспорт возможен
только как явное обезличенное диагностическое действие пользователя.

## 8. Probe pipeline

### 8.1 Один deadline на target

При старте target создается `ProbeBudget` с абсолютным `Instant` deadline.
Каждый этап получает только оставшийся бюджет:

1. DNS resolve;
2. Happy Eyeballs connect;
3. TLS или QUIC handshake;
4. HTTP evidence;
5. typed classification.

Вложенная операция не может начать новый полный timeout. DNS retry допускается
только внутри оставшегося бюджета. Сон между повторами также входит в budget.

### 8.2 Адреса и соединение

- Выбирается не более одного IPv6 и одного IPv4 адреса.
- Первая семья стартует сразу, вторая — с коротким stagger.
- Первый успешный transport path отменяет оставшиеся attempts.
- Ошибки сохраняются типизированно для диагностики.
- Последовательный предварительный TCP connect перед `reqwest` удаляется.
- Успешный HTTPS подтверждает DNS, TCP и TLS; отказ классифицируется по явному
  DNS result и существующей reqwest error chain.

QUIC сохраняет уже реализованный remaining-budget подход и staggered address
race. TLS приводится к тому же контракту.

### 8.3 Серии и кворум

Targets одного раунда остаются параллельными. Раунды сохраняют существующий
порядок, но серия заканчивается раньше в двух случаях:

- требуемое число успехов уже получено и дополнительные раунды не способны
  изменить verdict/confidence;
- `successes + remaining_rounds < required_successes`.

Правило применяется к TLS и QUIC. Перед положительным завершением coordinator
выдерживает короткое Eyes quiet window внутри общего candidate budget. Его
длительность калибруется по частоте поступления Eyes evidence и не заменяет
существующий veto.

## 9. Search lifecycle и cancellation

### 9.1 Session ownership

Для активной сессии создается `SessionTaskSet`. Он хранит handles для:

- preparation и network identity;
- target discovery/calibration;
- candidate stabilization/probes;
- candidate spawn;
- rollback/base recheck.

Отмена сначала закрывает session generation, затем сигнализирует tasks и
ожидает их bounded completion. Неподчинившийся task abort-ится после deadline.
Вложенные detached spawns запрещены.

`prepare_search_data` использует существующий single-flight resolver вместо
прямого `netid::resolve` в отдельной задаче.

### 9.2 Responsive coordinator

`SearchCoordinator` не ожидает process spawn, probes или rollback inline. Он
отправляет typed command worker-задаче и продолжает читать control queue.
Результат возвращается как событие, содержащее session, attempt и generation.

До любой побочной операции выполняется guard. Второй guard выполняется перед:

- записью `context.last_probe`;
- `adaptive://probe` и status emit;
- подтверждением/сохранением cache;
- переходом model state.

### 9.3 Rollback

Rollback хранит исходный snapshot и отдельный once-guard. Cancel, candidate
failure и shutdown могут запросить rollback одновременно, но исполняется одна
операция. UI различает `cancelling` и `rolling_back`, не создавая иллюзию
мгновенного завершения до фактического restore.

Base recheck является отменяемой частью rollback worker и не блокирует control
queue. Recovery mode сохраняет действующее правило: заведомо неработающая база
не требует повторного сетевого успеха после exact restore.

## 10. DPI process supervision

### 10.1 Readiness

Факт «процесс не завершился за 500 мс» перестает быть единственным признаком
готовности. Supervisor использует составной сигнал:

- PID зарегистрирован сразу после spawn;
- процесс не завершился;
- получен известный startup marker либо успешна ограниченная readiness probe;
- при отсутствии надежного marker действует bounded fallback, а не
  неограниченное ожидание.

Только после readiness запускаются Eyes и candidate probe. Fixed stabilization
остается временным fallback и удаляется после подтверждения нового сигнала на
поддерживаемых Windows-конфигурациях.

### 10.2 Teardown

- Eyes stop получает soft и hard deadline.
- Capture/tracker join не может бессрочно блокировать обычный respawn.
- Owned PIDs завершаются параллельно и обязательно проверяются после kill.
- Ожидание освобождения WinDivert опирается на exit/handle evidence.
- Фиксированные 500 мс остаются только аварийным fallback.
- DNS flush выполняется при изменении DNS/host mappings или подтвержденной
  stale-resolution проблеме, но не при каждой смене стратегии.

Начальная цель normal stop/respawn — p95 не более 2 секунд. Начальный hard
deadline — 5 секунд; его изменение допускается только после Windows baseline и
с документированной причиной.

## 11. Frontend frame architecture

### 11.1 FrameScheduler

Существующие visibility/reduced-motion gates и `createRenderLoop` сохраняются.
Поверх них вводится единый `FrameScheduler`, который:

- измеряет реальную частоту display по серии rAF samples;
- повторяет измерение после смены display и resume;
- выбирает target cadence, совместимый с refresh rate;
- ведет frame-time histogram и dropped-frame counter;
- распределяет budget между full-screen field, HeroCore и secondary effects;
- полностью останавливается в скрытом WebView.

На 144/240 Гц scheduler не обязан рисовать каждый дешевый canvas на каждом
vsync и не фиксирует Rain на несовместимых 60 FPS. Target выбирается как
устойчивый делитель refresh rate с учетом измеренного frame cost.

### 11.2 Dynamic quality

Quality tier меняется с hysteresis, чтобы не дрожать между уровнями. Он может
регулировать:

- backing resolution canvas/WebGL;
- частоту тяжелой simulation при сохранении render interpolation;
- плотность вторичных частиц и texture update cadence;
- дорогие post-processing детали, если они не меняют композицию.

Текущий статический коэффициент около `0.6` является reference, а не потолком.
На мощном ПК backing quality может подниматься до native `1.0`. Минимальные
границы задаются отдельно для сцены после P0 profiling; узнаваемые элементы и
основная геометрия не удаляются.

### 11.3 Rain

Три независимых Rain loops объединяются в один фазовый кадр:

1. coalesced pointer input;
2. dt-based smoothing `1 - exp(-k * dt)`;
3. simulation;
4. water-map/texture update;
5. WebGL draw.

Renderer никогда не загружает texture из состояния, которое simulation еще не
завершила. Pointer normalization использует CSS/client rect, а не backing
resolution canvas.

### 11.4 Theme transition и glass

- Universal `[data-theme-morph] *` заменяется списком semantic layers/tokens.
- Color, border и необходимые тени сохраняют тот же визуальный morph.
- Outgoing background замораживается последним кадром или paused loop.
- Crossfade сохраняется, но два full-screen engines не работают одновременно.
- `AnimatePresence mode="sync"` остается там, где нужен текущий характер
  перехода; high-frequency children уходящей ветки деактивируются.
- Glass blur сохраняется. Containment ставится на screen surfaces только после
  проверки, что он не создает новый backdrop root.

### 11.5 Pointer, stores и logs

- Parallax, Rain и spotlight используют один passive pointer bus.
- Pointer events coalesce один раз за frame.
- Spotlight rect кешируется на `pointerenter`, `ResizeObserver` и scroll/resize.
- Dpi, Ai, Lists, Profiles и Telegram переходят на narrow Zustand selectors.
- Частые status/test fragments отделяются от полного screen tree.
- Log events собираются в ring buffer и commit-ятся не чаще одного раза за
  frame.
- Log rows получают stable IDs; DOM ограничивается viewport window.
- Автоскролл выполняется только если пользователь находился у нижней границы.

Тяжелые альтернативные theme engines загружаются динамически с preload перед
выбором. Aurora может остаться eager как стартовая тема. Неиспользуемые Three/
R3F зависимости удаляются только после bundle analysis и подтверждения, что их
нет в планируемых сценах.

## 12. Bootstrap, resume и store races

### 12.1 Listener-first hydration

Frontend startup выполняется в следующем порядке:

1. зарегистрировать все subsystem listeners;
2. запросить единый `BootstrapSnapshot`;
3. применить каждую секцию snapshot по ее revision;
4. отбросить только ту секцию, которую уже обогнало событие;
5. завершить loaded state независимо по подсистемам.

Snapshot включает settings, DPI, proxy, hosts и adaptive state. Это устраняет
повторные `getSettings`/status IPC из отдельных stores.

### 12.2 Revisions

Общий frontend epoch для несвязанных подсистем удаляется. DPI, proxy, brain и
adaptive имеют собственные monotonic revisions. Несвязанное adaptive событие
не может заставить frontend отбросить актуализацию DPI после tray resume.

Backend snapshot читает sections с их revision. Полная транзакция между всеми
locks не требуется: frontend сравнивает каждую секцию отдельно.

### 12.3 Writes

Settings и DPI write queues сохраняют serialized last-write-wins. Каждая
операция получает ID; поздний resolve старой операции не очищает pending/error
новой. Cleanup listeners и bootstrap остаются идемпотентными при StrictMode
mount-cleanup-remount.

## 13. Тестовая стратегия

### 13.1 Rust unit и component tests

- `tokio::time::pause` для deadline, stagger, retry и cancellation;
- fake DNS с success, timeout, dual-stack и сменой ответа;
- fake transport для TCP/TLS/QUIC stage classification;
- quorum success/impossible tables для TLS и QUIC;
- session/attempt/generation stale result;
- cancel во время discovery, calibration, stabilization и probe;
- cancel/shutdown во время spawn, rollback и base recheck;
- rollback once-guard при конкурирующих причинах;
- poisoned mutex recovery, где это остается частью контракта.

### 13.2 Process fault injection

- ранний exit winws2;
- отсутствие readiness marker;
- зависший Eyes capture/tracker;
- WinDivert teardown timeout;
- taskkill failure и PID reuse guard;
- generation change между spawn и registration/confirmation.

### 13.3 Frontend Vitest

- listener зарегистрирован до snapshot resolve;
- событие между listener и snapshot;
- snapshot failure и последующее событие;
- StrictMode mount-cleanup-remount;
- независимые subsystem revisions на resume;
- deferred write promises и last-write-wins;
- batched logs и сохранение позиции ручного scroll.

### 13.4 Visual и live acceptance

- Playwright/browser frame harness для 60/120/144/165/240 Гц cadence;
- screenshots основных экранов и каждой темы до/после;
- 50 циклов theme/tab switch с memory sampling;
- повторные tray hide/show и sleep/resume;
- Windows live TLS/QUIC search, cancel, rollback и crash recovery;
- проверка startup/teardown с Legacy и Zapret2.

## 14. Этапы внедрения

### P0. Worktree и baseline

- инвентаризировать текущие изменения и отделить уже готовые улучшения;
- подтвердить намеренность удалений `AdaptiveStrategyPanel.tsx` и
  `SECURITY_SCOPE.md`;
- исключить несвязанный `install.ps1`;
- зафиксировать baseline tests, search timing, frame timing и resource use;
- привести документацию к одной traceability matrix.

### P1. Runtime safety

- `SessionTaskSet` и отменяемые workers;
- guard до context/emit/cache;
- responsive coordinator;
- rollback once-guard и bounded shutdown;
- concurrency regression tests.

### P2. Search latency и DPI supervisor

- `ProbeBudget` и TLS Happy Eyeballs;
- quorum-aware early termination;
- positive readiness;
- bounded Eyes/WinDivert teardown;
- conditional DNS flush;
- Windows latency acceptance.

### P3. Renderer performance

- frame telemetry и refresh detection;
- `FrameScheduler` и dynamic quality;
- единый Rain frame;
- semantic theme morph и frozen outgoing scene;
- pointer bus, narrow selectors и batched logs;
- visual/performance acceptance.

### P4. Bootstrap и архитектурные границы

- versioned `BootstrapSnapshot`;
- independent subsystem revisions;
- окончательное выделение coordinator/executor/supervisor modules;
- удаление временных compatibility paths;
- актуализация specs/plans и release checklist.

Каждая фаза реализуется test-first и заканчивается отдельным проверяемым commit
series. Фазы не смешиваются с несвязанными изменениями worktree.

## 15. Критерии приемки

### 15.1 Zapret2

- Target TLS wall time не превышает configured timeout более чем на 250 мс в
  детерминированных тестах.
- Fast/Balanced/Deep сохраняют существующие targets, quorum и validation.
- Golden scenarios дают тот же либо более строгий verdict, чем baseline.
- Cancel observable status появляется не позднее 150 мс.
- После Cancel/Shutdown нет stale UI event, `last_probe` mutation или cache
  commit старой сессии.
- Normal stop/respawn p95 не превышает 2 секунд на acceptance-машине.
- Ни один обычный teardown не выходит за hard deadline без typed error и
  аварийного cleanup path.

### 15.2 Launcher

- p95 frame interval не превышает `1.15 x` выбранный target interval.
- В установившейся сцене менее 1% пропущенных target frames за минуту.
- Нет long task более 50 мс в steady state.
- После 50 theme/tab cycles нет устойчивого роста памяти.
- В tray отсутствуют активные render loops и video decoders.
- Reduced motion и все темы визуально эквивалентны baseline.

### 15.3 Reliability

- Все concurrency/fault-injection tests детерминированны.
- `cargo test --all-targets`, `npm test`, `npm run build`, `cargo fmt --check` и
  `cargo clippy --all-targets -- -D warnings` проходят.
- Listener/snapshot и resume races покрыты отдельными тестами.
- Traceability matrix связывает каждое критическое требование с кодом, тестом
  и метрикой.

## 16. Traceability matrix верхнего уровня

| Требование | Основное решение | Проверка | Метрика |
| --- | --- | --- | --- |
| Качество подбора | Неизменный quorum, Eyes veto, golden scenarios | Probe/model tests + Windows live | Нет ослабленного verdict |
| Ограниченная задержка | `ProbeBudget`, Happy Eyeballs | Paused-time transport tests | timeout + 250 мс |
| Безопасная отмена | `SessionTaskSet`, generation guards | Cancel во всех фазах | status <= 150 мс, 0 stale commits |
| Надежный DPI lifecycle | `DpiProcessSupervisor` | Process fault injection | stop/respawn p95 <= 2 с |
| Плавный launcher | `FrameScheduler`, dynamic quality | Frame harness + live WebView2 | p95 <= 1.15 target interval |
| Отсутствие bootstrap race | Versioned `BootstrapSnapshot` | Deferred-promise Vitest | 0 потерянных updates |

## 17. Документация и hygiene

Существующие планы имеют противоречащие статусы: старый Gate D plan остается
неотмеченным, хотя значительная часть реализована, а confidence spec все еще
говорит об ожидании owner review. P0 создает один актуальный index/traceability
document и помечает устаревшие документы `superseded`, не удаляя исторический
контекст.

Удаление `SECURITY_SCOPE.md` в worktree не считается автоматически
подтвержденным. До реализации владелец выбирает восстановление документа либо
осознанную замену. Модернизация не расширяет зафиксированный security scope.

## 18. Дополнительные возможности после критических фаз

Эти функции не блокируют P0-P4:

- обезличенный экспорт search trace;
- локальный timeline DNS/TCP/TLS/QUIC/readiness/rollback;
- автоматическое обнаружение frame-time degradation;
- crash journal последней незавершенной DPI-операции;
- безопасное сравнение найденного кандидата с последней подтвержденной
  стратегией без нарушения активного соединения.

## 19. Риски и меры снижения

- **Readiness marker нестабилен между версиями winws2.** Использовать несколько
  известных markers и bounded fallback, подтвержденный live tests.
- **Dynamic quality может заметно переключаться.** Применять hysteresis,
  cooldown и изменение только одного tier за окно.
- **Containment может сломать backdrop.** Вводить только после screenshot и GPU
  layer проверки каждого экрана.
- **Early success может изменить confidence metadata.** Завершать серию только
  если итоговый verdict и используемые confidence-поля уже неизменяемы.
- **Новый task ownership может усложнить rollback.** Сначала добавить stale и
  cancel tests, затем переносить side effects по одному.
- **Dirty worktree затрудняет attribution.** P0 разделяет изменения по смыслу и
  не включает файлы без явной связи с фазой.

## 20. Утвержденное решение

Владелец утвердил поэтапную measurement-first модернизацию и все четыре раздела
дизайна 2026-07-17. Следующий артефакт после review этой спецификации —
детальный implementation plan с небольшими test-first задачами и отдельными
commit boundaries.
