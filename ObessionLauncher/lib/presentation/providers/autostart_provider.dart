import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../domain/usecases/autostart_usecases.dart';
import 'dependency_providers.dart';

class AutostartState {
  final bool isEnabled;
  final String error;

  const AutostartState({this.isEnabled = false, this.error = ''});

  AutostartState copyWith({bool? isEnabled, String? error}) =>
      AutostartState(isEnabled: isEnabled ?? this.isEnabled, error: error ?? this.error);
}

final autostartProvider = StateNotifierProvider<AutostartNotifier, AutostartState>((ref) {
  return AutostartNotifier(
    checkUseCase: ref.watch(checkAutostartUseCaseProvider),
    toggleUseCase: ref.watch(toggleAutostartUseCaseProvider),
  );
});

class AutostartNotifier extends StateNotifier<AutostartState> {
  AutostartNotifier({
    required this._checkUseCase,
    required this._toggleUseCase,
  })  : super(const AutostartState()) {
    _load();
  }

  final CheckAutostartUseCase _checkUseCase;
  final ToggleAutostartUseCase _toggleUseCase;

  Future<void> _load() async {
    final result = await _checkUseCase(const NoParams());
    state = state.copyWith(isEnabled: result.valueOrNull ?? false);
  }

  Future<void> toggle() async {
    final result = await _toggleUseCase(const NoParams());
    if (result.isSuccess) {
      await _load();
    } else {
      state = state.copyWith(
        error: result.failureOrNull?.failure.message ?? 'Не удалось изменить автозапуск',
      );
    }
  }
}
