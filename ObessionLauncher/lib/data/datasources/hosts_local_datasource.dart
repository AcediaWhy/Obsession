import 'dart:convert';
import 'dart:io' as io;

import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;

import '../../core/constants/app_constants.dart';
import '../../domain/entities/hosts_config.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';

/// Управление hosts-файлом и ИИ-обходом.
class HostsLocalDataSource {
  HostsLocalDataSource({
    required this._paths,
    required this._log,
  });

  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  String get _hostsPath => AppConstants.hostsPath;

  /// Максимальный допустимый размер скачиваемого hosts-файла (10 МиБ).
  /// Защита от записи аномально большого/вредоносного ответа в системный hosts.
  static const int _maxHostsBytes = 10 * 1024 * 1024;

  Future<String> readHosts() async {
    try {
      final file = io.File(_hostsPath);
      if (!file.existsSync()) return '';
      return await file.readAsString(encoding: utf8);
    } catch (e) {
      _log.error('Не удалось прочитать hosts: $e');
      return '';
    }
  }

  /// Уникальный маркер, который приложение записывает в hosts при установке,
  /// чтобы надёжно отличать «свою» установку от ручных правок пользователя.
  static const _markerPrefix = '# obsession:ai-provider=';

  String _marker(AiProvider provider) => '$_markerPrefix${provider.name}';

  Future<bool> isInstalled(AiProvider provider) async {
    final content = await readHosts();
    // Приоритет — собственному маркеру приложения.
    if (content.contains(_marker(provider))) return true;
    // Обратная совместимость: проверяем по сигнатурным DNS-записям для
    // установок, сделанных предыдущими версиями без маркера.
    if (provider == AiProvider.geohide) {
      return content.contains('dns.geohide.ru');
    }
    return content.contains('dns.malw.link') && !content.contains('dns.geohide.ru');
  }

  Future<String> downloadHosts(AiProvider provider) async {
    try {
      final resp = await http.get(
        Uri.parse(provider.hostsUrl),
        headers: {'User-Agent': '${AppConstants.appName}/${AppConstants.appVersion}'},
      ).timeout(const Duration(seconds: 30));
      if (resp.statusCode != 200) throw Exception('HTTP ${resp.statusCode}');
      if (resp.bodyBytes.length > _maxHostsBytes) {
        _log.error('hosts-файл (${provider.label}) слишком большой '
            '(${resp.bodyBytes.length} байт) — отклонён.');
        return '';
      }
      return resp.body;
    } catch (e) {
      _log.error('Не удалось скачать hosts (${provider.label}): $e');
      return '';
    }
  }

  Future<String> downloadAdditionalHosts() async {
    try {
      final resp = await http.get(
        Uri.parse(AppConstants.additionalHostsUrl),
        headers: {'User-Agent': '${AppConstants.appName}/${AppConstants.appVersion}'},
      ).timeout(const Duration(seconds: 30));
      if (resp.statusCode != 200) return '';
      final data = jsonDecode(resp.body) as Map<String, dynamic>;
      final version = data['version'] as String? ?? '';
      final hostsBlock = (data['hosts'] as String? ?? '').trim();
      if (hostsBlock.isEmpty) return '';
      return '# additional_hosts_version $version\n$hostsBlock';
    } catch (e) {
      _log.warning('Не удалось скачать additional_hosts: $e');
      return '';
    }
  }

  Future<io.File?> backupHosts(String action) async {
    try {
      final src = io.File(_hostsPath);
      if (!src.existsSync()) return null;
      final data = await src.readAsBytes();
      final dir = io.Directory(_paths.backupsDir);
      await dir.create(recursive: true);
      final ts = DateTime.now().toIso8601String().replaceAll(':', '-').split('.').first;
      final name = 'hosts_backup_${action}_$ts.txt';
      final backup = io.File(p.join(dir.path, name));
      final header = utf8.encode(
        '# Obsession hosts backup\n# action: $action\n# created_at: ${DateTime.now()}\n# source: $_hostsPath\n\n',
      );
      await backup.writeAsBytes(header + data);
      _log.info('Бэкап hosts создан: ${backup.path}');
      return backup;
    } catch (e) {
      _log.error('Ошибка бэкапа hosts: $e');
      return null;
    }
  }

  Future<bool> applyHosts(String content) async {
    final file = io.File(_hostsPath);
    // Атомарная запись: пишем во временный файл рядом с hosts, затем
    // переименовываем поверх. Rename в пределах одного тома атомарен на NTFS,
    // поэтому системный hosts никогда не остаётся в полузаписанном состоянии
    // (защита от повреждения критического файла ОС при краше/сбое питания).
    final tmp = io.File('$_hostsPath.obsession.tmp');
    try {
      // Явный UTF-8 (без BOM) — согласовано с чтением в readHosts.
      await tmp.writeAsBytes(utf8.encode(content), flush: true);
      try {
        await tmp.rename(_hostsPath);
      } on io.FileSystemException {
        // На некоторых конфигурациях rename поверх существующего файла может
        // не сработать — используем прямую перезапись как fallback.
        await file.writeAsBytes(utf8.encode(content), flush: true);
        await _deleteQuietly(tmp);
      }
      await io.Process.run(
        'ipconfig',
        ['/flushdns'],
        runInShell: false,
      ).timeout(const Duration(seconds: 10), onTimeout: () {
        _log.warning('ipconfig /flushdns превысил время ожидания.');
        return io.ProcessResult(0, 0, '', '');
      });
      _log.success('hosts-файл обновлён, DNS кэш очищен.');
      return true;
    } catch (e) {
      _log.error('Не удалось записать hosts (нет прав?): $e');
      await _deleteQuietly(tmp);
      return false;
    }
  }

  Future<void> _deleteQuietly(io.File f) async {
    try {
      if (await f.exists()) await f.delete();
    } catch (_) {}
  }

  Future<bool> install(AiProvider provider) async {
    _log.info('Установка ИИ-обхода (${provider.label})...');
    if (await backupHosts('install') == null) {
      _log.error('Не удалось создать бэкап, установка отменена.');
      return false;
    }

    final hosts = await downloadHosts(provider);
    if (hosts.isEmpty) {
      _log.error('Не удалось скачать hosts-файл.');
      return false;
    }

    final additional = await downloadAdditionalHosts();
    final combined = additional.isEmpty ? hosts : '$hosts\n$additional';
    // Записываем собственный маркер, чтобы потом надёжно определить установку.
    final marked = '${_marker(provider)}\n$combined';
    return applyHosts(marked);
  }

  Future<bool> uninstall() async {
    _log.info('Удаление ИИ-обхода...');
    // Восстанавливаем последний бэкап, а не захардкоженный шаблон,
    // чтобы не уничтожить пользовательские записи в hosts.
    final backup = await backupHosts('uninstall');
    if (backup == null) {
      _log.error('Не удалось создать бэкап, удаление отменено.');
      return false;
    }

    final restored = await _restoreLatestBackup();
    if (restored) {
      _log.success('hosts-файл восстановлен из бэкапа.');
      return true;
    }

    // Если бэкапов для восстановления нет — пишем стандартный шаблон.
    _log.warning('Бэкапов для восстановления не найдено, записываю стандартный hosts.');
    const defaultHosts = '# Copyright (c) 1993-2009 Microsoft Corp.\n'
        '#\n'
        '# This is a sample HOSTS file used by Microsoft TCP/IP for Windows.\n'
        '#\n'
        '# localhost name resolution is handled within DNS itself.\n'
        '#   127.0.0.1       localhost\n'
        '#   ::1             localhost';
    return applyHosts(defaultHosts);
  }

  /// Восстанавливает hosts из последнего бэкапа (исключая только что созданный).
  Future<bool> _restoreLatestBackup() async {
    try {
      final dir = io.Directory(_paths.backupsDir);
      if (!dir.existsSync()) return false;
      final files = dir
          .listSync()
          .whereType<io.File>()
          .where((f) => f.path.endsWith('.txt') && f.path.contains('hosts_backup_'))
          .toList()
        ..sort((a, b) => b.path.compareTo(a.path));
      if (files.isEmpty) return false;
      // Берём предпоследний бэкап (последний — только что созданный перед uninstall).
      final target = files.length > 1 ? files[1] : files[0];
      final content = await target.readAsString(encoding: utf8);
      // Убираем заголовок бэкапа.
      final lines = content.split('\n');
      final hostLines = <String>[];
      var skipHeader = true;
      for (final line in lines) {
        if (skipHeader &&
            (line.startsWith('# Obsession') ||
                line.startsWith('# action') ||
                line.startsWith('# created_at') ||
                line.startsWith('# source') ||
                line.trim().isEmpty)) {
          continue;
        }
        skipHeader = false;
        hostLines.add(line);
      }
      return applyHosts(hostLines.join('\n').trim());
    } catch (e) {
      _log.warning('Не удалось восстановить hosts из бэкапа: $e');
      return false;
    }
  }

  Future<HostsConfig> checkStatus(AiProvider provider) async {
    final installed = await isInstalled(provider);
    if (!installed) {
      return HostsConfig(provider: provider, status: HostsStatus.notInstalled);
    }

    final content = await readHosts();
    final localMatch = RegExp(r'# update:\s*(.+)').firstMatch(content);
    final localVersion = localMatch?.group(1)?.trim() ?? '';

    String remoteVersion = '';
    bool networkError = false;
    try {
      final req = http.Request(
        'GET',
        Uri.parse('${provider.hostsUrl}?t=${DateTime.now().millisecondsSinceEpoch}'),
      )
        ..headers['User-Agent'] = '${AppConstants.appName}/${AppConstants.appVersion}'
        ..headers['Range'] = 'bytes=0-1024';
      final resp = await http.Client().send(req).timeout(const Duration(seconds: 15));
      final body = await resp.stream.bytesToString();
      final remoteMatch = RegExp(r'# update:\s*(.+)').firstMatch(body);
      remoteVersion = remoteMatch?.group(1)?.trim() ?? '';
    } catch (_) {
      networkError = true;
    }

    // Если не удалось получить remote-версию — статус offline (не outdated),
    // чтобы не вводить пользователя в заблуждение при отсутствии интернета.
    final HostsStatus status;
    if (networkError) {
      status = HostsStatus.offline;
    } else if (localVersion.isNotEmpty && localVersion == remoteVersion) {
      status = HostsStatus.installed;
    } else {
      status = HostsStatus.outdated;
    }

    return HostsConfig(
      provider: provider,
      status: status,
      localVersion: localVersion,
      remoteVersion: remoteVersion,
    );
  }
}
