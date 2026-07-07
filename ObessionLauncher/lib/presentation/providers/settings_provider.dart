import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../domain/entities/app_settings.dart';
import '../../domain/usecases/settings_usecases.dart';
import 'dependency_providers.dart';

final settingsProvider = StateNotifierProvider<SettingsNotifier, AppSettings>((ref) {
  return SettingsNotifier(
    loadUseCase: ref.watch(loadSettingsUseCaseProvider),
    saveUseCase: ref.watch(saveSettingsUseCaseProvider),
    resetUseCase: ref.watch(resetSettingsUseCaseProvider),
  );
});

class SettingsNotifier extends StateNotifier<AppSettings> {
  SettingsNotifier({
    required this._loadUseCase,
    required this._saveUseCase,
    required this._resetUseCase,
    AppSettings? initialState,
  })  : super(initialState ?? AppSettings.defaults());

  final LoadSettingsUseCase _loadUseCase;
  final SaveSettingsUseCase _saveUseCase;
  final ResetSettingsUseCase _resetUseCase;

  String _saveError = '';
  String get saveError => _saveError;

  Future<void> load() async {
    final result = await _loadUseCase(const NoParams());
    result.when(
      success: (settings) => state = settings,
      failure: (_) {},
    );
  }

  Future<void> update(AppSettings settings) async {
    final oldState = state;
    state = settings; // Оптимистичное обновление UI.
    final result = await _saveUseCase(settings);
    if (result.isFailure) {
      // Откатываем, если сохранение не удалось — UI и диск согласованы.
      _saveError = result.failureOrNull?.failure.message ?? 'Не удалось сохранить настройки';
      state = oldState;
    } else {
      _saveError = '';
    }
  }

  Future<void> reset() async {
    final result = await _resetUseCase(const NoParams());
    result.when(
      success: (_) => state = AppSettings.defaults(),
      failure: (_) {},
    );
  }
}
