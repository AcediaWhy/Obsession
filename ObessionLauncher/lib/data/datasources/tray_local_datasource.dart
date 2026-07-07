import 'dart:io' as io;
import 'package:flutter/services.dart' show rootBundle;
import 'package:path/path.dart' as p;
import 'package:system_tray/system_tray.dart';
import 'package:window_manager/window_manager.dart';

import '../../core/constants/app_constants.dart';
import '../../core/lifecycle/app_shutdown.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/paths_local_datasource.dart';
import '../datasources/process_local_datasource.dart';

/// Локализуемые подписи пунктов меню трея.
class TrayLabels {
  final String showWindow;
  final String stopAll;
  final String exit;
  const TrayLabels({
    required this.showWindow,
    required this.stopAll,
    required this.exit,
  });

  static const ru = TrayLabels(
    showWindow: 'Показать окно',
    stopAll: 'Остановить всё',
    exit: 'Выйти',
  );

  static const en = TrayLabels(
    showWindow: 'Show window',
    stopAll: 'Stop all',
    exit: 'Exit',
  );

  static TrayLabels forLocale(String locale) =>
      locale.toLowerCase().startsWith('ru') ? ru : en;
}

/// Управление иконкой в системном трее.
class TrayLocalDataSource {
  TrayLocalDataSource({
    required this._processDataSource,
    required this._paths,
    required this._log,
    this.shutdown,
  });

  final ProcessLocalDataSource _processDataSource;
  final PathsLocalDataSource _paths;
  final LogLocalDataSource _log;

  /// Координатор завершения. Если задан, пункт «Выход» в трее использует его,
  /// чтобы корректно остановить DPI + прокси + логгер (фикс утечки ресурсов).
  /// Если не задан — fallback на stopAll() только DPI.
  final AppShutdownCoordinator? shutdown;

  final SystemTray _tray = SystemTray();
  bool _initialized = false;
  TrayLabels _labels = TrayLabels.ru;

  Future<void> init({TrayLabels? labels}) async {
    if (_initialized) return;
    if (labels != null) _labels = labels;

    try {
      // Гарантируем, что иконка извлечена из ассетов.
      await _ensureIconExtracted();

      final iconPath = _paths.trayIconPath;
      _log.info('Инициализация трея, iconPath: $iconPath, exists: ${io.File(iconPath).existsSync()}');

      await _tray.initSystemTray(
        title: 'Obsession',
        // Абсолютный путь к иконке (распакованной из ассетов).
        iconPath: iconPath,
        toolTip: AppConstants.appName,
      );

      final menu = Menu();
      await menu.buildFrom([
        MenuItemLabel(label: _labels.showWindow, onClicked: (_) => _showWindow()),
        MenuSeparator(),
        MenuItemLabel(
          label: _labels.stopAll,
          onClicked: (_) => _processDataSource.stopAll(),
        ),
        MenuSeparator(),
        MenuItemLabel(label: _labels.exit, onClicked: (_) => _exitApp()),
      ]);
      await _tray.setContextMenu(menu);

      _tray.registerSystemTrayEventHandler((eventName) {
        if (eventName == kSystemTrayEventClick) {
          _showWindow();
        } else if (eventName == kSystemTrayEventRightClick) {
          _tray.popUpContextMenu();
        }
      });

      _initialized = true;
      _log.info('Иконка трея инициализирована.');
    } catch (e, st) {
      _log.error('Не удалось инициализировать трей: $e\n$st');
      // Пробрасываем исключение, чтобы репозиторий и провайдер знали о неудаче
      // и не устанавливали isInitialized = true.
      rethrow;
    }
  }

  /// Извлекает tray.ico из rootBundle, если он ещё не распакован.
  Future<void> _ensureIconExtracted() async {
    final dest = p.join(_paths.iconsDir, 'tray.ico');
    if (io.File(dest).existsSync()) return;
    try {
      final data = await rootBundle.load('assets/icons/tray.ico');
      await io.File(dest).create(recursive: true);
      await io.File(dest).writeAsBytes(data.buffer.asUint8List(), flush: true);
      _log.info('tray.ico извлечён из ассетов.');
    } catch (e) {
      _log.warning('Не удалось извлечь tray.ico из rootBundle: $e');
    }
  }

  Future<void> _showWindow() async {
    await windowManager.show();
    await windowManager.focus();
  }

  Future<void> showWindow() => _showWindow();

  Future<void> hideToTray() async {
    await windowManager.hide();
    _log.info('Скрыто в трей.');
  }

  void _exitApp() {
    // Через единый координатор: останавливает DPI + прокси + логгер, затем
    // уничтожает окно. Идемпотентен.
    final coord = shutdown;
    if (coord != null) {
      coord.shutdown();
      return;
    }
    // Fallback (координатор не прокинут) — хотя бы остановим DPI.
    _processDataSource.stopAll().then((_) async {
      await dispose();
      await windowManager.destroy();
    });
  }

  Future<void> dispose() async {
    if (!_initialized) return;
    await _tray.destroy();
    _initialized = false;
  }
}
