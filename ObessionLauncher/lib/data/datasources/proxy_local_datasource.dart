import 'dart:async';
import 'dart:convert';
import 'dart:io' as io;
import 'dart:math';
import 'dart:typed_data';

import '../../core/constants/app_constants.dart';
import '../../domain/entities/proxy_config.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';

/// Управление TgWsProxy.exe.
class ProxyLocalDataSource {
  ProxyLocalDataSource({
    required this._paths,
    required this._log,
  });

  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  io.Process? _process;
  DateTime? _startedAt;
  String _proxyLink = '';
  StreamSubscription<String>? _stdoutSub;
  StreamSubscription<String>? _stderrSub;

  bool get isRunning => _process != null;

  String get proxyLink => _proxyLink;

  Future<bool> start(ProxyConfig config) async {
    await stop();

    final exePath = _paths.findTgProxyExe();
    if (exePath == null) {
      _log.error('TgWsProxy.exe не найден в bin/. Положите ${AppConstants.tgProxyExe} в папку bin.');
      return false;
    }

    _proxyLink = '';
    final secret = config.secret ?? _generateSecret();

    final args = <String>[
      '--port', config.port.toString(),
      '--secret', secret,
    ];
    if (config.fakeTlsDomain.isNotEmpty) {
      args.addAll(['--fake-tls-domain', config.fakeTlsDomain]);
    }

    _log.info('Запуск Telegram-прокси на порту ${config.port}...');

    try {
      final proc = await io.Process.start(
        exePath,
        args,
        workingDirectory: _paths.binDir,
        runInShell: false,
      );
      _process = proc;
      _startedAt = DateTime.now();

      // Completer срабатывает, как только в stdout появляется ссылка tg://proxy.
      final linkCompleter = Completer<void>();
      _stdoutSub = proc.stdout
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen((line) {
        final msg = line.trim();
        if (msg.isEmpty) return;
        _log.info('[tg] $msg');
        final match = RegExp(r'tg://proxy\?[^\s]+').firstMatch(msg);
        if (match != null && _proxyLink.isEmpty) {
          _proxyLink = match.group(0)!;
          _log.success('Ссылка Telegram: $_proxyLink');
          if (!linkCompleter.isCompleted) linkCompleter.complete();
        }
      });

      _stderrSub = proc.stderr
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .listen((line) {
        final msg = line.trim();
        if (msg.isNotEmpty) _log.error('[tg] $msg');
      });

      unawaited(proc.exitCode.then((code) {
        final livedMs = _startedAt != null
            ? DateTime.now().difference(_startedAt!).inMilliseconds
            : 0;
        if (livedMs < 2000) {
          _log.error('[tg] Прокси умер через $livedMs мс (код $code). Проверьте exe.');
        } else {
          _log.info('[tg] Прокси остановлен (код $code)');
        }
        if (!linkCompleter.isCompleted) {
          linkCompleter.completeError(StateError('Прокси завершился до выдачи ссылки'));
        }
        _cleanup();
      }));

      // Ждём появления ссылки из stdout с таймаутом вместо фиксированной задержки.
      try {
        await linkCompleter.future.timeout(const Duration(seconds: 5));
      } catch (_) {
        if (_proxyLink.isEmpty) {
          _generateLinkManually(config, secret);
        }
      }

      // Достоверный статус: если процесс уже завершился (exitCode-хендлер
      // обнулил _process через _cleanup), запуск НЕ считается успешным —
      // раньше метод возвращал true даже для мгновенно упавшего прокси.
      if (_process == null || identical(_process, proc) == false) {
        _log.error('[tg] Прокси завершился сразу после запуска.');
        return false;
      }

      _log.success('Telegram-прокси запущен на порту ${config.port}');
      return true;
    } catch (e) {
      _log.error('[tg] Не удалось запустить: $e');
      return false;
    }
  }

  Future<void> stop() async {
    final proc = _process;
    if (proc != null) {
      try {
        proc.kill(io.ProcessSignal.sigkill);
      } catch (_) {}
      // Fallback: убиваем только наш PID, не затрагивая сторонние экземпляры.
      try {
        final kill = await io.Process.start(
          'taskkill',
          ['/F', '/PID', proc.pid.toString()],
          mode: io.ProcessStartMode.normal,
          runInShell: false,
        );
        await kill.exitCode.timeout(const Duration(seconds: 2), onTimeout: () {
          kill.kill(io.ProcessSignal.sigkill);
          return -1;
        });
      } catch (_) {}
      _process = null;
    }

    _cleanup();
    _log.info('Telegram-прокси остановлен.');
  }

  Future<bool> openInTelegram() async {
    if (_proxyLink.isEmpty) {
      _log.warning('[tg] Нет ссылки для открытия. Сначала запустите прокси.');
      return false;
    }
    try {
      // Открываем через explorer.exe: ссылка передаётся как отдельный аргумент,
      // поэтому символ '&' в query (tg://proxy?server=...&port=...) не рвёт
      // команду. Прежний вариант `cmd /c start "" <url>` обрезал ссылку на
      // первом '&' (cmd трактует его как разделитель команд).
      final proc = await io.Process.start(
        'explorer',
        [_proxyLink],
        runInShell: false,
        mode: io.ProcessStartMode.detached,
      );
      // explorer.exe возвращает ненулевой код даже при успешном открытии URL,
      // поэтому не полагаемся на exitCode — сам факт запуска считаем успехом.
      _log.info('[tg] Открытие в Telegram...');
      unawaited(proc.exitCode);
      return true;
    } catch (e) {
      _log.warning('[tg] Не удалось открыть Telegram: $e');
      return false;
    }
  }

  Future<void> copyLink() async {
    if (_proxyLink.isEmpty) return;
    try {
      final proc = await io.Process.start('clip', [], mode: io.ProcessStartMode.normal);
      // clip.exe корректно распознаёт UTF-16 LE с BOM.
      proc.stdin.add(_encodeUtf16Le(_proxyLink));
      await proc.stdin.close();
      await proc.exitCode;
      _log.success('[tg] Ссылка скопирована в буфер обмена.');
    } catch (e) {
      _log.error('[tg] Не удалось скопировать: $e');
    }
  }

  Uint8List _encodeUtf16Le(String value) {
    final bytes = <int>[0xFF, 0xFE]; // BOM
    for (final unit in value.codeUnits) {
      bytes.add(unit & 0xFF);
      bytes.add((unit >> 8) & 0xFF);
    }
    return Uint8List.fromList(bytes);
  }

  void _generateLinkManually(ProxyConfig config, String secret) {
    const host = '127.0.0.1';
    if (config.fakeTlsDomain.isNotEmpty) {
      // Кодируем домен в UTF-8 байты (не UTF-16 codeUnits) для совместимости
      // с IDN/Unicode-доменами и форматом Telegram secret=ee...
      final domainHex = utf8.encode(config.fakeTlsDomain)
          .map((b) => b.toRadixString(16).padLeft(2, '0'))
          .join();
      _proxyLink = 'tg://proxy?server=$host&port=${config.port}&secret=ee$secret$domainHex';
    } else {
      _proxyLink = 'tg://proxy?server=$host&port=${config.port}&secret=dd$secret';
    }
    _log.info('Ссылка Telegram (сгенерирована): $_proxyLink');
  }

  String _generateSecret() {
    final rng = Random.secure();
    final bytes = List<int>.generate(16, (_) => rng.nextInt(256));
    return bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  }

  void _cleanup() {
    _stdoutSub?.cancel();
    _stdoutSub = null;
    _stderrSub?.cancel();
    _stderrSub = null;
    _proxyLink = '';
    _process = null;
  }
}
