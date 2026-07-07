import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../../data/network/network_tester.dart';
import '../entities/dpi_config.dart';
import '../entities/process_info.dart';
import '../repositories/i_log_repository.dart';
import '../repositories/i_process_repository.dart';

class GetDpiCategoriesUseCase implements UseCase<List<String>, NoParams> {
  final IProcessRepository repository;
  const GetDpiCategoriesUseCase(this.repository);

  @override
  Future<Result<List<String>>> call(NoParams params) async {
    return Success(repository.getCategories());
  }
}

class GetDpiConfigsUseCase implements UseCase<List<String>, String> {
  final IProcessRepository repository;
  const GetDpiConfigsUseCase(this.repository);

  @override
  Future<Result<List<String>>> call(String category) async {
    return Success(repository.getConfigsForCategory(category));
  }
}

class StartDpiUseCase implements UseCase<List<ProcessInfo>, List<DpiConfig>> {
  final IProcessRepository repository;
  final ILogRepository log;

  const StartDpiUseCase(this.repository, {required this.log});

  @override
  Future<Result<List<ProcessInfo>>> call(List<DpiConfig> configs) async {
    log.info('StartDpiUseCase: stopping previous processes');
    final stopwatch = Stopwatch()..start();

    await repository.stopAll();
    log.info(
      'StartDpiUseCase: stopAll finished in ${stopwatch.elapsedMilliseconds}ms',
    );

    final started = <ProcessInfo>[];
    for (var i = 0; i < configs.length; i++) {
      final config = configs[i];
      log.info(
        'StartDpiUseCase: starting ${i + 1}/${configs.length} (${config.category}/${config.configFile})',
      );
      final sw = Stopwatch()..start();
      final result = await repository.start(config);
      result.when(
        success: (info) {
          log.success(
            'StartDpiUseCase: ${config.category} started (PID ${info.pid}) in ${sw.elapsedMilliseconds}ms',
          );
          started.add(info);
        },
        failure: (failure) {
          log.error(
            'StartDpiUseCase: ${config.category} failed in ${sw.elapsedMilliseconds}ms: ${failure.message}',
          );
        },
      );
    }

    log.info(
      'StartDpiUseCase: total time ${stopwatch.elapsedMilliseconds}ms, started ${started.length}/${configs.length}',
    );

    if (started.isEmpty) {
      return const Failure(
        ProcessFailure('Ни один DPI-процесс не был запущен'),
      );
    }
    return Success(started);
  }
}

class StopDpiUseCase implements UseCase<void, NoParams> {
  final IProcessRepository repository;
  const StopDpiUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.stopAll();
}

class TestDpiConfigUseCase implements UseCase<bool, DpiConfig> {
  final IProcessRepository repository;
  final INetworkTester networkTester;

  const TestDpiConfigUseCase(this.repository, this.networkTester);

  static const _testUrls = <String, List<String>>{
    'discord': ['https://discord.com/'],
    'youtube_twitch': [
      'https://www.youtube.com/',
      'https://www.twitch.tv/',
    ],
    // Steam в white-list.txt (исключён из обхода) — тест прошёл бы ложно.
    // Используем Epic Games (в gaming.txt, не в white-list).
    'gaming': ['https://www.epicgames.com/', 'https://signin.ea.com/'],
    'universal': ['https://www.google.com/'],
  };

  @override
  Future<Result<bool>> call(DpiConfig config) async {
    await repository.stopAll();
    // Увеличена задержка для корректной выгрузки драйвера WinDivert.
    await Future.delayed(const Duration(milliseconds: 500));

    final result = await repository.start(config);
    final info = result.valueOrNull;

    if (info == null) {
      await repository.stopAll();
      return const Success(false);
    }

    // Ждём инициализации WinDivert.
    await Future.delayed(const Duration(milliseconds: 1000));

    final urls = _testUrls[config.category] ??
        const ['https://www.google.com/'];
    var connected = false;
    for (final testUrl in urls) {
      final testResult = await networkTester.testUrl(
        testUrl,
        timeout: const Duration(seconds: 4),
      );
      if (testResult.valueOrNull ?? false) {
        connected = true;
        break;
      }
    }

    await repository.stop(info.pid);
    await repository.stopAll();

    return Success(connected);
  }
}
