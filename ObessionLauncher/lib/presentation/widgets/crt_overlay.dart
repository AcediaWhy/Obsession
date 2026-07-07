import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../providers/settings_provider.dart';
import '../theme/app_theme_preset.dart';

/// CRT-оверлей для Terminal preset.
/// Добавляет scanlines, виньетку, лёгкое мерцание и glow текста.
class CrtOverlay extends ConsumerStatefulWidget {
  final Widget child;

  const CrtOverlay({super.key, required this.child});

  @override
  ConsumerState<CrtOverlay> createState() => _CrtOverlayState();
}

class _CrtOverlayState extends ConsumerState<CrtOverlay>
    with SingleTickerProviderStateMixin {
  late final AnimationController _flickerController;

  @override
  void initState() {
    super.initState();
    _flickerController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 120),
    )..repeat(reverse: true);
  }

  @override
  void dispose() {
    _flickerController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final isTerminal = settings.themePreset == AppThemePreset.terminal;

    if (!isTerminal) return widget.child;

    return Stack(
      children: [
        widget.child,
        Positioned.fill(
          child: IgnorePointer(
            child: CustomPaint(
              painter: _ScanlinesPainter(),
            ),
          ),
        ),
        Positioned.fill(
          child: IgnorePointer(
            child: CustomPaint(
              painter: _CrtVignettePainter(),
            ),
          ),
        ),
        Positioned.fill(
          child: IgnorePointer(
            child: AnimatedBuilder(
              animation: _flickerController,
              builder: (context, child) {
                return Opacity(
                  opacity: 0.015 + 0.01 * sin(_flickerController.value * pi * 2),
                  child: child,
                );
              },
              child: Container(
                color: Colors.black,
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _ScanlinesPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = Colors.black.withValues(alpha: 0.12)
      ..strokeWidth = 1;

    const lineHeight = 3.0;
    for (double y = 0; y < size.height; y += lineHeight) {
      canvas.drawLine(
        Offset(0, y),
        Offset(size.width, y),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}

class _CrtVignettePainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final rect = Rect.fromLTWH(0, 0, size.width, size.height);

    // Vignette
    final vignettePaint = Paint()
      ..shader = RadialGradient(
        colors: [
          Colors.transparent,
          Colors.black.withValues(alpha: 0.35),
        ],
        stops: const [0.5, 1.0],
      ).createShader(rect);
    canvas.drawRect(rect, vignettePaint);

    // Subtle curved edges (barrel distortion feel)
    final edgePaint = Paint()
      ..shader = LinearGradient(
        begin: Alignment.centerLeft,
        end: Alignment.centerRight,
        colors: [
          Colors.black.withValues(alpha: 0.15),
          Colors.transparent,
          Colors.transparent,
          Colors.black.withValues(alpha: 0.15),
        ],
      ).createShader(rect);
    canvas.drawRect(rect, edgePaint);
  }

  @override
  bool shouldRepaint(covariant CustomPainter oldDelegate) => false;
}
