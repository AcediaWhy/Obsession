import '../../core/errors/result.dart';

/// Репозиторий для управления автозапуском приложения.
abstract class IAutostartRepository {
  Future<bool> isEnabled();
  Future<Result<void>> enable();
  Future<Result<void>> disable();
  Future<Result<bool>> toggle();
}
