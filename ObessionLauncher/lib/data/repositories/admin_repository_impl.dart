import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/repositories/i_admin_repository.dart';
import '../datasources/admin_local_datasource.dart';
import '../datasources/log_local_datasource.dart';

class AdminRepositoryImpl implements IAdminRepository {
  AdminRepositoryImpl({
    required this._dataSource,
    required this._log,
  });

  final AdminLocalDataSource _dataSource;
  final LogLocalDataSource _log;

  @override
  Future<bool> isAdmin() => _dataSource.isAdmin();

  @override
  Future<Result<void>> relaunchAsAdmin() async {
    try {
      _log.warning('Нет прав администратора. Запрашиваю...');
      final ok = await _dataSource.relaunchAsAdmin();
      if (ok) {
        return const Success(null);
      }
      return const Failure(AdminFailure());
    } catch (e, st) {
      return Failure(AdminFailure(error: e, stackTrace: st));
    }
  }
}
