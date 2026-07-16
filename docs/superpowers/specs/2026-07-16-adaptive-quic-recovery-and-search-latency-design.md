# Adaptive QUIC Recovery and Search Latency Design

**Дата:** 2026-07-16
**Статус:** утвержденное направление

## 1. Контекст

Живая Gaming + GitHub сессия показала два связанных дефекта.

1. QUIC search действительно запускался, но UI оставался в прежнем состоянии,
   пока runtime синхронно выполнял baseline calibration. Пользователь около 30
   секунд не видел реакции на кнопку.
2. Gaming baseline QUIC завершился тремя полными сериями timeout. После этого
   runtime отказался запускать кандидатов, хотя назначение recovery search как
   раз состоит в поиске стратегии для неработающего транспорта.

Дополнительно один target мог занять около 10 секунд при настройке timeout 5
секунд. Причина: timeout применялся отдельно к каждому разрешенному IP-адресу,
а не ко всей операции target probe. Жестко заданные Roblox и Epic endpoints
считались QUIC core без доказательства, что они поддерживают HTTP/3.

## 2. Цели

1. Разрешить QUIC recovery search при неработающем category baseline.
2. Отличать неподдерживаемый endpoint от заблокированного QUIC.
3. Ограничить время каждого target probe единым абсолютным deadline.
4. Ускорить отрицательные попытки без ослабления правила подтверждения.
5. Немедленно показывать пользователю стадию, транспорт, раунд и причину
   ожидания.
6. Сохранить exact rollback, generation safety и ручное подтверждение
   найденного кандидата.

## 3. Не входит в работу

- проверка произвольного игрового UDP, matchmaking, voice или STUN;
- признание ipset data plane рабочим по результату HTTP/3;
- отключение TLS/QUIC certificate verification;
- случайный или неограниченный перебор адресов и стратегий;
- изменение Fast/Balanced/Deep в небезопасный однократный тест;
- ускорение первой компиляции `tauri dev`; production startup рассматривается
  отдельно от adaptive search latency.

## 4. Режимы сессии

После discovery и baseline calibration runtime выбирает один из двух режимов.

### 4.1 Comparison mode

Baseline выбранного транспорта проходит проверку. Существующий процесс
сохраняется: кандидат должен дать подтвержденный результат, а после его провала
runtime восстанавливает baseline и выполняет короткий base recheck.

### 4.2 Recovery mode

Baseline QUIC не проходит, но среда измерения пригодна: DNS работает, есть
проверяемые category endpoints, Zapret2 жив и generation не менялся. Провал
baseline фиксируется как исходная проблема, после чего запускаются отличающиеся
QUIC candidates.

После неудачного candidate runtime делает exact rollback к исходному snapshot,
проверяет успешность spawn и generation, но не требует сетевого успеха от уже
известно неработающей базы. Бесполезный base recheck в recovery mode пропускается.
Следующий кандидат стартует только после подтвержденного rollback.

Провал DNS, отсутствие проверяемых HTTP/3 endpoints, ошибка snapshot/spawn или
внешняя смена generation не переводятся в recovery mode: это ненадежная среда,
а не доказанный QUIC failure.

## 5. QUIC target discovery

Для каждой категории остается небольшой allowlisted pool HTTPS endpoints.
Перед QUIC calibration runtime выполняет TLS preflight и проверяет `Alt-Svc`.
Endpoint допускается в QUIC core только если он объявляет актуальный `h3`
protocol либо уже доказал QUIC handshake в текущей сессии.

Правила discovery:

- certificate verification остается включенной;
- HTTP status `200..=499` допустим;
- redirect проходить не требуется;
- `Alt-Svc: h3=...`, включая параметры и несколько значений, разбирается
  структурированным parser helper;
- результат живет только в текущей search session и не становится постоянным
  доказательством поддержки HTTP/3;
- минимум одна core-цель обязательна, две предпочтительны;
- optional endpoint не может отклонить candidate.

Если ни одна Gaming-цель не подтверждает HTTP/3, UI получает отдельный результат
`quic_targets_unavailable`. Он не отображается как "стратегии не найдены".

## 6. Probe deadlines и адреса

`probe_timeout` становится deadline всей target operation: DNS, выбор адреса,
socket setup и TLS/QUIC handshake вместе. Вложенная операция не может продлить
его повторным timeout.

После DNS выбирается ограниченный набор адресов:

- не более одного IPv6 и одного IPv4 адреса;
- первая попытка стартует немедленно;
- адрес другой семьи стартует с коротким stagger;
- первый успешный результат отменяет остальные attempts;
- ошибки агрегируются типизированно, без ожидания всех адресов после успеха.

Balanced сохраняет устойчивость `2 из 3`, но probe series завершается раньше,
если требуемое число успехов уже математически недостижимо. Ранний успех не
завершает окно до того, как собраны необходимые раунды и проверен Eyes veto.

## 7. UI и observable state

Нажатие Start Search немедленно переводит state machine из terminal/suggested
состояния в активную фазу до любых сетевых await.

Добавляются видимые стадии:

- `discovering_quic` - поиск проверяемых HTTP/3 endpoints;
- `calibrating` - baseline round `N/M`;
- существующие candidate/rollback/verification phases.

Status содержит выбранный transport, session id, текущий round, total rounds,
search mode и session mode (`comparison` или `recovery`). Кнопки блокируются по
backend phase, а не только по локальному `busy`. Ошибка Tauri-команды остается
видимой в панели и toast.

Discovery и calibration не выполняются inline внутри control event loop.
Coordinator принимает Start Search, создает session id, публикует status и
запускает отдельную bounded task с cancellation token. Поэтому Cancel, Stop и
Shutdown обрабатываются немедленно, прерывают DNS/socket/handshake futures и не
ждут истечения сетевых deadline. Результат фоновой task принимается только при
совпадении session id и runtime generation.

Для recovery mode UI явно сообщает: исходный QUIC не работает, выполняется
поиск отличающейся стратегии. Возврат к исходному snapshot не называется
восстановлением сетевой работоспособности.

## 8. Data flow

```text
StartSearch(category, QUIC)
  -> allocate session id and cancellation token
  -> emit discovering_quic immediately
  -> run discovery/calibration in cancellable task
  -> TLS preflight + Alt-Svc discovery under deadline
  -> no eligible target: quic_targets_unavailable
  -> emit calibrating round progress
  -> baseline succeeds: Comparison mode
  -> baseline QUIC fails with reliable DNS/runtime: Recovery mode
  -> generate distinct bounded candidates
  -> candidate probe against discovered category QUIC core
  -> fail: exact rollback
       -> Comparison: base recheck, then continue
       -> Recovery: process/generation check, then continue
  -> pass: manual verification
  -> confirm: persist category + transport candidate
```

## 9. Ошибки и завершение

- `quic_targets_unavailable`: endpoints не доказали поддержку HTTP/3;
- `probe_unreliable`: DNS или probe environment нельзя измерить;
- `recovery_exhausted`: кандидаты исчерпаны, возвращен исходный snapshot,
  который может оставаться нерабочим для QUIC;
- `base_unhealthy`: используется только в comparison mode;
- `internal_error`: snapshot, spawn, persistence или rollback invariant нарушен;
- cancel/shutdown всегда выполняют exact rollback текущего candidate.

Повторное нажатие Start Search во время discovery/calibration отклоняется явной
ошибкой `search_already_running`, а не запускает вторую сессию.

## 10. Производительность

Ожидаемый Balanced budget после исправления:

- discovery: один параллельный preflight pool под общими target deadlines;
- baseline: не более трех bounded rounds;
- очевидный hard failure: обычно завершается после двух раундов;
- один target не превышает configured timeout независимо от числа DNS-адресов;
- recovery rollback не расходует время на заведомо провальный base recheck.

Цель для текущей сети: сократить неуспешную Gaming QUIC calibration примерно с
30 секунд до 8-12 секунд и убрать полностью немую задержку после клика. Это не
является жестким SLA для любой сети; hard deadline остается источником истины.

## 11. Тестирование

### Unit

- `Alt-Svc` parser принимает `h3`, параметры и несколько protocol values;
- endpoints без `h3` не становятся QUIC core;
- target deadline не умножается на число адресов;
- IPv4/IPv6 race возвращает первый успех и отменяет остаток;
- impossible `2 из 3` series завершается после второго hard failure;
- recovery mode не требует успешного base recheck;
- comparison mode сохраняет существующий base recheck;
- повторный Start Search получает `search_already_running`.

### Integration

- status `discovering_quic` публикуется до первого сетевого await;
- failed QUIC baseline запускает кандидата в recovery mode;
- отсутствие HTTP/3 targets не запускает кандидаты;
- candidate failure -> exact rollback -> следующий candidate;
- rollback generation mismatch завершает session как `internal_error`;
- successful candidate по-прежнему требует ручного подтверждения;
- cancel во время discovery/calibration не оставляет фоновые probes или orphan
  winws2.

### Live Windows acceptance

1. Gaming QUIC click немедленно меняет состояние UI.
2. В UI и журнале видны discovery и calibration rounds.
3. Endpoint без HTTP/3 не создает ложный QUIC timeout.
4. Неработающий builtin QUIC переводит сессию в recovery mode.
5. Один target не превышает Balanced deadline при нескольких IP.
6. Провальный кандидат быстро возвращает точный snapshot и продолжает поиск.
7. TLS search, Discord и YouTube не регрессируют.

## 12. Критерии готовности

Работа завершена, когда QUIC search реагирует сразу, использует только доказанно
QUIC-capable category endpoints, запускает bounded candidates при неработающем
baseline, не умножает timeout на DNS-адреса, различает comparison/recovery
rollback и проходит Rust/frontend/live regression checks.
