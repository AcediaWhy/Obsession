import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../domain/entities/dpi_config.dart';
import '../../domain/entities/profile.dart';
import '../../domain/usecases/profile_usecases.dart';
import 'dependency_providers.dart';
import 'dpi_provider.dart';
import 'hosts_provider.dart';
import 'proxy_provider.dart';
import 'settings_provider.dart';

class ProfileState {
  final List<Profile> profiles;
  final Profile? activeProfile;
  final bool isLoading;
  final String? error;

  const ProfileState({
    this.profiles = const [],
    this.activeProfile,
    this.isLoading = false,
    this.error,
  });

  ProfileState copyWith({
    List<Profile>? profiles,
    Profile? activeProfile,
    bool? isLoading,
    String? error,
    bool clearActiveProfile = false,
    bool clearError = false,
  }) =>
      ProfileState(
        profiles: profiles ?? this.profiles,
        activeProfile:
            clearActiveProfile ? null : (activeProfile ?? this.activeProfile),
        isLoading: isLoading ?? this.isLoading,
        error: clearError ? null : (error ?? this.error),
      );
}

final profileProvider = StateNotifierProvider<ProfileNotifier, ProfileState>((ref) {
  return ProfileNotifier(
    getProfilesUseCase: ref.watch(getProfilesUseCaseProvider),
    getActiveUseCase: ref.watch(getActiveProfileUseCaseProvider),
    createUseCase: ref.watch(createProfileUseCaseProvider),
    saveUseCase: ref.watch(saveProfileUseCaseProvider),
    deleteUseCase: ref.watch(deleteProfileUseCaseProvider),
    switchUseCase: ref.watch(switchProfileUseCaseProvider),
    settingsNotifier: ref.watch(settingsProvider.notifier),
    dpiNotifier: ref.watch(dpiProvider.notifier),
    hostsNotifier: ref.watch(hostsProvider.notifier),
    proxyNotifier: ref.watch(proxyProvider.notifier),
  );
});

class ProfileNotifier extends StateNotifier<ProfileState> {
  ProfileNotifier({
    required this._getProfilesUseCase,
    required this._getActiveUseCase,
    required this._createUseCase,
    required this._saveUseCase,
    required this._deleteUseCase,
    required this._switchUseCase,
    required this._settingsNotifier,
    required this._dpiNotifier,
    required this._hostsNotifier,
    required this._proxyNotifier,
  })  : super(const ProfileState());

  final GetProfilesUseCase _getProfilesUseCase;
  final GetActiveProfileUseCase _getActiveUseCase;
  final CreateProfileUseCase _createUseCase;
  final SaveProfileUseCase _saveUseCase;
  final DeleteProfileUseCase _deleteUseCase;
  final SwitchProfileUseCase _switchUseCase;
  final SettingsNotifier _settingsNotifier;
  final DpiNotifier _dpiNotifier;
  final HostsNotifier _hostsNotifier;
  final ProxyNotifier _proxyNotifier;

  Future<void> load() async {
    state = state.copyWith(isLoading: true, clearError: true);
    final profilesResult = await _getProfilesUseCase(const NoParams());
    final activeResult = await _getActiveUseCase(const NoParams());
    final active = activeResult.valueOrNull;
    state = ProfileState(
      profiles: profilesResult.valueOrNull ?? const [],
      activeProfile: active,
      isLoading: false,
    );
  }

  Future<void> create(String name) async {
    final result = await _createUseCase(name);
    result.when(
      success: (_) => load(),
      failure: (f) => state = state.copyWith(error: f.message),
    );
  }

  Future<void> save(Profile profile) async {
    final result = await _saveUseCase(profile);
    result.when(
      success: (_) => load(),
      failure: (f) => state = state.copyWith(error: f.message),
    );
  }

  Future<void> delete(String id) async {
    final result = await _deleteUseCase(id);
    result.when(
      success: (_) => load(),
      failure: (f) => state = state.copyWith(error: f.message),
    );
  }

  Future<void> switchProfile(String id) async {
    if (state.isLoading) return;
    state = state.copyWith(isLoading: true, clearError: true);
    // Сохраняем текущее состояние в активный профиль перед переключением.
    await _saveCurrentState();

    final result = await _switchUseCase(id);
    await result.when(
      success: (_) async {
        await load();
        await _applyActiveProfile();
      },
      failure: (f) async {
        state = state.copyWith(isLoading: false, error: f.message);
      },
    );
  }

  void clearError() {
    if (state.error != null) state = state.copyWith(clearError: true);
  }

  Future<void> _saveCurrentState() async {
    final active = state.activeProfile;
    if (active == null) return;

    final updated = active.copyWith(
      settings: _settingsNotifier.state,
      dpiConfigs: _dpiNotifier.state.selectedCategories
          .where((cat) => _dpiNotifier.state.selectedConfigs.containsKey(cat))
          .map(
            (cat) => DpiConfig(
              category: cat,
              configFile: _dpiNotifier.state.selectedConfigs[cat]!,
            ),
          )
          .toSet(),
      hostsProvider: _hostsNotifier.state.provider,
      proxyConfig: _proxyNotifier.state.config,
      updatedAt: DateTime.now(),
    );
    await _saveUseCase(updated);
  }

  Future<void> _applyActiveProfile() async {
    final active = state.activeProfile;
    if (active == null) return;

    await _settingsNotifier.update(active.settings);
    _hostsNotifier.setProvider(active.hostsProvider);
    _proxyNotifier.setPort(active.proxyConfig.port);
    _proxyNotifier.setFakeTlsDomain(active.proxyConfig.fakeTlsDomain);
    _dpiNotifier.applyProfileConfigs(active.dpiConfigs);
  }
}
