import '../../core/errors/result.dart';
import '../../data/datasources/tray_local_datasource.dart' show TrayLabels;

/// Репозиторий для управления системным треем.
abstract class ITrayRepository {
  Future<Result<void>> init({TrayLabels? labels});
  Future<Result<void>> showWindow();
  Future<Result<void>> hideToTray();
  Future<Result<void>> dispose();
}
