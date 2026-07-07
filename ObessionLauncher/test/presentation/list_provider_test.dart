import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:obsession/core/errors/result.dart';
import 'package:obsession/core/usecases/usecase.dart';
import 'package:obsession/domain/entities/list_entry.dart';
import 'package:obsession/domain/repositories/i_list_repository.dart';
import 'package:obsession/domain/usecases/list_usecases.dart';
import 'package:obsession/presentation/providers/dependency_providers.dart';
import 'package:obsession/presentation/providers/list_provider.dart';

class _FakeListRepo implements IListRepository {
  List<String> names = ['default.txt'];
  final Map<String, List<ListEntry>> _lists = {};
  List<ListEntry>? lastSaved;
  String? lastSavedName;

  @override
  Future<Result<List<String>>> getListNames() async => Success(names);

  @override
  Future<Result<List<ListEntry>>> loadList(String name) async {
    return Success(_lists[name] ?? []);
  }

  @override
  Future<Result<void>> saveList(String name, List<ListEntry> entries) async {
    lastSavedName = name;
    lastSaved = entries;
    _lists[name] = entries;
    return const Success(null);
  }

  @override
  Future<Result<String>> getListPath(String name) async => Success('/fake/$name');

  @override
  Future<Result<void>> backupList(String name) async => const Success(null);
}

class _FakeNamesUseCase extends GetListNamesUseCase {
  _FakeNamesUseCase() : super(_FakeListRepo());
  @override
  Future<Result<List<String>>> call(NoParams params) async => Success(['default.txt', 'custom.txt']);
}

class _FakeLoadUseCase extends LoadListUseCase {
  _FakeLoadUseCase() : super(_FakeListRepo());
  @override
  Future<Result<List<ListEntry>>> call(String name) async => Success([
    ListEntry(value: 'example.com'),
    ListEntry(value: 'invalid value'),
  ]);
}

class _FakeSaveUseCase extends SaveListUseCase {
  SaveListParams? saved;
  _FakeSaveUseCase() : super(_FakeListRepo());
  @override
  Future<Result<void>> call(SaveListParams params) async {
    saved = params;
    return const Success(null);
  }
}

class _FakeImportUseCase extends ImportListUseCase {
  _FakeImportUseCase() : super(_FakeListRepo());
  @override
  Future<Result<List<ListEntry>>> call(ImportListParams params) async => Success([
    ListEntry(value: 'imported.com'),
  ]);
}

class _FakeExportUseCase extends ExportListUseCase {
  ExportListParams? exported;
  _FakeExportUseCase() : super(_FakeListRepo());
  @override
  Future<Result<void>> call(ExportListParams params) async {
    exported = params;
    return const Success(null);
  }
}

ProviderContainer _createContainer() {
  return ProviderContainer(
    overrides: [
      getListNamesUseCaseProvider.overrideWithValue(_FakeNamesUseCase()),
      loadListUseCaseProvider.overrideWithValue(_FakeLoadUseCase()),
      saveListUseCaseProvider.overrideWithValue(_FakeSaveUseCase()),
      validateListEntryUseCaseProvider.overrideWithValue(const ValidateListEntryUseCase()),
      importListUseCaseProvider.overrideWithValue(_FakeImportUseCase()),
      exportListUseCaseProvider.overrideWithValue(_FakeExportUseCase()),
    ],
  );
}

void main() {
  group('ListEditorNotifier', () {
    test('loadNames populates listNames', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(listEditorProvider.notifier);
      await notifier.loadNames();

      expect(container.read(listEditorProvider).listNames, ['default.txt', 'custom.txt']);
    });

    test('selectList loads entries and validates them', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(listEditorProvider.notifier);
      await notifier.selectList('custom.txt');

      final state = container.read(listEditorProvider);
      expect(state.selectedList, 'custom.txt');
      expect(state.entries.length, 2);
      expect(state.validationResults[state.entries[0].id], isTrue);
      expect(state.validationResults[state.entries[1].id], isFalse);
      expect(state.isLoading, isFalse);
    });

    test('addEntry appends and validates', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(listEditorProvider.notifier);
      await notifier.selectList('custom.txt');
      notifier.addEntry(ListEntry(value: 'new.com'));

      await Future.delayed(const Duration(milliseconds: 50));
      final state = container.read(listEditorProvider);
      expect(state.entries.length, 3);
      final added = state.entries.last;
      expect(state.validationResults[added.id], isTrue);
    });

    test('removeEntry deletes and removes validation', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(listEditorProvider.notifier);
      await notifier.selectList('custom.txt');
      final removedId = container.read(listEditorProvider).entries[0].id;
      notifier.removeEntry(0);

      final state = container.read(listEditorProvider);
      expect(state.entries.length, 1);
      expect(state.validationResults.containsKey(removedId), isFalse);
    });

    test('save calls use case with current entries', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(listEditorProvider.notifier);
      await notifier.selectList('custom.txt');
      await notifier.save();

      final saveUseCase = container.read(saveListUseCaseProvider) as _FakeSaveUseCase;
      expect(saveUseCase.saved, isNotNull);
      expect(saveUseCase.saved!.name, 'custom.txt');
      expect(saveUseCase.saved!.entries.length, 2);
    });

    test('clearEntries removes all entries', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(listEditorProvider.notifier);
      await notifier.selectList('custom.txt');
      notifier.clearEntries();

      final state = container.read(listEditorProvider);
      expect(state.entries, isEmpty);
      expect(state.validationResults, isEmpty);
    });

    test('removeDuplicates keeps unique values', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(listEditorProvider.notifier);
      await notifier.selectList('custom.txt');
      notifier.addEntry(ListEntry(value: 'example.com'));
      await Future.delayed(const Duration(milliseconds: 50));
      notifier.removeDuplicates();
      await Future.delayed(const Duration(milliseconds: 50));

      final state = container.read(listEditorProvider);
      expect(state.entries.where((e) => e.value == 'example.com').length, 1);
    });
  });
}
