import 'app_settings.dart';
import 'dpi_config.dart';
import 'hosts_config.dart';
import 'proxy_config.dart';

/// Профиль пользователя: сохраняет всё состояние приложения.
class Profile {
  final String id;
  final String name;
  final AppSettings settings;
  final Set<DpiConfig> dpiConfigs;
  final AiProvider hostsProvider;
  final ProxyConfig proxyConfig;
  final DateTime createdAt;
  final DateTime updatedAt;

  const Profile({
    required this.id,
    required this.name,
    required this.settings,
    required this.dpiConfigs,
    required this.hostsProvider,
    required this.proxyConfig,
    required this.createdAt,
    required this.updatedAt,
  });

  factory Profile.create({
    required String id,
    required String name,
    AppSettings? settings,
    Set<DpiConfig>? dpiConfigs,
    AiProvider? hostsProvider,
    ProxyConfig? proxyConfig,
  }) {
    final now = DateTime.now();
    return Profile(
      id: id,
      name: name,
      settings: settings ?? AppSettings.defaults(),
      dpiConfigs: dpiConfigs ?? const {},
      hostsProvider: hostsProvider ?? AiProvider.malw,
      proxyConfig: proxyConfig ?? ProxyConfig.defaults(),
      createdAt: now,
      updatedAt: now,
    );
  }

  Profile copyWith({
    String? id,
    String? name,
    AppSettings? settings,
    Set<DpiConfig>? dpiConfigs,
    AiProvider? hostsProvider,
    ProxyConfig? proxyConfig,
    DateTime? createdAt,
    DateTime? updatedAt,
  }) =>
      Profile(
        id: id ?? this.id,
        name: name ?? this.name,
        settings: settings ?? this.settings,
        dpiConfigs: dpiConfigs ?? this.dpiConfigs,
        hostsProvider: hostsProvider ?? this.hostsProvider,
        proxyConfig: proxyConfig ?? this.proxyConfig,
        createdAt: createdAt ?? this.createdAt,
        updatedAt: updatedAt ?? this.updatedAt,
      );
}
