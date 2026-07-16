# Adaptive Zapret2 Strategy Confidence and Gaming Prediction Design

**Дата:** 2026-07-16  
**Статус:** утверждённое направление; ожидает review владельца

## 1. Контекст

Live-проверка показала принципиальную проблему Gaming + GitHub adaptive search.
Roblox в текущей сети доступен без обхода, GitHub также работает штатно.
Положительный probe такого endpoint не доказывает эффективность candidate:
зелёным будет и рабочий desync, и бесполезный desync, и отсутствие desync.

Единственный обнаруженный Gaming HTTP/3 endpoint — `www.epicgames.com` —
не представляет всю категорию. В Balanced search восемь различных QUIC
кандидатов, включая `send:ipfrag`, корректно запустились, но Epic не ответил
ни на один. Такой результат допустим для Epic HTTP/3, но не должен
отображаться как «Gaming + GitHub: обход не найден».

Одновременно YouTube QUIC live-логи выявили отдельный измерительный дефект:
baseline проходил 3/3, candidate временно проходил один раунд, но новый DNS
lookup внутри каждого candidate/base recheck мог занять весь target budget.
Одиночный DNS timeout после успешного rollback ошибочно завершал сессию как
`base_unhealthy`.

## 2. Решение

Zapret2 получает явную модель доверия к стратегии:

1. `prepared` — профиль технически готов, но обход блокировки не доказан;
2. `recommended` — профиль выбран по подтверждённым данным этой сети, но не
   проверен на заблокированном endpoint своей категории;
3. `confirmed` — профиль реально прошёл recovery/verification для своей
   категории, транспорта и сети.

Доступность незаблокированного сайта не повышает доверие до `confirmed`.
Структурные тесты, `winws2 --dry-run`, успешный spawn и загрузка IPSet также
не являются доказательством обхода.

## 3. Цели

1. Перестать выдавать доступный Gaming/GitHub endpoint за проверку обхода.
2. Сохранить полезный Gaming + GitHub IPSet-профиль как технически готовый.
3. Использовать подтверждённые результаты Discord/YouTube как сетевой prior
   для рекомендаций Gaming того же транспорта.
4. Никогда не переносить подтверждение между транспортами или категориями.
5. Сохранить возможность будущего live recovery при появлении реальной
   блокировки.
6. Устранить ложные YouTube QUIC ошибки из-за повторного DNS lookup.
7. Изменять только Zapret2 runtime, adaptive subsystem и Zapret2 UI.

## 4. Не входит в работу

- внешняя общественная база и сервер телеметрии;
- утверждение, что подготовленный IPSet профиль уже обходит конкретную игру;
- автоматическая проверка proprietary game UDP без протокольного oracle;
- копирование TLS candidate в QUIC или наоборот;
- автоматическое применение рекомендации без действия пользователя;
- изменение Legacy UI, HeroCore, тем, Diagnostics, Brain или общей компоновки;
- внешний LLM/API.

Общественная база по провайдеру и региону остаётся возможным следующим этапом,
но текущая модель не должна зависеть от её наличия.

## 5. Модель доверия

### 5.1. Prepared

Статус `prepared` означает:

- Strategy Pack и manifest прошли проверку;
- бинарники, blobs, hostlist и IPSet имеют ожидаемую целостность;
- профиль компилируется в безопасный argv;
- `winws2 --dry-run` или эквивалентная grammar-проверка успешны;
- runtime способен запустить профиль;
- IPSet действительно загружен.

Для Gaming data plane это нормальный базовый статус. Он не является ошибкой и
не требует искусственного сетевого probe.

### 5.2. Recommended

Статус `recommended` означает, что candidate получен из подтверждённого
результата этой же сети и того же транспорта.

Допустимые источники:

- подтверждённый Discord TLS или YouTube TLS может рекомендовать Gaming TLS;
- подтверждённый YouTube QUIC может рекомендовать Gaming QUIC;
- Discord не является источником QUIC, поскольку Discord QUIC/media не входит
  в текущий adaptive scope.

При переносе меняется только категория и list scope. Safe Strategy steps,
transport и allowlisted arguments сохраняются, после чего candidate заново
проходит validator/compiler. Подтверждение исходной категории не переносится.

Рекомендация показывает источник доказательства текстом, например:

`Рекомендовано по YouTube QUIC в этой сети`.

Процент уверенности не рисуется: без внешней статистики он был бы выдуманным.

### 5.3. Confirmed

Статус `confirmed` возможен только после всех условий:

1. зафиксирована реальная неисправность baseline нужной категории/транспорта;
2. candidate отличается по effective fingerprint;
3. candidate проходит требуемое число probe rounds либо protocol-specific
   evidence;
4. нет Eyes reset/blackhole veto;
5. пользователь завершает manual verification;
6. cache write и exact rollback invariants соблюдены.

Ни Prepared, ни Recommended автоматически не переходят в Confirmed.

## 6. Network evidence и ranking

Recommendation engine работает детерминированно, без случайной генерации.

Приоритет источников:

1. Confirmed candidate точной пары category + transport + network.
2. Confirmed candidate другой категории той же сети и того же transport.
3. Builtin prepared profile.
4. Отсутствие рекомендации.

Если несколько same-transport источников расходятся, ranking учитывает:

- точное совпадение network key;
- количество успешных confirmation events;
- отсутствие последующих failure invalidations;
- свежесть подтверждения;
- валидность candidate для текущего Strategy Pack и list fingerprints.

ASN/region без общественной статистики не повышает статус до Recommended.
Он может отображаться в технических деталях, но не участвует как фиктивное
доказательство.

## 7. Поведение Gaming + GitHub

### 7.1. Data plane

`gaming_ipset_http`, `gaming_ipset_tls` и `gaming_ipset_udp` остаются
builtin profiles и получают статус Prepared после технической проверки.

Их UI-описание:

`IPSet-профиль подготовлен; эффективность на блокировке не проверялась`.

Доступность Roblox, GitHub или Epic не меняет этот статус.

### 7.2. Control plane

Если same-network evidence существует, launcher строит TLS/QUIC Recommended
candidate для Gaming и предлагает явное действие:

`Применить рекомендацию`.

Применение запускает профиль, но статус остаётся Recommended. Он не записывается
как confirmed только потому, что доступный endpoint открылся.

Если evidence отсутствует, UI оставляет builtin control profile Prepared и
пишет `Недостаточно данных этой сети для рекомендации`.

### 7.3. Active search

Обычная кнопка Gaming search не запускает comparison по доступному сайту.
Вместо неё доступны два сценария:

- `Подобрать рекомендацию` — мгновенный локальный ranking без сетевого
  перебора;
- `Восстановить после сбоя` — появляется только при достоверном passive или
  manual failure evidence.

Epic HTTP/3 остаётся optional diagnostic в «Технических деталях». Его провал
не является результатом всей категории Gaming + GitHub.

## 8. Хранение и миграция

Adaptive cache schema получает поле trust:

```text
prepared | recommended | confirmed
```

Правила миграции:

- существующие валидные Discord/YouTube cache entries получают `confirmed`;
- legacy Gaming entries не переносятся: старые доступные endpoints не доказывали
  recovery и после миграции вычисляются как Prepared;
- builtin profiles не записываются в adaptive cache и вычисляются как Prepared;
- cross-category derived candidate записывается только как `recommended`;
- APIs, называющиеся `confirmed_candidate_*`, обязаны фильтровать trust;
- failure history Recommended candidate не инвалидирует источник Confirmed;
- recommendation связывается с source candidate id/fingerprint, engine version,
  network key и list scope fingerprints.

Сброс рекомендации не удаляет подтверждённый source candidate.

## 9. DNS и probe reliability

Один search session использует общий bounded DNS cache для:

- discovery;
- baseline calibration;
- candidate rounds;
- rollback base recheck.

Cache живёт только в session и не переживает смену сети. Он хранит не более
одного IPv4 и одного IPv6 адреса на target.

Если comparison rollback восстановил process/generation, а первый base recheck
провалился только на DNS:

1. выполняется один повторный recheck с тем же session cache;
2. session не получает `base_unhealthy` после единственного DNS timeout;
3. повторный DNS failure завершает session как `probe_unreliable`, а не как
   доказательство поломки baseline.

Правило Balanced `2 из 3` для candidate не ослабляется.

## 10. Backend state

Zapret2 descriptors и adaptive status получают отдельные поля:

```text
trust
evidence_source
source_candidate_id
source_transport
recommendation_reason
```

Trust не выводится из цвета probe автоматически. Backend является единственным
источником состояния.

Переходы:

```text
builtin valid -> Prepared
same-network same-transport evidence -> Recommended
real failure + candidate success + manual confirm -> Confirmed
candidate failure -> previous trust preserved
list/engine scope change -> Confirmed/Recommended invalidated to Prepared
```

## 11. Zapret2 UI

Изменения ограничены компонентом Zapret2.

Для каждой transport/data строки показывается компактный badge:

- `Подготовлен`;
- `Рекомендован`;
- `Подтверждён`.

Основной экран не показывает candidate IDs, абсолютные пути, fingerprints и
probe JSON. Они остаются в «Технических деталях».

Gaming + GitHub не показывает «обход не найден», если отсутствовала проверяемая
блокировка. Корректные terminal сообщения:

- `Профиль подготовлен`;
- `Недостаточно данных для рекомендации`;
- `Рекомендация доступна`;
- `Реальная блокировка не обнаружена — подтверждение невозможно`.

## 12. Ошибки и безопасность

- Cross-transport recommendation запрещена validator/runtime.
- Recommended candidate проходит Safe Strategy validator как новый Gaming
  candidate; raw Lua/CLI не копируются.
- Нельзя повысить trust по одному успешному незаблокированному probe.
- Нельзя автоматически применять recommendation при старте приложения.
- Generation mismatch, spawn failure и rollback failure сохраняют существующие
  terminal safety rules.
- Network key и полные IP-адреса не выводятся в обычный UI/лог.
- DNS cache очищается при завершении session, смене generation или сети.

## 13. Тестирование

### Unit

- legacy Discord/YouTube cache entry мигрирует в Confirmed;
- legacy Gaming cache entry сбрасывается до Prepared;
- Prepared profile не появляется в confirmed lookup;
- Recommended entry не проходит confirmed lookup;
- TLS source не создаёт QUIC recommendation;
- YouTube QUIC source создаёт валидный Gaming QUIC recommendation;
- recommendation сохраняет Safe Strategy steps, но меняет category/scope;
- list fingerprint mismatch сбрасывает trust до Prepared;
- одиночный DNS base recheck failure выполняет retry;
- двойной DNS failure становится ProbeUnreliable;
- session DNS cache используется candidate и rollback;
- Discord TLS калибровка изолирует стабильно недоступный core target только
  когда другой core target выдержал полный quorum;
- отсутствие жизнеспособных Discord targets остаётся ProbeUnreliable;
- доступный Gaming endpoint не создаёт Confirmed.

### Integration

- Gaming без evidence сразу показывает Prepared без долгого search;
- same-network evidence показывает Recommended и source category;
- ручное применение recommendation не повышает trust;
- реальный recovery + confirm повышает только точную category/transport пару;
- reset recommendation не удаляет source confirmation;
- существующие Discord/YouTube confirmed overrides продолжают применяться;
- candidate и rollback Discord probes используют один session-scoped набор
  жизнеспособных targets;
- orphaned frontend transitioning после HMR сверяется с backend snapshot и не
  оставляет HeroCore в ложном busy-состоянии;
- UI вне Zapret2 не меняется.

### Live Windows acceptance

1. При доступных Roblox/GitHub Gaming показывает Prepared/Recommended, но не
   Confirmed.
2. Кнопка рекомендации не запускает восьмикандидатный Epic loop.
3. Recommended profile запускается и корректно откатывается вручную.
4. YouTube QUIC search не прерывается после одиночного DNS base recheck timeout.
5. При TCP timeout discord.com и стабильном gateway.discord.gg Discord search
   продолжает comparison и пишет исключённый target в технический лог.
6. Подпись режима находится над управлением, а Быстро, Баланс и Глубоко
   занимают один ряд из трёх равных колонок внутри Zapret2.
7. Discord, YouTube TLS и builtin Gaming IPSet не регрессируют.

## 14. Критерии готовности

Работа завершена, когда Obsession:

1. различает техническую готовность, прогноз и реальное подтверждение;
2. не использует доступный сайт как доказательство обхода;
3. строит Gaming recommendations только из same-network/same-transport evidence;
4. сохраняет Gaming IPSet как Prepared без ложного зелёного результата;
5. не завершает YouTube comparison как `base_unhealthy` после одного DNS
   timeout;
6. меняет только Zapret2 runtime/adaptive UI и проходит полный regression suite.

