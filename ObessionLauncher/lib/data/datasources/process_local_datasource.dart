import 'dart:async';
import 'dart:convert';
import 'dart:io' as io;

import '../../core/constants/app_constants.dart';
import '../../domain/entities/process_info.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';

/// Управляет запуском и остановкой DPI-процессов winws.
class ProcessLocalDataSource {
  ProcessLocalDataSource({
    required this._paths,
    required this._log,
  });

  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  /// Callback, вызываемый когда процесс завершается (в т.ч. аварийно).
  /// Используется провайдером для обновления isActive в UI.
  ///
  /// Settable: подключается [DpiNotifier] после создания, чтобы избежать
  /// цикла зависимостей между провайдерами (datasource не зависит от
  /// dpiProvider).
  void Function(int pid, String category, int exitCode, bool livedShort)? onProcessDied;

  final Map<int, ProcessInfo> _processes = {};
  final Map<int, StreamSubscription<String>> _stdoutSubs = {};
  final Map<int, StreamSubscription<String>> _stderrSubs = {};

  List<ProcessInfo> get activeProcesses => List.unmodifiable(_processes.values);
  bool get isActive => _processes.isNotEmpty;

  Future<ProcessInfo?> start(String category, String configFile) async {
    final binPath = _paths.winwsPath;
    final confPath = _paths.configPath(category, configFile);
    final basePath = _paths.baseDir;

    if (!io.File(binPath).existsSync()) {
      _log.error('Ядро winws.exe не найдено по пути $binPath');
      return null;
    }
    if (!io.File(confPath).existsSync()) {
      _log.error('Конфиг не найден: $confPath');
      return null;
    }

    _log.info('Запускаю: $category -> $configFile (working dir: $basePath)');
    final stopwatch = Stopwatch()..start();

    try {
      _log.info('ProcessLocalDataSource: before Process.start');
      final proc = await io.Process.start(
        binPath,
        ['@$confPath'],
        workingDirectory: basePath,
        runInShell: false,
      );
      _log.info(
        'ProcessLocalDataSource: Process.start returned PID ${proc.pid} in ${stopwatch.elapsedMilliseconds}ms',
      );

      final info = ProcessInfo(
        pid: proc.pid,
        category: category,
        configFile: configFile,
        startedAt: DateTime.now(),
      );
      _processes[proc.pid] = info;

      final stderrBuffer = <String>[];

      try {
        _stdoutSubs[proc.pid] = proc.stdout
            .transform(utf8.decoder)
            .transform(const LineSplitter())
            .listen((line) {
          final msg = line.trim();
          if (msg.isNotEmpty) _log.info('[$category] $msg');
        });
      } catch (e) {
        _log.warning('[$category] Не удалось подключиться к stdout: $e');
      }

      try {
        _stderrSubs[proc.pid] = proc.stderr
            .transform(utf8.decoder)
            .transform(const LineSplitter())
            .listen((line) {
          final msg = line.trim();
          if (msg.isNotEmpty) {
            stderrBuffer.add(msg);
            if (stderrBuffer.length > 20) stderrBuffer.removeAt(0);
            _log.error('[$category] $msg');
          }
        });
      } catch (e) {
        _log.warning('[$category] Не удалось подключиться к stderr: $e');
      }

      // Проверяем, не завершился ли процесс сразу после запуска.
      final earlyExit = await proc.exitCode.then<int?>((code) => code).timeout(
            const Duration(milliseconds: 500),
            onTimeout: () => null,
          );
      if (earlyExit != null) {
        final livedMs = DateTime.now().difference(info.startedAt).inMilliseconds;
        final stderrText = stderrBuffer.join('\n');
        _log.error(
          '[$category] winws завершился сразу после запуска (код $earlyExit) через $livedMs мс. '
          'Stderr: ${stderrText.isEmpty ? "(нет сообщений)" : stderrText}',
        );
        _cleanup(proc.pid);
        return null;
      }

      unawaited(proc.exitCode.then((code) {
        final livedMs = DateTime.now().difference(info.startedAt).inMilliseconds;
        if (code != 0 && stderrBuffer.isNotEmpty) {
          _log.error(
            '[$category] Последние stderr-сообщения перед завершением:\n'
            '${stderrBuffer.join('\n')}',
          );
        }
        if (livedMs < 2000) {
          _log.error(
            '[$category] ВНИМАНИЕ: winws умер через $livedMs мс (код $code). '
            'Обход НЕ работает. Причины: антивирус, конфликт, некорректный конфиг.',
          );
        } else {
          _log.info('Процесс $category завершился (код $code)');
        }
        _cleanup(proc.pid);
        // Уведомляем провайдер об аварийном завершении, чтобы UI обновился.
        onProcessDied?.call(proc.pid, category, code, livedMs < 2000);
      }));

      return info;
    } catch (e) {
      _log.error('[$category] Не удалось запустить процесс: $e');
      return null;
    }
  }

  Future<bool> stop(int pid) async {
    final info = _processes[pid];
    if (info == null) return false;

    await _killPid(pid);
    _cleanup(pid);
    return true;
  }

  /// Останавливает все **отслеживаемые** (свои) DPI-процессы.
  ///
  /// В отличие от [emergencyKillAllByName], не затрагивает сторонние экземпляры
  /// `winws.exe`, запущенные другими инструментами пользователя. Orphan-процессы
  /// от прошлых запусков (после краша/Task Manager) обнаруживаются через
  /// [detectOrphanedWinws] и очищаются через [emergencyKillAllByName] только
  /// по явному действию пользователя (Emergency Stop / очистка при старте).
  Future<void> stopAll() async {
    _log.info('ProcessLocalDataSource.stopAll: start, tracked PIDs: ${_processes.keys.toList()}');
    final stopwatch = Stopwatch()..start();

    // 1. Убиваем только отслеживаемые (свои) процессы.
    final pids = _processes.keys.toList();
    for (final pid in pids) {
      await _killPid(pid);
      _cleanup(pid);
    }

    _log.info(
      'ProcessLocalDataSource.stopAll: tracked PIDs cleaned in ${stopwatch.elapsedMilliseconds}ms',
    );

    // 2. Даём драйверу WinDivert время выгрузиться (иначе конфликт фильтров).
    await Future.delayed(const Duration(milliseconds: 500));

    // Сбрасываем DNS кэш без ожидания завершения, чтобы UI не замирал.
    try {
      final flush = await io.Process.start(
        'ipconfig',
        ['/flushdns'],
        runInShell: false,
      );
      unawaited(
        flush.exitCode
            .timeout(const Duration(seconds: 1), onTimeout: () {
          flush.kill(io.ProcessSignal.sigkill);
          return -1;
        })
            .then(
          (_) => _log.info('DNS кэш очищен.'),
          onError: (_) {},
        ),
      );
    } catch (_) {}
  }

  /// Обнаруживает orphan-процессы `winws.exe` в системе, не запущенные этим
  /// приложением. Возвращает список их PID (пусто, если orphan нет).
  ///
  /// Используется при старте приложения, чтобы предложить пользователю очистить
  /// остатки от крашнувшегося предыдущего запуска (иначе новый winws упадёт с
  /// "a copy of winws is already running with the same filter").
  Future<List<int>> detectOrphanedWinws() async {
    try {
      final result = await io.Process.run(
        'tasklist',
        ['/FI', 'IMAGENAME eq ${AppConstants.winwsExe}', '/NH', '/FO', 'CSV'],
        stdoutEncoding: io.systemEncoding,
        stderrEncoding: io.systemEncoding,
      ).timeout(const Duration(seconds: 2));
      if (result.exitCode != 0) return const [];
      final out = result.stdout as String;
      final orphanPids = <int>[];
      // CSV-строка вида "winws.exe","1234","Console","1","12 345 K".
      for (final line in out.split('\n')) {
        final trimmed = line.trim();
        if (trimmed.isEmpty) continue;
        final match = RegExp(r'"(\d+)"').firstMatch(trimmed);
        if (match == null) continue;
        final pid = int.tryParse(match.group(1)!);
        if (pid == null) continue;
        // Исключаем процессы, которыми управляем сами.
        if (_processes.containsKey(pid)) continue;
        orphanPids.add(pid);
      }
      return orphanPids;
    } catch (e) {
      _log.warning('Не удалось обнаружить orphan winws: $e');
      return const [];
    }
  }

  /// Принудительно убивает **все** процессы `winws.exe` в системе — включая
  /// сторонние экземпляры, запущенные другими DPI-инструментами пользователя.
  ///
  /// Использовать ТОЛЬКО как крайнюю меру по явному действию пользователя:
  /// Emergency Stop или подтверждённая очистка orphan-процессов при старте.
  Future<void> emergencyKillAllByName() async {
    _log.warning(
      'EMERGENCY: принудительное завершение ВСЕХ процессов ${AppConstants.winwsExe} в системе '
      '(включая сторонние экземпляры других DPI-инструментов).',
    );
    try {
      final kill = await io.Process.start(
        'taskkill',
        ['/F', '/IM', AppConstants.winwsExe],
        runInShell: false,
      );
      final code = await kill.exitCode.timeout(
        const Duration(milliseconds: 800),
        onTimeout: () {
          kill.kill(io.ProcessSignal.sigkill);
          return -1;
        },
      );
      if (code == 0 || code == 128) {
        _log.info('Все процессы winws.exe остановлены (emergency).');
      } else {
        _log.warning('taskkill завершился с кодом $code');
      }
      // Очищаем локальный трекинг — всё убито.
      final pids = _processes.keys.toList();
      for (final pid in pids) {
        _cleanup(pid);
      }
    } catch (e) {
      _log.warning('taskkill не выполнен: $e');
    }
  }

  Future<void> _killPid(int pid) async {
    try {
      final killed = io.Process.killPid(pid);
      if (!killed) {
        // Fallback на taskkill по PID без блокировки UI.
        final kill = await io.Process.start(
          'taskkill',
          ['/F', '/PID', pid.toString()],
          runInShell: false,
        );
        unawaited(
          kill.exitCode
              .timeout(const Duration(milliseconds: 300), onTimeout: () {
            kill.kill(io.ProcessSignal.sigkill);
            return -1;
          })
              .catchError((_) => -1),
        );
      }
    } catch (e) {
      _log.warning('Не удалось остановить PID $pid: $e');
    }
  }

  void _cleanup(int pid) {
    _processes.remove(pid);
    _stdoutSubs[pid]?.cancel();
    _stdoutSubs.remove(pid);
    _stderrSubs[pid]?.cancel();
    _stderrSubs.remove(pid);
  }

  Future<bool> isRunning(int pid) async {
    if (!_processes.containsKey(pid)) return false;
    try {
      // Асинхронная проверка, не блокирует UI-поток.
      final result = await io.Process.run(
        'tasklist',
        ['/FI', 'PID eq $pid', '/NH', '/FO', 'CSV'],
        stdoutEncoding: io.systemEncoding,
        stderrEncoding: io.systemEncoding,
      ).timeout(const Duration(seconds: 5));
      if (result.exitCode != 0) return false;
      final out = result.stdout as String;
      // Локаль-независимая проверка: при живом процессе tasklist в режиме CSV
      // выводит строку вида "winws.exe","1234","Console","1","12 345 K".
      // Ищем CSV-запись, где ВТОРОЕ поле равно искомому PID, а не подстроку —
      // это исключает зависимость от локализованных INFO-сообщений
      // ("no tasks"/"нет задач"/"не существует" и т.п. в других языках).
      final pidField = RegExp('^"[^"]*","$pid"', multiLine: true);
      return pidField.hasMatch(out);
    } catch (e) {
      _log.warning('Не удалось проверить статус PID $pid: $e');
      return false;
    }
  }
}
