import 'dart:io' as io;

/// Управление автозапуском через schtasks.
class AutostartLocalDataSource {
  AutostartLocalDataSource();

  static const _taskName = 'Obsession_Autorun';

  String get _exePath => io.Platform.executable;

  Future<bool> isEnabled() async {
    try {
      final result = await io.Process.run(
        'schtasks',
        ['/query', '/tn', _taskName],
        stdoutEncoding: null,
        stderrEncoding: null,
      ).timeout(const Duration(seconds: 10));
      return result.exitCode == 0;
    } catch (_) {
      return false;
    }
  }

  Future<bool> enable() async {
    try {
      // Путь к exe оборачиваем в кавычки внутри значения /tr: без них
      // schtasks некорректно разбирает пути с пробелами (C:\Program Files\...)
      // и задача либо не создаётся, либо запускает не тот файл.
      final action = '"$_exePath"';
      final result = await io.Process.run(
        'schtasks',
        ['/create', '/tn', _taskName, '/tr', action, '/sc', 'onlogon', '/rl', 'highest', '/f'],
        stdoutEncoding: null,
        stderrEncoding: null,
      ).timeout(const Duration(seconds: 15));
      return result.exitCode == 0;
    } catch (_) {
      return false;
    }
  }

  Future<bool> disable() async {
    try {
      final result = await io.Process.run(
        'schtasks',
        ['/delete', '/tn', _taskName, '/f'],
        stdoutEncoding: null,
        stderrEncoding: null,
      ).timeout(const Duration(seconds: 10));
      return result.exitCode == 0;
    } catch (_) {
      return false;
    }
  }
}
