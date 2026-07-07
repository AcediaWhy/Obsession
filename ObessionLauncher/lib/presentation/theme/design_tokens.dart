import 'package:flutter/material.dart';

/// Единая шкала дизайн-токенов для консистентности spacing, radius,
/// elevation и анимаций по всему приложению.
///
/// Используйте через `DesignTokens.of(context)` или импортируйте
/// константы напрямую, если не зависите от темы.
class DesignTokens {
  DesignTokens._();

  // ---- Spacing ----
  static const double space0 = 0;
  static const double space1 = 4;
  static const double space2 = 8;
  static const double space3 = 12;
  static const double space4 = 16;
  static const double space5 = 24;
  static const double space6 = 32;
  static const double space7 = 48;
  static const double space8 = 64;

  // ---- Radius ----
  static const double radiusNone = 0;
  static const double radiusSmall = 6;
  static const double radiusMedium = 10;
  static const double radiusLarge = 14;
  static const double radiusXLarge = 20;
  static const double radiusMax = 999;

  // ---- Elevation ----
  static List<Shadow> elevationLow(Color shadowColor) => [
        BoxShadow(
          color: shadowColor.withValues(alpha: 0.08),
          blurRadius: 8,
          offset: const Offset(0, 2),
        ),
      ];

  static List<Shadow> elevationMedium(Color shadowColor) => [
        BoxShadow(
          color: shadowColor.withValues(alpha: 0.12),
          blurRadius: 16,
          offset: const Offset(0, 4),
        ),
      ];

  static List<Shadow> elevationHigh(Color shadowColor) => [
        BoxShadow(
          color: shadowColor.withValues(alpha: 0.16),
          blurRadius: 32,
          offset: const Offset(0, 8),
        ),
      ];

  // ---- Animation ----
  static const Duration durationFast = Duration(milliseconds: 150);
  static const Duration durationMedium = Duration(milliseconds: 250);
  static const Duration durationSlow = Duration(milliseconds: 400);

  // ---- Typography scale ----
  static const double fontSizeXs = 10;
  static const double fontSizeSm = 12;
  static const double fontSizeBase = 13;
  static const double fontSizeMd = 14;
  static const double fontSizeLg = 16;
  static const double fontSizeXl = 20;
  static const double fontSize2Xl = 24;
  static const double fontSize3Xl = 28;

  // ---- Motion ----
  /// Определяет, нужно ли отключить анимации согласно
  /// `MediaQuery.of(context).disableAnimations` (reduce-motion).
  static bool reduceMotion(BuildContext context) {
    return MediaQuery.of(context).devicePixelRatio == 0
        ? false
        : MediaQuery.of(context).disableAnimations;
  }

  /// Применяет duration длительности анимации, учитывая reduce-motion.
  static Duration adaptiveDuration(BuildContext context, Duration normal) {
    return reduceMotion(context) ? Duration.zero : normal;
  }
}
