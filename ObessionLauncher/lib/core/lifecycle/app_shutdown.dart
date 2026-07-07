import 'dart:async';

import 'package:window_manager/window_manager.dart';

import '../../data/datasources/log_local_datasource.dart';
import '../../data/datasources/process_local_datasource.dart';
import '../../data/datasources/proxy_local_datasource.dart';

/// Единый координатор завершения приложения.
///
/// Гарантирует, что при любом пути выхода (кнопка закрытия окна, пункт «Выход»
/// в трее, перезапуск с правами админа, системное завершение) корректно
/// освобождаются все ресурсы:
///
/// 1. Останавливаются DPI-процессы `winws.exe` (только отслеживаемые/свои —
///    сторонние экземпляры других инструментов не затрагиваются).
/// 2. Останавливается Telegram-прокси `TgWsProxy` (раньше оставался висеть
///    с захваченным портом).
/// 3. Закрывается логгер (flush буфера в файл + закрытие StreamController).
/// 4. Уничтожается нативное окно.
///
/// Идемпотентен: повторные вызовы `shutdown()` во время уже идущего завершения
/// возвращают тот же Future (защита от двойного срабатывания нескольких
/// exit-хуков одновременно).
class AppShutdownCoordinator {
  AppShutdownCoordinator({
    required this._processDataSource,
    required this._proxyDataSource,
    required this._logDataSource,
  });

  final ProcessLocalDataSource _processDataSource;
  final ProxyLocalDataSource _proxyDataSource;
  final LogLocalDataSource _logDataSource;

  bool _shuttingDown = false;
  Completer<void>? _completer;

  /// True, если завершение уже в процессе (для защиты от двойных exit-хуков).
  bool get isShuttingDown => _shuttingDown;

  /// Корректно останавливает все процессы и освобождает ресурсы, затем
  /// уничтожает окно. Идемпотентен.
  Future<void> shutdown() async {
    if (_shuttingDown) {
      // Возвращаем уже идущий Future, чтобы вызывающий мог дождаться.
      return _completer?.future ?? Future<void>.value();
    }
    _shuttingDown = true;
    _completer = Completer<void>();

    try {
      // 1. Останавливаем свои DPI-процессы (не затрагивая сторонние winws).
      await _processDataSource.stopAll();

      // 2. Останавливаем Telegram-прокси (фикс утечки порта/процесса при выходе).
      await _proxyDataSource.stop();

      // 3. Закрываем логгер: flush буфера в файл + закрытие StreamController.
      //    dispose() синхронный, но внутри закрывает IOSink (async) —
      //    даём ему квант времени через microtask перед уничтожением окна.
      _logDataSource.dispose();
      await Future<void>.delayed(Duration.zero);
    } catch (e) {
      // Завершение не должно падать даже при ошибке освобождения ресурсов.
      // Игнорируем — окно всё равно уничтожим.
    }

    // 4. Уничтожаем нативное окно.
    try {
      await windowManager.destroy();
    } catch (_) {
      // Окно могло уже быть уничтожено.
    }

    _completer?.complete();
  }
}
