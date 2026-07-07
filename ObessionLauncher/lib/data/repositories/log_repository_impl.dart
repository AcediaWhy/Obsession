import '../../domain/entities/log_entry.dart';
import '../../domain/repositories/i_log_repository.dart';
import '../datasources/log_local_datasource.dart';

class LogRepositoryImpl implements ILogRepository {
  LogRepositoryImpl({required this._dataSource});

  final LogLocalDataSource _dataSource;

  @override
  Stream<LogEntry> get stream => _dataSource.stream;

  @override
  List<LogEntry> get history => _dataSource.history;

  @override
  void info(String message) => _dataSource.info(message);

  @override
  void success(String message) => _dataSource.success(message);

  @override
  void warning(String message) => _dataSource.warning(message);

  @override
  void error(String message, {Object? error, StackTrace? stackTrace}) =>
      _dataSource.error(message, error: error, stackTrace: stackTrace);

  @override
  void clear() => _dataSource.clear();

  @override
  void dispose() => _dataSource.dispose();
}
