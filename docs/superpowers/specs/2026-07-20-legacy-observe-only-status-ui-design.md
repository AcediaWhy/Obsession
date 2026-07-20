# Legacy Observe-only Status UI Design

**Дата:** 2026-07-20
**Статус:** утверждено владельцем проекта

## 1. Проблема

После запуска Legacy интерфейс показывает включённое «Авто-восстановление» и
состояние «Ожидание». Это статус старого глобального Brain, которому во время
миграции намеренно не передаётся `SessionStart`. Новый Legacy Reliability
Manager запускается отдельно, не управляется этим тумблером и не публикует свой
status во frontend. В результате активный `winws` и работающий observe-only
Manager отображаются как бездействующее автоматическое восстановление.

## 2. Цель и границы

Phase 1 должна показывать фактическое состояние observe-only Manager и честно
объяснять его возможности. Мониторинг включается и выключается вместе с Legacy
session; отдельного пользовательского переключателя у него нет.

В этот срез не входят assessment категорий, рекомендации, переключение `.conf`,
rollback, cooldown, Assisted/Automatic и изменение поведения Zapret2. Старые
`.conf` остаются независимыми и не переписываются.

## 3. Отдельный wire contract

Backend публикует отдельный versioned `LegacyReliabilityStatus`. Он не
переиспользует `BrainStatus` и событие `brain://status`.

Минимальный публичный payload содержит:

- `mode: "observe_only"`;
- `phase: "inactive" | "starting" | "observing" | "degraded" | "blind"`;
- активные категории текущей Legacy session;
- необязательные `sessionId` и `sensorGeneration` для диагностики.

`phase` является lifecycle-проекцией, а не оценкой работоспособности сайта:

| Phase | Источник истины | Текст UI |
|---|---|---|
| `inactive` | Legacy session отсутствует | `Ожидание запуска` |
| `starting` | `winws` запущен, Manager/Eyes ещё устанавливаются | `Запуск наблюдения` |
| `observing` | Manager установлен, sensor health `Ready` | `Наблюдение` |
| `degraded` | sensor health `Degraded` либо зафиксирован Gap до clean window | `Наблюдение ограничено` |
| `blind` | Manager/Eyes не созданы для активной session либо health `Blind`/`Stopped` | `Наблюдение недоступно` |

Активный DPI-процесс сам по себе не даёт права показывать `observing`: Legacy
может продолжить обход при ошибке registry, Manager или Eyes. После завершения
startup активная Legacy session без установленного Manager должна перейти в
`blind`, а не остаться в `starting`.

Status входит в bootstrap snapshot и обновляется отдельным versioned событием
`legacy-reliability://status`. Revision увеличивается при каждом lifecycle- или
health-переходе. Listener-first bootstrap применяет те же правила защиты от
устаревшего snapshot, что DPI, Brain и Adaptive stores.

Добавление обязательной versioned-секции повышает `BOOTSTRAP_SCHEMA_VERSION`.
Rust и TypeScript контракты обновляются вместе; frontend отклоняет snapshot с
неподдерживаемой версией по существующему bootstrap failure path.

## 4. Lifecycle и fencing

`starting` публикуется только для поколения запуска, которое ещё принадлежит
текущему Legacy runtime. Успешная установка Manager публикует phase из его
фактического `ObserveOnlySnapshot`. Registry/spawn/Eyes failure публикует
`blind`, сохраняя активный обход. Stop, engine switch и emergency teardown
сначала инвалидируют session, затем публикуют `inactive`; поздний callback со
старой generation не может вернуть старый статус.

Периодические snapshot Manager не должны создавать UI-event storm: событие
эмитируется только при изменении публичной phase/lifecycle-проекции. Потеря
frontend listener не влияет на Manager, Eyes или обход.

## 5. Интерфейс

Для выбранного Legacy старая карточка Brain заменяется read-only блоком:

- заголовок `Контроль надёжности`;
- строка `Режим — Только наблюдение`;
- строка `Состояние` с текстом из таблицы phase;
- пояснение `Сбои фиксируются, конфигурации не меняются.`

Тумблер «Авто-восстановление», ladder level, стратегия старого Brain и обещание
автоматического подключения к сессии в Legacy не показываются. `observing`
использует спокойный положительный тон; `starting` и `degraded` — нейтральный и
warning; `blind` — error. Цвет не заменяет текст.

Zapret2 продолжает использовать свой Adaptive UI. Старый глобальный Brain status
не выдаётся за состояние ни Legacy Manager, ни Zapret2 coordinator.

Английская debug-строка `Legacy Brain SessionStart suppressed: observe-only
migration phase` удаляется из пользовательского лога. Успешный старт получает
понятную запись: `Мониторинг Legacy запущен в режиме наблюдения; конфигурации не
изменяются автоматически.` Реальные failure-записи остаются `error`.

## 6. Совместимость настройки

Сохранённый `auto_recovery=true` не включает автоматические действия Legacy и
не отображается как активный тумблер. Поле пока сохраняется для обратной
совместимости и будущей явной миграции режимов. Этот UI-срез не удаляет setting
и не меняет его значение на диске.

## 7. Проверка

Rust-тесты проверяют сериализацию payload, mapping health/lifecycle в phase,
монотонную revision, startup failure, stop и отбрасывание позднего поколения.
Frontend-тесты проверяют listener-first store и матрицу `inactive`, `starting`,
`observing`, `degraded`, `blind`, а также отсутствие Legacy Brain toggle.

Полная регрессия включает `cargo test --all-targets`, Clippy с запретом warnings,
format check, `npm test` и production build. В dev-сборке проверяются успешный
Legacy start/stop и контролируемая ошибка Eyes/Manager без ложного зелёного
статуса.

## 8. Критерии готовности

1. При активном и здоровом observe-only Manager UI показывает `Наблюдение`.
2. При активном `winws`, но недоступном Manager/Eyes UI показывает
   `Наблюдение недоступно`.
3. До запуска и после stop UI показывает `Ожидание запуска`.
4. В Legacy нет тумблера или текста, обещающего автоматическую замену `.conf`.
5. Status старого Brain не влияет на карточку Legacy.
6. Ни один новый status path не запускает process, switch, cache write или
   другое управляющее действие.
