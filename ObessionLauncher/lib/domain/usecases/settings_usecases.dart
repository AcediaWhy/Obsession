import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../entities/app_settings.dart';
import '../repositories/i_settings_repository.dart';

class LoadSettingsUseCase implements UseCase<AppSettings, NoParams> {
  final ISettingsRepository repository;
  const LoadSettingsUseCase(this.repository);

  @override
  Future<Result<AppSettings>> call(NoParams params) => repository.load();
}

class SaveSettingsUseCase implements UseCase<void, AppSettings> {
  final ISettingsRepository repository;
  const SaveSettingsUseCase(this.repository);

  @override
  Future<Result<void>> call(AppSettings settings) => repository.save(settings);
}

class ResetSettingsUseCase implements UseCase<void, NoParams> {
  final ISettingsRepository repository;
  const ResetSettingsUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.reset();
}

/// Параметр для сохранения выбранного DPI-конфига.
class SelectedConfigParams {
  final String category;
  final String configFile;
  const SelectedConfigParams({required this.category, required this.configFile});
}

class GetSelectedConfigsUseCase {
  final ISettingsRepository repository;
  const GetSelectedConfigsUseCase(this.repository);

  Map<String, String> call() => repository.getSelectedConfigs();
}

class SaveSelectedConfigUseCase implements UseCase<void, SelectedConfigParams> {
  final ISettingsRepository repository;
  const SaveSelectedConfigUseCase(this.repository);

  @override
  Future<Result<void>> call(SelectedConfigParams params) =>
      repository.setSelectedConfig(params.category, params.configFile);
}
