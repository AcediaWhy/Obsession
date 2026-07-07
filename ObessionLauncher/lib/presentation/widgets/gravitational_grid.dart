import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../providers/settings_provider.dart';

/// Тонкая сетка, искривлённая гравитацией чёрной дыры.
/// Рисуется поверх фона, но под контентом.
class GravitationalGrid extends ConsumerStatefulWidget {
  const GravitationalGrid({super.key});

  @override
  ConsumerState<GravitationalGrid> createState() => _GravitationalGridState();
}

class _GravitationalGridState extends ConsumerState<GravitationalGrid> {
  Offset _mouse = Offset.zero;
  DateTime _lastRealTime = DateTime.now();

  void _onMouseMove(PointerEvent e) {
    final settings = ref.read(settingsProvider);
    // Не реагируем на движение мыши, если анимации отключены — иначе сетка
    // продолжает перерисовываться при наведении даже без движущегося фона.
    if (!settings.animationsEnabled) return;
    // Throttle mouse-move до ~30 FPS, чтобы не перерисовывать сетку на каждый пиксель.
    final now = DateTime.now();
    if (now.difference(_lastRealTime) < const Duration(milliseconds: 33)) return;
    _lastRealTime = now;
    setState(() => _mouse = e.position);
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final size = MediaQuery.sizeOf(context);
    final center = Offset(size.width / 2, size.height / 2);

    return Listener(
      onPointerMove: _onMouseMove,
      onPointerHover: _onMouseMove,
      behavior: HitTestBehavior.translucent,
      child: CustomPaint(
        size: size,
        painter: _GravitationalGridPainter(
          mouse: _mouse,
          center: center,
          accent: settings.accentColor,
          lensIntensity: settings.blackHoleLensIntensity,
          blackHoleRadius: settings.blackHoleRadius,
        ),
      ),
    );
  }
}

class _GravitationalGridPainter extends CustomPainter {
  final Offset mouse;
  final Offset center;
  final Color accent;
  final double lensIntensity;
  final double blackHoleRadius;
  Size _size = Size.zero;

  _GravitationalGridPainter({
    required this.mouse,
    required this.center,
    required this.accent,
    required this.lensIntensity,
    required this.blackHoleRadius,
  });

  @override
  void paint(Canvas canvas, Size size) {
    _size = size;
    if (lensIntensity <= 0.05) return;

    final paint = Paint()
      ..color = accent.withValues(alpha: 0.04 * lensIntensity)
      ..strokeWidth = 1
      ..style = PaintingStyle.stroke;

    final mouseOffset = (mouse - center) * 0.08 * lensIntensity;
    final blackHole = center + mouseOffset;
    final radius = size.shortestSide * blackHoleRadius;

    const steps = 12;
    final hStep = size.width / steps;
    final vStep = size.height / steps;

    // Vertical lines
    for (int i = 1; i < steps; i++) {
      final x = hStep * i;
      final path = Path();
      path.moveTo(x, 0);

      for (double y = 0; y <= size.height; y += 8) {
        final point = Offset(x, y);
        final distorted = _gravitate(point, blackHole, radius);
        path.lineTo(distorted.dx, distorted.dy);
      }

      canvas.drawPath(path, paint);
    }

    // Horizontal lines
    for (int i = 1; i < steps; i++) {
      final y = vStep * i;
      final path = Path();
      path.moveTo(0, y);

      for (double x = 0; x <= size.width; x += 8) {
        final point = Offset(x, y);
        final distorted = _gravitate(point, blackHole, radius);
        path.lineTo(distorted.dx, distorted.dy);
      }

      canvas.drawPath(path, paint);
    }
  }

  Offset _gravitate(Offset point, Offset blackHole, double radius) {
    final delta = point - blackHole;
    final dist = delta.distance;
    if (dist < radius * 0.5) return point;

    // Gravitational deflection: stronger near the black hole
    final strength = lensIntensity * radius / (dist + radius * 0.3);
    final direction = delta / dist;
    final falloff = (1.0 - (dist / (_size.shortestSide * 0.6))).clamp(0.0, 1.0);

    return point - direction * strength * 20 * falloff;
  }

  @override
  bool shouldRepaint(covariant _GravitationalGridPainter oldDelegate) =>
      oldDelegate.mouse != mouse ||
      oldDelegate.center != center ||
      oldDelegate.accent != accent ||
      oldDelegate.lensIntensity != lensIntensity ||
      oldDelegate.blackHoleRadius != blackHoleRadius;
}
