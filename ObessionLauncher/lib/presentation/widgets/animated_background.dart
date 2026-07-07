import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';

/// Анимированный космический фон: глубокий тёмный фон + дрейфующие небулы.
class AnimatedBackground extends ConsumerWidget {
  const AnimatedBackground({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final accent = settings.accentColor;
    // Блобы теперь тематичны: главный — акцентный, остальные — близкие
    // оттенки, вычисленные через HSL, а не фиксированные фиолетовый/синий.
    // Раньше Terminal (зелёный) и Eclipse (оранжевый) теряли идентичность,
    // показывая фиолетово-синие пятна.
    final blobColors = _blobColorsFor(accent);

    return Container(
      decoration: const BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [
            AppTheme.bgDeep,
            Color(0xFF080B14),
            AppTheme.bgDeep,
          ],
        ),
      ),
      child: Stack(
        children: [
          if (settings.animationsEnabled) ...[
            _buildBlob(
              color: blobColors[0].withValues(alpha: 0.12),
              size: 400,
              top: -100,
              left: -50,
              end: const Offset(100, 80),
              duration: 8000,
            ),
            _buildBlob(
              color: blobColors[1].withValues(alpha: 0.10),
              size: 500,
              bottom: -120,
              right: -80,
              end: const Offset(-120, -60),
              duration: 10000,
            ),
            _buildBlob(
              color: blobColors[2].withValues(alpha: 0.08),
              size: 300,
              top: 200,
              right: 100,
              end: const Offset(60, -80),
              duration: 7000,
            ),
          ],
        ],
      ),
    );
  }

  /// Генерирует три гармоничных оттенка на основе акцентного цвета пресета.
  List<Color> _blobColorsFor(Color accent) {
    final hsl = HSLColor.fromColor(accent);
    return [
      accent,
      hsl.withHue((hsl.hue + 30) % 360).toColor(),
      hsl.withHue((hsl.hue - 30) % 360).withLightness((hsl.lightness * 0.8).clamp(0.0, 1.0)).toColor(),
    ];
  }

  Widget _buildBlob({
    required Color color,
    required double size,
    double? top,
    double? left,
    double? bottom,
    double? right,
    required Offset end,
    required int duration,
  }) {
    return Positioned(
      top: top,
      left: left,
      bottom: bottom,
      right: right,
      child: _Blob(color: color, size: size)
          .animate(onPlay: (c) => c.repeat(reverse: true))
          .move(
            begin: const Offset(0, 0),
            end: end,
            duration: duration.ms,
            curve: Curves.easeInOutSine,
          ),
    );
  }
}

class _Blob extends ConsumerWidget {
  final Color color;
  final double size;

  const _Blob({required this.color, required this.size});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final blur = settings.blurStrength * 2;

    return ImageFiltered(
      imageFilter: ImageFilter.blur(sigmaX: blur, sigmaY: blur),
      child: Container(
        width: size,
        height: size,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          gradient: RadialGradient(
            colors: [color, color.withValues(alpha: 0)],
          ),
        ),
      ),
    );
  }
}
