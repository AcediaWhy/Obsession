# Discord: проверка больших ClientHello и CDN — 03.10.2026

Исправлен пробел отбора: короткий Rustls-запрос мог подтвердить конфиг, хотя
реальный Discord с большим ClientHello зависал до ServerHello примерно на19с.
В полном журнале повтор на том же IP затем проходил за50мс; разрешённые8
структурных повторов дополнительно показали различие между формами запросов.
Точная причина зависимости от формы, влияние SNI отдельно и исполнение Lua
на реальных сокетах клиента этим не доказаны.

## Изменения

- Discord/TLS проверяет API, Gateway и CDN. Updater остаётся вспомогательным.
  CDN загружает публичный `/embed/avatars/0.png`: нужны200, полный ограниченный
  ответ и PNG header/IHDR с ненулевыми размерами.404/HTML/оборванный ответ не проходят.
- API и CDN требуют два свежих полноценных TLS/HTTP соединения на одном IP:
  большой ClientHello с ранним SNI (<512) и поздним SNI (>=1024). Gateway требует
  такие же две формы, каждая с настоящим HELLO и совпадающим RFC6455 pong.
- Rustls AWS-LC генерирует новые X25519MLKEM768 ключи (share1216байт) и GREASE ECH;
  форма должна быть1500–4096байт. Встроенная рандомизация порядка расширений
  выбирает подходящую форму за максимум32 локальных попытки. Неподходящие формы
  не отправляются. GREASE — не использование настоящего ECH.
- Состояние именно выбранного Rustls ClientConnection сохраняется. Первый
  flight осматривается в памяти и отправляется без изменения ровно один раз;
  библиотека продолжает полное рукопожатие, проверяет сертификат/Finished.
  Journal bytes, сохранённые key shares, аккаунты и credentials не используются.
- Альтернативные системные IP и обе формы делят прежний дедлайн цели, без
  увеличения поискового бюджета или системных изменений DNS/hosts/routes.
  Пары проверяются на одном адресе; успехи разных IP не складываются.
  Если первая форма отказала, кандидат уже не подтверждён; второй запрос не
  расходует оставшийся бюджет этой пары. Отмена освобождает принадлежащие сокеты.
- В receipts есть класс, размер, SNI offset, размер hybrid share, завершение
  authenticated TLS и application check. Малый ClientHello, одна форма,
  ServerHello без полного TLS или старые HTTP-only флаги не удовлетворяют quorum.
- Новые policy tags входят в fingerprint Discord. Старые подтверждения не
  заменяют новую проверку. QUIC/другие категории сохраняют прежнюю TLS-политику;
  их Ring provider выбирается явно, чтобы добавление AWS-LC не меняло её.

Политики: `discord_large_clienthello_early_late_sni_cdn_v1` и
`discord_gateway_hello_pong_large_early_late_reconnect_v2`.

## Проверки и сборка

- App:931passed,3ignored. Новые проверки: независимые свежие ключи, оба layout,
  повреждённый/малый hello, полнота quorum, реальные два HTTPS/TLS сокета без
  credentials, сохранение отказа второй формы, отказ404/HTML/обрыва CDN.
- Gateway fixtures проверяют полный TLS с новым генератором, HELLO/pong,
  повторное соединение, неверный сертификат, медленный HELLO и отмену.
- Frontend:477passed. TypeScript и production Vite build прошли.
- Готов `dist-release/Obsession-Setup_1.1.0_x64-clienthello-compat.exe`,31291392байта.
  SHA256 `70564df8877d0a0417b339edecb7154f0172765b9cf78b7d80431f9e7b9bf4cc`.
  App SHA256 `50341a2c52d01697b521678753758084599c7385103ebbb39ca5c808448442ed`.
  Все94 payload-файла сверены с источниками, точное embedding и подпись
  update.json подтверждены. Source hashes25 файлов совпали после сборки.
  Данные: `DISCORD_CLIENTHELLO_COMPAT_SOURCE_2026-10-03.json` и
  `DISCORD_CLIENTHELLO_COMPAT_RELEASE_2026-10-03.json`.
- Runtime/service, Lua executor и pack0.9.1 сохранены. Установщик не запускался;
  живые поиски новой сборкой и ускорение реального клиента ещё не измерены.

Это два класса свежего Rustls ClientHello, не точная эмуляция всего Chromium.
Не проверяются все перестановки расширений, authenticated READY/Resume, голос,
полное выполнение UI или устойчивость всю ночь. Предположение об ускорении
реального Discord требует нового живого запуска после установки.

После установки: обычное «Начать измерение» для Discord/TLS без VPN, затем
проверка запуска Discord. Обычный подбор сохраняет полностью подтверждённый
кандидат. Диагностический «Подбор с нуля»/сравнение возвращают исходный конфиг.

## Первичные источники

- [Discord CDN image formatting](https://docs.discord.com/developers/reference#image-formatting):
  Default User Avatar `embed/avatars/[index].png`, официальный CDN.
- [Rustls X25519MLKEM768](https://docs.rs/rustls/0.23.43/rustls/crypto/aws_lc_rs/kx_group/static.X25519MLKEM768.html):
  поддерживаемая hybrid группа. Код собран с зафиксированным Rustls0.23.41;
  размеры и работа API проверены локальными тестами этой версии.
- [Tokio-rustls connect_with](https://docs.rs/tokio-rustls/0.26.4/tokio_rustls/client/struct.TlsConnector.html):
  публичный callback ClientConnection до выполнения async handshake;
  точный порядок подтверждён чтением кода закреплённой версии.
