import '../entities/log_entry.dart';

/// Репозиторий для логирования.
abstract class ILogRepository {
  Stream<LogEntry> get stream;
  List<LogEntry> get history;

  void info(String message);
  void success(String message);
  void warning(String message);
  void error(String message, {Object? error, StackTrace? stackTrace});
  void clear();
  void dispose();
}
