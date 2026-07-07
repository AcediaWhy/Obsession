import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/usecases/usecase.dart';
import '../../domain/entities/update_info.dart';
import '../../domain/usecases/update_usecases.dart';
import 'dependency_providers.dart';

/// Состояние процесса обновления.
enum UpdateStatus {
  idle,
  checking,
  available,
  downloading,
  readyToInstall,
  upToDate,
  error,
}

class UpdateState {
  final UpdateStatus status;
  final UpdateInfo? info;
  final String? error;
  final double progress;
  final String? downloadedPath;
  final String? fileHash;

  const UpdateState({
    this.status = UpdateStatus.idle,
    this.info,
    this.error,
    this.progress = 0,
    this.downloadedPath,
    this.fileHash,
  });

  UpdateState copyWith({
    UpdateStatus? status,
    UpdateInfo? info,
    String? error,
    double? progress,
    String? downloadedPath,
    String? fileHash,
    bool clearError = false,
    bool clearDownloadedPath = false,
    bool clearFileHash = false,
  }) =>
      UpdateState(
        status: status ?? this.status,
        info: info ?? this.info,
        error: clearError ? null : (error ?? this.error),
        progress: progress ?? this.progress,
        downloadedPath:
            clearDownloadedPath ? null : (downloadedPath ?? this.downloadedPath),
        fileHash: clearFileHash ? null : (fileHash ?? this.fileHash),
      );
}

final updateProvider = StateNotifierProvider<UpdateNotifier, UpdateState>((ref) {
  return UpdateNotifier(
    checkUseCase: ref.watch(checkForUpdateUseCaseProvider),
    downloadUseCase: ref.watch(downloadUpdateUseCaseProvider),
  );
});

class UpdateNotifier extends StateNotifier<UpdateState> {
  UpdateNotifier({
    required this._checkUseCase,
    required this._downloadUseCase,
  })  : super(const UpdateState());

  final CheckForUpdateUseCase _checkUseCase;
  final DownloadUpdateUseCase _downloadUseCase;

  Future<void> check() async {
    state = state.copyWith(status: UpdateStatus.checking, clearError: true);
    final result = await _checkUseCase(const NoParams());
    result.when(
      success: (info) {
        if (info == null) {
          state = state.copyWith(status: UpdateStatus.upToDate);
        } else {
          state = state.copyWith(status: UpdateStatus.available, info: info);
        }
      },
      failure: (failure) => state = state.copyWith(
        status: UpdateStatus.error,
        error: failure.message,
      ),
    );
  }

  Future<void> download() async {
    final info = state.info;
    if (info == null) return;
    state = state.copyWith(
      status: UpdateStatus.downloading,
      progress: 0,
      clearError: true,
      clearDownloadedPath: true,
      clearFileHash: true,
    );
    final result = await _downloadUseCase.callWithProgress(
      info,
      (progress) {
        // progress == -1 означает, что размер неизвестен — не обновляем долю.
        if (progress >= 0) {
          state = state.copyWith(progress: progress);
        }
      },
    );
    result.when(
      success: (path) async {
        final hash = await _sha256File(path);
        final expected = info.sha256;
        // Верификация целостности: если релиз опубликовал контрольную сумму,
        // хеш скачанного файла обязан совпасть. Иначе — отказ (возможна
        // подмена/повреждение при загрузке).
        if (expected != null && expected.toLowerCase() != hash.toLowerCase()) {
          await _deleteQuietly(path);
          state = state.copyWith(
            status: UpdateStatus.error,
            error: 'Контрольная сумма не совпала — файл повреждён или подменён. '
                'Установка отменена.',
            clearDownloadedPath: true,
            clearFileHash: true,
          );
          return;
        }
        state = state.copyWith(
          status: UpdateStatus.readyToInstall,
          progress: 1,
          downloadedPath: path,
          fileHash: hash,
        );
      },
      failure: (failure) => state = state.copyWith(
        status: UpdateStatus.error,
        error: failure.message,
      ),
    );
  }

  Future<void> _deleteQuietly(String path) async {
    try {
      final f = File(path);
      if (await f.exists()) await f.delete();
    } catch (_) {}
  }

  Future<String> _sha256File(String path) async {
    final file = File(path);
    final bytes = await file.readAsBytes();
    return sha256.convert(bytes).toString();
  }

  Future<bool> openFolder() async {
    final path = state.downloadedPath;
    if (path == null || path.isEmpty) return false;
    try {
      final result = await Process.run(
        'explorer',
        ['/select,', path],
        runInShell: false,
      );
      return result.exitCode == 0;
    } catch (e) {
      state = state.copyWith(error: 'Не удалось открыть папку: $e');
      return false;
    }
  }

  Future<bool> runInstaller() async {
    final path = state.downloadedPath;
    if (path == null || path.isEmpty) return false;
    final info = state.info;
    try {
      final file = File(path);
      if (!await file.exists()) {
        state = state.copyWith(
          status: UpdateStatus.error,
          error: 'Файл установщика не найден.',
        );
        return false;
      }
      // Defense-in-depth: повторная сверка хеша непосредственно перед запуском
      // (файл мог измениться между скачиванием и установкой).
      final expected = info?.sha256;
      if (expected != null) {
        final actual = await _sha256File(path);
        if (expected.toLowerCase() != actual.toLowerCase()) {
          await _deleteQuietly(path);
          state = state.copyWith(
            status: UpdateStatus.error,
            error: 'Контрольная сумма не совпала — запуск установщика отменён.',
            clearDownloadedPath: true,
          );
          return false;
        }
      }
      // Запуск напрямую (без `cmd /c start`): путь передаётся как отдельный
      // аргумент, поэтому пробелы и спецсимволы не ломают команду.
      await Process.start(
        path,
        const [],
        runInShell: false,
        mode: ProcessStartMode.detached,
      );
      return true;
    } catch (e) {
      state = state.copyWith(error: 'Не удалось запустить установщик: $e');
      return false;
    }
  }

  void dismiss() => state = const UpdateState(status: UpdateStatus.idle);
}
