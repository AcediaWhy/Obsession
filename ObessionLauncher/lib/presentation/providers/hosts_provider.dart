import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../domain/entities/hosts_config.dart';
import '../../domain/usecases/hosts_usecases.dart';
import 'dependency_providers.dart';

class HostsState {
  final AiProvider provider;
  final HostsStatus status;
  final String localVersion;
  final String remoteVersion;
  final bool isBusy;
  final String? error;

  const HostsState({
    this.provider = AiProvider.malw,
    this.status = HostsStatus.notInstalled,
    this.localVersion = '',
    this.remoteVersion = '',
    this.isBusy = false,
    this.error,
  });

  HostsState copyWith({
    AiProvider? provider,
    HostsStatus? status,
    String? localVersion,
    String? remoteVersion,
    bool? isBusy,
    String? error,
    bool clearError = false,
  }) =>
      HostsState(
        provider: provider ?? this.provider,
        status: status ?? this.status,
        localVersion: localVersion ?? this.localVersion,
        remoteVersion: remoteVersion ?? this.remoteVersion,
        isBusy: isBusy ?? this.isBusy,
        error: clearError ? null : (error ?? this.error),
      );
}

final hostsProvider = StateNotifierProvider<HostsNotifier, HostsState>((ref) {
  return HostsNotifier(
    checkUseCase: ref.watch(checkHostsStatusUseCaseProvider),
    installUseCase: ref.watch(installHostsUseCaseProvider),
    uninstallUseCase: ref.watch(uninstallHostsUseCaseProvider),
  );
});

class HostsNotifier extends StateNotifier<HostsState> {
  HostsNotifier({
    required this._checkUseCase,
    required this._installUseCase,
    required this._uninstallUseCase,
  })  : super(const HostsState());

  final CheckHostsStatusUseCase _checkUseCase;
  final InstallHostsUseCase _installUseCase;
  final UninstallHostsUseCase _uninstallUseCase;

  Future<void> setProvider(AiProvider provider) async {
    state = state.copyWith(provider: provider, status: HostsStatus.notInstalled);
    await refreshStatus();
  }

  Future<void> refreshStatus() async {
    state = state.copyWith(isBusy: true, clearError: true);
    final result = await _checkUseCase(state.provider);
    result.when(
      success: (config) {
        state = HostsState(
          provider: config.provider,
          status: config.status,
          localVersion: config.localVersion,
          remoteVersion: config.remoteVersion,
          isBusy: false,
        );
      },
      failure: (failure) {
        state = state.copyWith(isBusy: false, error: failure.message);
      },
    );
  }

  Future<void> install() async {
    state = state.copyWith(isBusy: true, clearError: true);
    final result = await _installUseCase(state.provider);
    state = state.copyWith(isBusy: false);
    if (result.isSuccess) {
      await refreshStatus();
    } else {
      state = state.copyWith(
        error: result.failureOrNull?.failure.message ??
            'Не удалось установить ИИ-обход',
      );
    }
  }

  Future<void> uninstall() async {
    state = state.copyWith(isBusy: true, clearError: true);
    final result = await _uninstallUseCase(const NoParams());
    state = state.copyWith(isBusy: false);
    if (result.isSuccess) {
      await refreshStatus();
    } else {
      state = state.copyWith(
        error: result.failureOrNull?.failure.message ??
            'Не удалось удалить ИИ-обход',
      );
    }
  }

  /// Сбрасывает ошибку после показа пользователю.
  void clearError() => state = state.copyWith(clearError: true);
}
