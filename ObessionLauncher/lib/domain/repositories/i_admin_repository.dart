import '../../core/errors/result.dart';

/// Репозиторий для проверки и запроса прав администратора.
abstract class IAdminRepository {
  Future<bool> isAdmin();
  Future<Result<void>> relaunchAsAdmin();
}
