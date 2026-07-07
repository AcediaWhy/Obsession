import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../domain/entities/proxy_config.dart';
import '../../domain/usecases/proxy_usecases.dart';
import 'dependency_providers.dart';

class ProxyState {
  final bool isActive;
  final bool isAvailable;
  final bool isTransitioning;
  final String proxyLink;
  final ProxyConfig config;
  final String error;

  const ProxyState({
    this.isActive = false,
    this.isAvailable = false,
    this.isTransitioning = false,
    this.proxyLink = '',
    this.config = const ProxyConfig(port: 1443),
    this.error = '',
  });

  ProxyState copyWith({
    bool? isActive,
    bool? isAvailable,
    bool? isTransitioning,
    String? proxyLink,
    ProxyConfig? config,
    String? error,
  }) =>
      ProxyState(
        isActive: isActive ?? this.isActive,
        isAvailable: isAvailable ?? this.isAvailable,
        isTransitioning: isTransitioning ?? this.isTransitioning,
        proxyLink: proxyLink ?? this.proxyLink,
        config: config ?? this.config,
        error: error ?? this.error,
      );
}

final proxyProvider = StateNotifierProvider<ProxyNotifier, ProxyState>((ref) {
  return ProxyNotifier(
    availableUseCase: ref.watch(checkProxyAvailableUseCaseProvider),
    startUseCase: ref.watch(startProxyUseCaseProvider),
    stopUseCase: ref.watch(stopProxyUseCaseProvider),
    linkUseCase: ref.watch(getProxyLinkUseCaseProvider),
    openUseCase: ref.watch(openProxyInTelegramUseCaseProvider),
    copyUseCase: ref.watch(copyProxyLinkUseCaseProvider),
  );
});

class ProxyNotifier extends StateNotifier<ProxyState> {
  ProxyNotifier({
    required this._availableUseCase,
    required this._startUseCase,
    required this._stopUseCase,
    required this._linkUseCase,
    required this._openUseCase,
    required this._copyUseCase,
  })  : super(const ProxyState());

  final CheckProxyAvailableUseCase _availableUseCase;
  final StartProxyUseCase _startUseCase;
  final StopProxyUseCase _stopUseCase;
  final GetProxyLinkUseCase _linkUseCase;
  final OpenProxyInTelegramUseCase _openUseCase;
  final CopyProxyLinkUseCase _copyUseCase;

  void clearError() {
    if (state.error.isNotEmpty) state = state.copyWith(error: '');
  }

  Future<void> checkAvailability() async {
    final result = await _availableUseCase(const NoParams());
    if (!mounted) return;
    state = state.copyWith(
      isAvailable: result.valueOrNull ?? false,
      error: result.isFailure ? (result.failureOrNull?.failure.message ?? '') : state.error,
    );
  }

  void setPort(int port) {
    state = state.copyWith(config: state.config.copyWith(port: port));
  }

  void setFakeTlsDomain(String domain) {
    state = state.copyWith(config: state.config.copyWith(fakeTlsDomain: domain));
  }

  Future<void> start() async {
    // Re-entrancy guard: быстрые повторные тапы по power-button не должны
    // запускать несколько прокси-процессов одновременно.
    if (state.isTransitioning) return;
    state = state.copyWith(isTransitioning: true, isActive: true, proxyLink: '', error: '');
    final result = await _startUseCase(state.config);
    if (!mounted) return;
    if (result.isSuccess) {
      final linkResult = await _linkUseCase(const NoParams());
      if (!mounted) return;
      state = state.copyWith(
        isTransitioning: false,
        isActive: true,
        proxyLink: linkResult.valueOrNull ?? '',
      );
    } else {
      state = state.copyWith(
        isTransitioning: false,
        isActive: false,
        error: result.failureOrNull?.failure.message ?? 'Не удалось запустить прокси',
      );
    }
  }

  Future<void> stop() async {
    if (state.isTransitioning) return;
    state = state.copyWith(isTransitioning: true, isActive: false, error: '');
    await _stopUseCase(const NoParams());
    if (!mounted) return;
    state = state.copyWith(isTransitioning: false, proxyLink: '');
  }

  Future<void> openInTelegram() async {
    final result = await _openUseCase(const NoParams());
    if (!mounted) return;
    result.when(
      success: (_) {},
      failure: (f) => state = state.copyWith(error: f.message),
    );
  }

  Future<void> copyLink() async {
    final result = await _copyUseCase(const NoParams());
    if (!mounted) return;
    result.when(
      success: (_) {},
      failure: (f) => state = state.copyWith(error: f.message),
    );
  }
}
