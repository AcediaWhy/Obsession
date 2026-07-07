import 'dart:io' as io;

import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../entities/list_entry.dart';
import '../repositories/i_list_repository.dart';

class GetListNamesUseCase implements UseCase<List<String>, NoParams> {
  final IListRepository repository;
  const GetListNamesUseCase(this.repository);

  @override
  Future<Result<List<String>>> call(NoParams params) => repository.getListNames();
}

class LoadListUseCase implements UseCase<List<ListEntry>, String> {
  final IListRepository repository;
  const LoadListUseCase(this.repository);

  @override
  Future<Result<List<ListEntry>>> call(String name) => repository.loadList(name);
}

class SaveListParams {
  final String name;
  final List<ListEntry> entries;
  const SaveListParams({required this.name, required this.entries});
}

class SaveListUseCase implements UseCase<void, SaveListParams> {
  final IListRepository repository;
  const SaveListUseCase(this.repository);

  @override
  Future<Result<void>> call(SaveListParams params) =>
      repository.saveList(params.name, params.entries);
}

class ValidateListEntryUseCase implements UseCase<bool, ListEntry> {
  const ValidateListEntryUseCase();

  static final _domainRegex = RegExp(
    r'^(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?\.)*[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])$',
  );
  static final _ipv4Regex = RegExp(
    r'^((25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(25[0-5]|2[0-4]\d|[01]?\d\d?)$',
  );

  @override
  Future<Result<bool>> call(ListEntry entry) async {
    if (entry.isBlank || entry.isComment) return const Success(true);
    final value = entry.value.trim();
    final valid = _domainRegex.hasMatch(value) || _ipv4Regex.hasMatch(value);
    return Success(valid);
  }
}

class ImportListParams {
  final String targetName;
  final String sourcePath;
  const ImportListParams({required this.targetName, required this.sourcePath});
}

class ImportListUseCase implements UseCase<List<ListEntry>, ImportListParams> {
  final IListRepository repository;
  const ImportListUseCase(this.repository);

  @override
  Future<Result<List<ListEntry>>> call(ImportListParams params) async {
    try {
      final file = io.File(params.sourcePath);
      if (!file.existsSync()) {
        return const Failure(FileFailure('Файл не найден'));
      }
      final lines = await file.readAsLines();
      final entries = lines.map(_parseLine).toList();
      await repository.saveList(params.targetName, entries);
      return Success(entries);
    } catch (e, st) {
      return Failure(FileFailure('Не удалось импортировать список', error: e, stackTrace: st));
    }
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

class ExportListParams {
  final String sourceName;
  final String targetPath;
  const ExportListParams({required this.sourceName, required this.targetPath});
}

class ExportListUseCase implements UseCase<void, ExportListParams> {
  final IListRepository repository;
  const ExportListUseCase(this.repository);

  @override
  Future<Result<void>> call(ExportListParams params) async {
    try {
      final result = await repository.loadList(params.sourceName);
      final entries = result.valueOrNull ?? [];
      final content = entries.map((e) => e.toString()).join('\n');
      final file = io.File(params.targetPath);
      await file.create(recursive: true);
      await file.writeAsString(content);
      return const Success(null);
    } catch (e, st) {
      return Failure(FileFailure('Не удалось экспортировать список', error: e, stackTrace: st));
    }
  }
}

