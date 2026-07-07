import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/lifecycle/app_shutdown.dart';
import '../../data/datasources/admin_local_datasource.dart';
import '../../data/datasources/autostart_local_datasource.dart';
import '../../data/datasources/hosts_local_datasource.dart';
import '../../data/datasources/list_local_datasource.dart';
import '../../data/datasources/log_local_datasource.dart';
import '../../data/datasources/paths_local_datasource.dart';
import '../../data/datasources/process_local_datasource.dart';
import '../../data/datasources/profile_local_datasource.dart';
import '../../data/datasources/proxy_local_datasource.dart';
import '../../data/datasources/settings_local_datasource.dart';
import '../../data/datasources/tray_local_datasource.dart';
import '../../data/datasources/update_remote_datasource.dart';
import '../../data/network/network_tester.dart';
import '../../data/repositories/admin_repository_impl.dart';
import '../../data/repositories/autostart_repository_impl.dart';
import '../../data/repositories/hosts_repository_impl.dart';
import '../../data/repositories/list_repository_impl.dart';
import '../../data/repositories/log_repository_impl.dart';
import '../../data/repositories/process_repository_impl.dart';
import '../../data/repositories/profile_repository_impl.dart';
import '../../data/repositories/proxy_repository_impl.dart';
import '../../data/repositories/settings_repository_impl.dart';
import '../../data/repositories/tray_repository_impl.dart';
import '../../domain/repositories/i_admin_repository.dart';
import '../../domain/repositories/i_autostart_repository.dart';
import '../../domain/repositories/i_hosts_repository.dart';
import '../../domain/repositories/i_list_repository.dart';
import '../../domain/repositories/i_log_repository.dart';
import '../../domain/repositories/i_process_repository.dart';
import '../../domain/repositories/i_profile_repository.dart';
import '../../domain/repositories/i_proxy_repository.dart';
import '../../domain/repositories/i_settings_repository.dart';
import '../../domain/repositories/i_tray_repository.dart';
import '../../domain/usecases/admin_usecases.dart';
import '../../domain/usecases/autostart_usecases.dart';
import '../../domain/usecases/dpi_usecases.dart';
import '../../domain/usecases/hosts_usecases.dart';
import '../../domain/usecases/list_usecases.dart';
import '../../domain/usecases/profile_usecases.dart';
import '../../domain/usecases/proxy_usecases.dart';
import '../../domain/usecases/settings_usecases.dart';
import '../../domain/usecases/update_usecases.dart';

// ==================== DATA SOURCES ====================

/// Переопределяется в main.dart после инициализации путей.
final pathsDataSourceProvider = Provider<PathsLocalDataSource>((ref) {
  throw UnimplementedError('pathsDataSourceProvider must be overridden');
});

/// Переопределяется в main.dart после инициализации логгера.
final logDataSourceProvider = Provider<LogLocalDataSource>((ref) {
  throw UnimplementedError('logDataSourceProvider must be overridden');
});

/// Переопределяется в main.dart после инициализации SharedPreferences.
final settingsDataSourceProvider = Provider<SettingsLocalDataSource>((ref) {
  throw UnimplementedError('settingsDataSourceProvider must be overridden');
});

// ==================== REPOSITORIES ====================

final logRepositoryProvider = Provider<ILogRepository>((ref) {
  return LogRepositoryImpl(dataSource: ref.watch(logDataSourceProvider));
});

final adminRepositoryProvider = Provider<IAdminRepository>((ref) {
  return AdminRepositoryImpl(
    dataSource: AdminLocalDataSource(),
    log: ref.watch(logDataSourceProvider),
  );
});

final settingsRepositoryProvider = Provider<ISettingsRepository>((ref) {
  return SettingsRepositoryImpl(
    dataSource: ref.watch(settingsDataSourceProvider),
    log: ref.watch(logDataSourceProvider),
  );
});

final autostartRepositoryProvider = Provider<IAutostartRepository>((ref) {
  return AutostartRepositoryImpl(
    dataSource: AutostartLocalDataSource(),
    log: ref.watch(logDataSourceProvider),
  );
});

/// Общий datasource для процессов DPI.
/// Разделяется между processRepository и tray, чтобы «Остановить всё»
/// из трея управляло теми же процессами, что и UI.
///
/// Колбэк `onProcessDied` подключается в [DpiNotifier] (через
/// [ProcessLocalDataSource.onProcessDied]), чтобы UI обновлялся мгновенно
/// при аварийном завершении winws (раньше для этого использовался поллинг
/// каждые 2 секунды). Здесь провайдер НЕ зависит от dpiProvider, чтобы
/// избежать цикла зависимостей.
final processDataSourceProvider = Provider<ProcessLocalDataSource>((ref) {
  return ProcessLocalDataSource(
    paths: ref.watch(pathsDataSourceProvider),
    log: ref.watch(logDataSourceProvider),
  );
});

final processRepositoryProvider = Provider<IProcessRepository>((ref) {
  return ProcessRepositoryImpl(
    dataSource: ref.watch(processDataSourceProvider),
    paths: ref.watch(pathsDataSourceProvider),
    log: ref.watch(logDataSourceProvider),
  );
});

final hostsRepositoryProvider = Provider<IHostsRepository>((ref) {
  return HostsRepositoryImpl(
    dataSource: HostsLocalDataSource(
      paths: ref.watch(pathsDataSourceProvider),
      log: ref.watch(logDataSourceProvider),
    ),
    log: ref.watch(logDataSourceProvider),
  );
});

/// Общий datasource для Telegram-прокси.
/// Разделяется между proxyRepository и shutdown coordinator, чтобы при выходе
/// из приложения прокси корректно останавливался (фикс утечки порта/процесса).
final proxyDataSourceProvider = Provider<ProxyLocalDataSource>((ref) {
  return ProxyLocalDataSource(
    paths: ref.watch(pathsDataSourceProvider),
    log: ref.watch(logDataSourceProvider),
  );
});

final proxyRepositoryProvider = Provider<IProxyRepository>((ref) {
  return ProxyRepositoryImpl(
    dataSource: ref.watch(proxyDataSourceProvider),
    paths: ref.watch(pathsDataSourceProvider),
    log: ref.watch(logDataSourceProvider),
  );
});

final profileRepositoryProvider = Provider<IProfileRepository>((ref) {
  return ProfileRepositoryImpl(
    dataSource: ProfileLocalDataSource(
      paths: ref.watch(pathsDataSourceProvider),
      log: ref.watch(logDataSourceProvider),
    ),
    log: ref.watch(logDataSourceProvider),
  );
});

final listRepositoryProvider = Provider<IListRepository>((ref) {
  return ListRepositoryImpl(
    dataSource: ListLocalDataSource(
      paths: ref.watch(pathsDataSourceProvider),
      log: ref.watch(logDataSourceProvider),
    ),
    log: ref.watch(logDataSourceProvider),
  );
});

final trayRepositoryProvider = Provider<ITrayRepository>((ref) {
  return TrayRepositoryImpl(
    dataSource: TrayLocalDataSource(
      processDataSource: ref.watch(processDataSourceProvider),
      paths: ref.watch(pathsDataSourceProvider),
      log: ref.watch(logDataSourceProvider),
      shutdown: ref.watch(shutdownCoordinatorProvider),
    ),
    log: ref.watch(logDataSourceProvider),
  );
});

// ==================== NETWORK ====================

final networkTesterProvider = Provider<INetworkTester>((ref) {
  return NetworkTesterImpl();
});

// ==================== LIFECYCLE ====================

/// Единый координатор завершения приложения.
/// Используется всеми exit-хуками (закрытие окна, трей «Выход», перезапуск
/// с правами админа) для гарантированного освобождения ресурсов.
final shutdownCoordinatorProvider = Provider<AppShutdownCoordinator>((ref) {
  return AppShutdownCoordinator(
    processDataSource: ref.watch(processDataSourceProvider),
    proxyDataSource: ref.watch(proxyDataSourceProvider),
    logDataSource: ref.watch(logDataSourceProvider),
  );
});

// ==================== USE CASES ====================

final checkAdminUseCaseProvider = Provider<CheckAdminUseCase>((ref) {
  return CheckAdminUseCase(ref.watch(adminRepositoryProvider));
});

final relaunchAsAdminUseCaseProvider = Provider<RelaunchAsAdminUseCase>((ref) {
  return RelaunchAsAdminUseCase(ref.watch(adminRepositoryProvider));
});

final loadSettingsUseCaseProvider = Provider<LoadSettingsUseCase>((ref) {
  return LoadSettingsUseCase(ref.watch(settingsRepositoryProvider));
});

final saveSettingsUseCaseProvider = Provider<SaveSettingsUseCase>((ref) {
  return SaveSettingsUseCase(ref.watch(settingsRepositoryProvider));
});

final resetSettingsUseCaseProvider = Provider<ResetSettingsUseCase>((ref) {
  return ResetSettingsUseCase(ref.watch(settingsRepositoryProvider));
});

final getSelectedConfigsUseCaseProvider = Provider<GetSelectedConfigsUseCase>((ref) {
  return GetSelectedConfigsUseCase(ref.watch(settingsRepositoryProvider));
});

final saveSelectedConfigUseCaseProvider = Provider<SaveSelectedConfigUseCase>((ref) {
  return SaveSelectedConfigUseCase(ref.watch(settingsRepositoryProvider));
});

final checkAutostartUseCaseProvider = Provider<CheckAutostartUseCase>((ref) {
  return CheckAutostartUseCase(ref.watch(autostartRepositoryProvider));
});

final enableAutostartUseCaseProvider = Provider<EnableAutostartUseCase>((ref) {
  return EnableAutostartUseCase(ref.watch(autostartRepositoryProvider));
});

final disableAutostartUseCaseProvider = Provider<DisableAutostartUseCase>((ref) {
  return DisableAutostartUseCase(ref.watch(autostartRepositoryProvider));
});

final toggleAutostartUseCaseProvider = Provider<ToggleAutostartUseCase>((ref) {
  return ToggleAutostartUseCase(ref.watch(autostartRepositoryProvider));
});

final getDpiCategoriesUseCaseProvider = Provider<GetDpiCategoriesUseCase>((ref) {
  return GetDpiCategoriesUseCase(ref.watch(processRepositoryProvider));
});

final getDpiConfigsUseCaseProvider = Provider<GetDpiConfigsUseCase>((ref) {
  return GetDpiConfigsUseCase(ref.watch(processRepositoryProvider));
});

final startDpiUseCaseProvider = Provider<StartDpiUseCase>((ref) {
  return StartDpiUseCase(
    ref.watch(processRepositoryProvider),
    log: ref.watch(logRepositoryProvider),
  );
});

final stopDpiUseCaseProvider = Provider<StopDpiUseCase>((ref) {
  return StopDpiUseCase(ref.watch(processRepositoryProvider));
});

final testDpiConfigUseCaseProvider = Provider<TestDpiConfigUseCase>((ref) {
  return TestDpiConfigUseCase(
    ref.watch(processRepositoryProvider),
    ref.watch(networkTesterProvider),
  );
});

final checkHostsStatusUseCaseProvider = Provider<CheckHostsStatusUseCase>((ref) {
  return CheckHostsStatusUseCase(ref.watch(hostsRepositoryProvider));
});

final installHostsUseCaseProvider = Provider<InstallHostsUseCase>((ref) {
  return InstallHostsUseCase(ref.watch(hostsRepositoryProvider));
});

final uninstallHostsUseCaseProvider = Provider<UninstallHostsUseCase>((ref) {
  return UninstallHostsUseCase(ref.watch(hostsRepositoryProvider));
});

final checkProxyAvailableUseCaseProvider = Provider<CheckProxyAvailableUseCase>((ref) {
  return CheckProxyAvailableUseCase(ref.watch(proxyRepositoryProvider));
});

final startProxyUseCaseProvider = Provider<StartProxyUseCase>((ref) {
  return StartProxyUseCase(ref.watch(proxyRepositoryProvider));
});

final stopProxyUseCaseProvider = Provider<StopProxyUseCase>((ref) {
  return StopProxyUseCase(ref.watch(proxyRepositoryProvider));
});

final getProxyLinkUseCaseProvider = Provider<GetProxyLinkUseCase>((ref) {
  return GetProxyLinkUseCase(ref.watch(proxyRepositoryProvider));
});

final openProxyInTelegramUseCaseProvider = Provider<OpenProxyInTelegramUseCase>((ref) {
  return OpenProxyInTelegramUseCase(ref.watch(proxyRepositoryProvider));
});

final copyProxyLinkUseCaseProvider = Provider<CopyProxyLinkUseCase>((ref) {
  return CopyProxyLinkUseCase(ref.watch(proxyRepositoryProvider));
});

final getProfilesUseCaseProvider = Provider<GetProfilesUseCase>((ref) {
  return GetProfilesUseCase(ref.watch(profileRepositoryProvider));
});

final getActiveProfileUseCaseProvider = Provider<GetActiveProfileUseCase>((ref) {
  return GetActiveProfileUseCase(ref.watch(profileRepositoryProvider));
});

final createProfileUseCaseProvider = Provider<CreateProfileUseCase>((ref) {
  return CreateProfileUseCase(ref.watch(profileRepositoryProvider));
});

final saveProfileUseCaseProvider = Provider<SaveProfileUseCase>((ref) {
  return SaveProfileUseCase(ref.watch(profileRepositoryProvider));
});

final deleteProfileUseCaseProvider = Provider<DeleteProfileUseCase>((ref) {
  return DeleteProfileUseCase(ref.watch(profileRepositoryProvider));
});

final switchProfileUseCaseProvider = Provider<SwitchProfileUseCase>((ref) {
  return SwitchProfileUseCase(ref.watch(profileRepositoryProvider));
});

final getListNamesUseCaseProvider = Provider<GetListNamesUseCase>((ref) {
  return GetListNamesUseCase(ref.watch(listRepositoryProvider));
});

final loadListUseCaseProvider = Provider<LoadListUseCase>((ref) {
  return LoadListUseCase(ref.watch(listRepositoryProvider));
});

final saveListUseCaseProvider = Provider<SaveListUseCase>((ref) {
  return SaveListUseCase(ref.watch(listRepositoryProvider));
});

final validateListEntryUseCaseProvider = Provider<ValidateListEntryUseCase>((ref) {
  return const ValidateListEntryUseCase();
});

final importListUseCaseProvider = Provider<ImportListUseCase>((ref) {
  return ImportListUseCase(ref.watch(listRepositoryProvider));
});

final exportListUseCaseProvider = Provider<ExportListUseCase>((ref) {
  return ExportListUseCase(ref.watch(listRepositoryProvider));
});

// ==================== UPDATE ====================

final updateRemoteDataSourceProvider = Provider<IUpdateRemoteDataSource>((ref) {
  return UpdateRemoteDataSource();
});

final checkForUpdateUseCaseProvider = Provider<CheckForUpdateUseCase>((ref) {
  return CheckForUpdateUseCase(ref.watch(updateRemoteDataSourceProvider));
});

final downloadUpdateUseCaseProvider = Provider<DownloadUpdateUseCase>((ref) {
  return DownloadUpdateUseCase(ref.watch(updateRemoteDataSourceProvider));
});
