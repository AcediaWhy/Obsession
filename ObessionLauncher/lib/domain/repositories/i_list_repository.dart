import '../../core/errors/result.dart';
import '../entities/list_entry.dart';

/// Репозиторий для управления списками доменов/IP.
abstract class IListRepository {
  /// Возвращает все доступные списки.
  Future<Result<List<String>>> getListNames();

  /// Загружает записи списка.
  Future<Result<List<ListEntry>>> loadList(String name);

  /// Сохраняет записи списка.
  Future<Result<void>> saveList(String name, List<ListEntry> entries);

  /// Возвращает путь к файлу списка.
  Future<Result<String>> getListPath(String name);

  /// Создаёт бэкап списка.
  Future<Result<void>> backupList(String name);
}
