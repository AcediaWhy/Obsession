/// Информация о доступном обновлении.
class UpdateInfo {
  final String version;
  final String downloadUrl;
  final String releaseNotes;
  final DateTime? publishedAt;

  /// Ожидаемый SHA-256 скачиваемого ассета в hex (нижний регистр), если
  /// опубликован в релизе (sidecar-ассет `<name>.sha256`). `null`, если
  /// контрольная сумма недоступна — тогда установка блокируется как
  /// непроверяемая (см. [UpdateNotifier.runInstaller]).
  final String? sha256;

  const UpdateInfo({
    required this.version,
    required this.downloadUrl,
    required this.releaseNotes,
    this.publishedAt,
    this.sha256,
  });
}
