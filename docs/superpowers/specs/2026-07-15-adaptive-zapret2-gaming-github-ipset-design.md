# Adaptive Zapret2 Gaming + GitHub IP-set Design

**Дата:** 2026-07-15
**Статус:** реализовано; Windows live acceptance ожидает проверки владельцем

## 1. Контекст

Obsession уже запускает Zapret2 для категорий `discord` и
`youtube_twitch`, умеет подменять один транспорт подтверждённым adaptive
candidate и хранит результат отдельно для каждой сети. Категория `gaming`
пока остаётся Legacy-конфигурацией.

Legacy Gaming сочетает два разных механизма:

- доменные профили для HTTP, TLS и QUIC через `gaming.txt`;
- IP/CIDR-профили через `ipset-gaming.txt`;
- глобальный десинк UDP `1024-65535`, который не ограничен ipset.

Последний механизм имеет слишком широкий радиус воздействия. Пользователь
выбрал IP-set scoped вариант: высокие порты допустимы только для адресов из
`ipset-gaming.txt`, без глобального UDP-профиля.

`ipset` не меняет внешний IP и не направляет трафик через зарубежный сервер.
Он только ограничивает, к каким адресам Zapret2 применяет выбранный desync.
Текущий `ipset-gaming.txt` при этом байт-в-байт совпадает с широким
`ipset-global.txt` и содержит около 31 тысячи записей. Поэтому в этой итерации
он используется как совместимый существующий фильтр, но не называется узким
или исключительно игровым. Его сужение является отдельной работой с отдельной
live-проверкой.


GitHub сейчас находится в `atrisk.txt`, но новая пользовательская категория
должна восприниматься как `Gaming + GitHub`. При этом существующая категория
`atrisk` и её Legacy-поведение не удаляются.

## 2. Цели

1. Добавить в типизированный Zapret2 runtime нативную поддержку `--ipset`.
2. Добавить builtin-категорию `gaming` с отображаемым именем
   `Gaming + GitHub`.
3. Разделить web/control traffic и игровой data traffic, чтобы их можно было
   оценивать и развивать независимо.
4. Ограничить игровой high-port UDP обязательным `ipset-gaming.txt`.
5. Разрешить adaptive search только для наблюдаемого TLS/QUIC control plane.
6. Не объявлять произвольный игровой UDP рабочим только по результату HTTPS
   или QUIC-пробы.
7. Показывать в UI реальные активные Zapret2-профили вместо имён Legacy `.conf`.
8. Сохранять подтверждённый Gaming + GitHub candidate тем же безопасным
   per-network способом, который уже используется Discord и YouTube.

## 3. Не входит в эту итерацию

- VPN, proxy, подмена адреса или зарубежная маршрутизация;
- автоматическое доказательство работоспособности матчмейкинга, voice chat или
  произвольного игрового UDP;
- глобальный UDP `1024-65535` без ipset;
- автоматическое редактирование `ipset-gaming.txt`;
- удаление GitHub из `atrisk.txt`;
- обучение модели или использование внешнего LLM/API;
- адаптивная мутация raw Lua, CLI-строк или произвольных портов.

## 4. Выбранная архитектура

Категория `gaming` состоит из двух независимых частей.

### 4.1. Gaming Web/Control Plane

Control plane обслуживает сайты, авторизацию, launchers, storefronts, update
APIs, CDN и GitHub. Он использует доменный список и поддерживает два транспорта:

- TLS over TCP;
- QUIC over UDP/443.

Эти профили могут участвовать в adaptive search, потому что их результат можно
проверить активными HTTPS/QUIC-пробами и Eyes `ServerHello` evidence.

Для Zapret2 создаётся отдельный доменный список `gaming-github.txt`. Он
формируется как версионируемый bundled resource и содержит:

- включённые домены из текущего `gaming.txt`;
- GitHub: `github.com`, `githubusercontent.com`, `githubassets.com`,
  `github.io`, `ghcr.io`, `github.dev`, `githubcopilot.com`.

Отдельный файл нужен, чтобы не менять семантику существующего Legacy
`gaming.txt` и не удалять GitHub из `atrisk.txt`. Zapret2 manifest явно
привязывает control-plane стратегии к `gaming-github.txt`; runtime больше не
обязан выводить имя hostlist только из ключа категории.

### 4.2. Gaming Data Plane

Data plane использует только `ipset-gaming.txt` и фиксированные builtin
профили. Он не зависит от доменного списка и не подменяется adaptive candidate.

Разрешённые диапазоны первой итерации:

- TCP `80,443`, только с `--ipset=ipset-gaming.txt`;
- UDP `443,1024-65535`, только с `--ipset=ipset-gaming.txt`.

Профиль с high-port диапазоном считается невалидным, если у него нет ipset.
Это относится и к manifest validation, и к runtime defensive check перед
spawn. `wf-*-out` может захватывать широкий диапазон для работы WinDivert, но
desync-профиль обязан отфильтровать назначения через ipset.

В первой итерации data-plane recipe фиксирован в builtin Strategy Pack и
является прямым типизированным переносом базовой Legacy-идеи:

- HTTP/80: `payload=http_req`, fake HTTP + `multisplit`;
- TLS/443: `payload=tls_client_hello`, fake TLS + `multisplit`;
- UDP/443 и `1024-65535`: fake QUIC blob, `payload=all`, ограничение первыми
  пятью исходящими data packets.

Конкретные параметры fake используют только bundled/default blobs и допустимые
Zapret2 Lua arguments. Профиль сначала обязан пройти `winws2 --dry-run`, затем
ручную live-проверку. Его нельзя считать подтверждённым только по структурным
тестам. Adaptive runtime не записывает data-plane recipe в cache как найденную
стратегию.

## 5. Типизированная модель профиля

`Zapret2Profile` получает поле:

```rust
pub ipset: Option<String>
```

`build_winws2_args` сериализует его как `--ipset=<absolute-path>` после
`--hostlist` и до range/payload/desync аргументов.

`StrategyDef` получает безопасные ссылки на пользовательские/bundled списки:

```rust
pub hostlist: Option<String>
pub ipset: Option<String>
```

Значения являются только именами файлов внутри runtime `lists` directory, без
абсолютного пути, `..`, обратных слешей и управляющих символов. Manifest
validator проверяет расширение `.txt` и допустимый basename. Runtime разрешает
имя относительно `Paths::lists_dir()`, канонизирует существующий файл и не
запускает профиль при ошибке разрешения.

Для старых стратегий оба поля необязательны. Если `hostlist` не указан,
сохраняется текущий fallback `<category>.txt`. Это обеспечивает обратную
совместимость Discord и YouTube. Gaming manifest всегда задаёт ссылки явно.

`profile_from_strategy_def` принимает уже разрешённые абсолютные `hostlist` и
`ipset`. Он не обращается к файловой системе и остаётся чистым builder helper.

## 6. Builtin Strategy Pack

Builtin pack повышает minor-версию и добавляет категорию `gaming`.

Минимальный набор профилей:

1. `gaming_control_tls`:
   TCP/443, L7 TLS, payload `tls_client_hello`, hostlist
   `gaming-github.txt`, мягкий проверенный builtin recipe.
2. `gaming_control_quic`:
   UDP/443, L7 QUIC, payload `quic_initial`, hostlist
   `gaming-github.txt`, мягкий проверенный builtin recipe.
3. `gaming_ipset_http`:
   TCP/80, L7 HTTP, ipset `ipset-gaming.txt`, фиксированный fake + multisplit,
   без hostlist.
4. `gaming_ipset_tls`:
   TCP/443, L7 TLS, ipset `ipset-gaming.txt`, фиксированный fake + multisplit,
   без hostlist.
5. `gaming_ipset_udp`:
   UDP `443,1024-65535`, ipset `ipset-gaming.txt`, фиксированный recipe,
   без hostlist.

Порядок является частью контракта: оба `gaming_control_*` профиля идут раньше
ipset fallback. Поэтому известный hostname на TCP/UDP 443 попадает в control
profile и может использовать adaptive override; ipset profiles обрабатывают
оставшийся трафик. Тесты фиксируют этот порядок и effective argv.


Manifest strategy schema получает явные `filter_tcp` и `filter_udp`, потому
что текущая логика жёстко выводит порт `443` из транспорта и не может выразить
ipset-scoped high-port профиль. Поля `transports` сохраняются для определения
транспорта и совместимости, но конкретный filter range имеет приоритет.

Validator принимает только цифры, запятые, дефисы и корректные порты
`1..=65535`. Для `1024-65535` требуется `ipset`. Неизвестные/пустые диапазоны
отклоняются до запуска.

## 7. Adaptive category и кандидаты

Safe Strategy DSL получает `AdaptiveCategory::Gaming` с cache key `gaming`.
Candidate продолжает описывать только один транспорт: TLS или QUIC.

Adaptive override для `gaming` заменяет только соответствующий control-plane
профиль:

- TLS candidate заменяет `gaming_control_tls`;
- QUIC candidate заменяет `gaming_control_quic`;
- `gaming_ipset_http`, `gaming_ipset_tls` и `gaming_ipset_udp` всегда остаются
  builtin.

Compiler получает разрешённый `gaming-github.txt` и никогда не добавляет
ipset к adaptive profile. Таким образом candidate не может случайно расширить
область воздействия на игровые IP или high ports.

Runtime и cache переходят от одного candidate на категорию к одному candidate
на пару `(category, transport)`. Это позволяет одновременно держать
подтверждённые Gaming TLS и QUIC overrides. Для Discord и YouTube старые записи
мигрируют в transport, указанный внутри сохранённого candidate.

Generator использует общую безопасную лестницу TLS/QUIC и исключает точные
builtin baseline fingerprints так же, как для Discord и YouTube. Для `gaming`
будут доступны те же пользовательские режимы поиска:

- `balanced` по умолчанию;
- `fast` как сокращённая вариация;
- `deep` как расширенная вариация.

Конкретные бюджеты, таймауты и negative-cache policy определяются отдельным
implementation plan поиска; эта спецификация фиксирует, что режим не может
менять data-plane профили или ослаблять ipset-инвариант.

## 8. Проверка результата

Control-plane probe set зависит от проверяемого транспорта:

- TLS core: `github.com` и `www.roblox.com`; `api.github.com` и
  `www.epicgames.com` остаются optional diagnostics;
- QUIC core: `www.roblox.com` и `www.epicgames.com`; GitHub endpoints остаются
  optional, потому что GitHub не гарантирует HTTP/3 и не должен ложно отклонять
  рабочую QUIC-стратегию.

HTTP-код `4xx` сам по себе не является TLS failure, если соединение, TLS и
ответ сервера получены. DNS/transport ошибки классифицируются существующим
evidence pipeline, а Eyes `ServerHello` учитывается только для нужного домена и
текущего evidence window.

Candidate подтверждается только если:

1. baseline текущей категории был измерен;
2. прошли обязательные core probes выбранного транспорта;
3. нет hard TLS/QUIC failure;
4. candidate выдержал confirmation round выбранного search mode;
5. session generation и PID Zapret2 не были изменены внешним действием.

Положительный control-plane результат не означает, что игровые high-port
соединения проверены. UI и логи должны явно различать:

- `Control plane: verified`;
- `Data plane: builtin, not actively verified`.

## 9. Runtime и rollback

Сборка invocation происходит в таком порядке:

1. загрузить и проверить Strategy Pack;
2. разрешить manifest list references внутри `lists_dir`;
3. проверить наличие и тип содержимого hostlist/ipset;
4. применить confirmed adaptive override только к control transport;
5. добавить неизменённые builtin data profiles;
6. вычислить объединённые `wf_tcp_out` и `wf_udp_out` из effective filters;
7. запустить один `winws2` process с полными профилями и разделителями `--new`.

Если hostlist или ipset отсутствует/невалиден, Gaming + GitHub не запускается
частично: runtime возвращает понятную ошибку, оставляет предыдущую рабочую
сессию или выполняет существующий generation-safe rollback. Запрещено молча
запустить high-port профиль без ipset.

При провале adaptive candidate runtime восстанавливает точный builtin
control-plane baseline; data-plane профили остаются теми же. Cache не
перезаписывается провальным candidate.

## 10. Хранение результатов

Подтверждённые Gaming control-plane candidates сохраняются в существующий:

`%APPDATA%/Obsession/adaptive-strategies.json`

Ключ остаётся составным: network key + category + transport. Для категории
используется `gaming`. В cache хранится только Safe Strategy DSL candidate и
evidence metadata, но не копия Lua, hostlist или ipset.

IP-set и доменные списки остаются ресурсами приложения. Их изменение не должно
превращать старое подтверждение в доказательство новой области трафика. Cache
entry поэтому дополнительно связывается с fingerprint эффективных list files;
при несовпадении fingerprint candidate требует повторной проверки.

## 11. Отображение в UI

Экран Zapret2 перестаёт показывать Legacy `.conf` как активную стратегию.
Вместо этого backend возвращает effective profile descriptors, а UI выводит:

- category и profile id;
- source: `builtin` или `adaptive`;
- transport и port filter;
- scope: hostlist, ipset или оба;
- adaptive candidate ID;
- control-plane verification status;
- data-plane status `builtin / not actively verified`.

Для Gaming пользователь видит отдельные строки control и data plane. Путь к
файлу можно показывать сокращённо до basename; абсолютный путь остаётся в
диагностике/логах. Legacy selector сохраняется только для Legacy engine.

## 12. Ошибки и безопасность

- Manifest не может ссылаться на файл вне `lists_dir`.
- Adaptive DSL не получает raw `ipset`, filter range или Lua.
- High-port profile без ipset отклоняется validator и runtime.
- Пустой/невалидный `ipset-gaming.txt` блокирует Gaming data plane.
- Дубликат GitHub в Legacy `atrisk` не удаляется автоматически.
- Если одновременно выбраны `gaming` и `atrisk`, Zapret2 de-duplicator не
  создаёт два одинаковых effective hostlist profile для одного транспорта.
- Любая manual verification/search session сериализуется общим adaptive gate;
  параллельный поиск не запускается.
- Логи не записывают содержимое списков или сетевые идентификаторы целиком.

## 13. Тестирование

### Unit

- `Zapret2Profile` сериализует `--ipset` в правильном месте;
- multi-profile `--new` grammar не регрессирует;
- manifest list path traversal и абсолютные пути отклоняются;
- explicit port filters валидируются;
- любой high-port профиль без ipset отклоняется;
- `AdaptiveCategory::Gaming` стабильно сериализуется как `gaming`;
- Gaming adaptive override заменяет только control transport;
- list fingerprint invalidates stale Gaming cache entry;
- effective profile descriptors отражают builtin/adaptive source.

### Integration

- invocation содержит пять Gaming-профилей и один процесс winws2;
- UDP high ports присутствуют только у ipset-scoped profile;
- отсутствие `ipset-gaming.txt` даёт контролируемую ошибку без partial spawn;
- подтверждённый Gaming TLS candidate переживает restart в той же сети;
- смена сети или list fingerprint не применяет stale candidate;
- одновременный выбор Gaming и At Risk не дублирует GitHub control profile;
- Discord и YouTube invocation/cache остаются совместимыми.

### Live acceptance на Windows

- GitHub web/API открываются через Gaming + GitHub;
- минимум два игровых launcher/web endpoints проходят control probes;
- минимум одна выбранная пользователем игра проходит login/matchmaking;
- при отключении adaptive используется рабочий builtin control baseline;
- логи показывают adaptive ID только для control plane;
- UI показывает реальные Zapret2 profile ids и scope;
- packet/log inspection подтверждает отсутствие desync high-port трафика к
  адресам вне `ipset-gaming.txt`.

## 14. Порядок реализации
- UI предупреждает, что текущий `ipset-gaming.txt` является широким набором и
  не подменяет IP/регион пользователя.

1. Расширить manifest и `Zapret2Profile` полями list bindings и filters.
2. Добавить resolver, path validation и high-port/ipset invariant.
3. Добавить `gaming-github.txt` и Gaming builtin profiles.
4. Добавить `AdaptiveCategory::Gaming`, generator/probes/cache fingerprint.
5. Научить runtime заменять только control profiles.
6. Добавить effective profile descriptors и исправить Zapret2 UI.
7. Выполнить unit/integration suite.
8. Провести Windows live acceptance для GitHub, control endpoints и игры.

## 15. Критерии готовности

Работа завершена, когда Gaming + GitHub запускается в Zapret2 одним процессом,
все high-port профили ограничены `ipset-gaming.txt`, GitHub и gaming control
plane могут получить подтверждённый per-network adaptive candidate, data plane
остаётся фиксированным и честно помечен как не проверенный активными probes, а
UI показывает фактические builtin/adaptive Zapret2 profiles без имён Legacy
`.conf`.
