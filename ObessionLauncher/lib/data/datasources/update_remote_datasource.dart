import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;

import '../../core/constants/app_constants.dart';
import '../../core/errors/failures.dart';
import '../../core/errors/result.dart';
import '../../domain/entities/update_info.dart';

/// Интерфейс для проверки и скачивания обновлений.
abstract class IUpdateRemoteDataSource {
  Future<Result<UpdateInfo>> checkLatest();

  /// Скачивает файл по [url] в [destinationPath].
  /// [onProgress] вызывается с долей (0.0–1.0) по мере загрузки.
  Future<Result<String>> download(
    String url,
    String destinationPath, {
    void Function(double progress)? onProgress,
  });
}

/// Реализация через GitHub Releases API.
class UpdateRemoteDataSource implements IUpdateRemoteDataSource {
  final http.Client _client;

  UpdateRemoteDataSource({http.Client? client}) : _client = client ?? http.Client();

  @override
  Future<Result<UpdateInfo>> checkLatest() async {
    try {
      final response = await _client.get(
        Uri.parse(AppConstants.githubApiLatestRelease),
        headers: {'Accept': 'application/vnd.github+json'},
      ).timeout(const Duration(seconds: 15));
      if (response.statusCode != 200) {
        return Failure(UpdateFailure('Ошибка сервера: ${response.statusCode}'));
      }
      final json = jsonDecode(response.body) as Map<String, dynamic>;
      // Убираем ТОЛЬКО ведущий 'v' (v1.2.3 → 1.2.3), не любой 'v' в строке.
      final rawTag = json['tag_name'] as String? ?? '';
      final tagName = rawTag.startsWith('v') ? rawTag.substring(1) : rawTag;
      if (tagName.isEmpty) {
        return Failure(UpdateFailure('Не удалось определить версию релиза'));
      }
      final assets = (json['assets'] as List<dynamic>? ?? []).cast<Map<String, dynamic>>();
      final asset = assets.firstWhere(
        (a) => a['name'] == AppConstants.singleExeAssetName,
        orElse: () => <String, dynamic>{},
      );
      final downloadUrl = asset['browser_download_url'] as String?;
      if (downloadUrl == null || downloadUrl.isEmpty) {
        return Failure(UpdateFailure('Ассет ${AppConstants.singleExeAssetName} не найден'));
      }
      // Sidecar-ассет с контрольной суммой: <exe>.sha256.
      final expectedSha = await _fetchSha256(assets);
      final publishedAt = DateTime.tryParse(json['published_at'] as String? ?? '');
      return Success(UpdateInfo(
        version: tagName,
        downloadUrl: downloadUrl,
        releaseNotes: json['body'] as String? ?? '',
        publishedAt: publishedAt,
        sha256: expectedSha,
      ));
    } on TimeoutException {
      return Failure(UpdateFailure('Превышено время ожидания сервера обновлений'));
    } on FormatException catch (e) {
      return Failure(UpdateFailure('Неверный ответ сервера: $e'));
    } catch (e) {
      return Failure(UpdateFailure('Не удалось проверить обновления: $e'));
    }
  }

  /// Скачивает и парсит sidecar-ассет `<exe>.sha256` (если он есть в релизе).
  /// Формат — стандартный вывод sha256sum: `<hex>  <filename>` или просто hex.
  /// Возвращает hex в нижнем регистре или `null`, если ассет отсутствует
  /// либо не удалось распарсить валидный 64-символьный hex.
  Future<String?> _fetchSha256(List<Map<String, dynamic>> assets) async {
    final shaAsset = assets.firstWhere(
      (a) => a['name'] == '${AppConstants.singleExeAssetName}.sha256',
      orElse: () => <String, dynamic>{},
    );
    final shaUrl = shaAsset['browser_download_url'] as String?;
    if (shaUrl == null || shaUrl.isEmpty) return null;
    try {
      final resp = await _client
          .get(Uri.parse(shaUrl))
          .timeout(const Duration(seconds: 15));
      if (resp.statusCode != 200) return null;
      final token = resp.body.trim().split(RegExp(r'\s+')).first.toLowerCase();
      if (RegExp(r'^[0-9a-f]{64}$').hasMatch(token)) return token;
      return null;
    } catch (_) {
      return null;
    }
  }

  @override
  Future<Result<String>> download(
    String url,
    String destinationPath, {
    void Function(double progress)? onProgress,
  }) async {
    IOSink? sink;
    final file = File(destinationPath);
    try {
      final request = http.Request('GET', Uri.parse(url));
      final response = await _client.send(request).timeout(
        const Duration(seconds: 30),
      );
      if (response.statusCode != 200) {
        return Failure(UpdateFailure('Ошибка загрузки: ${response.statusCode}'));
      }

      final total = response.contentLength ?? 0;
      await file.create(recursive: true);
      sink = file.openWrite();

      var received = 0;
      // Idle-timeout на каждый чанк: зависшее соединение в середине загрузки
      // прерывается, а не висит бесконечно (ранее таймаут был только на send).
      await for (final chunk
          in response.stream.timeout(const Duration(seconds: 30))) {
        sink.add(chunk);
        received += chunk.length;
        if (onProgress != null) {
          if (total > 0) {
            onProgress((received / total).clamp(0.0, 1.0));
          } else {
            // Размер неизвестен — сообщаем о прогрессе кусками.
            onProgress(-1);
          }
        }
      }
      await sink.flush();
      await sink.close();
      sink = null;

      if (onProgress != null) onProgress(1.0);
      return Success(destinationPath);
    } catch (e) {
      // Удаляем частично скачанный файл, чтобы не запустить битый установщик.
      await sink?.close();
      sink = null;
      try {
        if (await file.exists()) await file.delete();
      } catch (_) {}
      return Failure(UpdateFailure('Не удалось скачать обновление: $e'));
    } finally {
      await sink?.close();
    }
  }
}
