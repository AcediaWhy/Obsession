import 'dart:convert';
import 'dart:ui';

import '../../domain/entities/app_settings.dart';
import '../../domain/entities/dpi_config.dart';
import '../../domain/entities/hosts_config.dart';
import '../../domain/entities/profile.dart';
import '../../domain/entities/proxy_config.dart';
import '../../presentation/theme/app_theme_preset.dart';

/// JSON-модель профиля.
class ProfileModel {
  final String id;
  final String name;
  final AppSettings settings;
  final Set<DpiConfig> dpiConfigs;
  final AiProvider hostsProvider;
  final ProxyConfig proxyConfig;
  final DateTime createdAt;
  final DateTime updatedAt;

  const ProfileModel({
    required this.id,
    required this.name,
    required this.settings,
    required this.dpiConfigs,
    required this.hostsProvider,
    required this.proxyConfig,
    required this.createdAt,
    required this.updatedAt,
  });

  factory ProfileModel.fromEntity(Profile profile) => ProfileModel(
        id: profile.id,
        name: profile.name,
        settings: profile.settings,
        dpiConfigs: profile.dpiConfigs,
        hostsProvider: profile.hostsProvider,
        proxyConfig: profile.proxyConfig,
        createdAt: profile.createdAt,
        updatedAt: profile.updatedAt,
      );

  Profile toEntity() => Profile(
        id: id,
        name: name,
        settings: settings,
        dpiConfigs: dpiConfigs,
        hostsProvider: hostsProvider,
        proxyConfig: proxyConfig,
        createdAt: createdAt,
        updatedAt: updatedAt,
      );

  factory ProfileModel.fromJson(Map<String, dynamic> json) {
    final settingsJson = json['settings'] as Map<String, dynamic>? ?? {};
    final dpiJson = (json['dpiConfigs'] as List<dynamic>?) ?? [];
    return ProfileModel(
      id: json['id'] as String? ?? '',
      name: json['name'] as String? ?? 'Default',
      settings: AppSettings(
        locale: settingsJson['locale'] as String? ?? 'ru',
        themePreset: AppThemePreset.values.firstWhere(
          (p) => p.name == (settingsJson['themePreset'] as String? ?? 'obsession'),
          orElse: () => AppThemePreset.obsession,
        ),
        accentColor: Color(settingsJson['accentColor'] as int? ?? 0xFF6366F1),
        glowIntensity: (settingsJson['glowIntensity'] as num?)?.toDouble() ?? 0.6,
        blurStrength: (settingsJson['blurStrength'] as num?)?.toDouble() ?? 20.0,
        backgroundBlobSpeed: (settingsJson['backgroundBlobSpeed'] as num?)?.toDouble() ?? 1.0,
        animationsEnabled: settingsJson['animationsEnabled'] as bool? ?? true,
        minimizeToTray: settingsJson['minimizeToTray'] as bool? ?? true,
        autostart: settingsJson['autostart'] as bool? ?? false,
        blackHoleEnabled: settingsJson['blackHoleEnabled'] as bool? ?? true,
        particleDensity: (settingsJson['particleDensity'] as num?)?.toDouble() ?? 0.5,
        blackHoleRadius: (settingsJson['blackHoleRadius'] as num?)?.toDouble() ?? 0.18,
        blackHoleDiskBrightness: (settingsJson['blackHoleDiskBrightness'] as num?)?.toDouble() ?? 1.0,
        blackHoleLensIntensity: (settingsJson['blackHoleLensIntensity'] as num?)?.toDouble() ?? 0.6,
        hasCompletedOnboarding: settingsJson['hasCompletedOnboarding'] as bool? ?? true,
        backgroundMode: _readBackgroundMode(settingsJson['backgroundMode'] as String?),
      ),
      dpiConfigs: dpiJson
          .map((e) => DpiConfig(
                category: e['category'] as String,
                configFile: e['configFile'] as String,
              ))
          .toSet(),
      hostsProvider: AiProvider.values.firstWhere(
        (p) => p.name == (json['hostsProvider'] as String? ?? 'malw'),
        orElse: () => AiProvider.malw,
      ),
      proxyConfig: ProxyConfig(
        port: json['proxyPort'] as int? ?? 1443,
        fakeTlsDomain: json['proxyFakeTlsDomain'] as String? ?? '',
      ),
      createdAt: DateTime.tryParse(json['createdAt'] as String? ?? '') ?? DateTime.now(),
      updatedAt: DateTime.tryParse(json['updatedAt'] as String? ?? '') ?? DateTime.now(),
    );
  }

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'settings': {
          'locale': settings.locale,
          'themePreset': settings.themePreset.name,
          'accentColor': settings.accentColor.toARGB32(),
          'glowIntensity': settings.glowIntensity,
          'blurStrength': settings.blurStrength,
          'backgroundBlobSpeed': settings.backgroundBlobSpeed,
          'animationsEnabled': settings.animationsEnabled,
          'minimizeToTray': settings.minimizeToTray,
          'autostart': settings.autostart,
          'blackHoleEnabled': settings.blackHoleEnabled,
          'particleDensity': settings.particleDensity,
          'blackHoleRadius': settings.blackHoleRadius,
          'blackHoleDiskBrightness': settings.blackHoleDiskBrightness,
          'blackHoleLensIntensity': settings.blackHoleLensIntensity,
          'hasCompletedOnboarding': settings.hasCompletedOnboarding,
          'backgroundMode': settings.backgroundMode?.name ?? 'auto',
        },
        'dpiConfigs': dpiConfigs
            .map((c) => {'category': c.category, 'configFile': c.configFile})
            .toList(),
        'hostsProvider': hostsProvider.name,
        'proxyPort': proxyConfig.port,
        'proxyFakeTlsDomain': proxyConfig.fakeTlsDomain,
        'createdAt': createdAt.toIso8601String(),
        'updatedAt': updatedAt.toIso8601String(),
      };

  String toRawJson() => jsonEncode(toJson());

  static BackgroundMode? _readBackgroundMode(String? raw) {
    if (raw == null || raw.isEmpty || raw == 'auto') return null;
    try {
      return BackgroundMode.values.firstWhere((e) => e.name == raw);
    } catch (_) {
      return null;
    }
  }
}
