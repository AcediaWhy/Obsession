import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/repositories/i_autostart_repository.dart';
import '../datasources/autostart_local_datasource.dart';
import '../datasources/log_local_datasource.dart';

class AutostartRepositoryImpl implements IAutostartRepository {
  AutostartRepositoryImpl({
    required this._dataSource,
    required this._log,
  });

  final AutostartLocalDataSource _dataSource;
  final LogLocalDataSource _log;

  @override
  Future<bool> isEnabled() => _dataSource.isEnabled();

  @override
  Future<Result<void>> enable() async {
    final ok = await _dataSource.enable();
    if (ok) {
      _log.success('Автозапуск включён.');
      return const Success(null);
    }
    _log.error('Не удалось включить автозапуск.');
    return const Failure(FileFailure('Не удалось включить автозапуск'));
  }

  @override
  Future<Result<void>> disable() async {
    final ok = await _dataSource.disable();
    if (ok) {
      _log.info('Автозапуск выключен.');
      return const Success(null);
    }
    return const Failure(FileFailure('Не удалось выключить автозапуск'));
  }

  @override
  Future<Result<bool>> toggle() async {
    final enabled = await isEnabled();
    final result = enabled ? await disable() : await enable();
    return result.when(
      success: (_) => Success(!enabled),
      failure: (f) => Failure(f),
    );
  }
}
