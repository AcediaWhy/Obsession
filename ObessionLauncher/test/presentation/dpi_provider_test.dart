import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:obsession/core/errors/result.dart';
import 'package:obsession/core/usecases/usecase.dart';
import 'package:obsession/domain/entities/app_settings.dart';
import 'package:obsession/domain/entities/dpi_config.dart';
import 'package:obsession/domain/entities/log_entry.dart';
import 'package:obsession/domain/entities/process_info.dart';
import 'package:obsession/data/network/network_tester.dart';
import 'package:obsession/domain/repositories/i_log_repository.dart';
import 'package:obsession/domain/repositories/i_process_repository.dart';
import 'package:obsession/domain/repositories/i_settings_repository.dart';
import 'package:obsession/domain/usecases/dpi_usecases.dart';
import 'package:obsession/domain/usecases/settings_usecases.dart';
import 'package:obsession/presentation/providers/dependency_providers.dart';
import 'package:obsession/presentation/providers/dpi_provider.dart';
import 'package:obsession/data/datasources/log_local_datasource.dart';
import 'package:obsession/data/datasources/paths_local_datasource.dart';
import 'package:obsession/data/datasources/process_local_datasource.dart';

class _FakeSettingsRepo implements ISettingsRepository {
  final Map<String, String> _selectedConfigs = {};

  @override
  Future<Result<AppSettings>> load() async =>
      Success(AppSettings.defaults());

  @override
  Future<Result<void>> save(AppSettings settings) async =>
      const Success(null);

  @override
  Future<Result<void>> reset() async => const Success(null);

  @override
  Map<String, String> getSelectedConfigs() => Map.of(_selectedConfigs);

  @override
  Future<Result<void>> setSelectedConfig(
      String category, String configFile) async {
    _selectedConfigs[category] = configFile;
    return const Success(null);
  }
}

class _FakeProcessRepo implements IProcessRepository {
  @override
  List<String> getCategories() => ['discord', 'gaming'];

  @override
  List<String> getConfigsForCategory(String category) => [
    '${category}_1.conf',
    '${category}_2.conf',
  ];

  @override
  Future<Result<ProcessInfo>> start(DpiConfig config) async => Success(
    ProcessInfo(pid: 123, category: config.category, configFile: config.configFile, startedAt: DateTime.now()),
  );

  @override
  Future<Result<void>> stop(int pid) async => const Success(null);

  @override
  Future<Result<void>> stopAll() async => const Success(null);

  @override
  Future<bool> isRunning(int pid) async => true;

  @override
  List<ProcessInfo> get activeProcesses => [];

  @override
  bool get isActive => false;
}

class _FakeLogRepo implements ILogRepository {
  @override
  Stream<LogEntry> get stream => const Stream<LogEntry>.empty();

  @override
  List<LogEntry> get history => [];

  @override
  void info(String message) {}

  @override
  void success(String message) {}

  @override
  void warning(String message) {}

  @override
  void error(String message, {Object? error, StackTrace? stackTrace}) {}

  @override
  void clear() {}

  @override
  void dispose() {}
}

class _FakePaths extends PathsLocalDataSource {
  @override
  Future<void> init() async {}
}

class _FakeLog extends LogLocalDataSource {
  _FakeLog() : super(paths: _FakePaths());

  @override
  Future<void> init() async {}
}

class _FakeStartDpi extends StartDpiUseCase {
  _FakeStartDpi() : super(_FakeProcessRepo(), log: _FakeLogRepo());
  @override
  Future<Result<List<ProcessInfo>>> call(List<DpiConfig> configs) async => Success(
    configs.map((c) => ProcessInfo(pid: 123, category: c.category, configFile: c.configFile, startedAt: DateTime.now())).toList(),
  );
}

class _FakeStopDpi extends StopDpiUseCase {
  _FakeStopDpi() : super(_FakeProcessRepo());
  @override
  Future<Result<void>> call(NoParams params) async => const Success(null);
}

class _FakeTestDpi extends TestDpiConfigUseCase {
  final Set<String> _working;
  _FakeTestDpi({Set<String>? working})
      : _working = working ?? const {},
        super(_FakeProcessRepo(), _FakeNetworkTester());

  @override
  Future<Result<bool>> call(DpiConfig config) async => Success(_working.contains(config.configFile));
}

class _FakeNetworkTester implements INetworkTester {
  @override
  Future<Result<bool>> testUrl(String url, {Duration timeout = const Duration(seconds: 5)}) async =>
      const Success(true);
}

ProviderContainer _createContainer({Set<String>? workingConfigs}) {
  final fakePaths = _FakePaths();
  return ProviderContainer(
    overrides: [
      // Override processDataSource to avoid watching paths/log data sources.
      processDataSourceProvider.overrideWithValue(
        ProcessLocalDataSource(paths: fakePaths, log: _FakeLog()),
      ),
      processRepositoryProvider.overrideWithValue(_FakeProcessRepo()),
      logRepositoryProvider.overrideWithValue(_FakeLogRepo()),
      startDpiUseCaseProvider.overrideWithValue(_FakeStartDpi()),
      stopDpiUseCaseProvider.overrideWithValue(_FakeStopDpi()),
      testDpiConfigUseCaseProvider.overrideWithValue(_FakeTestDpi(working: workingConfigs)),
      getSelectedConfigsUseCaseProvider.overrideWithValue(
        GetSelectedConfigsUseCase(_FakeSettingsRepo()),
      ),
      saveSelectedConfigUseCaseProvider.overrideWithValue(
        SaveSelectedConfigUseCase(_FakeSettingsRepo()),
      ),
    ],
  );
}

void main() {
  group('DpiNotifier', () {
    test('toggleCategory adds and removes category', () {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      notifier.toggleCategory('gaming');
      expect(container.read(dpiProvider).selectedCategories, {'discord', 'gaming'});

      notifier.toggleCategory('gaming');
      expect(container.read(dpiProvider).selectedCategories, {'discord'});
    });

    test('cannot toggle last category off', () {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      notifier.toggleCategory('discord');
      expect(container.read(dpiProvider).selectedCategories, {'discord'});
    });

    test('setConfig updates selected config', () {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      notifier.setConfig('discord', 'discord_custom.conf');
      expect(container.read(dpiProvider).selectedConfigs['discord'], 'discord_custom.conf');
    });

    test('getSelectedConfig returns default with _1.conf', () {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      final config = notifier.getSelectedConfig('gaming');
      expect(config, 'gaming_1.conf');
    });

    test('start activates DPI and stores processes', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      await notifier.start();

      final state = container.read(dpiProvider);
      expect(state.isActive, isTrue);
      expect(state.activeProcesses.length, 1);
    });

    test('stop deactivates DPI', () async {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      await notifier.start();
      await notifier.stop();

      final state = container.read(dpiProvider);
      expect(state.isActive, isFalse);
      expect(state.activeProcesses, isEmpty);
    });

    test('testConfig stores result', () async {
      final container = _createContainer(workingConfigs: {'discord_1.conf'});
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      await notifier.testConfig('discord', 'discord_1.conf');

      final state = container.read(dpiProvider);
      expect(state.testResults['discord_1.conf'], isTrue);
      expect(state.isTesting, isFalse);
    });

    test('autoConfigure picks first working config and starts', () async {
      final container = _createContainer(workingConfigs: {'discord_2.conf'});
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      await notifier.autoConfigure(startAfter: true);

      final state = container.read(dpiProvider);
      expect(state.isTesting, isFalse);
      expect(state.selectedConfigs['discord'], 'discord_2.conf');
      expect(state.isActive, isTrue);
    });

    test('applyProfileConfigs sets categories and configs', () {
      final container = _createContainer();
      addTearDown(container.dispose);

      final notifier = container.read(dpiProvider.notifier);
      notifier.applyProfileConfigs({
        DpiConfig(category: 'gaming', configFile: 'gaming_2.conf'),
      });

      final state = container.read(dpiProvider);
      expect(state.selectedCategories, {'gaming'});
      expect(state.selectedConfigs['gaming'], 'gaming_2.conf');
    });
  });
}
