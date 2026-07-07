import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/repositories/i_tray_repository.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/tray_local_datasource.dart';

class TrayRepositoryImpl implements ITrayRepository {
  TrayRepositoryImpl({
    required this._dataSource,
    required this._log,
  });

  final TrayLocalDataSource _dataSource;
  final LogLocalDataSource _log;

  @override
  Future<Result<void>> init({TrayLabels? labels}) async {
    try {
      await _dataSource.init(labels: labels);
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка инициализации трея', error: e, stackTrace: st);
      return Failure(UnknownFailure(error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> showWindow() async {
    try {
      await _dataSource.showWindow();
      return const Success(null);
    } catch (e, st) {
      return Failure(UnknownFailure(error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> hideToTray() async {
    try {
      await _dataSource.hideToTray();
      return const Success(null);
    } catch (e, st) {
      return Failure(UnknownFailure(error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> dispose() async {
    try {
      await _dataSource.dispose();
      return const Success(null);
    } catch (e, st) {
      return Failure(UnknownFailure(error: e, stackTrace: st));
    }
  }
}
