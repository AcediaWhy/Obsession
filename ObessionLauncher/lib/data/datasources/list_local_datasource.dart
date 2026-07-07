import 'dart:io' as io;

import 'package:path/path.dart' as p;

import '../../domain/entities/list_entry.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';

/// Управление списками доменов/IP.
class ListLocalDataSource {
  ListLocalDataSource({
    required this._paths,
    required this._log,
  });

  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  List<String> getListNames() => _paths.getListNames();

  Future<List<ListEntry>> loadList(String name) async {
    final path = _paths.listPath(name);
    final file = io.File(path);
    if (!file.existsSync()) return [];

    final lines = await file.readAsLines();
    return lines.map(_parseLine).toList();
  }

  Future<void> saveList(String name, List<ListEntry> entries) async {
    final path = _paths.listPath(name);
    final file = io.File(path);
    await file.create(recursive: true);
    final content = entries.map((e) => e.toString()).join('\n');
    await file.writeAsString(content);
  }

  String getListPath(String name) => _paths.listPath(name);

  Future<void> backupList(String name) async {
    final path = _paths.listPath(name);
    final file = io.File(path);
    if (!file.existsSync()) return;

    final dir = io.Directory(p.join(_paths.backupsDir, 'lists'));
    await dir.create(recursive: true);
    final ts = DateTime.now().toIso8601String().replaceAll(':', '-').split('.').first;
    final backup = io.File(p.join(dir.path, '${name}_$ts.txt'));
    await file.copy(backup.path);
    _log.info('Бэкап списка $name создан: ${backup.path}');
  }

  ListEntry _parseLine(String line) {
    final trimmed = line.trim();
    if (trimmed.isEmpty) return ListEntry.blank();
    if (trimmed.startsWith('#')) {
      return ListEntry.comment(trimmed.replaceFirst(RegExp(r'^#\s*'), ''));
    }
    final parts = trimmed.split(RegExp(r'\s+#\s*'));
    final value = parts[0].trim();
    final comment = parts.length > 1 ? parts.sublist(1).join(' # ').trim() : null;
    return ListEntry(value: value, comment: comment);
  }
}
