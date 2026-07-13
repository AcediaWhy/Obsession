# Obsession modernization implementation plan

**Дата:** 14 июля 2026 года

**Основание:** `docs/superpowers/specs/2026-07-14-obsession-launcher-modernization-design.md`

## Цель выполнения

Пошагово реализовать утверждённую модернизацию без замены работающих механизмов на месте и без радикальных изменений интерфейса.

Исполнение разбито на независимые workstream. Каждый workstream должен завершаться собственными автоматическими проверками, ручным smoke-тестом и отдельным коммитом. Нельзя смешивать незавершённый runtime Phase 0 с AI safety или Zapret2 в одном коммите.

## Текущий baseline

На момент создания плана:

- текущая версия приложения — 1.1.0;
- в рабочем дереве находится незакоммиченная реализация спецификации runtime reliability от 13 июля 2026 года;
- изменено 35 отслеживаемых файлов, около 2082 добавленных и 670 удалённых строк;
- `cargo test` проходит: 73 теста;
- `npm run build` проходит;
- `cargo fmt -- --check` показывает три форматных расхождения в `commands.rs`, `dpi.rs` и `proxy.rs`;
- Zapret2 1.0.2 для Windows x64 скачан в `zapret2-v1.0.2/`;
- текущий Malw установлен и работает;
- встроенный TgWsProxy имеет версию 1.8.1.

## Общие правила выполнения

1. Перед редактированием файла проверить его текущий незакоммиченный diff.
2. Не откатывать и не перезаписывать существующие пользовательские изменения.
3. Сначала писать или уточнять regression-тест, затем менять поведение.
4. Новые системные операции реализовывать по схеме Prepare → Validate → Snapshot → Apply → Verify → Commit/Rollback.
5. Не добавлять автоматический выбор Zapret2 или GeoHide до отдельного acceptance gate.
6. Не загружать Lua из сети; первые Strategy Packs только встроенные.
7. После каждого workstream выполнять `cargo fmt -- --check`, `cargo test`, `npm run build` и `git diff --check`.
8. Release/manual проверки, требующие UAC, выполнять только после успешных unit/build проверок.

---

## Workstream 0 — завершить текущий runtime Phase 0

### Задача 0.1 — привести Rust diff к форматированному состоянию

**Файлы:**

- `ObsessionTauri/src-tauri/src/commands.rs`
- `ObsessionTauri/src-tauri/src/dpi.rs`
- `ObsessionTauri/src-tauri/src/proxy.rs`

**Действия:**

1. Запустить `cargo fmt` из `ObsessionTauri/src-tauri`.
2. Проверить, что форматирование не изменило поведение и затронуло только Rust-файлы.
3. Выполнить `cargo fmt -- --check`.

### Задача 0.2 — сопоставить реализацию со спецификацией runtime reliability

**Проверяемые области:**

- proxy gate/generation;
- half-close и abort дочерних proxy connections;
- bounded queues Eyes/Brain;
- time-driven tracker ticks;
- DPI session gate для UI/tray/hotkey/shutdown/emergency;
- корректная HTTPS-проверка;
- patch-настройки без lost update;
- UI suspend/resume;
- runtime snapshot и защита от stale response;
- корректное освобождение WebView/тем при уходе в трей.

**Действия:**

1. Для каждого пункта указать реализующий файл и существующий тест.
2. Если теста нет, добавить минимальный regression-тест до исправления.
3. Не добавлять новые продуктовые возможности в этом workstream.

### Задача 0.3 — автоматические проверки Phase 0

**Команды:**

```powershell
cd ObsessionTauri/src-tauri
cargo fmt -- --check
cargo test

cd ..
npm run build
```

**Дополнительно:**

- `git diff --check`;
- убедиться, что `dist/` и `target/` не попали в Git;
- проверить, что число Rust-тестов не уменьшилось;
- проверить отсутствие новых warning, связанных с изменённым кодом.

### Задача 0.4 — release smoke и стресс

**Проверки:**

1. Собрать release Tauri.
2. Выполнить не менее 20 быстрых proxy start/stop циклов перед длинным stress-run.
3. Выполнить не менее 20 DPI start/stop циклов.
4. Закрыть приложение во время первых 500 мс запуска DPI и проверить отсутствие orphan `winws.exe`.
5. Закрыть приложение при активном TgWsProxy и проверить отсутствие процесса и firewall rule.
6. Выполнить серию hide/show для тяжёлых тем и проверить сохранение несохранённого текста.
7. Проверить tray/hotkey изменения при скрытом UI и корректный snapshot после открытия.

Длинные 100-цикловые и 30-минутные измерения выполняются после успешного короткого smoke.

### Задача 0.5 — зафиксировать Phase 0 отдельно

Перед коммитом:

- staged diff содержит только runtime reliability изменения и связанную документацию;
- Zapret2 release archive, AI safety и новые Strategy Packs не включены;
- все автоматические проверки проходят;
- известные ограничения ручных тестов перечислены в сообщении коммита или отдельном отчёте.

---

## Workstream 1 — Malw transactional safety

### Задача 1.1 — ввести тестируемые пути и чистые модели

**Файлы:**

- изменить `ObsessionTauri/src-tauri/src/hosts.rs`;
- добавить `ObsessionTauri/src-tauri/src/hosts_snapshot.rs`;
- добавить `ObsessionTauri/src-tauri/src/hosts_validate.rs`;
- подключить модули в `ObsessionTauri/src-tauri/src/lib.rs`.

**Модели:**

- `HostsProvider`/существующий `Provider`;
- `HostsSnapshotMeta`;
- `HostsManagedState`;
- `HostsValidationReport`;
- `HostsApplyResult`;
- `HostsProbeResult`.

Системный путь `C:\Windows\System32\drivers\etc\hosts` не должен быть зашит внутрь чистых функций. Функции валидации, snapshot и атомарной замены получают путь аргументом, чтобы тесты работали во временной директории.

### Задача 1.2 — валидатор Malw/GeoHide payload

**Тесты сначала:**

- принимает комментарии и корректные IPv4/IPv6 hosts-строки;
- отклоняет пустой payload;
- отклоняет payload выше лимита;
- отклоняет строки с URL, shell-конструкциями, NUL и недопустимой кодировкой;
- отклоняет конфликтующие адреса для одного домена внутри объединённого payload;
- требует минимальный набор ожидаемых AI-доменов для выбранного провайдера;
- сохраняет комментарии версии;
- нормализует окончания строк без изменения семантики.

**Поведение:**

`download_hosts` только загружает bytes. Преобразование в применимый файл выполняется после успешного `validate_hosts_payload`.

### Задача 1.3 — metadata и точные snapshots

**Расположение:**

- `%APPDATA%\Obsession\hosts-state.json`;
- `%APPDATA%\Obsession\hosts-backups\`.

**Действия:**

1. Перед любой записью создать snapshot текущего файла.
2. Записать metadata через атомарный temp + replace.
3. Хранить отдельный `original_snapshot`.
4. Хранить указатель `last_known_good` на провайдера.
5. Хранить не более пяти подтверждённых snapshots на провайдера плюс original.
6. Никогда не выбирать snapshot по эвристике «предпоследний файл».

**Тесты:**

- metadata roundtrip;
- точный byte-for-byte restore;
- cleanup не удаляет original и active last-known-good;
- повреждённый state JSON не приводит к удалению backups;
- конкурентные операции сериализованы.

### Задача 1.4 — обнаружение внешнего изменения hosts

Перед обновлением сравнить SHA-256 текущего файла с `applied_sha256`.

Если хеш не совпадает:

- не перезаписывать файл автоматически;
- вернуть отдельный статус `externally_modified`;
- сохранить новую внешнюю версию отдельным snapshot;
- потребовать явного подтверждения пользователя для продолжения.

Добавить Tauri command для подтверждённого применения после внешнего изменения, не объединяя это действие с обычной кнопкой обновления.

### Задача 1.5 — транзакция apply/rollback

**Порядок:**

1. Acquire hosts operation gate.
2. Read current file and state.
3. Detect external modification.
4. Download provider и additional payload во временную область.
5. Validate each payload и их объединение.
6. Create exact pre-operation snapshot.
7. Write prepared file atomically.
8. Re-read and verify byte/hash equality.
9. Flush DNS.
10. Run read-only probes.
11. Mark last-known-good либо restore operation snapshot.
12. Release gate и emit structured result.

Rollback должен использовать snapshot текущей операции, а не общий поиск файлов.

### Задача 1.6 — безопасные probes

Изменить `ObsessionTauri/src-tauri/src/net.rs` либо добавить специализированные AI probe helpers.

Probe не использует аккаунт, cookies или токены. Проверяется:

- DNS result;
- TCP connect;
- TLS handshake/HTTPS response с системной проверкой сертификата;
- ограниченный timeout;
- отсутствие передачи пользовательских данных.

Сбой одного необязательного сервиса отображается, но не откатывает весь payload. Автоматический rollback выполняется, когда провалены все core probes или записанный файл не прошёл hash verification.

### Задача 1.7 — команды и UI без редизайна

**Файлы:**

- `ObsessionTauri/src-tauri/src/commands.rs`;
- `ObsessionTauri/src/lib/tauri.ts`;
- `ObsessionTauri/src/store/hostsStore.ts`;
- `ObsessionTauri/src/screens/Ai.tsx`.

**Добавить:**

- структурированный transaction status;
- `externally_modified`;
- `rollback_available`;
- кнопку «Вернуть рабочую версию»;
- явное подтверждение перезаписи внешне изменённого файла;
- GeoHide как ручной резерв без auto-failover;
- список результатов probes в существующей карточке, без нового экрана.

### Задача 1.8 — acceptance Malw

1. Установленный рабочий Malw остаётся byte-for-byte неизменным до действия пользователя.
2. Network/download/validation failure не меняет системный `hosts`.
3. Искусственно неуспешная post-check восстанавливает exact snapshot.
4. Внешнее изменение блокирует silent overwrite.
5. Кнопка восстановления возвращает last-known-good.
6. GeoHide не включается автоматически.

---

## Workstream 2 — TgWsProxy hardening

### Задача 2.1 — вынести LAN publication policy

**Файлы:**

- изменить `ObsessionTauri/src-tauri/src/proxy.rs`;
- при росте файла добавить `ObsessionTauri/src-tauri/src/proxy_lan.rs`;
- изменить `ObsessionTauri/src-tauri/src/state.rs`.

Модель `ProxyLanSession` содержит generation, port, allowed subnets, expiry, firewall rule и forwarder handle.

### Задача 2.2 — ограниченное правило Firewall

- профиль только `Private`;
- remote IP только локальные подсети выбранных физических интерфейсов;
- правило имеет уникальный generation-aware name;
- удаляется только правило текущей сессии;
- таймаут публикации не останавливает локальный proxy для Telegram Desktop.

Командную строку `netsh` строить из валидированных числовых port/IP значений, не принимать произвольные строки от frontend.

### Задача 2.3 — таймер, ротация и observability

- настраиваемый LAN timeout;
- countdown в UI;
- число активных LAN connections;
- явная кнопка закрытия публикации;
- опциональная ротация secret после завершения телефонной сессии;
- отдельные статусы process/listener/WS/text/media/DC/transport.

### Задача 2.4 — acceptance Telegram

- телефон подключается в Private LAN;
- после timeout порт недоступен из LAN;
- локальный Telegram Desktop proxy продолжает работать;
- старое поколение не удаляет ресурсы нового;
- shutdown не оставляет process, listener или firewall rule.

---

## Workstream 3 — Zapret1/Zapret2 dual-engine Beta

### Задача 3.1 — добавить фиксированные ресурсы Zapret2

Копировать только необходимые x64 runtime-файлы из официального релиза 1.0.2:

- `winws2.exe`;
- совместимые `WinDivert.dll` и `WinDivert64.sys`;
- базовые Lua libraries;
- необходимые fake payloads;
- manifest с версиями и SHA-256.

Не включать `zapret2-master/` и полный release archive в Tauri resources.

### Задача 3.2 — интерфейс DpiEngine

**Предлагаемая структура:**

```text
src-tauri/src/dpi_engine/
  mod.rs
  legacy.rs
  zapret2.rs
  manifest.rs
```

Интерфейс покрывает validate/start/stop/probe/capabilities. Существующая внешняя Tauri command surface сохраняется либо расширяется обратно совместимыми полями.

### Задача 3.3 — встроенный Strategy Pack schema

Добавить parser/validator manifest, compatibility gate по версии winws2/Lua API и SHA-256 каждого файла.

Тесты:

- valid built-in pack;
- missing file;
- hash mismatch;
- incompatible engine version;
- incompatible Lua API;
- duplicate strategy id;
- path traversal в manifest.

### Задача 3.4 — Beta lifecycle и fallback

- Legacy остаётся default;
- включение Zapret2 только вручную;
- перед Beta сохраняется последний рабочий Legacy selection;
- engine switch сериализован;
- движки не работают одновременно на пересекающихся фильтрах;
- crash Zapret2 приводит к cleanup и автоматическому возврату сохранённого Legacy selection;
- отсутствие Legacy selection приводит к stopped state и явной ошибке.

### Задача 3.5 — минимальный UI

Добавить в существующий DPI-экран:

- chip/select `Zapret Legacy` / `Zapret2 Beta`;
- версию движка;
- предупреждение Beta;
- статус fallback;
- запрет автоматического Brain выбора Zapret2.

Новый экран и изменение NavRail не требуются.

---

## Workstream 4 — Eyes protocol expansion

Этот workstream начинается только после стабильного dual-engine lifecycle.

### Задачи

1. Добавить отдельный bounded UDP/QUIC capture pipeline.
2. Реализовать минимальный QUIC Initial parser либо использовать данные winws2 probes, не внедряя полный QUIC stack.
3. Добавить DNS active probes без перехвата пользовательского содержимого.
4. Нормализовать диагнозы Working/DnsFailure/TcpReset/TcpBlackhole/TlsBlackhole/QuicBlocked/UdpBlocked/HttpBlockPage/Throttled/IpUnreachable/Unknown.
5. Добавить pacing и drop counters.
6. Расширить replay/unit tests синтетическими UDP/QUIC/DNS сценариями.

---

## Workstream 5 — Brain multi-engine recovery

Начинается после появления стабильных диагнозов Eyes.

### Задачи

1. Расширить Candidate до engine + strategy + category.
2. Сохранить обратную совместимость существующего netcache schema через migration/version gate.
3. Выбирать кандидатов по диагнозу и aggressiveness.
4. Не менять более одной категории без необходимости.
5. Записывать success после окна подтверждений.
6. Реализовать last-known-good full selection и exact rollback.
7. Не включать Zapret2 auto-selection до отдельного feature gate.

---

## Workstream 6 — lists и Strategy Pack delivery

### Задачи

1. Зафиксировать provenance/version каждого встроенного списка.
2. Разобраться с идентичными global/gaming ipsets.
3. Разделить встроенный и пользовательский слой.
4. Добавить manifest и hash verification обновлений.
5. Подготовить подпись Strategy Packs; до этого remote Lua disabled.
6. Заполнять региональный рейтинг только локальными проверенными результатами и встроенными данными.

---

## Контрольные точки

### Gate A — Phase 0 ready

- fmt/test/build проходят;
- release smoke пройден;
- нет orphan runtime;
- Phase 0 зафиксирован отдельно.

### Gate B — AI safety ready

- текущий Malw не меняется без действия;
- exact rollback протестирован;
- external modification защищён;
- UI остаётся в существующем AI-экране.

### Gate C — Telegram hardening ready

- LAN ограничен subnet/profile/time;
- Desktop proxy не зависит от LAN session;
- cleanup generation-safe.

### Gate D — Zapret2 Beta ready

- встроенный pack валиден;
- Zapret2 вручную запускается/останавливается;
- crash fallback возвращает Legacy;
- auto-selection выключен.

### Gate E — Adaptive ready

- диагнозы стабильны;
- bounded queues и pacing подтверждены;
- Brain rollback проверен;
- решение о расширении Beta принимается отдельно.

## Первый исполняемый шаг

Начать с Workstream 0:

1. Применить `cargo fmt`.
2. Повторить `cargo fmt -- --check`, `cargo test`, `npm run build`.
3. Составить traceability-таблицу «требование runtime spec → файл → тест → manual check».
4. Закрыть отсутствующие regression-тесты.
5. Выполнить короткий release smoke.
