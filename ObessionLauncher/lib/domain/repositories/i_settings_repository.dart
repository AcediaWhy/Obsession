import '../../core/errors/result.dart';
import '../entities/app_settings.dart';

/// Репозиторий для управления настройками приложения.
abstract class ISettingsRepository {
  Future<Result<AppSettings>> load();
  Future<Result<void>> save(AppSettings settings);
  Future<Result<void>> reset();

  /// Загружает выбранные DPI-конфиги (category -> configFile).
  Map<String, String> getSelectedConfigs();

  /// Сохраняет выбранный DPI-конфиг для категории.
  Future<Result<void>> setSelectedConfig(String category, String configFile);
}
