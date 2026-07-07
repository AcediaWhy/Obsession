import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/entities/log_entry.dart';
import '../../domain/repositories/i_log_repository.dart';
import 'dependency_providers.dart';

final logRepositoryRiverpodProvider = Provider<ILogRepository>((ref) {
  return ref.watch(logRepositoryProvider);
});

final logStreamProvider = StreamProvider<LogEntry>((ref) {
  return ref.watch(logRepositoryRiverpodProvider).stream;
});

final logHistoryProvider = Provider<List<LogEntry>>((ref) {
  ref.watch(logStreamProvider);
  return ref.watch(logRepositoryRiverpodProvider).history;
});
