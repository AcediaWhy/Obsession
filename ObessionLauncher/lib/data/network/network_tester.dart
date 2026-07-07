import 'dart:async';
import 'dart:io';

import '../../core/constants/app_constants.dart';
import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';

/// Интерфейс для проверки сетевой доступности.
abstract class INetworkTester {
  /// Проверяет доступность хоста по URL.
  Future<Result<bool>> testUrl(String url, {Duration timeout});
}

class NetworkTesterImpl implements INetworkTester {
  @override
  Future<Result<bool>> testUrl(
    String url, {
    Duration timeout = const Duration(seconds: 5),
  }) async {
    final uri = Uri.tryParse(url);
    if (uri == null || uri.host.isEmpty) {
      return Failure(NetworkFailure('Некорректный URL: $url'));
    }

    // 1. Быстрая TCP-проверка: можем ли мы достучаться до 443 порта.
    // Это работает даже если сервер отдаёт 403/429/капчу.
    try {
      final socket = await Socket.connect(uri.host, 443, timeout: timeout);
      await socket.close();
      return const Success(true);
    } on SocketException {
      // Fallback к HTTP.
    } on TimeoutException {
      // Fallback к HTTP.
    } catch (_) {
      // Fallback к HTTP.
    }

    // 2. Fallback HTTP: любой ответ (кроме timeout/socket) означает, что
    // соединение установлено и DPI работает.
    HttpClient? client;
    try {
      client = HttpClient()
        ..connectionTimeout = timeout;
      // Сертификаты валидируются штатно: невалидный/self-signed/просроченный
      // серт не принимается (ранее callback принимал любой серт целевого хоста,
      // что отключало TLS-аутентификацию соединения). Ошибка рукопожатия
      // трактуется как «хост недоступен», а не как успех.

      final request = await client.getUrl(uri);
      request.followRedirects = false;
      request.headers.set(
        HttpHeaders.userAgentHeader,
        '${AppConstants.appName}/${AppConstants.appVersion}',
      );

      final response = await request.close().timeout(timeout);
      await response.drain<void>().timeout(timeout);

      // Любой HTTP-ответ, который мы смогли получить, означает, что хост
      // доступен по сети. Даже 403/429 не говорят о неработающем DPI.
      return const Success(true);
    } on HandshakeException {
      // Проваленное TLS-рукопожатие — хост недоступен/подменён.
      return const Success(false);
    } on SocketException {
      return const Success(false);
    } on TimeoutException {
      return const Success(false);
    } on HttpException {
      return const Success(false);
    } catch (e) {
      return Failure(NetworkFailure(e.toString()));
    } finally {
      client?.close();
    }
  }
}

class NetworkTesterUseCase implements UseCase<bool, String> {
  final INetworkTester tester;
  const NetworkTesterUseCase(this.tester);

  @override
  Future<Result<bool>> call(String url) => tester.testUrl(url);
}
