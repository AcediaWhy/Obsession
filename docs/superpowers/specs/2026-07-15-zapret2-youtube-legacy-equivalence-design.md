# Zapret2 YouTube Legacy-Equivalence Design

## Контекст

После исправления поставки `cygwin1.dll` Zapret2 1.0.2 корректно запускает три
профиля Gate D. Профиль `discord_tls_text` обеспечивает открытие Discord и
отправку текстовых сообщений. При этом текущие `youtube_tls` и `youtube_quic`
не открывают даже главную страницу YouTube, тогда как выбранный Legacy-конфиг
`youtube_twitch_1.conf` на той же машине и у того же провайдера работает.

## Решение

Не копировать Legacy CLI-конфиг в `winws2.exe`, а перенести подтверждённый
алгоритм десинхронизации в нативную Lua-грамматику Zapret2.

### YouTube TLS

Рабочая Legacy-схема:

```text
multidisorder + split-pos=1,midsld + fooling=md5sig,badseq
```

Эквивалентный профиль Zapret2:

```text
multidisorder:pos=1,midsld:tcp_md5:tcp_seq=-10000
```

Текущий предварительный `fake` с модификацией ClientHello удаляется из
YouTube TLS-профиля. Фильтры остаются прежними: TCP/443, TLS ClientHello и
hostlist категории `youtube_twitch`.

### YouTube QUIC

Оставить fake QUIC Initial из встроенного проверенного blob, но привести число
повторов к рабочему Legacy-конфигу:

```text
fake:blob=quic_google:repeats=8
```

Фильтры остаются прежними: UDP/443, QUIC Initial и hostlist категории
`youtube_twitch`.

### Версионирование и доставка

- повысить версию `builtin.base` с `0.2.0` до `0.2.1`;
- Discord-профиль и его поведение не менять;
- в dev-режиме исходный Strategy Pack должен перезаписывать устаревшую копию в
  AppData после rebuild, чтобы live-проверка использовала именно `0.2.1`;
- целостность Lua и blob продолжает проверяться существующим manifest loader.

## Проверка

1. Автоматические тесты manifest/builder и `cargo fmt --check`.
2. В AppData загружен pack `builtin.base` версии `0.2.1`.
3. Zapret2 запускает ровно `discord_tls_text`, `youtube_tls`, `youtube_quic`.
4. Discord открывается и отправляет текстовые сообщения.
5. Главная страница YouTube открывается.
6. Видео YouTube стабильно воспроизводится не менее 10 минут.

Если сайт не откроется, pack откатывается к `0.2.0`, а следующим отдельным
экспериментом проверяется официальный базовый порядок `fake -> multidisorder`.

## Вне области изменения

- перенос остальных Legacy-конфигов;
- автоматический выбор Zapret2 Мозгом;
- изменения UI выбора конфигураций;
- Discord voice/media/STUN;
- изменение системного `hosts`.
