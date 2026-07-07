import 'dart:async';
import 'dart:io' as io;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'core/constants/app_constants.dart';
import 'core/lifecycle/app_shutdown.dart';
import 'core/usecases/usecase.dart';
import 'data/datasources/admin_local_datasource.dart';
import 'data/datasources/log_local_datasource.dart';
import 'data/datasources/paths_local_datasource.dart';
import 'data/datasources/process_local_datasource.dart';
import 'data/datasources/proxy_local_datasource.dart';
import 'data/datasources/settings_local_datasource.dart';
import 'data/repositories/admin_repository_impl.dart';
import 'domain/usecases/admin_usecases.dart';
import 'presentation/providers/dependency_providers.dart';
import 'presentation/providers/settings_provider.dart';
import 'presentation/theme/app_theme.dart';

/// Следит за жизненным циклом приложения, чтобы корректно освободить
/// ресурсы (DPI-процессы, прокси, логгер) при выходе (BUG-13, BUG: утечка
/// процессов).
///
/// Реализует [WindowListener] для перехвата закрытия окна и
/// [WidgetsBindingObserver] для реакции на системные события жизненного
/// цикла. Все пути выхода делегируют в [AppShutdownCoordinator.shutdown],
/// который идемпотентен.
class AppLifecycleObserver extends WidgetsBindingObserver {
  AppLifecycleObserver(this._shutdown);
  final AppShutdownCoordinator _shutdown;

  /// Вызывается вручную при перезапуске с правами админа (до runApp).
  void shutdown() => _shutdown.shutdown();
}

void main() async {
  await runZonedGuarded<Future<void>>(
    () async {
      WidgetsFlutterBinding.ensureInitialized();
      await windowManager.ensureInitialized();

      // Глобальный обработчик ошибок Flutter (виджеты, рендеринг).
      FlutterError.onError = (FlutterErrorDetails details) {
        FlutterError.presentError(details);
      };

      // Инициализация data sources ДО показа окна.
      final paths = PathsLocalDataSource();
      await paths.init();

      final log = LogLocalDataSource(paths: paths);
      await log.init();

      final settings = SettingsLocalDataSource();
      await settings.init();
      final initialSettings = settings.load();

      // BUG-13: корректное освобождение ресурсов при выходе.
      // Единый координатор завершения: останавливает DPI + прокси + логгер,
      // затем уничтожает окно. Идемпотентен — безопасен при одновременном
      // срабатывании нескольких exit-хуков.
      final processDataSource = ProcessLocalDataSource(paths: paths, log: log);
      final proxyDataSource = ProxyLocalDataSource(paths: paths, log: log);
      final shutdown = AppShutdownCoordinator(
        processDataSource: processDataSource,
        proxyDataSource: proxyDataSource,
        logDataSource: log,
      );
      final lifecycleObserver = AppLifecycleObserver(shutdown);
      WidgetsBinding.instance.addObserver(lifecycleObserver);

      // Проверка прав администратора ДО показа окна.
      final adminRepo = AdminRepositoryImpl(
        dataSource: AdminLocalDataSource(),
        log: log,
      );
      final isAdmin = await adminRepo.isAdmin();

      if (!isAdmin) {
        log.warning('Нет прав администратора. Запрашиваю через UAC...');
        final result = await RelaunchAsAdminUseCase(adminRepo).call(const NoParams());
        if (result.isSuccess) {
          // Перезапуск с правами администратора — корректно завершаемся
          // (освобождаем ресурсы перед выходом).
          await shutdown.shutdown();
          io.exit(0);
        }
        // UAC отклонён — продолжаем с ограничениями (не ложный успех).
        log.error('Не удалось получить права администратора. Приложение будет работать с ограничениями.');
      } else {
        log.success('Права администратора получены.');
      }

      // Показываем окно только после всех проверок.
      final initialWindowBg = AppTheme.windowBackgroundColor(initialSettings);
      await windowManager.waitUntilReadyToShow(
        const WindowOptions(
          size: Size(1000, 680),
          minimumSize: Size(800, 600),
          title: AppConstants.appName,
          titleBarStyle: TitleBarStyle.hidden,
          backgroundColor: Color(0xFF05070D),
        ),
        () async {
          // Динамический цвет окна под стартовую тему (тёмный/светлый).
          await windowManager.setBackgroundColor(initialWindowBg);
          await windowManager.show();
          await windowManager.focus();
        },
      );

      runApp(
        ProviderScope(
          overrides: [
            pathsDataSourceProvider.overrideWithValue(paths),
            logDataSourceProvider.overrideWithValue(log),
            settingsDataSourceProvider.overrideWithValue(settings),
            processDataSourceProvider.overrideWithValue(processDataSource),
            proxyDataSourceProvider.overrideWithValue(proxyDataSource),
            shutdownCoordinatorProvider.overrideWithValue(shutdown),
            settingsProvider.overrideWith((ref) {
              return SettingsNotifier(
                loadUseCase: ref.watch(loadSettingsUseCaseProvider),
                saveUseCase: ref.watch(saveSettingsUseCaseProvider),
                resetUseCase: ref.watch(resetSettingsUseCaseProvider),
                initialState: initialSettings,
              );
            }),
          ],
          child: const ObsessionApp(),
        ),
      );
    },
    (error, stack) {
      // Неперехваченные ошибки async-кода не должны «съедаться» молча.
      io.stderr.writeln('Uncaught async error: $error\n$stack');
    },
  );
}
