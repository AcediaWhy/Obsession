# Adaptive Zapret2 Search Reliability Design

## 1. Статус и связь с исходным дизайном

Этот документ уточняет и исправляет уже реализованный Adaptive Zapret2 Strategy
Brain из
`2026-07-15-adaptive-zapret2-strategy-brain-design.md`. Исходные ограничения
Safe Strategy DSL, validator, ручное подтверждение, network cache и лимит в 12
кандидатов остаются в силе.

Исправление не добавляет LLM или облачный сервис. "Мнимый ИИ" остаётся локальным
детерминированным автотюнером, но теперь следующие кандидаты выбираются по
измеренным причинам провала предыдущей попытки.

## 2. Наблюдаемая проблема

Встроенный Strategy Pack `builtin.base 0.2.3` открывает YouTube и Discord без
перебора. Adaptive search при этом отклоняет все кандидаты и возвращает базу.
Live-журнал показал 12 запусков: 11 YouTube-кандидатов и один
Discord-кандидат. Первый YouTube-кандидат использовал тот же рабочий TLS desync,
что и база, `multidisorder_legacy:pos=1,midsld`, но всё равно был отклонён.

Причины текущего поведения:

- автоматический успех требует однократного прохождения каждой core-цели;
- `ProbeBatch` отправляется в UI, но не записывается полностью в `app.log`;
- QUIC-кандидаты оцениваются обычными TCP/HTTPS probes и не доказывают работу
  QUIC;
- Eyes запускается с Legacy, но не сопровождает обычный и adaptive Zapret2;
- генератор возвращает статический список seed-кандидатов и не использует
  evidence предыдущей попытки;
- кандидат, семантически совпадающий с рабочей базой, может попасть в поиск;
- после отката не проверяется, что база по-прежнему здорова.

Unit-тесты DSL и state machine проходят, но не моделируют эту live-ошибку.

## 3. Цель и критерии успеха

Цель: Adaptive search должен отличать сбой измерения от сбоя стратегии и
находить новую рабочую конфигурацию, семантически отличающуюся от текущей
рабочей базы.

Исправление принято, когда выполняются все условия:

1. База используется только для калибровки и rollback и никогда не выдаётся как
   найденный кандидат.
2. Эквивалентные базе или уже проверенным argv не расходуют попытки.
3. TLS-кандидат оценивается TLS/HTTPS evidence, QUIC-кандидат - реальным QUIC
   evidence.
4. Один случайный timeout не отклоняет рабочий кандидат.
5. Повторные Reset/Blackhole от Eyes отклоняют кандидат даже при частичном
   успехе HTTP.
6. Причина каждого решения видна в UI и полностью восстанавливается из
   `app.log`.
7. Провал кандидата возвращает точный исходный runtime и проверяет его
   здоровье.
8. В cache попадает только автоматически успешный и вручную подтверждённый
   кандидат.

## 4. Не цели

- создание или исполнение произвольного Lua;
- подбор категорий кроме YouTube/Twitch и Discord;
- обход DNS-блокировок изменением DPI-стратегии;
- фоновый бесконечный поиск;
- автоматическое сохранение без ручной проверки;
- превращение локальной эвристики в модель машинного обучения;
- изменение проверенного `builtin.base 0.2.3` в рамках этого исправления.

## 5. Архитектура сессии

Одна recovery-сессия обрабатывает одну категорию и не более 12 уникальных
кандидатов:

```text
Capture immutable baseline snapshot
  -> Calibrate baseline probes and Eyes
  -> Select transport search line
  -> Generate one distinct candidate
  -> Start candidate with Eyes
  -> Stabilize
  -> Run transport-specific probe rounds
  -> Analyze evidence
      -> pass: 60-second manual verification
          -> confirm: persist and keep candidate
          -> reject: exact rollback, base recheck, next candidate
          -> timeout/cancel: exact rollback and stop
      -> fail: exact rollback, base recheck, next candidate
      -> base unhealthy: stop search
      -> 12 attempts exhausted: stop on baseline
```

`BaselineSnapshot` содержит точный `DpiLaunchSpec`, generation, effective
профили обеих категорий, adaptive overrides и нормализованные fingerprints.
Снимок неизменяем в течение сессии. Следующий кандидат всегда строится от него,
а не от предыдущего кандидата.

Для YouTube существуют независимые TLS- и QUIC-линии. Диагноз определяет, какая
линия идёт первой; общий лимит обеих линий остаётся равен 12. Discord в MVP
использует TLS/WebSocket-линию. Переключение линии не позволяет оценивать QUIC
через рабочий базовый TLS-профиль.

## 6. Нормализация и исключение базы

До генерации backend компилирует effective профиль транспорта в каноническое
представление. Fingerprint учитывает:

- категорию и транспорт;
- функции и их нормализованные аргументы;
- payload/L7 filters;
- диапазон обработки и bounded transport options;
- fake blob по стабильному resource id и hash.

Fingerprint не зависит от абсолютного пути, порядка несемантических argv и
формата исходного JSON. Одновременно сохраняется канонический effective argv
для диагностики.

Генератор обязан исключить:

- fingerprint текущей baseline-конфигурации;
- fingerprints уже проверенных кандидатов;
- разные DSL, компилирующиеся в эквивалентный effective argv;
- невалидные или не относящиеся к выбранной категории/линии варианты.

Исключённый вариант не увеличивает счётчик попыток. Если уникальных мутаций
больше нет, сессия завершается `no_distinct_strategy_found`, а не сообщает об
успехе базы.

## 7. Калибровка и оценщик

### 7.1 Общая модель раунда

Каждый target возвращает не один boolean, а последовательный результат:

```text
transport
dns_ok
tcp_ok
tls_or_quic_ok
https_ok
http_status
reset_count
blackhole_count
latency_ms
failure_stage
detail
```

`failure_stage` принимает одно из значений: `dns`, `tcp`, `tls`, `quic`,
`https`, `eyes_reset`, `eyes_blackhole`, `spawn`, `stability` или `none`.
Текст `detail` предназначен для диагностики, но решение строится по типизированным
полям.

Раунд считается успешным, если все core-цели дошли до обязательной для линии
стадии. Optional-цели записываются, но не отклоняют кандидат. Для TLS обязательны
DNS, TCP, TLS и приемлемый HTTPS-ответ. Для QUIC обязательны DNS и валидный QUIC packet в ответ на QUIC Initial либо
успешный HTTP/3 обмен; обычный `reqwest` по TCP не может подтвердить
QUIC-кандидат. Приемлемым HTTPS-ответом считается завершённый TLS-сеанс и
HTTP status в диапазоне 200-499; redirect не требуется проходить до конечной
страницы.

### 7.2 Baseline calibration

Перед первым кандидатом исходная база проходит три раунда с интервалом 850 мс.
Калибровка успешна при двух успешных раундах из трёх и отсутствии
повторного Reset/Blackhole в окне Eyes.

Если калибровка не проходит, кандидаты не запускаются. Сессия завершается
`probe_unreliable`, сохраняя исходный runtime. DNS-провал также относится к
ненадёжной среде измерения: DPI-генератор не пытается "лечить" его стратегией.

### 7.3 Candidate evaluation

После запуска кандидат получает фиксированное окно стабилизации 1000 мс. Затем
выполняются три раунда с тем же интервалом 850 мс. Кандидат проходит
автоматическую оценку, если:

- успешны как минимум два раунда из трёх;
- обязательный transport evidence получен в каждом успешном раунде;
- Eyes не сформировал повторный Reset или Blackhole;
- процесс Zapret2 оставался жив и generation не был изменён внешней операцией.

Таким образом один transient timeout допустим. Два отрицательных Eyes-сигнала
одного типа либо итоговый Blackhole являются veto и отклоняют кандидат даже при
HTTP-успехе. Один необязательный target или один неуспешный HTTP-раунд не
перевешивает чистое transport evidence.

### 7.4 Проверка после rollback

После каждого неуспешного или вручную отклонённого кандидата coordinator
восстанавливает точный `BaselineSnapshot`, ждёт 1000 мс и выполняет два коротких
контрольных раунда. База считается восстановленной, если хотя бы один раунд
успешен и Eyes не сформировал повторный Reset/Blackhole.

Если оба раунда неуспешны или Eyes подтверждает проблему, поиск немедленно
завершается `base_unhealthy`. Это не считается провалом последнего кандидата:
условия сети изменились и сравнение больше недостоверно.

## 8. Eyes вместе с Zapret2

Lifecycle Eyes становится частью общего DPI runtime, а не только Legacy-ветки.
Обычный и adaptive запуск Zapret2 используют тот же helper владения Eyes:

- на один активный DPI runtime существует не более одного `EyesHandle`;
- Eyes стартует после успешного spawn Zapret2 и до начала стабилизации;
- stop, rollback, crash, cancel и shutdown останавливают старый Eyes вместе с
  соответствующим runtime;
- generation/session id не позволяют stale Eyes event повлиять на новую
  попытку;
- перед calibration и каждым candidate probe открывается новое evidence window,
  поэтому сигналы предыдущего процесса не загрязняют результат;
- capture filters включают только необходимые категории и TLS/QUIC traffic.

Сырые packet payload не сохраняются. Evaluator получает только агрегированные
Reset/Blackhole counters и типизированные причины.

## 9. Evidence-driven generator

Генератор больше не возвращает заранее подготовленный полный список. На каждом
шаге он получает baseline, выбранную transport line, множество fingerprints и
evidence последней попытки. Он создаёт следующий кандидат изменением одного
безопасного параметра относительно baseline. Это сохраняет объяснимость и
позволяет связать результат с конкретной мутацией.

Общий вход генератора:

```text
category
transport
baseline_candidate
baseline_fingerprint
tried_fingerprints
last_evidence
attempts_remaining
```

Правила выбора:

- `tls`/`eyes_reset`: менять split/disorder function, position или безопасные
  TCP sequence options;
- `eyes_blackhole`/`stability`: менять порядок desync steps, bounded repeats или
  диапазон обработки;
- `quic`: менять только QUIC Initial fake blob/repeats и QUIC-safe options;
- `https` после успешного handshake: сначала менять минимальный диапазон или
  repeats, не перескакивая сразу к агрессивным комбинациям;
- `dns`: прекратить генерацию и вернуть `probe_unreliable`;
- `spawn`: отклонить вариант как runtime error; после успешного base recheck
  выбрать другую валидную мутацию;
- отсутствие непробованной мутации в линии: перейти к другой применимой линии
  либо завершить `no_distinct_strategy_found`.

Known-good seeds могут использоваться как каталог разрешённых мутаций, но не
как неизменяемая очередь. Результат предыдущего кандидата обязан влиять на
следующий выбор. С одинаковым baseline и evidence порядок остаётся
детерминированным.

## 10. State machine и результаты

Существующая generation-safe state machine расширяется явными рабочими
стадиями `calibrating`, `stabilizing`, `probing`, `analyzing`, `base_recheck` и
`manual_verification`. Внешние команды Stop, смена DPI engine и shutdown имеют
приоритет и инвалидируют session generation.

Терминальные результаты:

- `applied`: кандидат прошёл автоматику и подтверждён пользователем;
- `probe_unreliable`: baseline нельзя достоверно измерить;
- `base_unhealthy`: ранее рабочая база перестала проходить контроль после
  rollback;
- `no_distinct_strategy_found`: 12 уникальных попыток исчерпаны либо уникальных
  безопасных мутаций больше нет;
- `cancelled`: пользователь отменил поиск, baseline восстановлен;
- `internal_error`: snapshot или rollback не удалось безопасно завершить.

`candidate_rejected` является результатом попытки, но не всей сессии. Причина
отклонения и `failure_stage` передаются следующему шагу генератора.

Rollback failure нельзя маскировать как `cancelled` или `exhausted`. Coordinator
останавливает дальнейший перебор и сообщает `internal_error`, потому что
инвариант last-known-good не подтверждён.

## 11. Ручная проверка и cache

Только автоматически успешный кандидат переходит в существующее 60-секундное
окно ручной проверки.

- `Работает - сохранить`: кандидат записывается для текущего network
  fingerprint и остаётся активным;
- `Не работает - продолжить поиск`: точный rollback, base recheck и следующая
  evidence-driven мутация;
- `Отмена и возврат` или timeout: rollback и завершение сессии;
- ошибка сохранения: rollback и терминальный `internal_error` с error code
  `persistence_failed`; кандидат не считается применённым.

Cache хранит нормализованный DSL подтверждённого транспорта и probe summary.
Baseline calibration, автоматически успешный, но не подтверждённый кандидат и
провальные попытки не создают confirmed cache entry.

## 12. UI и журналирование

`RecoveryStatus` должен позволять UI показать без интерпретации свободного
текста:

- категорию и transport line;
- номер уникальной попытки из 12;
- текущую стадию;
- candidate fingerprint/короткий id;
- последний `failure_stage` и итог попытки;
- прогресс probe rounds;
- таймер ручной проверки;
- результат rollback/base recheck.

Возврат к базе никогда не отображается как найденная стратегия. При
`no_distinct_strategy_found` UI прямо сообщает, что новая отличающаяся рабочая
конфигурация не найдена и активна исходная база.

Каждая сессия пишет в `app.log` структурированные adaptive events с
`session_id`, `attempt_id`, category, transport, stage и timestamp. Для каждого
target записываются timings, stage booleans, HTTP status, Eyes counters,
`failure_stage` и detail. Также записываются:

- baseline fingerprint и calibration summary;
- candidate fingerprint и канонические безопасные параметры;
- решение evaluator и причина;
- rollback generation и base recheck;
- ручное решение и результат persistence;
- терминальный результат сессии.

Лог не содержит packet payload, токены, открытый SSID и пользовательские
сообщения. Пути к bundled resources нормализуются до resource id.

## 13. Тестирование

### 13.1 Unit tests

- канонически эквивалентные DSL/argv имеют один fingerprint;
- baseline и ранее проверенные fingerprints исключаются без расхода attempt;
- generator меняет один параметр и выбирает мутацию по `failure_stage`;
- TLS и QUIC evidence не взаимозаменяемы;
- правило двух успешных раундов из трёх допускает один timeout;
- повторный Eyes Reset/Blackhole имеет veto;
- DNS failure не порождает бессмысленную DPI-мутацию;
- лимит считает только 12 реально запущенных уникальных кандидатов.

### 13.2 State-machine and integration tests

С fake process supervisor, fake Eyes и scripted probe runner проверяются:

- успешная calibration до первого candidate spawn;
- `probe_unreliable` без запуска кандидатов;
- Eyes start/stop вместе с обычным и adaptive Zapret2;
- очистка evidence window между попытками;
- TLS success с одним transient timeout;
- QUIC candidate не проходит по одному TCP/HTTPS успеху;
- candidate failure -> exact rollback -> successful base recheck -> next
  evidence-driven candidate;
- failed base recheck -> `base_unhealthy` и остановка;
- manual confirm/reject/timeout/cancel;
- persistence failure и rollback failure;
- stale session/attempt/generation events игнорируются;
- exhaustion возвращает baseline и `no_distinct_strategy_found`.

Обязательный regression fixture воспроизводит текущий live-сценарий: рабочий
`multidisorder_legacy:pos=1,midsld` проходит calibration, исключается как
baseline и не может быть ложно засчитан новой найденной стратегией. Следующая
мутация определяется evidence, а не позицией в статическом seed list.

### 13.3 Live acceptance

1. На `builtin.base 0.2.3` YouTube и Discord проходят baseline calibration.
2. В журнале видны все targets, stages и timings.
3. Для YouTube запускается уникальный TLS-кандидат; один искусственный timeout
   не отклоняет его при результате 2/3 и чистом Eyes.
4. QUIC-кандидат требует QUIC Initial/HTTP3 evidence.
5. Нерабочий кандидат возвращает точный baseline, который проходит base
   recheck.
6. Новый рабочий кандидат переходит в 60-секундную ручную проверку.
7. После подтверждения он сохраняется и применяется на той же сети.
8. После отклонения поиск продолжает другую evidence-driven мутацию.
9. После 12 провалов UI сообщает, что новая стратегия не найдена, а baseline
   остаётся активным.
10. Смена внешних условий, из-за которой baseline перестал работать, завершает
    поиск как `base_unhealthy`.

Live acceptance выполняется отдельно для YouTube TLS, YouTube QUIC и Discord.

## 14. Порядок реализации

1. Добавить typed probe evidence, полное журналирование и deterministic scripted
   evaluator tests.
2. Ввести baseline calibration, candidate rounds и base recheck.
3. Подключить Eyes к общему lifecycle Zapret2 и изолировать evidence windows.
4. Добавить настоящий QUIC probe и разделить transport evaluators.
5. Ввести canonical effective fingerprint и исключение baseline/equivalent
   argv.
6. Заменить статическую очередь на пошаговый evidence-driven generator.
7. Расширить state/status/UI терминальными причинами и прогрессом раундов.
8. Прогнать Rust/frontend tests и выполнить live acceptance под feature flag.

Каждый этап сохраняет exact rollback и не меняет обычную рабочую базу. До
прохождения live acceptance Adaptive Strategy Brain остаётся выключенным по
умолчанию.
