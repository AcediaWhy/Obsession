import 'dart:ui';

import 'app_theme_preset.dart';

/// Пользовательские настройки приложения.
class AppSettings {
  final String locale;
  final AppThemePreset themePreset;
  final Color accentColor;
  final double glowIntensity;
  final double blurStrength;
  final double backgroundBlobSpeed;
  final bool animationsEnabled;
  final bool minimizeToTray;
  final bool autostart;
  final bool blackHoleEnabled;
  final double particleDensity;
  final double blackHoleRadius;
  final double blackHoleDiskBrightness;
  final double blackHoleLensIntensity;
  final bool hasCompletedOnboarding;

  /// Режим анимированного фона. `null` = автоматически из пресета темы.
  final BackgroundMode? backgroundMode;

  const AppSettings({
    required this.locale,
    required this.themePreset,
    required this.accentColor,
    required this.glowIntensity,
    required this.blurStrength,
    required this.backgroundBlobSpeed,
    required this.animationsEnabled,
    required this.minimizeToTray,
    required this.autostart,
    required this.blackHoleEnabled,
    required this.particleDensity,
    required this.blackHoleRadius,
    required this.blackHoleDiskBrightness,
    required this.blackHoleLensIntensity,
    required this.hasCompletedOnboarding,
    this.backgroundMode,
  });

  factory AppSettings.defaults() => const AppSettings(
        locale: 'ru',
        themePreset: AppThemePreset.obsession,
        accentColor: Color(0xFF6366F1),
        glowIntensity: 0.6,
        blurStrength: 20.0,
        backgroundBlobSpeed: 1.0,
        animationsEnabled: true,
        minimizeToTray: true,
        autostart: false,
        blackHoleEnabled: true,
        particleDensity: 0.5,
        blackHoleRadius: 0.18,
        blackHoleDiskBrightness: 1.0,
        blackHoleLensIntensity: 0.6,
        hasCompletedOnboarding: false,
      );

  AppSettings copyWith({
    String? locale,
    AppThemePreset? themePreset,
    Color? accentColor,
    double? glowIntensity,
    double? blurStrength,
    double? backgroundBlobSpeed,
    bool? animationsEnabled,
    bool? minimizeToTray,
    bool? autostart,
    bool? blackHoleEnabled,
    double? particleDensity,
    double? blackHoleRadius,
    double? blackHoleDiskBrightness,
    double? blackHoleLensIntensity,
    bool? hasCompletedOnboarding,
    BackgroundMode? backgroundMode,
    bool clearBackgroundMode = false,
  }) =>
      AppSettings(
        locale: locale ?? this.locale,
        themePreset: themePreset ?? this.themePreset,
        accentColor: accentColor ?? this.accentColor,
        glowIntensity: glowIntensity ?? this.glowIntensity,
        blurStrength: blurStrength ?? this.blurStrength,
        backgroundBlobSpeed: backgroundBlobSpeed ?? this.backgroundBlobSpeed,
        animationsEnabled: animationsEnabled ?? this.animationsEnabled,
        minimizeToTray: minimizeToTray ?? this.minimizeToTray,
        autostart: autostart ?? this.autostart,
        blackHoleEnabled: blackHoleEnabled ?? this.blackHoleEnabled,
        particleDensity: particleDensity ?? this.particleDensity,
        blackHoleRadius: blackHoleRadius ?? this.blackHoleRadius,
        blackHoleDiskBrightness: blackHoleDiskBrightness ?? this.blackHoleDiskBrightness,
        blackHoleLensIntensity: blackHoleLensIntensity ?? this.blackHoleLensIntensity,
        hasCompletedOnboarding: hasCompletedOnboarding ?? this.hasCompletedOnboarding,
        backgroundMode: clearBackgroundMode ? null : (backgroundMode ?? this.backgroundMode),
      );

  /// Эффективный режим фона: явный оверрайд [backgroundMode], иначе дефолт пресета.
  BackgroundMode get effectiveBackgroundMode =>
      backgroundMode ?? themePreset.data.defaultBackgroundMode;
}
