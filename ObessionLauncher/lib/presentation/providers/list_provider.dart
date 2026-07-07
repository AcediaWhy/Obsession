import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../domain/entities/list_entry.dart';
import '../../domain/usecases/list_usecases.dart';
import 'dependency_providers.dart';

class ListEditorState {
  final List<String> listNames;
  final String? selectedList;
  final List<ListEntry> entries;
  final bool isLoading;
  final String? error;

  /// Результаты валидации по стабильному [ListEntry.id] (не по индексу),
  /// чтобы удаление/вставка строк не рассинхронизировали иконки.
  final Map<int, bool> validationResults;

  const ListEditorState({
    this.listNames = const [],
    this.selectedList,
    this.entries = const [],
    this.isLoading = false,
    this.error,
    this.validationResults = const {},
  });

  ListEditorState copyWith({
    List<String>? listNames,
    String? selectedList,
    List<ListEntry>? entries,
    bool? isLoading,
    String? error,
    Map<int, bool>? validationResults,
    bool clearSelectedList = false,
    bool clearError = false,
  }) =>
      ListEditorState(
        listNames: listNames ?? this.listNames,
        selectedList:
            clearSelectedList ? null : (selectedList ?? this.selectedList),
        entries: entries ?? this.entries,
        isLoading: isLoading ?? this.isLoading,
        error: clearError ? null : (error ?? this.error),
        validationResults: validationResults ?? this.validationResults,
      );
}

final listEditorProvider = StateNotifierProvider<ListEditorNotifier, ListEditorState>((ref) {
  return ListEditorNotifier(
    namesUseCase: ref.watch(getListNamesUseCaseProvider),
    loadUseCase: ref.watch(loadListUseCaseProvider),
    saveUseCase: ref.watch(saveListUseCaseProvider),
    validateUseCase: ref.watch(validateListEntryUseCaseProvider),
    importUseCase: ref.watch(importListUseCaseProvider),
    exportUseCase: ref.watch(exportListUseCaseProvider),
  );
});

class ListEditorNotifier extends StateNotifier<ListEditorState> {
  ListEditorNotifier({
    required this._namesUseCase,
    required this._loadUseCase,
    required this._saveUseCase,
    required this._validateUseCase,
    required this._importUseCase,
    required this._exportUseCase,
  })  : super(const ListEditorState());

  final GetListNamesUseCase _namesUseCase;
  final LoadListUseCase _loadUseCase;
  final SaveListUseCase _saveUseCase;
  final ValidateListEntryUseCase _validateUseCase;
  final ImportListUseCase _importUseCase;
  final ExportListUseCase _exportUseCase;

  Future<void> loadNames() async {
    final result = await _namesUseCase(const NoParams());
    state = state.copyWith(listNames: result.valueOrNull ?? []);
  }

  /// Присваивает стабильные id только что загруженным записям (из файла/импорта).
  List<ListEntry> _withIds(List<ListEntry> entries) =>
      entries.map((e) => e.ensureId()).toList();

  Future<void> selectList(String name) async {
    state = state.copyWith(selectedList: name, isLoading: true, clearError: true);
    final result = await _loadUseCase(name);
    await result.when(
      success: (loaded) async {
        final entries = _withIds(loaded);
        state = state.copyWith(
          selectedList: name,
          entries: entries,
          isLoading: false,
        );
        await _validateAll();
      },
      failure: (f) async {
        state = state.copyWith(isLoading: false, error: f.message);
      },
    );
  }

  void updateEntry(int index, ListEntry entry) {
    final entries = List<ListEntry>.from(state.entries);
    // Сохраняем id существующей записи, чтобы результат валидации остался
    // привязан к той же строке.
    final withId = entry.copyWith(id: entries[index].id).ensureId();
    entries[index] = withId;
    state = state.copyWith(entries: entries);
    _validateEntry(withId);
  }

  void addEntry(ListEntry entry) {
    final withId = entry.ensureId();
    final entries = List<ListEntry>.from(state.entries)..add(withId);
    state = state.copyWith(entries: entries);
    _validateEntry(withId);
  }

  void removeEntry(int index) {
    final entries = List<ListEntry>.from(state.entries);
    final removed = entries.removeAt(index);
    final validation = Map<int, bool>.from(state.validationResults)
      ..remove(removed.id);
    state = state.copyWith(entries: entries, validationResults: validation);
  }

  Future<void> save() async {
    final name = state.selectedList;
    if (name == null) return;
    final result =
        await _saveUseCase(SaveListParams(name: name, entries: state.entries));
    result.when(
      success: (_) {},
      failure: (f) => state = state.copyWith(error: f.message),
    );
  }

  Future<void> _validateAll() async {
    final validation = <int, bool>{};
    for (final entry in state.entries) {
      final result = await _validateUseCase(entry);
      validation[entry.id] = result.valueOrNull ?? false;
    }
    state = state.copyWith(validationResults: validation);
  }

  Future<void> _validateEntry(ListEntry entry) async {
    final result = await _validateUseCase(entry);
    final validation = Map<int, bool>.from(state.validationResults);
    validation[entry.id] = result.valueOrNull ?? false;
    state = state.copyWith(validationResults: validation);
  }

  Future<void> importFromFile(String sourcePath) async {
    final name = state.selectedList;
    if (name == null) return;
    state = state.copyWith(isLoading: true, clearError: true);
    final result = await _importUseCase(
      ImportListParams(targetName: name, sourcePath: sourcePath),
    );
    await result.when(
      success: (loaded) async {
        state = state.copyWith(entries: _withIds(loaded), isLoading: false);
        await _validateAll();
      },
      failure: (f) async {
        state = state.copyWith(isLoading: false, error: f.message);
      },
    );
  }

  Future<void> exportToFile(String targetPath) async {
    final name = state.selectedList;
    if (name == null) return;
    final result = await _exportUseCase(
      ExportListParams(sourceName: name, targetPath: targetPath),
    );
    result.when(
      success: (_) {},
      failure: (f) => state = state.copyWith(error: f.message),
    );
  }

  void clearError() {
    if (state.error != null) state = state.copyWith(clearError: true);
  }

  void clearEntries() {
    state = state.copyWith(entries: [], validationResults: {});
  }

  void removeDuplicates() {
    final seen = <String>{};
    final unique = <ListEntry>[];
    for (final entry in state.entries) {
      if (entry.isBlank || entry.isComment) {
        unique.add(entry);
        continue;
      }
      final key = entry.value.trim().toLowerCase();
      if (!seen.contains(key)) {
        seen.add(key);
        unique.add(entry);
      }
    }
    state = state.copyWith(entries: unique);
    _validateAll();
  }

  void addComment() {
    addEntry(ListEntry.comment(''));
  }

  void addIp() {
    addEntry(ListEntry.ip('0.0.0.0'));
  }
}
