# Диагностика обработки Zapret2

Для проверки случая из `DISCORD_FAILURE_2026-10-01.md` используется кнопка
**«Проверить подбор с нуля»** в Discord → TLS. Пользователь отключает VPN и
параллельные программы обхода, запускает один тест и дожидается восстановления
исходной конфигурации. Отчёт сохраняется в обычный каталог `diagnostics`.
Установка обновления нужна для синхронного обновления приложения и службы.

Диагностика включается только для контрольного и пробных запусков этого теста.
Обычный запуск DPI и обычный автоматический подбор не включают наблюдатель.
Штатные Lua-файлы остаются прежними; последний дополнительный файл оборачивает
функции стратегий и нативные `rawsend` / `rawsend_dissect`. Перед цепочкой функций
профиля выполняется наблюдатель с `VERDICT_PASS`. Последний профиль наблюдает
захваченный трафик, не совпавший с выбранными профилями, и возвращает PASS.

В событии `engine_diagnostics` находятся точная generation, PID, код завершения,
метаданные stdout/stderr и сводка:

| Наблюдение | Что подтверждается |
| --- | --- |
| `observer_ready` | Lua-наблюдатель загрузился в этом процессе |
| `target_profile_events` | Выполнялся профиль требуемой категории |
| `target_function_calls` | Вызвана функция стратегии этой категории |
| `target_send_api_ok` | Нативный API отправки вернул успех |
| `target_send_api_failed` | API вернул отказ или выбросил ошибку |
| `lua_errors` | Перехвачена и повторно выброшена ошибка Lua |
| `unmatched_client_hello_events` | Захваченный ClientHello не совпал с выбранными профилями |

Успех API отправки **не доказывает** выход пакета на физический интерфейс,
доставку серверу или восстановление HTTPS. Ошибки/ответы HTTP продолжают
измеряться независимыми штатными пробами. Активность YouTube или игрового
профиля не засчитывается как обработка Discord. Отсутствие событий означает
отсутствие наблюдения, а не окончательный диагноз неисправного WinDivert.

Наблюдение ограничено 120 секундами и 512 событиями Lua на процесс.
Служба сохраняет максимум 128 строк и 16 КиБ метаданных в памяти; пакеты и
блобы не дампятся, подробный debug движка не включается. Вывод читается и
отбрасывается после достижения лимита, чтобы процесс не заблокировался на pipe.
Переполнение и закрытие потока отражаются явно.

Дочерний процесс наследует только pipe вывода и NUL ввода через
`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`; служебные handles не наследуются.
Чтение метаданных требует точной активной generation через типизированный IPC.
Пути, команды и дескрипторы клиент не передаёт. Наблюдатель входит в список
защищённых ресурсов и проверяется по SHA-256 перед запуском.

Проверки без изменения сетевого состояния:

- `npm run test:engine-diagnostics`: проверка Lua с наборами API Lua 5.1 и
  Lua 5.3; передача аргументов, nil/несколько результатов, исходные ошибки,
  отказ журналирования и лимиты. `winws2 --dry-run` отдельно проверяет CLI.
- `scripts/test-engine-diagnostics-luajit.py`: тот же контракт в настоящем
  LuaJIT 2.1 через временную тестовую зависимость `lupa==2.6`. Для запуска
  директория тестовой установки передаётся через `PYTHONPATH`; в приложение
  или установщик Python/Lupa не входят.
- Rust-тесты: ограничения буфера и IPC, непрерывное чтение заполненного pipe,
  generation-fence, сохранение исходных аргументов стратегии, контроль хеша Lua.
- Изолированный parser-only дочерний процесс проверяет наследование вывода и
  закрытие потока. Никакой драйвер или захват пакетов этим тестом не запускается.
- Проверка bundle компилирует диагностические adaptive-конфиги и передаёт их
  настоящему парсеру движка. `--dry-run` не выполняет Lua-init; это не живая
  проверка обхода. Поведение в реальном процессе проверяется следующим тестом
  пользователя.

Источники для нативного API и Lua-init:
[закреплённое руководство](https://github.com/bol-van/zapret2/blob/v1.0.5.2/docs/manual.md),
[реализация Lua API](https://github.com/bol-van/zapret2/blob/v1.0.5.2/nfq2/lua.c),
[Windows Lua-init](https://github.com/bol-van/zapret2/blob/v1.0.5.2/nfq2/nfqws.c),
[table API закреплённого LuaJIT](https://github.com/openresty/luajit2/blob/v2.1-20250826/src/lib_table.c).

Регрессия выпуска 01.10.2026, 23:40 (Самара):

Два теста в 23:43 и 23:44 завершились ещё на контрольной замене:
`ExitedBeforeReady { exit_code: 87 }`. API, сайт и gateway Discord прошли
короткую проверку до замены и после автоматического восстановления исходного
конфига. Поиск кандидатов в этих двух тестах не запускался.

Наблюдатель ошибочно использовал `table.pack` и `table.unpack`. Движок собран
с LuaJIT 2.1 / Lua 5.1 без `LUAJIT_ENABLE_LUA52COMPAT`; эти API отсутствуют.
Старый наблюдатель воспроизвёл ошибку `attempt to call field 'pack' (a nil value)`
в настоящем LuaJIT. Windows-ветка движка возвращает 87 при отказе Lua-init.
Ранее тест использовал Lua 5.3, а `--dry-run` пропускал выполнение Lua-init.
Вывод раннего завершения терялся при очистке поколения, поэтому в старых
отчётах нет исходного сообщения Lua; связь с живым запуском следует из
воспроизведённой ошибки и порядка инициализации движка.

Исправление от 02.10.2026:

- Локальный `pack` на `select("#", ...)` и совместимый `unpack` сохраняют nil
  и несколько результатов. Исправленный наблюдатель прошёл контракт на
  настоящем LuaJIT и оба набора API в тестовой VM.
- При раннем завершении точного дочернего процесса служба даёт pipe до 150 мс
  на дочитывание и возвращает ограниченную копию через `StartupFailed`.
  Метаданные сохраняются и при автоматическом восстановлении другого поколения.
- IPC версии 6 проверяет PID, код завершения, лимиты строк/байтов и отсутствие
  управляющих символов. Служба и приложение обновляются вместе.
- Полные метаданные записываются в `adaptive_log.data.structured` с
  `event=engine_startup_failure` и в app.log. Интерфейс показывает только код
  и первые 240 символов одной строки ошибки.
- Хеш Lua-наблюдателя: `d9f7e96db2b83a1bb8103e3530d65c41f375fe52795799750f18be9f6947a37a`.
- Протокол: 31 тест; служба: 195 обычных тестов и 7 named-pipe тестов;
  18 тестов проекции runtime. Отдельный parser-only ребёнок подтвердил
  копирование вывода раннего завершения перед очисткой.
- Исправление самого запуска не подтверждает работу подбора на живой сети.

Проверенный артефакт от 02.10.2026, 00:09:59 (Самара):

- `dist-release/Obsession-Setup_1.1.0_x64-diagnostics-fix.exe`, 30646784 байт.
- SHA-256: `bf772c21815b589c22a4a08277a084f9644c50a1f5999f9cbfe43374234b031c`.
- Приложение: `3a6d7974044fa9421f62905bdc2f48bd8f109385562e281d43ca85032769446c`.
- Служба: `9a4330a2304213cb0c05586955a125fa5d691b169e0205608d45b54c340291d6`.
- Все 93 файла payload совпали с актуальными бинарниками и ресурсами; точный
  compressed payload найден внутри Setup. Подпись update.json проверена
  закреплённым ключом. Файл с отдельным именем побайтово совпадает со стандартным
  `Obsession-Setup_1.1.0_x64.exe`, указанным в подписанном update.json.
- Версия приложения остаётся 1.1.0, IPC изменён на 6.

Для живой проверки требуется установка исправленного файла: закрыть Obsession,
установить обновление, выбрать Discord / TLS / «Глубоко», отключить VPN и
параллельные программы обхода, отметить готовность и нажать
«Проверить подбор с нуля». Дождаться завершения и восстановления исходного
конфига. Результаты двух живых запусков этой сборки теперь разобраны в
[DISCORD_DIAGNOSTICS_2026-10-02.md](DISCORD_DIAGNOSTICS_2026-10-02.md):
Lua-init исправлен; ночью новый вариант прошёл измеряемые цели, днём 11
кандидатов не подтвердились. Полная работа клиента этими проверками не измерена.
# Protected packet observation for Zapret2 diagnostic runs

The production Zapret2 launch does not start the old application-owned Eyes
event stream. Its zero counters must not be interpreted as measured zeros.
The Lua observer and the passive packet sensor are separate mechanisms.

`GetDpiDiagnostics` now optionally returns `packetCapture`, fenced to the
requested runtime generation. Only `zapret2Diagnostics: true` starts the
sensor. `active` means a receive-only WinDivert handle opened; health counters
and exact flow correlation are still needed before interpreting its data.
Startup failure, unavailable DLL, pending teardown, expiry and engine exit
are explicit states. This feature does not enable Legacy recovery controls.

Supplementary TCP/TLS measurements include `localAddress`. Report field
`packet_probe_correlation` matches both socket addresses, distinguishing ACK
progress, observed TLS records and RST while preserving capture gaps and
overlap conflicts. These sockets are separate from reqwest's HTTPS requests.
The sensor uses network-layer priority 0; processing order relative to winws2
and physical-wire delivery are unverified. Payload stays in bounded transient
RAM; packets, SNI and absolute TCP sequence/ACK values are never reported.

`eyes_event_stream_connected: false` remains explicit. Packet observations
are diagnostic receipts, not a substitute for repeated HTTPS success.
