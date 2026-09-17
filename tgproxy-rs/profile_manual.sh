#!/bin/bash
set -e

# Скрипт для профилирования obsession-tg-proxy с помощью flamegraph
# Используем простой подход: запускаем прокси, подключаемся к нему,
# затем вручную запускаем профилирование

PROXY_BIN="./target/release/obsession-tg-proxy"
FLAMEGRAPH_OUT="flamegraph.svg"

# Проверяем, что бинарник собран
if [ ! -f "$PROXY_BIN.exe" ]; then
    echo "❌ Бинарник $PROXY_BIN.exe не найден. Собираем..."
    cargo build --release
fi

# Включаем отладочную информацию для профилирования
# Это замедлит бинарник, но даст более точные данные
export CARGO_PROFILE_RELEASE_DEBUG=true

# Очищаем старые данные
rm -f "$FLAMEGRAPH_OUT"

# Запускаем прокси в фоне
echo "🔥 Запускаем obsession-tg-proxy в фоне..."
echo "   Используйте этот secret для подключения:"
echo "   tg://proxy?server=127.0.0.1&port=1443&secret=dd12345678901234567890123456789012"

# Запускаем прокси с нужным secret (32 hex символа = 16 байт)
# Для корректного handshake нужно использовать secret из 32 hex символов
# В данном случае мы используем: 12345678901234567890123456789012
timeout 45s "$PROXY_BIN.exe" --port 1443 --secret 12345678901234567890123456789012 > /dev/null 2>&1 &
PROXY_PID=$!

# Даём прокси время на запуск
sleep 2

# Проверяем, что прокси запустился
if ! kill -0 $PROXY_PID 2>/dev/null; then
    echo "❌ Прокси не запустился. Проверьте ошибки выше."
    exit 1
fi

echo "✅ Прокси запущен (PID: $PROXY_PID)."
echo "   Подключитесь к нему с помощью secret:"
echo "   tg://proxy?server=127.0.0.1&port=1443&secret=dd12345678901234567890123456789012"

# Даём время на подключение
sleep 3

# Теперь запускаем профилирование вручную
echo ""
echo "🔥 Запускаем профилирование (30 секунд)..."
echo "   Подключитесь к прокси и выполните несколько операций."
echo "   После завершения будет сгенерирован flamegraph.svg"

# Запускаем профилирование
timeout 30s cargo flamegraph \
    --bin obsession-tg-proxy \
    -- \
    --port 1443 \
    --secret 12345678901234567890123456789012 &
PROFILE_PID=$!

# Ждём завершения профилирования
wait $PROFILE_PID 2>/dev/null || true

# Останавливаем прокси
kill $PROXY_PID 2>/dev/null || true

# Проверяем, что flamegraph сгенерирован
if [ -f "$FLAMEGRAPH_OUT" ]; then
    echo "🎉 Flamegraph сгенерирован: $FLAMEGRAPH_OUT"
    echo ""
    echo "📊 Сводка flamegraph:"
    echo "   - Общий размер файла: $(du -h "$FLAMEGRAPH_OUT" | cut -f1)"
    echo "   - Количество строк: $(wc -l < "$FLAMEGRAPH_OUT")"
    echo "   - Верхние 5 функций по времени:"
    grep -oP 'title="[^"]+"[^>]+>' "$FLAMEGRAPH_OUT" | \
        sed 's/title="\([^"]*\)".*/\1/' | \
        sort | uniq -c | sort -nr | head -5 | \
        awk '{printf "     %s: %s\n", $2, $1}'
    
    echo ""
    echo "📝 Как читать flamegraph:"
    echo "   - Ширина блока = время выполнения функции"
    echo "   - Высота блока = стек вызовов"
    echo "   - Цвет не важен (детерминированный)"
    echo "   - Ищите широкие блоки в верхней части — это самые горячие функции"
else
    echo "❌ Не удалось сгенерировать flamegraph."
    echo "   Попробуйте:"
    echo "   1. Запустить прокси вручную:"
    echo "      ./target/release/obsession-tg-proxy.exe --port 1443 --secret 12345678901234567890123456789012"
    echo "   2. Подключиться к нему (например, через браузерный превью или Tauri-приложение)"
    echo "   3. Затем запустить профилирование вручную:"
    echo "      cargo flamegraph --bin obsession-tg-proxy -- --port 1443 --secret 12345678901234567890123456789012"
fi

echo "📊 Профилирование завершено."
