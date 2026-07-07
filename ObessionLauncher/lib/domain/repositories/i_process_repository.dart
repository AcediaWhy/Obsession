import '../../core/errors/result.dart';
import '../entities/dpi_config.dart';
import '../entities/process_info.dart';

/// Репозиторий для управления DPI-процессами winws.
abstract class IProcessRepository {
  /// Запускает процесс для указанной конфигурации.
  Future<Result<ProcessInfo>> start(DpiConfig config);

  /// Останавливает процесс по PID.
  Future<Result<void>> stop(int pid);

  /// Останавливает все отслеживаемые процессы.
  Future<Result<void>> stopAll();

  /// Проверяет, жив ли процесс.
  Future<bool> isRunning(int pid);

  /// Возвращает список активных процессов.
  List<ProcessInfo> get activeProcesses;

  /// Проверяет, активен ли хотя бы один процесс.
  bool get isActive;

  /// Возвращает доступные категории.
  List<String> getCategories();

  /// Возвращает список конфигов для категории.
  List<String> getConfigsForCategory(String category);
}
