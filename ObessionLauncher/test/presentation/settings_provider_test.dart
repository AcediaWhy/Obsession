import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:obsession/core/errors/result.dart';
import 'package:obsession/core/usecases/usecase.dart';
import 'package:obsession/domain/entities/app_settings.dart';
import 'package:obsession/domain/repositories/i_settings_repository.dart';
import 'package:obsession/domain/usecases/settings_usecases.dart';
import 'package:obsession/presentation/providers/dependency_providers.dart';
import 'package:obsession/presentation/providers/settings_provider.dart';

class _FakeSettingsRepo implements ISettingsRepository {
  AppSettings? saved;
  AppSettings? loaded;
  final Map<String, String> _selectedConfigs = {};

  @override
  Future<Result<AppSettings>> load() async => Success(loaded ?? AppSettings.defaults());

  @override
  Future<Result<void>> save(AppSettings settings) async {
    saved = settings;
    return const Success(null);
  }

  @override
  Future<Result<void>> reset() async => const Success(null);

  @override
  Map<String, String> getSelectedConfigs() => Map.of(_selectedConfigs);

  @override
  Future<Result<void>> setSelectedConfig(String category, String configFile) async {
    _selectedConfigs[category] = configFile;
    return const Success(null);
  }
}

class _FakeLoadSettings extends LoadSettingsUseCase {
  final AppSettings _settings;
  _FakeLoadSettings(this._settings) : super(_FakeSettingsRepo());

  @override
  Future<Result<AppSettings>> call(NoParams params) async => Success(_settings);
}

class _FakeSaveSettings extends SaveSettingsUseCase {
  AppSettings? saved;
  _FakeSaveSettings() : super(_FakeSettingsRepo());

  @override
  Future<Result<void>> call(AppSettings params) async {
    saved = params;
    return const Success(null);
  }
}

class _FakeResetSettings extends ResetSettingsUseCase {
  _FakeResetSettings() : super(_FakeSettingsRepo());

  @override
  Future<Result<void>> call(NoParams params) async => const Success(null);
}

void main() {
  group('SettingsNotifier', () {
    test('load updates state from use case', () async {
      final fakeSettings = AppSettings.defaults().copyWith(accentColor: const Color(0xFF123456));
      final container = ProviderContainer(
        overrides: [
          loadSettingsUseCaseProvider.overrideWithValue(_FakeLoadSettings(fakeSettings)),
          saveSettingsUseCaseProvider.overrideWithValue(_FakeSaveSettings()),
          resetSettingsUseCaseProvider.overrideWithValue(_FakeResetSettings()),
        ],
      );
      addTearDown(container.dispose);

      final notifier = container.read(settingsProvider.notifier);
      await notifier.load();

      expect(container.read(settingsProvider).accentColor, const Color(0xFF123456));
    });

    test('update changes state and calls save', () async {
      final saveUseCase = _FakeSaveSettings();
      final container = ProviderContainer(
        overrides: [
          loadSettingsUseCaseProvider.overrideWithValue(_FakeLoadSettings(AppSettings.defaults())),
          saveSettingsUseCaseProvider.overrideWithValue(saveUseCase),
          resetSettingsUseCaseProvider.overrideWithValue(_FakeResetSettings()),
        ],
      );
      addTearDown(container.dispose);

      final notifier = container.read(settingsProvider.notifier);
      await notifier.load();

      final newSettings = AppSettings.defaults().copyWith(glowIntensity: 0.9);
      await notifier.update(newSettings);

      expect(container.read(settingsProvider).glowIntensity, 0.9);
      expect(saveUseCase.saved?.glowIntensity, 0.9);
    });

    test('reset reverts to defaults', () async {
      final container = ProviderContainer(
        overrides: [
          loadSettingsUseCaseProvider.overrideWithValue(
            _FakeLoadSettings(AppSettings.defaults().copyWith(accentColor: const Color(0xFF123456))),
          ),
          saveSettingsUseCaseProvider.overrideWithValue(_FakeSaveSettings()),
          resetSettingsUseCaseProvider.overrideWithValue(_FakeResetSettings()),
        ],
      );
      addTearDown(container.dispose);

      final notifier = container.read(settingsProvider.notifier);
      await notifier.load();
      await notifier.reset();

      expect(container.read(settingsProvider).accentColor, AppSettings.defaults().accentColor);
    });
  });
}
