import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/entities/profile.dart';
import '../../domain/repositories/i_profile_repository.dart';
import '../datasources/log_local_datasource.dart';
import '../datasources/profile_local_datasource.dart';

class ProfileRepositoryImpl implements IProfileRepository {
  ProfileRepositoryImpl({
    required this._dataSource,
    required this._log,
  });

  final ProfileLocalDataSource _dataSource;
  final LogLocalDataSource _log;

  @override
  Future<Result<List<Profile>>> getProfiles() async {
    try {
      final profiles = await _dataSource.loadProfiles();
      return Success(profiles);
    } catch (e, st) {
      _log.error('Ошибка получения профилей', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось загрузить профили', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<Profile?>> getActiveProfile() async {
    try {
      final profile = await _dataSource.loadActiveProfile();
      return Success(profile);
    } catch (e, st) {
      _log.error('Ошибка получения активного профиля', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось загрузить активный профиль', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> saveProfile(Profile profile) async {
    try {
      final profiles = (await _dataSource.loadProfiles())..removeWhere((p) => p.id == profile.id);
      profiles.add(profile);
      await _dataSource.saveProfiles(profiles);
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка сохранения профиля', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось сохранить профиль', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> deleteProfile(String id) async {
    try {
      await _dataSource.deleteProfile(id);
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка удаления профиля', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось удалить профиль', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<void>> setActiveProfile(String id) async {
    try {
      final profiles = await _dataSource.loadProfiles();
      final profile = profiles.firstWhere((p) => p.id == id);
      await _dataSource.saveActiveProfile(profile);
      return const Success(null);
    } catch (e, st) {
      _log.error('Ошибка установки активного профиля', error: e, stackTrace: st);
      return Failure(SettingsFailure('Профиль не найден', error: e, stackTrace: st));
    }
  }

  @override
  Future<Result<Profile>> createProfile(String name) async {
    try {
      final profile = _dataSource.createProfile(name);
      final profiles = await _dataSource.loadProfiles()..add(profile);
      await _dataSource.saveProfiles(profiles);
      await _dataSource.saveActiveProfile(profile);
      return Success(profile);
    } catch (e, st) {
      _log.error('Ошибка создания профиля', error: e, stackTrace: st);
      return Failure(SettingsFailure('Не удалось создать профиль', error: e, stackTrace: st));
    }
  }
}
