import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/datasources/ping_local_datasource.dart';

/// Состояние мониторинга пинга.
class PingMonitorState {
  final Map<String, PingResult> results;
  final bool isMonitoring;

  const PingMonitorState({
    this.results = const {},
    this.isMonitoring = false,
  });

  PingMonitorState copyWith({
    Map<String, PingResult>? results,
    bool? isMonitoring,
  }) =>
      PingMonitorState(
        results: results ?? this.results,
        isMonitoring: isMonitoring ?? this.isMonitoring,
      );
}

/// Мониторит доступность Discord и YouTube.
class PingMonitorNotifier extends StateNotifier<PingMonitorState> {
  PingMonitorNotifier(this._dataSource) : super(const PingMonitorState()) {
    startMonitoring();
  }

  final PingLocalDataSource _dataSource;
  Timer? _timer;
  bool _tickInFlight = false;

  static const _hosts = [
    'discord.com',
    'youtube.com',
  ];

  void startMonitoring() {
    if (_timer != null) return;
    state = state.copyWith(isMonitoring: true);
    _checkAll();
    _timer = Timer.periodic(const Duration(seconds: 5), (_) => _checkAll());
  }

  void stopMonitoring() {
    _timer?.cancel();
    _timer = null;
    state = state.copyWith(isMonitoring: false);
  }

  Future<void> _checkAll() async {
    // Защита от наложения тиков: если предыдущий опрос ещё идёт (медленная
    // сеть), новый не запускается — иначе параллельные вызовы мутируют общий
    // results и возникает race.
    if (_tickInFlight) return;
    _tickInFlight = true;
    try {
      final results = Map<String, PingResult>.from(state.results);
      // Параллельно: раньше пинги шли последовательно, и при медленных
      // таймаутах один тик мог длиться дольше периода (5 c).
      final futures = _hosts.map((host) async {
        final result = await _dataSource.ping(host);
        return MapEntry(
          host,
          result.valueOrNull ?? PingResult.unreachable(host),
        );
      });
      final entries = await Future.wait(futures);
      // mounted-проверка: notifier мог быть disposed за время ожидания.
      if (!mounted) return;
      for (final entry in entries) {
        results[entry.key] = entry.value;
      }
      state = state.copyWith(results: results);
    } finally {
      _tickInFlight = false;
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}

final pingDataSourceProvider = Provider<PingLocalDataSource>((ref) {
  return PingLocalDataSource();
});

final pingMonitorProvider = StateNotifierProvider<PingMonitorNotifier, PingMonitorState>((ref) {
  return PingMonitorNotifier(ref.watch(pingDataSourceProvider));
});
