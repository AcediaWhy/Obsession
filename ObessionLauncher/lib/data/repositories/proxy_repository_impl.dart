import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/entities/proxy_config.dart';
import '../../domain/repositories/i_proxy_repository.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';
import '../datasources/proxy_local_datasource.dart';

class ProxyRepositoryImpl implements IProxyRepository {
  ProxyRepositoryImpl({
    required this._dataSource,
    required this._paths,
    required this._log,
  });

  final ProxyLocalDataSource _dataSource;
  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  @override
  Future<bool> isAvailable() async => _paths.findTgProxyExe() != null;

  @override
  Future<Result<void>> start(ProxyConfig config) async {
    try {
      final ok = await _dataSource.start(config);
      if (ok) return const Success(null);
      return const Failure(ProcessFailure('Не удалось запустить Telegram-прокси'));
    } catch (e, st) {
      _log.error('Ошибка запуска прокси', error: e, stackTrace: st);
      return Failure(ProcessFailure('Ошибка запуска прокси', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> stop() async {
    try {
      await _dataSource.stop();
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка остановки прокси', error: e, stackTrace: st);
      return Failure(ProcessFailure('Ошибка остановки прокси', error: e, stackTrace: st));
    }
  }

  @override
  bool get isRunning => _dataSource.isRunning;

  @override
  Future<Result<String>> getProxyLink() async {
    final link = _dataSource.proxyLink;
    if (link.isEmpty) {
      return const Failure(ProcessFailure('Ссылка прокси ещё не сгенерирована'));
    }
    return Success(link);
  }

  @override
  Future<Result<void>> openInTelegram() async {
    try {
      final ok = await _dataSource.openInTelegram();
      if (ok) return const Success(null);
      return const Failure(ProcessFailure('Не удалось открыть Telegram (возможно, не установлен)'));
    } catch (e, st) {
      return Failure(ProcessFailure('Не удалось открыть Telegram', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> copyLink() async {
    try {
      await _dataSource.copyLink();
      return const Success(null);
    } catch (e, st) {
      return Failure(ProcessFailure('Не удалось скопировать ссылку', error: e, stackTrace: st));
    }
  }
}
