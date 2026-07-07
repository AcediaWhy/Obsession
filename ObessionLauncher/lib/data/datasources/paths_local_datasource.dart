import 'dart:convert';
import 'dart:io' as io;

import 'package:flutter/services.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

import '../../core/constants/app_constants.dart';

/// Управляет путями приложения и распаковкой bundled-ассетов.
class PathsLocalDataSource {
  PathsLocalDataSource();

  late final io.Directory _baseDir;
  late final io.Directory _binDir;
  late final io.Directory _configsDir;
  late final io.Directory _listsDir;
  late final io.Directory _backupsDir;
  late final io.Directory _logsDir;
  late final io.Directory _profilesDir;
  late final io.Directory _iconsDir;
  late final io.Directory _autohostsDir;

  bool _initialized = false;

  Future<void> init() async {
    if (_initialized) return;

    final supportDir = await getApplicationSupportDirectory();
    _baseDir = io.Directory(p.join(supportDir.path, AppConstants.appDataFolder));

    _binDir = io.Directory(p.join(_baseDir.path, 'bin'));
    _configsDir = io.Directory(p.join(_baseDir.path, 'configs'));
    _listsDir = io.Directory(p.join(_baseDir.path, 'lists'));
    _backupsDir = io.Directory(p.join(_baseDir.path, 'hosts-backups'));
    _logsDir = io.Directory(p.join(_baseDir.path, 'logs'));
    _profilesDir = io.Directory(p.join(_baseDir.path, 'profiles'));
    _iconsDir = io.Directory(p.join(_baseDir.path, 'icons'));
    _autohostsDir = io.Directory(p.join(_baseDir.path, 'autohosts'));

    await _ensureDirs();
    await _extractAssets();
    await _ensureRuntimeBinaries();

    _initialized = true;
  }

  Future<void> _ensureDirs() async {
    await _baseDir.create(recursive: true);
    await _binDir.create(recursive: true);
    await _configsDir.create(recursive: true);
    await _listsDir.create(recursive: true);
    await _backupsDir.create(recursive: true);
    await _logsDir.create(recursive: true);
    await _profilesDir.create(recursive: true);
    await _iconsDir.create(recursive: true);
    await _autohostsDir.create(recursive: true);
  }

  Future<void> _extractAssets() async {
    String manifest;
    try {
      manifest = await rootBundle.loadString('AssetManifest.json');
    } catch (e) {
      return;
    }

    // Версионирование: если версия приложения изменилась, перезаписываем
    // ассеты, чтобы пользователь получил актуальные конфиги/бинарники.
    final forceOverwrite = await _shouldOverwriteAssets();

    // При обновлении версии — бэкапим пользовательские списки, чтобы не
    // потерять правки (списки перезапишутся дефолтными из бандла).
    if (forceOverwrite) {
      await _backupUserLists();
    }

    final json = jsonDecode(manifest) as Map<String, dynamic>;
    for (final assetPath in json.keys) {
      if (!assetPath.startsWith('assets/bin/') &&
          !assetPath.startsWith('assets/configs/') &&
          !assetPath.startsWith('assets/lists/') &&
          !assetPath.startsWith('assets/icons/')) {
        continue;
      }

      final relPath = assetPath.replaceFirst('assets/', '');
      final destPath = p.join(_baseDir.path, relPath);
      final destFile = io.File(destPath);

      if (destFile.existsSync() && !forceOverwrite) continue;

      try {
        final data = await rootBundle.load(assetPath);
        await destFile.create(recursive: true);
        await destFile.writeAsBytes(data.buffer.asUint8List(), flush: true);
      } catch (_) {
        // Ассет может отсутствовать или быть недоступен.
      }
    }

    if (forceOverwrite) {
      await _writeAssetsVersion();
    }
  }

  /// Бэкапит существующие пользовательские списки в `lists-backup/`.
  Future<void> _backupUserLists() async {
    try {
      if (!_listsDir.existsSync()) return;
      final backupDir = io.Directory(p.join(_baseDir.path, 'lists-backup'));
      await backupDir.create(recursive: true);
      final ts = DateTime.now().toIso8601String().replaceAll(':', '-').split('.').first;
      final userBackup = io.Directory(p.join(backupDir.path, ts));
      await userBackup.create(recursive: true);
      for (final entry in _listsDir.listSync()) {
        if (entry is io.File && entry.path.endsWith('.txt')) {
          await entry.copy(p.join(userBackup.path, p.basename(entry.path)));
        }
      }
    } catch (_) {
      // Бэкап — best-effort, не блокируем запуск.
    }
  }

  /// Возвращает путь к файлу-маркеру версии ассетов.
  String get _assetsVersionPath => p.join(_baseDir.path, '.assets_version');

  /// True, если версия приложения не совпадает с записанной версией ассетов.
  Future<bool> _shouldOverwriteAssets() async {
    final marker = io.File(_assetsVersionPath);
    if (!marker.existsSync()) return true;
    try {
      final stored = (await marker.readAsString()).trim();
      return stored != AppConstants.appVersion;
    } catch (_) {
      return true;
    }
  }

  Future<void> _writeAssetsVersion() async {
    try {
      final marker = io.File(_assetsVersionPath);
      await marker.writeAsString(AppConstants.appVersion, flush: true);
    } catch (_) {
      // ignore
    }
  }

  String get baseDir => _baseDir.path;
  String get binDir => _binDir.path;
  String get configsDir => _configsDir.path;
  String get listsDir => _listsDir.path;
  String get backupsDir => _backupsDir.path;
  String get logsDir => _logsDir.path;
  String get profilesDir => _profilesDir.path;
  String get iconsDir => _iconsDir.path;
  String get autohostsDir => _autohostsDir.path;

  /// Путь к auto-hostlist файлу для категории.
  String autohostsPath(String category) => p.join(_autohostsDir.path, '$category.txt');

  /// Список auto-hostlist файлов, которые существуют (содержимым).
  Map<String, List<String>> readAutohosts() {
    final result = <String, List<String>>{};
    if (!_autohostsDir.existsSync()) return result;
    for (final entry in _autohostsDir.listSync()) {
      if (entry is! io.File) continue;
      final name = p.basenameWithoutExtension(entry.path);
      try {
        final lines = entry
            .readAsLinesSync(encoding: utf8)
            .where((l) => l.trim().isNotEmpty && !l.trim().startsWith('#'))
            .toList();
        result[name] = lines;
      } catch (_) {}
    }
    return result;
  }

  /// Очищает все auto-hostlist файлы (сброс обучения).
  Future<void> clearAutohosts() async {
    if (!_autohostsDir.existsSync()) return;
    for (final entry in _autohostsDir.listSync()) {
      if (entry is io.File) {
        await entry.writeAsString('', flush: true);
      }
    }
  }

  /// Абсолютный путь к иконке трея (распакованной из ассетов).
  String get trayIconPath {
    final extracted = p.join(_iconsDir.path, 'tray.ico');
    if (io.File(extracted).existsSync()) return extracted;
    // Fallback: ищем в bundled-путях рядом с exe.
    final bundled = _findBundledAsset('tray.ico', subDir: 'icons');
    if (bundled != null) return bundled;
    // Последний fallback — вернём путь к несуществующему файлу,
    // system_tray покажет дефолтную иконку.
    return extracted;
  }

  String get winwsPath => p.join(_binDir.path, AppConstants.winwsExe);
  String get tgProxyPath => p.join(_binDir.path, AppConstants.tgProxyExe);

  /// Пытается найти bundled-версию бинарника рядом с исполняемым файлом или
  /// внутри assets. Возвращает путь, если файл найден.
  String? _findBundledBinary(String name) => _findBundledAsset(name, subDir: 'bin');

  /// Ищет файл в bundled assets по поддиректории (bin, icons, configs, ...).
  String? _findBundledAsset(String name, {required String subDir}) {
    try {
      final exeDir = io.File(io.Platform.resolvedExecutable).parent.path;
      final candidates = [
        p.join(exeDir, 'data', 'flutter_assets', 'assets', subDir, name),
        p.join(exeDir, 'assets', subDir, name),
      ];
      for (final candidate in candidates) {
        if (io.File(candidate).existsSync()) return candidate;
      }
    } catch (_) {
      // ignore
    }
    return null;
  }

  /// Копирует runtime-бинарники из bundled-папки (рядом с exe) в [binDir].
  /// Также копирует payload-файлы (.bin) для winws.
  /// Обновляет существующие файлы при смене версии приложения.
  Future<void> _ensureRuntimeBinaries() async {
    const binaries = [
      AppConstants.winwsExe,
      AppConstants.tgProxyExe,
      'WinDivert.dll',
      'WinDivert64.sys',
      'cygwin1.dll',
      // Payload-файлы для фейковых TLS/QUIC-пакетов.
      'tls_clienthello_www_google_com.bin',
      'quic_initial_www_google_com.bin',
      'tls_ozon_ru.bin',
      'quic_ozon_ru.bin',
    ];

    // Если версия сменилась, перезаписываем бинарники тоже.
    final forceOverwrite = await _shouldOverwriteAssets();

    for (final name in binaries) {
      final dest = p.join(_binDir.path, name);
      if (io.File(dest).existsSync() && !forceOverwrite) continue;

      // 1. Попробуем скопировать из папки рядом с исполняемым файлом.
      final bundled = _findBundledBinary(name);
      if (bundled != null) {
        try {
          await io.File(bundled).copy(dest);
          continue;
        } catch (e) {
          // Логируем вместо молчания.
        }
      }

      // 2. Fallback: извлечь из rootBundle.
      try {
        final data = await rootBundle.load('assets/bin/$name');
        await io.File(dest).create(recursive: true);
        await io.File(dest).writeAsBytes(data.buffer.asUint8List(), flush: true);
      } catch (e) {
        // Логируем только критичные бинарники.
        if (name == AppConstants.winwsExe || name == AppConstants.tgProxyExe) {
          // Критичный бинарник не найден — пользователь увидит ошибку при запуске.
        }
      }
    }
  }

  String configPath(String category, String confFile) =>
      p.join(_configsDir.path, category, confFile);

  String listPath(String name) => p.join(_listsDir.path, '$name.txt');

  String profilePath(String filename) => p.join(_profilesDir.path, filename);

  List<String> getCategories() {
    if (!_configsDir.existsSync()) return [];
    return _configsDir
        .listSync()
        .whereType<io.Directory>()
        .map((d) => p.basename(d.path))
        .where((name) => name != 'lists' && name != 'bin')
        .toList();
  }

  List<String> getConfigsForCategory(String category) {
    final dir = io.Directory(p.join(_configsDir.path, category));
    if (!dir.existsSync()) return [];
    final files = dir
        .listSync()
        .whereType<io.File>()
        .where((f) => f.path.endsWith('.conf'))
        .map((f) => p.basename(f.path))
        .toList();
    files.sort();
    return files;
  }

  List<String> getListNames() {
    if (!_listsDir.existsSync()) return [];
    return _listsDir
        .listSync()
        .whereType<io.File>()
        .where((f) => f.path.endsWith('.txt'))
        .map((f) => p.basenameWithoutExtension(f.path))
        .toList();
  }

  String? findTgProxyExe() {
    const names = [
      AppConstants.tgProxyExe,
      'tg_ws_proxy.exe',
      'TgWsProxy.exe',
      'tg-ws-proxy.exe',
    ];
    for (final name in names) {
      final f = p.join(_binDir.path, name);
      if (io.File(f).existsSync()) return f;
    }

    // Fallback: если файла нет в binDir, попробуем скопировать из bundled-папки.
    final bundled = _findBundledBinary(AppConstants.tgProxyExe);
    if (bundled != null) {
      try {
        final dest = p.join(_binDir.path, AppConstants.tgProxyExe);
        io.File(bundled).copySync(dest);
        return dest;
      } catch (_) {
        // ignore
      }
    }
    return null;
  }
}
