import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/entities/dpi_config.dart';
import '../../domain/entities/process_info.dart';
import '../../domain/repositories/i_process_repository.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';
import '../datasources/process_local_datasource.dart';

class ProcessRepositoryImpl implements IProcessRepository {
  ProcessRepositoryImpl({
    required this._dataSource,
    required this._paths,
    required this._log,
  });

  final ProcessLocalDataSource _dataSource;
  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  @override
  Future<Result<ProcessInfo>> start(DpiConfig config) async {
    try {
      final info = await _dataSource.start(config.category, config.configFile);
      if (info != null) {
        _log.success('Запущен процесс ${info.category} (PID ${info.pid})');
        return Success(info);
      }
      return const Failure(ProcessFailure('Не удалось запустить DPI-процесс'));
    } catch (e, st) {
      _log.error('Ошибка запуска DPI', error: e, stackTrace: st);
      return Failure(ProcessFailure('Ошибка запуска DPI', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> stop(int pid) async {
    try {
      final ok = await _dataSource.stop(pid);
      if (ok) return const Success(null);
      return Failure(ProcessFailure('Процесс PID $pid не найден'));
    } catch (e, st) {
      return Failure(ProcessFailure('Ошибка остановки процесса', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> stopAll() async {
    try {
      await _dataSource.stopAll();
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка остановки процессов', error: e, stackTrace: st);
      return Failure(ProcessFailure('Ошибка остановки процессов', error: e, stackTrace: st));
    }
  }

  @override
  Future<bool> isRunning(int pid) async => _dataSource.isRunning(pid);

  @override
  List<ProcessInfo> get activeProcesses => _dataSource.activeProcesses;

  @override
  bool get isActive => _dataSource.isActive;

  @override
  List<String> getCategories() => _paths.getCategories();

  @override
  List<String> getConfigsForCategory(String category) =>
      _paths.getConfigsForCategory(category);
}
