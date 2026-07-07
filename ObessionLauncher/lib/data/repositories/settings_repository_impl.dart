import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/entities/app_settings.dart';
import '../../domain/repositories/i_settings_repository.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/settings_local_datasource.dart';

class SettingsRepositoryImpl implements ISettingsRepository {
  SettingsRepositoryImpl({
    required this._dataSource,
    required this._log,
  });

  final SettingsLocalDataSource _dataSource;
  final LogLocalDataSource _log;

  @override
  Future<Result<AppSettings>> load() async {
    try {
      final settings = _dataSource.load();
      return Success(settings);
    } catch (e, st) {
      _log.error('Ошибка загрузки настроек', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось загрузить настройки', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> save(AppSettings settings) async {
    try {
      await _dataSource.save(settings);
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка сохранения настроек', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось сохранить настройки', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> reset() async {
    try {
      await _dataSource.reset();
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка сброса настроек', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось сбросить настройки', error: e, stackTrace: st));
    }
  }

  @override
  Map<String, String> getSelectedConfigs() {
    try {
      return _dataSource.getSelectedConfigs();
    } catch (e, st) {
      _log.error('Ошибка загрузки выбранных конфигов', error: e, stackTrace: st);
      return {};
    }
  }

  @override
  Future<Result<void>> setSelectedConfig(String category, String configFile) async {
    try {
      await _dataSource.setSelectedConfig(category, configFile);
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка сохранения выбранного конфига', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось сохранить конфиг', error: e, stackTrace: st));
    }
  }
}
