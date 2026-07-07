import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/errors/result.dart';
import '../../data/datasources/tray_local_datasource.dart';
import '../../domain/repositories/i_tray_repository.dart';
import 'dependency_providers.dart';

class TrayState {
  final bool isInitialized;

  const TrayState({this.isInitialized = false});

  TrayState copyWith({bool? isInitialized}) =>
      TrayState(isInitialized: isInitialized ?? this.isInitialized);
}

final trayProvider = StateNotifierProvider<TrayNotifier, TrayState>((ref) {
  return TrayNotifier(repository: ref.watch(trayRepositoryProvider));
});

class TrayNotifier extends StateNotifier<TrayState> {
  TrayNotifier({required this._repository})
      : super(const TrayState());

  final ITrayRepository _repository;

  Future<void> init({String locale = 'ru'}) async {
    final labels = TrayLabels.forLocale(locale);
    final result = await _repository.init(labels: labels);
    if (result is Success<void>) {
      state = state.copyWith(isInitialized: true);
    }
  }

  Future<void> show() async {
    await _repository.showWindow();
  }

  Future<void> hide() async {
    await _repository.hideToTray();
  }
}
