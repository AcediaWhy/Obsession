import '../../core/errors/result.dart';
import '../entities/proxy_config.dart';

/// Репозиторий для управления Telegram-прокси.
abstract class IProxyRepository {
  Future<bool> isAvailable();
  Future<Result<void>> start(ProxyConfig config);
  Future<Result<void>> stop();
  bool get isRunning;
  Future<Result<String>> getProxyLink();
  Future<Result<void>> openInTelegram();
  Future<Result<void>> copyLink();
}
