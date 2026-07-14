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

Итоговый эквивалентный профиль Zapret2:

```text
multidisorder_legacy:pos=1,midsld
```

Обычный `multidisorder` в Zapret2 не является точным эквивалентом nfqws1: он
работает с полностью reassembled ClientHello и меняет порядок сегментации.
Debug-проверка подтвердила корректное распознавание `youtube.com`, совпадение
hostlist и выбор `youtube_tls`, но соединение после отправки пересобранного
ClientHello не завершалось. `multidisorder_legacy` обрабатывает исходные части
reassembly отдельно и сохраняет порядок nfqws1. Параметры `tcp_md5/tcp_seq` не
переносятся на реальные сегменты: в nfqws1 они относились к fooling-фазе, а их
прямое применение в Lua делало отправленные части непригодными для сервера.

Фильтры остаются прежними: TCP/443, TLS ClientHello и hostlist категории
`youtube_twitch`.

### YouTube QUIC

Использовать встроенный стандартный fake QUIC Initial Zapret2:

```text
fake:blob=fake_default_quic:repeats=6
```

Фильтры остаются прежними: UDP/443, QUIC Initial и hostlist категории
`youtube_twitch`.

### Версионирование и доставка

- итоговая версия `builtin.base` — `0.2.3`;
- Discord-профиль и его поведение не менять;
- в dev-режиме исходный Strategy Pack должен перезаписывать устаревшую копию в
  AppData после rebuild, чтобы live-проверка использовала именно `0.2.3`;
- целостность Lua и blob продолжает проверяться существующим manifest loader.

## Проверка

1. Автоматические тесты manifest/builder и `cargo fmt --check`.
2. В AppData загружен pack `builtin.base` версии `0.2.3`.
3. Zapret2 запускает ровно `discord_tls_text`, `youtube_tls`, `youtube_quic`.
4. Discord открывается и отправляет текстовые сообщения.
5. Главная страница YouTube открывается.
6. Видео YouTube загружает 4K и быстро продолжает воспроизведение после
   перемотки; отдельный формальный soak не менее 10 минут остаётся расширенной
   проверкой стабильности.

## Результат live-проверки

- `0.2.1` с прямым переносом fooling-аргументов не открыл YouTube;
- `0.2.2` с обычным `fake -> multidisorder` также не открыл YouTube;
- debug `0.2.2` доказал, что ошибка не связана с hostlist, SNI, выбором профиля
  или загрузкой Lua;
- `0.2.3` с `multidisorder_legacy` открыл Discord и YouTube;
- пользователь подтвердил загрузку видео 4K и быструю перемотку.

## Вне области изменения

- перенос остальных Legacy-конфигов;
- автоматический выбор Zapret2 Мозгом;
- изменения UI выбора конфигураций;
- Discord voice/media/STUN;
- изменение системного `hosts`.
