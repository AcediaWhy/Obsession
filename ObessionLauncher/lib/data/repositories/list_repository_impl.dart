import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/entities/list_entry.dart';
import '../../domain/repositories/i_list_repository.dart';
import '../datasources/list_local_datasource.dart';
import '../datasources/log_local_datasource.dart';

class ListRepositoryImpl implements IListRepository {
  ListRepositoryImpl({
    required this._dataSource,
    required this._log,
  });

  final ListLocalDataSource _dataSource;
  final LogLocalDataSource _log;

  @override
  Future<Result<List<String>>> getListNames() async {
    try {
      return Success(_dataSource.getListNames());
    } catch (e, st) {
      _log.error('Ошибка получения списков', error: e, stackTrace: st);
      return Failure(FileFailure('Не удалось получить списки', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<List<ListEntry>>> loadList(String name) async {
    try {
      final entries = await _dataSource.loadList(name);
      return Success(entries);
    } catch (e, st) {
      _log.error('Ошибка загрузки списка $name', error: e, stackTrace: st);
      return Failure(FileFailure('Не удалось загрузить список $name', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> saveList(String name, List<ListEntry> entries) async {
    try {
      await _dataSource.saveList(name, entries);
      _log.success('Список $name сохранён.');
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка сохранения списка $name', error: e, stackTrace: st);
      return Failure(FileFailure('Не удалось сохранить список $name', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<String>> getListPath(String name) async {
    try {
      return Success(_dataSource.getListPath(name));
    } catch (e, st) {
      return Failure(FileFailure('Ошибка получения пути списка', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> backupList(String name) async {
    try {
      await _dataSource.backupList(name);
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка бэкапа списка $name', error: e, stackTrace: st);
      return Failure(FileFailure('Не удалось создать бэкап списка $name', error: e, stackTrace: st));
    }
  }
}
