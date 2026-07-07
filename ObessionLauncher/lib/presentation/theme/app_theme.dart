import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:google_fonts/google_fonts.dart';

import '../../domain/entities/app_settings.dart';
import 'app_theme_preset.dart';

/// Расширение темы, несущее параметры пресета Obsession.
///
/// Виджеты читают это расширение через `Theme.of(context).extension<ObsessionTheme>()`
/// вместо того, чтобы независимо `ref.watch(settingsProvider)` — это даёт
/// анимированный переход тем (через [lerp]) и убирает дублирование.
class ObsessionTheme extends ThemeExtension<ObsessionTheme> {
  final AppThemeData preset;

  const ObsessionTheme({required this.preset});

  // --- Общие ---
  Color get accent => preset.accentColor;
  Color get secondaryAccent => preset.secondaryAccent;
  double get glowIntensity => preset.glowIntensity;
  double get blackHoleRadius => preset.blackHoleRadius;
  double get blackHoleDiskBrightness => preset.blackHoleDiskBrightness;
  double get blackHoleLensIntensity => preset.blackHoleLensIntensity;
  double get particleDensity => preset.particleDensity;
  String get monoFont => preset.monoFont;
  double get borderRadius => preset.borderRadius;
  bool get sharpAngles => preset.sharpAngles;
  String get headingFont => preset.headingFont;
  String get bodyFont => preset.bodyFont;

  // --- Текст ---
  Color get textPrimary => preset.textPrimary;
  Color get textSecondary => preset.textSecondary;
  Color get textMuted => preset.textMuted;

  // --- Карточки ---
  Color get cardColor => preset.cardColor;
  double get cardOpacity => preset.cardOpacity;
  double get cardOpacityHover => preset.cardOpacityHover;
  Color get cardBorderColor => preset.cardBorderColor;
  Color get edgeBloomColor => preset.edgeBloomColor;

  // --- Светлый стек ---
  bool get isLight => preset.isLight;
  ThemeBrightness get brightness => preset.brightness;
  LightBackgroundConfig? get lightBackground => preset.lightBackground;
  double get iridescenceIntensity => preset.iridescenceIntensity;
  List<Color> get iridescenceTints => preset.iridescenceTints;

  /// Цвет поверхности для карточек/панелей с учётом яркости темы.
  /// Тёмные темы — тёмная поверхность, светлые — белый с прозрачностью.
  Color get surfaceColor =>
      isLight ? cardColor.withValues(alpha: cardOpacity) : const Color(0xFF0B0F1A);

  /// Цвет фона-заливки для вложенных контейнеров (status-карточки, логи).
  Color get insetSurfaceColor =>
      isLight ? const Color(0xFFFFFFFF).withValues(alpha: 0.45) : const Color(0xFF05070D).withValues(alpha: 0.6);

  /// Цвет тонкой границы по умолчанию.
  Color get hairlineBorder =>
      isLight ? const Color(0xFFFFFFFF).withValues(alpha: 0.6) : Colors.white.withValues(alpha: 0.08);

  TextStyle get monoStyle => GoogleFonts.jetBrainsMono(
        textStyle: TextStyle(
          color: textSecondary,
          fontSize: 12,
          height: 1.4,
        ),
      );

  /// Стиль заголовка (Nunito для светлых тем, Inter для тёмных).
  TextStyle headingStyle({double? fontSize, FontWeight? fontWeight, Color? color}) {
    final base = TextStyle(
      fontSize: fontSize,
      fontWeight: fontWeight ?? FontWeight.bold,
      color: color ?? textPrimary,
      fontFamily: isLight ? preset.headingFont : null,
    );
    if (isLight) {
      return base;
    }
    return GoogleFonts.inter(textStyle: base);
  }

  /// Стиль основного текста (Plus Jakarta Sans для светлых тем, Inter для тёмных).
  TextStyle bodyStyle({double? fontSize, FontWeight? fontWeight, Color? color, double? height}) {
    final base = TextStyle(
      fontSize: fontSize,
      fontWeight: fontWeight,
      color: color ?? textPrimary,
      height: height,
      fontFamily: isLight ? preset.bodyFont : null,
    );
    if (isLight) {
      return base;
    }
    return GoogleFonts.inter(textStyle: base);
  }

  @override
  ThemeExtension<ObsessionTheme> copyWith({AppThemeData? preset}) {
    return ObsessionTheme(preset: preset ?? this.preset);
  }

  @override
  ThemeExtension<ObsessionTheme> lerp(ObsessionTheme? other, double t) {
    if (other == null) return this;
    final a = preset;
    final b = other.preset;
    return ObsessionTheme(
      preset: AppThemeData(
        accentColor: Color.lerp(a.accentColor, b.accentColor, t)!,
        secondaryAccent: Color.lerp(a.secondaryAccent, b.secondaryAccent, t)!,
        glowIntensity: lerpDouble(a.glowIntensity, b.glowIntensity, t)!,
        blackHoleRadius: lerpDouble(a.blackHoleRadius, b.blackHoleRadius, t)!,
        blackHoleDiskBrightness: lerpDouble(a.blackHoleDiskBrightness, b.blackHoleDiskBrightness, t)!,
        blackHoleLensIntensity: lerpDouble(a.blackHoleLensIntensity, b.blackHoleLensIntensity, t)!,
        particleDensity: lerpDouble(a.particleDensity, b.particleDensity, t)!,
        uiFont: t < 0.5 ? a.uiFont : b.uiFont,
        monoFont: t < 0.5 ? a.monoFont : b.monoFont,
        borderRadius: lerpDouble(a.borderRadius, b.borderRadius, t)!,
        sharpAngles: t < 0.5 ? a.sharpAngles : b.sharpAngles,
        brightness: t < 0.5 ? a.brightness : b.brightness,
        defaultBackgroundMode: t < 0.5 ? a.defaultBackgroundMode : b.defaultBackgroundMode,
        textPrimary: Color.lerp(a.textPrimary, b.textPrimary, t)!,
        textSecondary: Color.lerp(a.textSecondary, b.textSecondary, t)!,
        textMuted: Color.lerp(a.textMuted, b.textMuted, t)!,
        cardColor: Color.lerp(a.cardColor, b.cardColor, t)!,
        cardOpacity: lerpDouble(a.cardOpacity, b.cardOpacity, t)!,
        cardOpacityHover: lerpDouble(a.cardOpacityHover, b.cardOpacityHover, t)!,
        cardBorderColor: Color.lerp(a.cardBorderColor, b.cardBorderColor, t)!,
        edgeBloomColor: Color.lerp(a.edgeBloomColor, b.edgeBloomColor, t)!,
        headingFont: t < 0.5 ? a.headingFont : b.headingFont,
        bodyFont: t < 0.5 ? a.bodyFont : b.bodyFont,
        lightBackground: t < 0.5 ? a.lightBackground : b.lightBackground,
        iridescenceIntensity: lerpDouble(a.iridescenceIntensity, b.iridescenceIntensity, t)!,
        iridescenceTints: t < 0.5 ? a.iridescenceTints : b.iridescenceTints,
      ),
    );
  }
}

/// Космическая неоновая палитра и тема приложения Obsession.
class AppTheme {
  AppTheme._();

  // Тёмная палитра (космический неон) — используется тёмными пресетами.
  static const Color bgDeep = Color(0xFF05070D);
  static const Color bgSurface = Color(0xFF0B0F1A);
  static const Color bgSurfaceAlt = Color(0xFF111827);
  static const Color border = Color(0xFF1E293B);

  static const Color textPrimary = Color(0xFFF8FAFC);
  static const Color textSecondary = Color(0xFF94A3B8);
  // Поднят с 0xFF64748B для соответствия WCAG AA (контраст ~4.6:1 на bgDeep).
  static const Color textMuted = Color(0xFF7C8AA1);

  static const Color success = Color(0xFF34D399);
  static const Color error = Color(0xFFFB7185);
  static const Color warning = Color(0xFFFBBF24);

  /// Базовый цвет нативного окна для пресета (для windowManager.setBackgroundColor).
  /// Тёмные темы — глубокий чёрный, светлые — молочно-белый.
  static Color windowBackgroundColor(AppSettings settings) {
    final preset = resolvePreset(settings);
    if (preset.isLight) {
      // Молочно-белый, совпадает с верхом baseGradient.
      return preset.lightBackground?.baseGradient.first ?? const Color(0xFFFFFCFF);
    }
    return bgDeep;
  }

  // --- Контекстные хелперы: цвета, адаптирующиеся под яркость темы ---

  static Color textPrimaryOf(BuildContext context) =>
      obsessionThemeOf(context)?.textPrimary ?? textPrimary;
  static Color textSecondaryOf(BuildContext context) =>
      obsessionThemeOf(context)?.textSecondary ?? textSecondary;
  static Color textMutedOf(BuildContext context) =>
      obsessionThemeOf(context)?.textMuted ?? textMuted;

  /// Цвет вложенной поверхности (status-карточки, логи, dropdown).
  static Color insetSurfaceOf(BuildContext context) {
    final t = obsessionThemeOf(context);
    return t?.insetSurfaceColor ?? bgSurface.withValues(alpha: 0.4);
  }

  /// Цвет фона для dropdown/dialog поверхностей (непрозрачный).
  static Color surfaceOf(BuildContext context) {
    final t = obsessionThemeOf(context);
    if (t == null) return bgSurfaceAlt;
    return t.isLight ? const Color(0xFFFFFFFF) : bgSurfaceAlt;
  }

  /// Тонкая граница по умолчанию.
  static Color hairlineOf(BuildContext context) {
    final t = obsessionThemeOf(context);
    return t?.hairlineBorder ?? Colors.white.withValues(alpha: 0.08);
  }

  /// Разрешает параметры текущего пресета.
  /// Для [AppThemePreset.custom] берёт значения из [AppSettings].
  static AppThemeData resolvePreset(AppSettings settings) {
    if (settings.themePreset == AppThemePreset.custom) {
      return AppThemeData(
        accentColor: settings.accentColor,
        secondaryAccent: settings.accentColor.withValues(alpha: 0.7),
        glowIntensity: settings.glowIntensity,
        blackHoleRadius: settings.blackHoleRadius,
        blackHoleDiskBrightness: settings.blackHoleDiskBrightness,
        blackHoleLensIntensity: settings.blackHoleLensIntensity,
        particleDensity: settings.particleDensity,
        uiFont: 'Inter',
        monoFont: 'JetBrains Mono',
        borderRadius: 12.0,
        sharpAngles: false,
      );
    }
    return settings.themePreset.data;
  }

  static ThemeData themeFrom(AppSettings settings) {
    final preset = resolvePreset(settings);
    final accent = preset.accentColor;
    final isLight = preset.isLight;

    if (isLight) {
      return _lightTheme(preset, accent);
    }
    return _darkTheme(preset, accent);
  }

  static ThemeData _darkTheme(AppThemeData preset, Color accent) {
    final baseTheme = ThemeData(
      useMaterial3: true,
      brightness: Brightness.dark,
      scaffoldBackgroundColor: Colors.transparent,
      canvasColor: bgDeep,
      colorScheme: ColorScheme.dark(
        primary: accent,
        secondary: preset.secondaryAccent,
        surface: bgSurface,
        error: error,
      ),
      fontFamily: preset.uiFont,
      textTheme: GoogleFonts.interTextTheme(
        const TextTheme(
          bodyLarge: TextStyle(color: textPrimary),
          bodyMedium: TextStyle(color: textSecondary),
          bodySmall: TextStyle(color: textMuted),
        ),
      ),
      dividerColor: border,
    );

    return baseTheme.copyWith(
      textTheme: GoogleFonts.interTextTheme(baseTheme.textTheme),
      snackBarTheme: SnackBarThemeData(
        behavior: SnackBarBehavior.floating,
        backgroundColor: bgSurfaceAlt,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        contentTextStyle: const TextStyle(color: textPrimary, fontSize: 13),
      ),
      extensions: [ObsessionTheme(preset: preset)],
    );
  }

  static ThemeData _lightTheme(AppThemeData preset, Color accent) {
    final baseGradient = preset.lightBackground?.baseGradient ??
        [const Color(0xFFFFFCFF), const Color(0xFFF7F4FF)];
    final base = baseGradient.first;

    final baseTheme = ThemeData(
      useMaterial3: true,
      brightness: Brightness.light,
      scaffoldBackgroundColor: Colors.transparent,
      canvasColor: base,
      colorScheme: ColorScheme.light(
        primary: accent,
        secondary: preset.secondaryAccent,
        surface: base,
        error: error,
        onSurface: preset.textPrimary,
      ),
      fontFamily: preset.bodyFont,
      textTheme: _lightTextTheme(preset),
      dividerColor: const Color(0xFFFFFFFF).withValues(alpha: 0.6),
    );

    return baseTheme.copyWith(
      textTheme: _lightTextTheme(preset),
      snackBarTheme: SnackBarThemeData(
        behavior: SnackBarBehavior.floating,
        backgroundColor: const Color(0xFFFFFFFF).withValues(alpha: 0.9),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(preset.borderRadius)),
        contentTextStyle: TextStyle(color: preset.textPrimary, fontSize: 13),
      ),
      extensions: [ObsessionTheme(preset: preset)],
    );
  }

  /// Светлая TextTheme на шрифтах пресета (Plus Jakarta Sans / Nunito) из assets.
  static TextTheme _lightTextTheme(AppThemeData preset) {
    final bodyFont = preset.bodyFont;
    final headingFont = preset.headingFont;
    final tp = preset.textPrimary;
    final ts = preset.textSecondary;
    final tm = preset.textMuted;

    return TextTheme(
      // Заголовки — Nunito.
      displayLarge: TextStyle(color: tp, fontWeight: FontWeight.bold, fontFamily: headingFont),
      displayMedium: TextStyle(color: tp, fontWeight: FontWeight.bold, fontFamily: headingFont),
      displaySmall: TextStyle(color: tp, fontWeight: FontWeight.bold, fontFamily: headingFont),
      headlineLarge: TextStyle(color: tp, fontWeight: FontWeight.bold, fontFamily: headingFont),
      headlineMedium: TextStyle(color: tp, fontWeight: FontWeight.bold, fontFamily: headingFont),
      headlineSmall: TextStyle(color: tp, fontWeight: FontWeight.w700, fontFamily: headingFont),
      titleLarge: TextStyle(color: tp, fontWeight: FontWeight.w700, fontFamily: headingFont),
      titleMedium: TextStyle(color: tp, fontWeight: FontWeight.w600, fontFamily: headingFont),
      titleSmall: TextStyle(color: ts, fontWeight: FontWeight.w600, fontFamily: headingFont),
      // Тело — Plus Jakarta Sans.
      bodyLarge: TextStyle(color: tp, fontFamily: bodyFont),
      bodyMedium: TextStyle(color: ts, fontFamily: bodyFont),
      bodySmall: TextStyle(color: tm, fontFamily: bodyFont),
      labelLarge: TextStyle(color: tp, fontWeight: FontWeight.w600, fontFamily: bodyFont),
      labelMedium: TextStyle(color: ts, fontWeight: FontWeight.w500, fontFamily: bodyFont),
      labelSmall: TextStyle(color: tm, fontFamily: bodyFont),
    );
  }
}

/// Утилита: получить [ObsessionTheme] из контекста.
ObsessionTheme? obsessionThemeOf(BuildContext context) =>
    Theme.of(context).extension<ObsessionTheme>();
