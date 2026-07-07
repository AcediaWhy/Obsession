import 'dart:convert';
import 'dart:ui';

import 'package:shared_preferences/shared_preferences.dart';

import '../../domain/entities/app_settings.dart';
import '../../presentation/theme/app_theme_preset.dart';

/// Хранение настроек в SharedPreferences.
class SettingsLocalDataSource {
  SettingsLocalDataSource();

  SharedPreferences? _prefs;

  static const _kLocale = 'locale';
  static const _kThemePreset = 'themePreset';
  static const _kAccent = 'accentColor';
  static const _kGlow = 'glowIntensity';
  static const _kBlur = 'blurStrength';
  static const _kBlobSpeed = 'backgroundBlobSpeed';
  static const _kAnimations = 'animationsEnabled';
  static const _kMinimizeTray = 'minimizeToTray';
  static const _kAutostart = 'autostart';
  static const _kBlackHole = 'blackHoleEnabled';
  static const _kParticleDensity = 'particleDensity';
  static const _kBlackHoleRadius = 'blackHoleRadius';
  static const _kBlackHoleDiskBrightness = 'blackHoleDiskBrightness';
  static const _kBlackHoleLensIntensity = 'blackHoleLensIntensity';
  static const _kOnboarding = 'hasCompletedOnboarding';
  static const _kSelectedConfigs = 'selectedConfigs';
  static const _kBackgroundMode = 'backgroundMode';

  Future<void> init() async {
    _prefs = await SharedPreferences.getInstance();
  }

  AppSettings load() {
    final p = _prefs;
    if (p == null) return AppSettings.defaults();

    final locale = p.getString(_kLocale) ?? 'ru';
    final presetName = p.getString(_kThemePreset);
    final preset = AppThemePreset.values.firstWhere(
      (e) => e.name == presetName,
      orElse: () => AppThemePreset.obsession,
    );

    return AppSettings(
      locale: locale,
      themePreset: preset,
      accentColor: Color(p.getInt(_kAccent) ?? 0xFF6366F1),
      glowIntensity: p.getDouble(_kGlow) ?? 0.6,
      blurStrength: p.getDouble(_kBlur) ?? 20.0,
      backgroundBlobSpeed: p.getDouble(_kBlobSpeed) ?? 1.0,
      animationsEnabled: p.getBool(_kAnimations) ?? true,
      minimizeToTray: p.getBool(_kMinimizeTray) ?? true,
      autostart: p.getBool(_kAutostart) ?? false,
      blackHoleEnabled: p.getBool(_kBlackHole) ?? true,
      particleDensity: p.getDouble(_kParticleDensity) ?? 0.5,
      blackHoleRadius: p.getDouble(_kBlackHoleRadius) ?? 0.18,
      blackHoleDiskBrightness: p.getDouble(_kBlackHoleDiskBrightness) ?? 1.0,
      blackHoleLensIntensity: p.getDouble(_kBlackHoleLensIntensity) ?? 0.6,
      hasCompletedOnboarding: p.getBool(_kOnboarding) ?? false,
      backgroundMode: _readBackgroundMode(p),
    );
  }

  BackgroundMode? _readBackgroundMode(SharedPreferences p) {
    final raw = p.getString(_kBackgroundMode);
    if (raw == null || raw.isEmpty || raw == 'auto') return null;
    try {
      return BackgroundMode.values.firstWhere((e) => e.name == raw);
    } catch (_) {
      return null;
    }
  }

  Future<void> save(AppSettings settings) async {
    final p = _prefs;
    if (p == null) return;
    await p.setString(_kLocale, settings.locale);
    await p.setString(_kThemePreset, settings.themePreset.name);
    await p.setInt(_kAccent, settings.accentColor.toARGB32());
    await p.setDouble(_kGlow, settings.glowIntensity);
    await p.setDouble(_kBlur, settings.blurStrength);
    await p.setDouble(_kBlobSpeed, settings.backgroundBlobSpeed);
    await p.setBool(_kAnimations, settings.animationsEnabled);
    await p.setBool(_kMinimizeTray, settings.minimizeToTray);
    await p.setBool(_kAutostart, settings.autostart);
    await p.setBool(_kBlackHole, settings.blackHoleEnabled);
    await p.setDouble(_kParticleDensity, settings.particleDensity);
    await p.setDouble(_kBlackHoleRadius, settings.blackHoleRadius);
    await p.setDouble(_kBlackHoleDiskBrightness, settings.blackHoleDiskBrightness);
    await p.setDouble(_kBlackHoleLensIntensity, settings.blackHoleLensIntensity);
    await p.setBool(_kOnboarding, settings.hasCompletedOnboarding);
    await p.setString(
      _kBackgroundMode,
      settings.backgroundMode?.name ?? 'auto',
    );
  }

  Future<void> reset() async => save(AppSettings.defaults());

  Map<String, String> getSelectedConfigs() {
    final p = _prefs;
    if (p == null) return {};
    final raw = p.getString(_kSelectedConfigs);
    if (raw == null || raw.isEmpty) return {};
    try {
      final decoded = jsonDecode(raw) as Map<String, dynamic>;
      return decoded.map((k, v) => MapEntry(k, v.toString()));
    } catch (_) {
      return {};
    }
  }

  Future<void> setSelectedConfig(String category, String configFile) async {
    final p = _prefs;
    if (p == null) return;
    final configs = getSelectedConfigs();
    configs[category] = configFile;
    await p.setString(_kSelectedConfigs, jsonEncode(configs));
  }
}
