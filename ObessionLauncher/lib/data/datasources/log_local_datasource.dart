import 'dart:async';
import 'dart:io' as io;

import 'package:path/path.dart' as p;

import '../../core/constants/app_constants.dart';
import '../../domain/entities/log_entry.dart';
import 'paths_local_datasource.dart';

/// Локальный источник логов: in-memory + файл с ротацией.
class LogLocalDataSource {
  LogLocalDataSource({required this._paths});

  final PathsLocalDataSource _paths;

  static const _maxLogSizeBytes = 5 * 1024 * 1024; // 5 MB
  static const _maxBackupCount = 3;
  static const _rotationCheckInterval = 10;

  final _controller = StreamController<LogEntry>.broadcast();
  final List<LogEntry> _history = [];
  io.IOSink? _logFileSink;
  String? _logFilePath;
  int _writeCount = 0;

  Stream<LogEntry> get stream => _controller.stream;
  List<LogEntry> get history => List.unmodifiable(_history);

  Future<void> init() async {
    _logFilePath = p.join(_paths.logsDir, 'obsession.log');
    await _openSink();
    await _rotateIfNeeded();
  }

  Future<void> _openSink() async {
    final logFile = io.File(_logFilePath!);
    await logFile.create(recursive: true);
    _logFileSink = logFile.openWrite(mode: io.FileMode.append);
  }

  Future<void> _rotateIfNeeded() async {
    final logFile = io.File(_logFilePath!);
    if (!await logFile.exists()) return;
    final size = await logFile.length();
    if (size < _maxLogSizeBytes) return;

    await _logFileSink?.close();
    _logFileSink = null;

    // Удаляем самый старый бэкап.
    final oldest = io.File('$_logFilePath.$_maxBackupCount');
    if (await oldest.exists()) {
      await oldest.delete();
    }

    // Сдвигаем существующие бэкапы.
    for (var i = _maxBackupCount - 1; i >= 1; i--) {
      final source = io.File('$_logFilePath.$i');
      if (await source.exists()) {
        await source.rename('$_logFilePath.${i + 1}');
      }
    }

    // Переименовываем текущий лог.
    await logFile.rename('$_logFilePath.1');

    await _openSink();
  }

  Future<void> _maybeRotate() async {
    _writeCount++;
    if (_writeCount % _rotationCheckInterval != 0) return;
    await _rotateIfNeeded();
  }

  void _emit(LogEntry entry) {
    _history.add(entry);
    if (_history.length > AppConstants.maxLogHistory) {
      _history.removeRange(0, _history.length - AppConstants.maxLogHistory);
    }
    _controller.add(entry);
    _logFileSink?.writeln('[${entry.timestamp}] ${entry.level.name}: ${entry.message}');
    unawaited(_maybeRotate());
  }

  void info(String message) => _emit(LogEntry(message: message, level: LogLevel.info));

  void success(String message) => _emit(LogEntry(message: message, level: LogLevel.success));

  void warning(String message) => _emit(LogEntry(message: message, level: LogLevel.warning));

  void error(
    String message, {
    Object? error,
    StackTrace? stackTrace,
  }) {
    _emit(LogEntry(message: message, level: LogLevel.error));
    if (error != null) {
      _logFileSink?.writeln('ERROR DETAILS: $error');
    }
    if (stackTrace != null) {
      _logFileSink?.writeln(stackTrace.toString());
    }
  }

  void clear() => _history.clear();

  void dispose() {
    _controller.close();
    _logFileSink?.close();
  }
}
