#!/bin/bash
set -e

# Скрипт для профилирования obsession-tg-proxy с помощью flamegraph
# Используем perf (Linux) или dtrace (macOS) для сбора данных
# На Windows используем cargo-flamegraph с отладочной информацией

PROXY_BIN="./target/release/obsession-tg-proxy"
FLAMEGRAPH_OUT="flamegraph.svg"

# Проверяем, что бинарник собран
if [ ! -f "$PROXY_BIN" ]; then
    echo "❌ Бинарник $PROXY_BIN не найден. Собираем..."
    cargo build --release
fi

# Включаем отладочную информацию для профилирования
# Это замедлит бинарник, но даст более точные данные
export CARGO_PROFILE_RELEASE_DEBUG=true

# Очищаем старые данные
rm -f "$FLAMEGRAPH_OUT"

# Запускаем профилирование с помощью cargo-flamegraph
# Параметры: --port 1443 --secret 12345678901234567890123456789012 (32 hex символа)
echo "🔥 Запускаем профилирование obsession-tg-proxy с помощью cargo-flamegraph..."
echo "   Собираем данные в течение 30 секунд..."
echo "   ⚠️  Профилирование будет медленнее из-за отладочной информации."

# Запускаем прокси в фоне и сразу начинаем профилирование
timeout 30s cargo flamegraph \
    --bin obsession-tg-proxy \
    -- \
    --port 1443 \
    --secret 12345678901234567890123456789012 &
PROXY_PID=$!

# Ждём завершения прокси или таймаута
wait $PROXY_PID 2>/dev/null || true

# Проверяем, что flamegraph сгенерирован
if [ -f "$FLAMEGRAPH_OUT" ]; then
    echo "🎉 Flamegraph сгенерирован: $FLAMEGRAPH_OUT"
    echo "   Откройте этот файл в браузере или редакторе SVG, чтобы увидеть профиль CPU."
    echo "   Пример:"
    echo "   - В браузере: просто откройте файл flamegraph.svg"
    echo "   - В VS Code: установите расширение SVG Preview"
    echo "   - В любом редакторе: откройте как текст/SVG"
    
    # Показываем краткую сводку flamegraph
    echo ""
    echo "📊 Сводка flamegraph:"
    echo "   - Общий размер файла: $(du -h "$FLAMEGRAPH_OUT" | cut -f1)"
    echo "   - Количество строк: $(wc -l < "$FLAMEGRAPH_OUT")"
    echo "   - Верхние 5 функций по времени:"
    grep -oP 'title="[^"]+"[^>]+>' "$FLAMEGRAPH_OUT" | \
        sed 's/title="\([^"]*\)".*/\1/' | \
        sort | uniq -c | sort -nr | head -5 | \
        awk '{printf "     %s: %s\n", $2, $1}'
else
    echo "❌ Не удалось сгенерировать flamegraph. Возможно, прокси не успел запуститься."
    echo "   Попробуйте:"
    echo "   1. Запустить прокси вручную: ./target/release/obsession-tg-proxy --port 1443 --secret 12345678901234567890123456789012"
    echo "   2. Подключиться к нему (например, через браузерный превью или Tauri-приложение)"
    echo "   3. Затем запустить профилирование вручную:"
    echo "      cargo flamegraph --bin obsession-tg-proxy -- --port 1443 --secret 12345678901234567890123456789012"
fi

echo "📊 Профилирование завершено."

