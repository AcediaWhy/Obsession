import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/entities/hosts_config.dart';
import '../../domain/repositories/i_hosts_repository.dart';
import '../datasources/hosts_local_datasource.dart';
import '../datasources/log_local_datasource.dart';

class HostsRepositoryImpl implements IHostsRepository {
  HostsRepositoryImpl({
    required this._dataSource,
    required this._log,
  });

  final HostsLocalDataSource _dataSource;
  final LogLocalDataSource _log;

  @override
  Future<Result<String>> readHosts() async {
    try {
      final content = await _dataSource.readHosts();
      return Success(content);
    } catch (e, st) {
      _log.error('Ошибка чтения hosts', error: e, stackTrace: st);
      return Failure(FileFailure('Не удалось прочитать hosts', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<bool>> isInstalled(AiProvider provider) async {
    try {
      final installed = await _dataSource.isInstalled(provider);
      return Success(installed);
    } catch (e, st) {
      return Failure(FileFailure('Ошибка проверки hosts', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<HostsConfig>> checkStatus(AiProvider provider) async {
    try {
      final status = await _dataSource.checkStatus(provider);
      return Success(status);
    } catch (e, st) {
      _log.error('Ошибка проверки статуса hosts', error: e, stackTrace: st);
      return Failure(NetworkFailure('Не удалось проверить статус', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> install(AiProvider provider) async {
    try {
      final ok = await _dataSource.install(provider);
      if (ok) return const Success(null);
      return const Failure(FileFailure('Не удалось установить ИИ-обход'));
    } catch (e, st) {
      _log.error('Ошибка установки hosts', error: e, stackTrace: st);
      return Failure(FileFailure('Ошибка установки hosts', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> uninstall() async {
    try {
      final ok = await _dataSource.uninstall();
      if (ok) return const Success(null);
      return const Failure(FileFailure('Не удалось удалить ИИ-обход'));
    } catch (e, st) {
      _log.error('Ошибка удаления hosts', error: e, stackTrace: st);
      return Failure(FileFailure('Ошибка удаления hosts', error: e, stackTrace: st));
    }
  }
}
