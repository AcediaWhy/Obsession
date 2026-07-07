import 'dart:io' as io;

import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';

/// Модель результата ping.
class PingResult {
  final String host;
  final int latencyMs;
  final bool isReachable;

  const PingResult({
    required this.host,
    required this.latencyMs,
    required this.isReachable,
  });

  factory PingResult.unreachable(String host) => PingResult(
        host: host,
        latencyMs: -1,
        isReachable: false,
      );
}

/// Локальный источник для проверки доступности хостов через ping.
class PingLocalDataSource {
  /// Пингует хост один раз с таймаутом 1000 мс.
  Future<Result<PingResult>> ping(String host) async {
    // Защита от argument-injection: значение, начинающееся с '-', может быть
    // воспринято ping как флаг. Отсекаем такие хосты заранее.
    final trimmedHost = host.trim();
    if (trimmedHost.isEmpty || trimmedHost.startsWith('-')) {
      return Success(PingResult.unreachable(host));
    }
    try {
      final result = await io.Process.run(
        'ping',
        ['-n', '1', '-w', '1000', trimmedHost],
        runInShell: false,
      ).timeout(const Duration(seconds: 5));

      if (result.exitCode != 0) {
        return Success(PingResult.unreachable(host));
      }

      final output = result.stdout.toString();
      final latency = _parseLatency(output);

      return Success(PingResult(
        host: host,
        latencyMs: latency ?? -1,
        isReachable: latency != null,
      ));
    } catch (e) {
      return Failure(NetworkFailure('Ping failed: $e'));
    }
  }

  int? _parseLatency(String output) {
    // Локаль-независимый парсинг: ищем число после известных маркеров времени
    // (time/время/zeit/temps/tiempo) или просто число перед ms/мс.
    // Если ничего не найдено — пробуем извлечь любое число с суффиксом ms/мс.
    final regex = RegExp(
      r'(?:time|время|zeit|temps|tiempo|tempo)[=<]?\s*(\d+)\s*(?:ms|мс)',
      caseSensitive: false,
    );
    var match = regex.firstMatch(output);
    if (match != null) {
      return int.tryParse(match.group(1) ?? '');
    }
    // Fallback: любое число + ms/мс в любом месте вывода.
    final fallback = RegExp(r'(\d+)\s*(?:ms|мс)', caseSensitive: false);
    match = fallback.firstMatch(output);
    if (match != null) {
      return int.tryParse(match.group(1) ?? '');
    }
    return null;
  }
}
