# Adaptive Strategy Evidence Memory Design

**Дата:** 2026-07-19  
**Статус:** согласованное направление, ожидает review перед реализацией

## 1. Контекст

Zapret2 уже имеет отдельный `adaptive_strategy` coordinator с безопасным DSL,
bounded-поиском, transport-aware probes, ручным подтверждением, generation-safe
переключением и точным rollback. Legacy Brain не выбирает стратегии Zapret2 и
остаётся отдельным контуром.

Текущий adaptive cache хранит одну запись на сочетание
`network + category + transport`. Этого достаточно для повторного применения
последнего подтверждённого кандидата, но недостаточно для обучения на прошлых
попытках. Кроме того, runtime записывает провал любого кандидата в сохранённую
запись того же транспорта, даже если `candidate_id` не совпадает.

## 2. Подтверждённые проблемы

1. Провал кандидата B может увеличить `failure_count` подтверждённого кандидата
   A той же категории и транспорта.
2. Провальные кандидаты не сохраняются между сессиями и снова попадают в тот же
   детерминированный перебор.
3. `confirmed_at` и `last_probe.measured_at` не участвуют в TTL: старое
   подтверждение может автоматически применяться бессрочно.
4. `latency_ms` измеряется per target, но не попадает в persistent evidence и
   не участвует в ранжировании.
5. `TemporaryVerification` следит за crash и таймером, но не отклоняет кандидат
   по новым Eyes reset/blackhole до ручного подтверждения.
6. Baseline TLS failure всегда классифицируется как `ProbeUnreliable`; recovery
   разрешён только для узкого QUIC-сценария. При реальной TLS-блокировке поиск
   может завершиться до первой попытки.
7. Load-modify-save adaptive cache не имеет общего process-wide lock. Runtime
   сериализован, но внешнее применение Gaming recommendation может перезаписать
   параллельное обновление.

## 3. Цель

Сделать Adaptive Strategy Brain локальным evidence-based ранжировщиком:

- каждый результат относится к конкретному effective strategy fingerprint;
- рабочая стратегия не теряет доверие из-за чужого эксперимента;
- прошлые провалы ускоряют следующий подбор, но не создают вечный blacklist;
- сетевой или измерительный сбой не считается провалом стратегии;
- старые подтверждения постепенно теряют приоритет;
- lifecycle, Safe Strategy DSL и exact rollback остаются без ослаблений.

## 4. Не входит в работу

- изменение фронтенда, Rain или общей компоновки;
- облачная телеметрия и общая база по провайдерам;
- LLM/ML и недетерминированная генерация;
- автоматическое постоянное применение без ручного подтверждения;
- изменение Safe Strategy DSL allowlist;
- объединение Legacy Brain и Zapret2 coordinator;
- ослабление quorum или Eyes veto ради скорости.

## 5. Идентичность результата

Ключом истории служит `effective_fingerprint`, который уже строится compiler-ом
из фактически значимой конфигурации. `candidate_id` остаётся UI/debug ID, но не
используется как единственная идентичность evidence.

Контекст результата:

```text
network_key
asn_region (если известен)
category
transport
engine_version
strategy_schema_version
scope_fingerprint
effective_fingerprint
```

Основным network key остаётся MAC шлюза. Если у сохранённого и текущего
контекста известен `asn_region` и он изменился, запись не применяется
автоматически, но остаётся историческим prior. Неизвестный ASN сам по себе не
инвалидирует данные.

## 6. Модель evidence

Cache schema v4 сохраняет отдельно подтверждённый выбор lane и bounded-историю
результатов кандидатов.

```text
Lane = category + transport

LaneState
  confirmed_fingerprint: optional
  candidates: map<effective_fingerprint, CandidateEvidence>

CandidateEvidence
  candidate_id
  candidate (только для confirmed/recommended)
  trust: prepared | recommended | confirmed | degraded
  first_seen_at
  last_seen_at
  last_success_at
  last_failure_at
  success_count
  strategy_failure_count
  runtime_failure_count
  consecutive_failures
  last_outcome
  last_failure_stage
  latency_ema_ms
  cooldown_until
  last_probe_summary
```

История bounded: не более 32 записей на lane. При очистке сначала удаляются
самые старые unknown/degraded записи; текущий confirmed и его источник
recommendation не удаляются.

## 7. Типизированный исход попытки

Каждая попытка завершается одним из исходов:

- `ConfirmedSuccess`: quorum пройден, Eyes veto отсутствует, ручная проверка
  подтверждена;
- `StrategyFailure`: DNS и базовая связность надёжны, но нужная TLS/QUIC/HTTPS
  стадия или Eyes evidence доказывает неработоспособность кандидата;
- `EnvironmentUnreliable`: DNS, маршрут, target или probe не позволяют честно
  судить о стратегии;
- `RuntimeFailure`: candidate не скомпилировался, не стартовал или процесс
  завершился во время проверки;
- `UserRejected`: кандидат не сохраняется как confirmed, но rejection не
  считается сетевым доказательством провала;
- `Cancelled`: evidence кандидата не меняется.

Только `StrategyFailure` и `RuntimeFailure` увеличивают candidate-specific
failure counters. `EnvironmentUnreliable`, cancel и timeout ручного решения не
штрафуют кандидата.

## 8. TTL и cooldown

Для confirmed evidence используются два срока:

- soft TTL: 7 дней. Кандидат ещё можно применить как last-known-good, но он
  получает пониженный ranking и требует раннего health confirmation;
- hard TTL: 30 дней. Кандидат не применяется автоматически, но проверяется
  первым в следующем ручном поиске.

Cooldown относится только к конкретному fingerprint:

- первый подтверждённый strategy failure: 10 минут;
- второй подряд: 30 минут;
- третий и последующие: 2 часа;
- runtime/validation failure: до смены engine/schema/scope либо ручного reset.

Успех обнуляет `consecutive_failures` и cooldown. Cooldown не удаляет историю и
не мешает exact rollback к последнему подтверждённому runtime snapshot.

## 9. Ранжирование

Порядок кандидатов внутри выбранного транспорта:

1. fresh confirmed этой сети, если он не совпадает с текущим заведомо
   деградировавшим runtime;
2. soft-stale confirmed;
3. кандидаты с успешной историей и без активного cooldown;
4. неизвестные allowlisted seeds в текущем evidence-based порядке;
5. ранее провалившиеся кандидаты после истечения cooldown.

Кандидаты с активным cooldown пропускаются. При равенстве используются:

1. меньше consecutive/strategy failures;
2. больше подтверждённых успехов;
3. более свежий успех;
4. меньший latency EMA;
5. детерминированный fingerprint.

Latency никогда не может превратить функционально неуспешный кандидат в
победителя: сначала проходят correctness gates, затем сравнивается скорость.

## 10. Baseline diagnosis

Калибровка возвращает три состояния:

- `BaseHealthy`: запускается comparison search только по явному действию
  пользователя;
- `BaseBlocked`: DNS и базовая связность надёжны, но transport/application или
  повторный Eyes reset/blackhole подтверждают проблему; разрешается recovery;
- `ProbeUnreliable`: DNS/маршрут/targets не дают доказательства; поиск не
  запускается и стратегии не штрафуются.

Для TLS `BaseBlocked` допустим при надёжном DNS/TCP и failure stage
`tls | https | eyes_reset | eyes_blackhole`. Для QUIC сохраняется реальный QUIC
evidence. DNS failure не превращается в DPI recovery.

## 11. Temporary verification

После успешных probes кандидат остаётся временным до ручного подтверждения.
В течение окна:

- crash немедленно запускает rollback;
- новые Eyes reset/blackhole продолжают копиться в generation-scoped window;
- достижение veto-порога немедленно запускает rollback как `StrategyFailure`;
- перед `UserConfirm` runtime повторно проверяет, что veto не появился;
- timeout и cancel возвращают baseline без штрафа кандидату.

Дополнительный постоянный сетевой polling в 60-секундном окне не добавляется.

## 12. Сериализация и гонки

Все load-modify-save операции adaptive cache проходят через один
process-wide cache gate. Файловая запись остаётся atomic. Gate не удерживается
через сетевые await или DPI respawn: под ним выполняются только чтение,
изменение in-memory cache и сохранение файла.

State machine продолжает принимать только события текущих
`session_id + attempt_id + candidate_id + generation`. Existing DPI gate,
owned tasks и deferred rollback сохраняются.

## 13. Миграция

Schema v3 мигрирует без потери confirmed/recommended записей:

- существующая lane entry становится `confirmed_fingerprint` либо recommendation;
- её counters и probe summary становятся первой `CandidateEvidence`;
- effective fingerprint вычисляется после validator/compiler проверки;
- disabled v3 entry мигрирует как `degraded`, а не как вечный blacklist;
- битая или несовместимая запись удаляется локально, не сбрасывая другие lanes.

## 14. Этапы реализации

### Этап A. Корректная атрибуция

- передавать fingerprint текущего кандидата в запись результата;
- никогда не менять confirmed entry при несовпадении fingerprint;
- добавить regression tests на A/B-сценарий;
- сохранить существующий cache schema и runtime contract.

### Этап B. Cache v4 и ranking

- добавить bounded candidate history, outcome types, TTL и cooldown;
- мигрировать v3;
- ранжировать regenerated seeds по сохранённому evidence;
- сериализовать cache updates через gate.

### Этап C. Recovery decision

- разделить `BaseBlocked` и `ProbeUnreliable` для TLS;
- подключить Eyes veto в temporary verification;
- добавить health confirmation для soft-stale confirmed;
- провести Windows live acceptance.

Этапы коммитятся отдельно. После каждого этапа проходят focused adaptive tests,
полный Rust test suite, `cargo fmt --check` и `cargo clippy` для затронутого
backend crate. Пуш не выполняется.

## 15. Критерии приёмки

1. Провал B не меняет counters/trust/cooldown подтверждённого A.
2. Следующая сессия не начинает с кандидата на активном cooldown.
3. Environment-unreliable probe не снижает доверие ни к одному кандидату.
4. Fresh confirmed применяется только в совместимом network/engine/scope.
5. Hard-stale confirmed не применяется автоматически.
6. Подтверждённая TLS-блокировка разрешает recovery; DNS failure нет.
7. Eyes veto в manual window вызывает exact rollback до сохранения.
8. Параллельные cache updates не теряют данные.
9. Отмена, shutdown, crash и stale event сохраняют текущие lifecycle guarantees.
10. Функция остаётся за существующим `adaptive_strategy_enabled` до live
    acceptance.
