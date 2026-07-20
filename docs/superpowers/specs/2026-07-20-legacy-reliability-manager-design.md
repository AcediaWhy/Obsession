# Legacy Reliability Manager

**Дата:** 2026-07-20
**Статус:** дизайн утверждён владельцем проекта; реализация начинается поэтапно
**Область:** Legacy engine в ObsessionTauri

## 1. Цель и границы

Цель первой версии — сделать автоматическое восстановление Legacy предсказуемым
и безопасным для уже работающих категорий. Система должна отличать проблему
конкретной цели от отсутствия интернета, ошибки DNS, деградации upstream и
слепоты сенсора. Каждый Legacy `.conf` остаётся самостоятельным файлом: конфиги
не объединяются и не пересобираются в общий процесс.

Zapret2 остаётся отдельным контуром и в эту работу не входит. Существующие
соединения намеренно не закрываются; короткое окно переключения допускается
только для новых подключений. В первой версии не выполняются автоматическое
восстановление UDP/QUIC, внешняя telemetry, получение новых команд winws из сети
и объединение конфигов.

## 2. Проблема текущего контура

Сейчас сырые события Eyes напрямую попадают в Legacy и Adaptive Brain, а
отдельного живого Manager нет. В результате отсутствуют единые session/generation
границы, health и gap-сигналы Eyes, per-category корреляция, согласованный
target mapping и точечный lifecycle одного `winws`. `start_many` предварительно
останавливает весь набор, поэтому изменение одной категории может затронуть
рабочие категории.

Целевая миграция не является rewrite: сначала вводятся контракты и fences, затем
корреляция и Environment Gate, и только после этого меняется executor. На каждом
этапе сохраняется текущий ручной запуск Legacy.

## 3. Целевая архитектура

```text
Legacy winws processes
        |
        v
Eyes (passive sensor)
        | EyeEvent::Flow/Health/Gap
        v
Reliability Manager
  session + generation + TargetRegistry + evidence + Environment Gate
        | Assessment
        v
Legacy Brain (pure policy/state machine)
        | DecisionIntent
        v
Legacy Executor
  preflight -> scoped stop -> start -> readiness -> confirm -> rollback
```

Eyes только наблюдает и не принимает решений о замене. Manager владеет
контекстом сессии, attribution, качеством сенсора и окнами evidence. Brain
получает типизированные assessment и возвращает intents; он не запускает
процессы и не пишет файлы. Executor является единственным владельцем stop/start
и проверяет generation перед каждым побочным эффектом. UI, tray, тесты и Brain
используют единый session coordinator. Zapret2 не подписывается на Legacy
Manager и не получает его intents.

## 4. Контракты данных

### 4.1 Session context

Каждый запуск Legacy создаёт неизменяемый `LegacySessionContext`:

- `session_id` — уникальный идентификатор жизненного цикла;
- `engine = Legacy`;
- `active_categories`;
- `network_fingerprint_at_start`.

Изменяемые epochs хранятся отдельно:

- `lane_generation[category]` увеличивается только при start/restart процесса
  этой категории;
- `sensor_generation` увеличивается при каждом restart Eyes;
- `target_registry_version` детерминированно меняется при смене snapshot или
  active selection и сравнивается только на точное равенство.

Повышение generation одной lane не инвалидирует события рабочих категорий.
Flow получает category и текущую `lane_generation` при создании потока, а не в
момент вынесения verdict. Поэтому поздний `Working` от предыдущего процесса не
может подтвердить нового кандидата.

Любое событие или результат операции, пришедшие от старой session/generation,
отбрасываются до изменения model, UI или cache. SessionStop закрывает контекст
до начала teardown, поэтому поздние события не могут воскресить старое состояние.

### 4.2 Eye events

Каждое событие содержит envelope с `session_id`, `sensor_generation` и
`target_registry_version`. Внутренний bounded nonblocking канал передаёт:

```text
Flow {
  envelope, category, lane_generation, flow_id, domain, destination_ip,
  transport, diagnosis, evidence, monotonic_ts
}
Health {
  envelope,
  Ready | Degraded | Blind | Stopped,
  packet_count, parse_errors, queue_drops, last_event_ts
}
Gap { envelope, from_ts, to_ts, dropped_events }
```

Callback Eyes не пишет синхронно на диск, не эмитит тяжёлые UI-операции и не
делает fanout в несколько runtime. Flow queue имеет capacity 1024 и при
переполнении отбрасывает новое событие. Atomic drop counter и out-of-band dirty
flag сохраняются даже при полной очереди; отдельный control channel capacity 64
доставляет Health/Gap, а dirty flag очищается только после принятого Gap.
Пересекающий evidence/confirmation window `Gap`, `Degraded` или `Blind`
запрещает вывод `Healthy`. Вернуться к оценке можно после десятисекундного
чистого окна без роста drop/error counters.
Неожиданный отказ capture/tracker является terminal `Blind` для текущего
`sensor_generation`; восстановление требует нового поколения Eyes.

Публичное событие для UI может быть проекцией Flow, но policy использует только
полный внутренний контракт. В Legacy v1 automatic evidence ограничен TLS over
TCP. TCP/80 входит в capture plan только ради packet/parse/drop health counters:
обычный HTTP/80 не создаёт policy Flow, `Working` или failure evidence. Событие
`syn_no_synack`, полученное через эвристику `IP -> last domain`, также остаётся
unattributed diagnostic (`category = lane_generation = None`) до появления
надёжной DNS/socket correlation. UDP/QUIC сохраняются как typed `out_of_scope`
события до отдельного этапа.

### 4.3 TargetRegistry

TargetRegistry — единственный источник attribution. В начале сессии он строится
из активных конфигов и всех eligible bundled candidates активных категорий и
содержит версию, category/config ownership и transport plan. Сопоставление
домена выполняется longest-suffix с границей label; неоднозначные домены
исключаются из automatic switching и остаются видимыми для диагностики.

Registry хранит bounded union метаданных candidates, но production capture plan
строится только из портов и hostlists текущих active selections. Union всех
candidates используется для attribution/preflight и никогда напрямую не
передаётся WinDivert. Фильтр direction-aware: outbound сверяется только с
удалённым `DstPort`, inbound — только с удалённым `SrcPort`; локальный ephemeral
port не расширяет capture. TCP/80 влияет только на health counters и не поступает
в policy. Обычные runtime Flow атрибутируются только active owners; более
глубокий suffix неактивного candidate не может перехватить lane. Полный
candidate attribution доступен лишь диагностике и fenced preflight.

В будущем scoped switch candidate добавляется в отдельный fenced sensor plan до
остановки его lane. Такая переинициализация пассивного Eyes повышает
`sensor_generation`, создаёт явный Gap и не останавливает DPI-процессы или
соединения рабочих категорий. Candidate становится active owner только после
успешного start/confirm, с новой registry version и generation его lane. Если
candidate отсутствует в snapshot или его content hash изменился, попытка
отменяется до stop и требуется новая сессия. В Legacy v1 UDP/443 и QUIC не
вооружают automatic recovery. Один и тот же registry используется Eyes, Manager
и Brain; локальные hardcoded suffix maps запрещены.

## 5. Reliability policy

Каждая категория имеет отдельную lane:

```text
Unknown -> Observing -> Healthy -> Suspect
                    -> Switching -> Confirming -> Healthy

BlockedCooldown | SensorUnreliable | ProcessFailed | Exhausted
```

`Healthy` означает одновременно живой процесс, Eyes не ниже `Ready` и quorum
активных probe/passive evidence. Истечение grace без `Working` не является
здоровьем. Один reset только переводит lane в `Suspect`; переключение возможно
после трёх reset от двух независимых целей в 30 секунд. Независимыми считаются
разные registry targets, а не повторные события одного flow. Два подтверждённых
атрибутированных TLS-level blackhole от разных flow и registry targets без
пересекающего gap переводят категорию в cooldown на 300 секунд. Диагностический
`syn_no_synack` в этот quorum не входит. После cooldown только новый здоровый
Environment Gate разрешает ровно одну candidate attempt. Отсутствие трафика само
по себе ничего не доказывает.

Crash процесса сначала вызывает bounded retry того же конфига. Один recovery
cycle проверяет ровно одного кандидата. Между автоматическими заменами минимум
30 секунд. Cache записывается только после успешного `Confirming`; проваленный
кандидат получает negative cooldown.

Brain возвращает только следующие intents:

```text
Wait
SwitchLane(category, candidate, reason)
FreezeLane(category, until, reason)
RetrySameConfig(category)
Rollback(category, previous_config)
```

Каждый intent обёрнут в `IntentEnvelope { session_id, attempt_id, category,
expected_lane_generation, expected_sensor_generation,
expected_registry_version, expected_network_fingerprint }`. Executor повторяет
fence непосредственно перед stop, start, commit и cache write. Результаты
исполнения возвращаются как `ExecutorResult` с тем же envelope. Process supervisor
эмитит `Ready`, `Exited { intentional }`, `StopTimedOut`, `StartFailed` и
`RollbackFailed`, привязанные к PID, process start identity, config fingerprint и
lane generation; одного PID без start identity недостаточно из-за его повторного
использования Windows.

## 6. Environment Gate

Перед автоматическим switch Manager проверяет локальный интерфейс, route,
gateway и network fingerprint; затем параллельно проверяет bundled versioned
control set с quorum 2/3:

- `https://cp.cloudflare.com/generate_204`;
- `https://www.gstatic.com/generate_204`;
- `https://www.msftconnecttest.com/connecttest.txt`.

На endpoint действует timeout пять секунд, общий deadline Gate — шесть секунд.
Затем проверяются несколько целей текущей категории, состояние Eyes и наличие
gap. Baseline — rolling median успешных control probes этой stable network с TTL
24 часа. Без baseline latency не классифицируется как `ServiceSlow` или DPI.
Gate result живёт не более десяти секунд; network fingerprint проверяется снова
непосредственно перед stop и cache commit.

Результаты классифицируются так:

| Gate result | Поведение |
| --- | --- |
| `Offline`, `DnsFailure`, `UpstreamDegraded` | ничего не переключать |
| одна недоступная цель | `TargetUnavailable`, lane не штрафуется |
| отвечающая, но медленная цель | `ServiceSlow`, не считать DPI |
| здоровый контрольный интернет + reset quorum после ClientHello | `DpiSuspected`, разрешить один switch |
| здоровая сеть + blackhole quorum | `DpiBlocked`, начать 300-секундный cooldown |
| Eyes degraded/blind или пересекающий gap | `SensorUnreliable`, ничего не переключать |

HTTP 4xx/5xx сами по себе не являются DPI-сбоем. `Offline`, `DnsFailure`,
`UpstreamDegraded`, `TargetUnavailable`, `ServiceSlow` и `SensorUnreliable`
никогда не штрафуют candidate. По окончании blackhole cooldown выполняется новый
Gate; только его свежий `Stable` разрешает одну попытку.

## 7. Покатегорийная замена

Попытка имеет `session_id`, `attempt_id`, `category`, previous/candidate config и
previous/candidate generation. Порядок:

1. Проверить bundled-файл и preflight кандидата.
2. Заблокировать только lane категории.
3. Выполнить intentional stop старого PID, bounded wait и проверку исчезновения.
4. Не трогать Eyes и другие категории.
5. Запустить candidate, дождаться readiness и выдать новую generation.
6. За общий deadline 20 секунд выполнить HTTPS probes по двум разным registry
   targets, либо по двум разным flow к единственной доступной цели, и получить
   соответствующие `Working` от Eyes; после них выдержать чистое окно пять секунд.
7. При успехе commit результата и cache.
8. При провале остановить candidate и запустить exact previous config; записать
   negative cooldown кандидата.

Два Legacy-процесса одной категории одновременно не запускаются. Существующие
соединения не закрываются намеренно; переключение влияет только на новые
соединения. До появления scoped executor текущий глобальный lifecycle остаётся
явно обозначенным migration limitation и не используется для automatic mode.

Adverse reset/blackhole с валидным sensor evidence является strategy failure и
создаёт negative cooldown. Target/environment failure, отсутствие связанного
`Working`, gap или деградация Eyes вызывают exact rollback без штрафа candidate
и состояние `SensorUnreliable`/`Unknown`. Stop timeout запрещает запуск candidate.
Start/readiness failure откатывает previous config. Rollback failure переводит
lane в `ProcessFailed`, прекращает automation и показывает ручное действие.
Unexpected process exit допускает один retry того же config на incident.

## 8. Память и cache

Ключ cache: `stable_network + category + config_fingerprint`. Stable network
fingerprint — локальный hash gateway MAC, gateway IP, route interface identity и,
когда доступно, Wi-Fi BSSID. ASN/region используется для ranking, но не заменяет
локальную identity. Config fingerprint — SHA-256 content bytes `.conf`,
referenced bundled hostlists и resource version. Изменение любого content hash
аннулирует старое trust. Запись содержит подтверждения, успехи/провалы,
timestamps, cooldown и причину.

Network identity имеет состояния `Stable`, `Unstable`, `Unknown`. Только Stable
разрешает auto-apply и cache write. ASN/region имеет TTL 24 часа, ошибка lookup —
negative TTL пять минут; offline `None` не сохраняется как постоянное значение.

Порядок кандидатов:

1. trusted current-network;
2. региональный рейтинг, если заполнен;
3. bundled default order;
4. исключить текущий и active negative-cooldown.

Один кандидат получает `Provisional` после одной проверки и `Trusted` после двух
разных сессий той же stable network. Не более одного успеха на candidate
засчитывается за сессию. Environment/site/sensor failures не меняют trust и не
создают cooldown; strategy/readiness failures создают persisted cooldown до
явного timestamp. Load-modify-save сериализуется process-wide gate и выполняется
атомарной заменой файла. Future schema игнорируется read-only, corrupt schema
карантинируется без потери остальных файлов. Observe-only ничего не пишет.
Один incident не запускает полный перебор.

## 9. Rollout и управление

Режимы включаются отдельно от engine:

- `Observe-only` — default; строит assessment и показывает предполагаемый intent;
- `Assisted` — предлагает замену категории, пользователь подтверждает в текущей
  session/generation;
- `Automatic` — явный opt-in только для Legacy, с kill switch и freeze lane.

Automatic недоступен при нестабильной сети, sensor gap/degraded/blind, cooldown,
отсутствующем кандидате или до истечения pacing.

Observe-only может выполнять bounded control/category probes, но не меняет
процессы, selection или cache. Assisted approval имеет TTL 30 секунд, после чего
отклоняется; перед исполнением всегда повторяются fences и Gate. Понижение режима
или kill switch запрещает новые attempts, а уже начатый stop/start доводится до
безопасного candidate либо exact rollback. Старое `auto_recovery=true` при
миграции становится `Observe-only`, а не `Automatic`. Переключение engine на
Zapret2 сначала закрывает Legacy session/Manager; Zapret2 не получает Legacy
events или intents.

UI по каждой lane показывает phase, активный конфиг, reason, confidence/evidence,
generation, время перехода и cooldown. Доступны confirm, retry same config,
rollback и freeze. `Unknown`, `SensorUnreliable`, `ProcessFailed` и `Exhausted`
отображаются раздельно.

Локальный ротируемый JSONL reliability log хранит session/attempt, category,
generation, event, reason, gate result, counters, fingerprints и durations.
Сырые payload и внешняя telemetry не записываются; домены/IP в диагностике
редактируются или хешируются; retention по умолчанию семь дней.

## 10. Этапы реализации

### Phase 1 — контракты и fencing

- ввести `LegacySessionContext`, generation и typed EyeEvent;
- добавить bounded queue, Health и Gap;
- построить единый TargetRegistry и заменить несовпадающие attribution rules;
- сделать replay и unit tests детерминированными;
- не включать automatic switching.

### Phase 2 — Manager assessment

- корреляция evidence per-category/transport;
- Environment Gate и typed classifications;
- передача assessment в чистый Brain;
- observe-only UI и локальный reliability log.

### Phase 3 — scoped executor

- per-category stop/start с readiness и generation checks;
- rollback и negative cooldown;
- assisted mode.

### Phase 4 — automatic Legacy

- explicit opt-in, pacing, freeze/kill switch;
- cache trust/TTL и Windows acceptance;
- оставить Zapret2 без изменений.

Каждая фаза отдельным локальным коммитом и с focused tests.

## 11. Тестовая матрица

Unit-тесты покрывают longest-suffix registry, ambiguous domains, capture port
plan, generation fencing, health/gap aggregation, gate classifier, Brain lanes,
cooldown, cache trust и fake clocks.

Integration-тесты покрывают late events, queue overflow, Eyes restart, process
crash/retry, scoped stop/start, readiness, rollback и отсутствие воздействия на
соседние категории.

Windows acceptance покрывает WinDivert capture plan по фактическим TCP-портам,
offline/DNS/upstream degradation, одну недоступную цель, slow target и
DPI-подобные RST/blackhole.

## 12. Критерии Legacy Stability v1

1. Замена одной категории не останавливает рабочие категории.
2. Late event старой session/generation не меняет текущую policy.
3. Нездоровая сеть или слепой Eyes запрещают automatic switch.
4. Тишина/отсутствие трафика не приводит к `Healthy`.
5. Успех подтверждается двумя HTTPS probes и пассивным окном.
6. Неуспех возвращает exact previous config и помещает candidate в cooldown.
7. Существующие соединения не закрываются Manager’ом намеренно.
8. Каждый `.conf` запускается самостоятельно и остаётся неизменённым.
9. Zapret2, config merge, UDP/QUIC auto-recovery и external telemetry не входят
   в v1.
10. Neighbor lane сохраняет PID, process identity, generation и active
    connections при замене другой категории.
11. Old session/lane/sensor/registry events, stale approval и duplicate intent не
    создают process, UI или cache effects.
12. Смена network fingerprint во время attempt отменяет stop либо вызывает exact
    rollback до commit.
13. Queue overflow, Eyes restart, stop timeout, start failure и rollback failure
    имеют детерминированные typed outcomes.
14. Environment/site/sensor failure не снижает trust и не создаёт cooldown.
15. До и после recovery content hash всех `.conf` остаётся неизменным, а Zapret2
    не производит и не потребляет Legacy Manager events.
