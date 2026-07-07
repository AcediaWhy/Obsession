import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../../core/constants/app_constants.dart';
import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../../data/datasources/update_remote_datasource.dart';
import '../../domain/entities/update_info.dart';

export '../../data/datasources/update_remote_datasource.dart' show IUpdateRemoteDataSource;

/// Сравнивает две версии формата major.minor.patch.
int compareVersions(String a, String b) {
  List<int> parseVersion(String v) =>
      v.split('.').map(int.tryParse).map((n) => n ?? 0).toList();
  final av = parseVersion(a);
  final bv = parseVersion(b);
  for (var i = 0; i < 3; i++) {
    final ai = i < av.length ? av[i] : 0;
    final bi = i < bv.length ? bv[i] : 0;
    if (ai != bi) return ai - bi;
  }
  return 0;
}

/// Проверяет, есть ли на GitHub более новая версия.
class CheckForUpdateUseCase implements UseCase<UpdateInfo?, NoParams> {
  final IUpdateRemoteDataSource _dataSource;

  CheckForUpdateUseCase(this._dataSource);

  @override
  Future<Result<UpdateInfo?>> call(NoParams params) async {
    final result = await _dataSource.checkLatest();
    return result.when(
      success: (info) {
        if (compareVersions(info.version, AppConstants.appVersion) > 0) {
          return Success(info);
        }
        return const Success(null);
      },
      failure: (failure) => Failure(failure),
    );
  }
}

/// Скачивает обновление во временную директорию.
class DownloadUpdateUseCase implements UseCase<String, UpdateInfo> {
  final IUpdateRemoteDataSource _dataSource;

  DownloadUpdateUseCase(this._dataSource);

  @override
  Future<Result<String>> call(UpdateInfo info) async {
    return callWithProgress(info, null);
  }

  /// Скачивает обновление с отчётом прогресса [onProgress] (0.0–1.0).
  Future<Result<String>> callWithProgress(
    UpdateInfo info,
    void Function(double progress)? onProgress,
  ) async {
    final tempDir = await getTemporaryDirectory();
    final path = '${tempDir.path}${Platform.pathSeparator}Obsession_update.exe';
    return _dataSource.download(info.downloadUrl, path, onProgress: onProgress);
  }
}
