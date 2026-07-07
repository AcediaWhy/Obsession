import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../data/datasources/process_local_datasource.dart';
import '../../domain/entities/dpi_config.dart';
import '../../domain/entities/process_info.dart';
import '../../domain/repositories/i_log_repository.dart';
import '../../domain/repositories/i_process_repository.dart';
import '../../domain/usecases/dpi_usecases.dart';
import '../../domain/usecases/settings_usecases.dart';
import 'dependency_providers.dart';

/// Состояние DPI-обхода.
class DpiState {
  final bool isActive;
  final List<ProcessInfo> activeProcesses;
  final Set<String> selectedCategories;
  final Map<String, String> selectedConfigs;
  final bool isTransitioning;
  final bool isTesting;
  final int testingCurrent;
  final int testingTotal;
  final String testingConfig;
  final Map<String, bool> testResults;
  final DateTime? startedAt;
  final String error;

  const DpiState({
    this.isActive = false,
    this.activeProcesses = const [],
    this.selectedCategories = const {'discord'},
    this.selectedConfigs = const {},
    this.isTransitioning = false,
    this.isTesting = false,
    this.testingCurrent = 0,
    this.testingTotal = 0,
    this.testingConfig = '',
    this.testResults = const {},
    this.startedAt,
    this.error = '',
  });

  DpiState copyWith({
    bool? isActive,
    List<ProcessInfo>? activeProcesses,
    Set<String>? selectedCategories,
    Map<String, String>? selectedConfigs,
    bool? isTransitioning,
    bool? isTesting,
    int? testingCurrent,
    int? testingTotal,
    String? testingConfig,
    Map<String, bool>? testResults,
    DateTime? startedAt,
    String? error,
    bool clearStartedAt = false,
  }) =>
      DpiState(
        isActive: isActive ?? this.isActive,
        activeProcesses: activeProcesses ?? this.activeProcesses,
        selectedCategories: selectedCategories ?? this.selectedCategories,
        selectedConfigs: selectedConfigs ?? this.selectedConfigs,
        isTransitioning: isTransitioning ?? this.isTransitioning,
        isTesting: isTesting ?? this.isTesting,
        testingCurrent: testingCurrent ?? this.testingCurrent,
        testingTotal: testingTotal ?? this.testingTotal,
        testingConfig: testingConfig ?? this.testingConfig,
        testResults: testResults ?? this.testResults,
        startedAt: clearStartedAt ? null : (startedAt ?? this.startedAt),
        error: error ?? this.error,
      );
}

final dpiProvider = StateNotifierProvider<DpiNotifier, DpiState>((ref) {
  return DpiNotifier(
    repository: ref.watch(processRepositoryProvider),
    processDataSource: ref.watch(processDataSourceProvider),
    startUseCase: ref.watch(startDpiUseCaseProvider),
    stopUseCase: ref.watch(stopDpiUseCaseProvider),
    testUseCase: ref.watch(testDpiConfigUseCaseProvider),
    getSelectedConfigsUseCase: ref.watch(getSelectedConfigsUseCaseProvider),
    saveSelectedConfigUseCase: ref.watch(saveSelectedConfigUseCaseProvider),
    log: ref.watch(logRepositoryProvider),
  );
});

class DpiNotifier extends StateNotifier<DpiState> {
  DpiNotifier({
    required IProcessRepository repository,
    required this._processDataSource,
    required this._startUseCase,
    required this._stopUseCase,
    required this._testUseCase,
    required GetSelectedConfigsUseCase getSelectedConfigsUseCase,
    required this._saveSelectedConfigUseCase,
    required this._log,
  })  : _repository = repository,
        super(DpiState(
          selectedConfigs: _loadConfigs(repository, getSelectedConfigsUseCase),
        )) {    // Подключаем колбэк аварийного завершения напрямую к datasource.
    // Заменяет прежний поллинг состояния каждые 2 секунды: UI обновляется
    // мгновенно при завершении winws. Подключаем здесь (а не в провайдере
    // datasource), чтобы избежать цикла зависимостей провайдеров.
    _processDataSource.onProcessDied = _onProcessDied;
  }

  @override
  void dispose() {
    // Снимаем колбэк, чтобы shared-синглтон datasource не удерживал ссылку
    // на уничтожённый notifier и не вызывал `state =` после dispose (что
    // бросает StateError). Снимаем только если колбэк всё ещё наш — иначе
    // можно затереть колбэк другого notifier, если провайдер был пересоздан.
    if (_processDataSource.onProcessDied == _onProcessDied) {
      _processDataSource.onProcessDied = null;
    }
    super.dispose();
  }

  /// Загружает сохранённые конфиги и дополняет недостающие категории дефолтами.
  static Map<String, String> _loadConfigs(
    IProcessRepository repository,
    GetSelectedConfigsUseCase getUseCase,
  ) {
    final saved = getUseCase.call();
    final configs = <String, String>{};
    for (final cat in ['discord', 'youtube_twitch', 'gaming', 'universal']) {
      final files = repository.getConfigsForCategory(cat);
      if (files.isEmpty) continue;
      // Приоритет — сохранённому выбору, если конфиг всё ещё существует.
      final stored = saved[cat];
      if (stored != null && stored.isNotEmpty && files.contains(stored)) {
        configs[cat] = stored;
        continue;
      }
      final def = files.firstWhere(
        (f) => f.contains('_1.conf'),
        orElse: () => files.first,
      );
      configs[cat] = def;
    }
    return configs;
  }

  final IProcessRepository _repository;
  final ProcessLocalDataSource _processDataSource;
  final StartDpiUseCase _startUseCase;
  final StopDpiUseCase _stopUseCase;
  final TestDpiConfigUseCase _testUseCase;
  final SaveSelectedConfigUseCase _saveSelectedConfigUseCase;
  final ILogRepository _log;

  void clearError() {
    if (state.error.isNotEmpty) {
      state = state.copyWith(error: '');
    }
  }

  /// Вызывается datasource'ом при завершении процесса winws (через колбэк
  /// `onProcessDied`). Заменяет прежний поллинг состояния каждые 2 секунды:
  /// UI обновляется мгновенно при аварийном завершении.
  ///
  /// [livedShort] = true, если процесс прожил менее 2 секунд (вероятно антивирус,
  /// конфликт или некорректный конфиг) — в этом случае показываем ошибку.
  void _onProcessDied(int pid, String category, int exitCode, bool livedShort) {
    if (!state.isActive) return;
    final actualActive = _repository.activeProcesses;
    if (actualActive.isEmpty) {
      // Последний процесс завершился — обход больше не работает.
      final errorMsg = livedShort
          ? 'DPI-процесс ($category) завершился аварийно. Проверьте лог.'
          : 'DPI-процесс ($category) остановлен.';
      _log.warning(errorMsg);
      state = state.copyWith(
        isActive: false,
        activeProcesses: [],
        clearStartedAt: true,
        error: livedShort ? errorMsg : '',
      );
    } else {
      // Часть процессов ещё жива — обновляем список.
      state = state.copyWith(activeProcesses: actualActive);
    }
  }

  void toggleCategory(String category) {
    if (state.isActive) return;
    final selected = Set<String>.from(state.selectedCategories);
    if (selected.contains(category)) {
      if (selected.length > 1) {
        selected.remove(category);
      }
    } else {
      selected.add(category);
    }
    state = state.copyWith(selectedCategories: selected);
  }

  void setConfig(String category, String configFile) {
    if (state.isActive) return;
    final configs = Map<String, String>.from(state.selectedConfigs);
    configs[category] = configFile;
    state = state.copyWith(selectedConfigs: configs);
    // BUG-3: сохраняем выбор между перезапусками.
    _saveSelectedConfigUseCase
        .call(SelectedConfigParams(category: category, configFile: configFile));
  }

  String getSelectedConfig(String category) {
    if (state.selectedConfigs.containsKey(category)) {
      return state.selectedConfigs[category]!;
    }
    final configs = _repository.getConfigsForCategory(category);
    if (configs.isEmpty) return '';
    return configs.firstWhere(
      (f) => f.contains('_1.conf'),
      orElse: () => configs.first,
    );
  }

  Future<void> start() async {
    if (state.isTransitioning) return;
    final configs = state.selectedCategories
        .map((cat) => DpiConfig(category: cat, configFile: getSelectedConfig(cat)))
        .toList();

    _log.info('DPI start requested for ${configs.length} config(s)');
    final stopwatch = Stopwatch()..start();

    state = state.copyWith(
      isTransitioning: true,
      isActive: true,
      activeProcesses: [],
      startedAt: DateTime.now(),
      error: '',
    );
    final result = await _startUseCase(configs);
    result.when(
      success: (processes) {
        _log.success(
          'DPI started ${processes.length} process(es) in ${stopwatch.elapsedMilliseconds}ms',
        );
        // Колбэк onProcessDied обновит состояние при завершении процессов —
        // поллинг больше не нужен.
        state = state.copyWith(isTransitioning: false, activeProcesses: processes);
      },
      failure: (failure) {
        _log.error(
          'DPI start failed after ${stopwatch.elapsedMilliseconds}ms: ${failure.message}',
        );
        state = state.copyWith(
          isTransitioning: false,
          isActive: false,
          clearStartedAt: true,
          error: failure.message,
        );
      },
    );
  }

  Future<void> stop() async {
    _log.info('DPI stop requested');
    final stopwatch = Stopwatch()..start();

    // Меняем UI мгновенно, чтобы кнопка не "залипала" на время остановки.
    state = state.copyWith(
      isTransitioning: true,
      isActive: false,
      activeProcesses: [],
      clearStartedAt: true,
    );
    final result = await _stopUseCase(const NoParams());
    result.when(
      success: (_) => _log.info(
        'DPI stopped in ${stopwatch.elapsedMilliseconds}ms',
      ),
      failure: (failure) => _log.error(
        'DPI stop failed after ${stopwatch.elapsedMilliseconds}ms: ${failure.message}',
      ),
    );
    state = state.copyWith(isTransitioning: false);
  }

  Future<void> testConfig(String category, String configFile) async {
    state = state.copyWith(
      isTesting: true,
      testingCurrent: 0,
      testingTotal: 1,
      testingConfig: configFile,
    );
    final result = await _testUseCase(
      DpiConfig(category: category, configFile: configFile),
    );
    final results = Map<String, bool>.from(state.testResults);
    results[configFile] = result.valueOrNull ?? false;
    state = state.copyWith(
      isTesting: false,
      testResults: results,
    );
  }

  Future<void> testAllConfigs(String category) async {
    final configFiles = _repository.getConfigsForCategory(category);

    state = state.copyWith(
      isTesting: true,
      testingCurrent: 0,
      testingTotal: configFiles.length,
      testingConfig: '',
      testResults: {},
    );

    final results = <String, bool>{};
    for (var i = 0; i < configFiles.length; i++) {
      final confFile = configFiles[i];
      state = state.copyWith(
        testingCurrent: i + 1,
        testingConfig: confFile,
      );
      final result = await _testUseCase(
        DpiConfig(category: category, configFile: confFile),
      );
      results[confFile] = result.valueOrNull ?? false;
      state = state.copyWith(testResults: Map<String, bool>.from(results));
    }

    state = state.copyWith(isTesting: false);
  }

  Future<void> autoConfigure({bool startAfter = false}) async {
    if (state.isActive || state.isTesting) return;

    final categories = state.selectedCategories;
    final configs = _repository.getConfigsForCategory;

    state = state.copyWith(
      isTesting: true,
      testingCurrent: 0,
      testingTotal: categories.length,
      testingConfig: '',
      testResults: {},
    );

    final selected = Map<String, String>.from(state.selectedConfigs);
    var currentStep = 0;

    for (final category in categories) {
      currentStep++;
      final configFiles = configs(category);
      state = state.copyWith(
        testingCurrent: currentStep,
        testingConfig: category,
      );

      String? workingConfig;
      for (final confFile in configFiles) {
        final result = await _testUseCase(
          DpiConfig(category: category, configFile: confFile),
        );
        if (result.valueOrNull ?? false) {
          workingConfig = confFile;
          break;
        }
      }

      if (workingConfig != null) {
        selected[category] = workingConfig;
      }
    }

    state = state.copyWith(
      isTesting: false,
      selectedConfigs: selected,
    );

    if (startAfter) {
      await start();
    }
  }

  void clearTestResults() {
    state = state.copyWith(testResults: {});
  }

  void applyProfileConfigs(Set<DpiConfig> configs) {
    if (state.isActive) return;
    final categories = configs.map((c) => c.category).toSet();
    final selected = Map<String, String>.fromEntries(
      configs.map((c) => MapEntry(c.category, c.configFile)),
    );
    state = state.copyWith(
      selectedCategories: categories,
      selectedConfigs: selected,
    );
  }
}
