#!/bin/bash
set -e

# Скрипт для профилирования obsession-tg-proxy с помощью flamegraph
# Запускает прокси на 30 секунд, собирает профиль CPU и генерирует flamegraph

PROXY_BIN="./target/release/obsession-tg-proxy"
FLAMEGRAPH_OUT="flamegraph.svg"
PROFILE_OUT="perf.data"

# Проверяем, что бинарник собран
if [ ! -f "$PROXY_BIN" ]; then
    echo "❌ Бинарник $PROXY_BIN не найден. Собираем..."
    cargo build --release
fi

# Очищаем старые данные профилирования
rm -f "$PROFILE_OUT" "$FLAMEGRAPH_OUT"

# Запускаем прокси в фоне и собираем профиль
# Параметры: --port 1443 --secret 12345678901234567890123456789012 (32 hex символа)
echo "🔥 Запускаем профилирование obsession-tg-proxy..."
echo "   Собираем данные в течение 30 секунд..."

# Запускаем прокси в фоне и сразу начинаем профилирование
timeout 30s perf record -g --output="$PROFILE_OUT" -- \
    "$PROXY_BIN" --port 1443 --secret 12345678901234567890123456789012 &
PROXY_PID=$!

# Ждём завершения прокси или таймаута
wait $PROXY_PID 2>/dev/null || true

# Конвертируем данные профиля в flamegraph
if [ -f "$PROFILE_OUT" ]; then
    echo "✅ Данные профиля собраны. Генерируем flamegraph..."
    perf script -i "$PROFILE_OUT" | \
        stackcollapse-perf.pl | \
        flamegraph.pl > "$FLAMEGRAPH_OUT"
    
    if [ -f "$FLAMEGRAPH_OUT" ]; then
        echo "🎉 Flamegraph сгенерирован: $FLAMEGRAPH_OUT"
        echo "   Откройте этот файл в браузере или редакторе SVG, чтобы увидеть профиль CPU."
    else
        echo "❌ Не удалось сгенерировать flamegraph. Убедитесь, что stackcollapse-perf.pl и flamegraph.pl доступны."
    fi
else
    echo "❌ Не удалось собрать данные профиля. Проверьте, что прокси запустился и отработал."
fi

echo "📊 Профилирование завершено."
