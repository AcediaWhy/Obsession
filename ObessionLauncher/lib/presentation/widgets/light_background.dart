import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';

import '../../domain/entities/app_theme_preset.dart';

/// Параметризованный светлый фон: молочно-белая база + дрейфующие pastel-halos
/// с mouse-параллаксом + опциональные вращающиеся кольца / микрогрид / sparkles
/// / soft bloom / god rays / cursor-halo.
///
/// Один виджет обслуживает все светлые пресеты через [LightBackgroundConfig]
/// (Aurora Mist, Candy Terminal, Seraphim). Интерактивность: halos и кольца
/// реагируют на курсор (параллакс/наклон), под курсором — мягкое свечение.
class LightBackground extends StatefulWidget {
  final LightBackgroundConfig config;
  final bool animationsEnabled;
  /// Множитель скорости дрейфа halos.
  final double blobSpeed;
  /// Сила размытия halos (из настроек blurStrength).
  final double blurStrength;

  const LightBackground({
    super.key,
    required this.config,
    this.animationsEnabled = true,
    this.blobSpeed = 1.0,
    this.blurStrength = 20.0,
  });

  @override
  State<LightBackground> createState() => _LightBackgroundState();
}

class _LightBackgroundState extends State<LightBackground>
    with SingleTickerProviderStateMixin {
  late final TickerProvider _vsync;
  Ticker? _ticker;
  Duration? _lastElapsed;
  double _time = 0; // секунды
  Offset _mouse = const Offset(0.5, 0.5); // нормализованная позиция
  Offset _targetMouse = const Offset(0.5, 0.5);
  final List<_Sparkle> _sparkles = [];

  @override
  void initState() {
    super.initState();
    _vsync = this;
    _initSparkles();
    if (widget.animationsEnabled) {
      _startTicker();
    }
  }

  void _initSparkles() {
    _sparkles.clear();
    final spec = widget.config.sparkles;
    if (spec == null) return;
    final rng = math.Random(42);
    for (int i = 0; i < spec.count; i++) {
      _sparkles.add(_Sparkle(
        x: rng.nextDouble(),
        y: rng.nextDouble(),
        phase: rng.nextDouble() * math.pi * 2,
        size: 0.4 + rng.nextDouble() * spec.maxSize,
        speed: 0.6 + rng.nextDouble() * 0.8,
      ));
    }
  }

  void _startTicker() {
    _ticker = _vsync.createTicker(_onTick);
    _ticker!.start();
  }

  void _onTick(Duration elapsed) {
    final last = _lastElapsed;
    _lastElapsed = elapsed;
    if (last == null) return;
    final dt = (elapsed - last).inMicroseconds / 1000000.0;
    if (dt <= 0 || dt > 0.1) return; // защита от скачков

    setState(() {
      _time += dt * widget.blobSpeed;
      // Плавное преследование курсора (easing) — parallax lag.
      _mouse = Offset(
        _mouse.dx + (_targetMouse.dx - _mouse.dx) * math.min(1, dt * 6),
        _mouse.dy + (_targetMouse.dy - _mouse.dy) * math.min(1, dt * 6),
      );
    });
  }

  @override
  void didUpdateWidget(covariant LightBackground old) {
    super.didUpdateWidget(old);
    if (old.config.sparkles != widget.config.sparkles) {
      _initSparkles();
    }
    if (old.animationsEnabled != widget.animationsEnabled) {
      if (widget.animationsEnabled && _ticker == null) {
        _startTicker();
      } else if (!widget.animationsEnabled && _ticker != null) {
        _ticker!.dispose();
        _ticker = null;
        _lastElapsed = null;
      }
    }
  }

  @override
  void dispose() {
    _ticker?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final size = Size(constraints.maxWidth, constraints.maxHeight);
        return MouseRegion(
          onHover: (e) {
            setState(() {
              _targetMouse = Offset(
                e.localPosition.dx / size.width,
                e.localPosition.dy / size.height,
              );
            });
          },
          onExit: (_) {
            setState(() => _targetMouse = const Offset(0.5, 0.5));
          },
          child: ClipRect(
            child: CustomPaint(
              size: size,
              painter: _LightBackgroundPainter(
                config: widget.config,
                size: size,
                time: _time,
                mouse: _mouse,
                blurStrength: widget.blurStrength,
                animationsEnabled: widget.animationsEnabled,
                sparkles: _sparkles,
              ),
            ),
          ),
        );
      },
    );
  }
}

class _Sparkle {
  final double x;
  final double y;
  final double phase;
  final double size;
  final double speed;
  _Sparkle({
    required this.x,
    required this.y,
    required this.phase,
    required this.size,
    required this.speed,
  });
}

class _LightBackgroundPainter extends CustomPainter {
  final LightBackgroundConfig config;
  final Size size;
  final double time;
  final Offset mouse;
  final double blurStrength;
  final bool animationsEnabled;
  final List<_Sparkle> sparkles;

  _LightBackgroundPainter({
    required this.config,
    required this.size,
    required this.time,
    required this.mouse,
    required this.blurStrength,
    required this.animationsEnabled,
    required this.sparkles,
  });

  @override
  void paint(Canvas canvas, Size canvasSize) {
    // 1. Базовый вертикальный градиент.
    final baseRect = Rect.fromLTWH(0, 0, size.width, size.height);
    final basePaint = Paint()
      ..shader = LinearGradient(
        begin: Alignment.topCenter,
        end: Alignment.bottomCenter,
        colors: config.baseGradient,
      ).createShader(baseRect);
    canvas.drawRect(baseRect, basePaint);

    // 2. Дрейфующие halos с параллаксом (blur через saveLayer + mask).
    _paintHalos(canvas);

    // 3. Микрогрид (Candy Terminal).
    if (config.grid != null) {
      _paintGrid(canvas);
    }

    // 4. Вращающиеся кольца (Seraphim).
    if (config.rings != null) {
      _paintRings(canvas);
    }

    // 5. God rays (Seraphim) — мягкие диагональные лучи.
    if (config.godRays) {
      _paintGodRays(canvas);
    }

    // 6. Sparkles — редкие мерцающие точки.
    if (config.sparkles != null && animationsEnabled) {
      _paintSparkles(canvas);
    }

    // 7. Cursor halo — мягкое свечение под курсором.
    if (config.cursorHaloColor != null) {
      _paintCursorHalo(canvas);
    }

    // 8. Общий soft bloom по краям.
    if (config.bloomIntensity > 0) {
      _paintBloom(canvas);
    }
  }

  void _paintHalos(Canvas canvas) {
    final blurSigma = blurStrength * 1.6;
    for (final halo in config.halos) {
      // Дрейф: синусоида по drift-вектору.
      final driftPhase = animationsEnabled
          ? math.sin(time * 2 * math.pi / (halo.durationMs / 1000.0))
          : 0.0;
      final drift = Offset(
        halo.drift.dx * driftPhase,
        halo.drift.dy * driftPhase,
      );
      // Параллакс от курсора.
      final px = (mouse.dx - 0.5) * size.width * halo.parallaxDepth;
      final py = (mouse.dy - 0.5) * size.height * halo.parallaxDepth;
      final cx = size.width * 0.5 + halo.startPosition.dx + drift.dx + px;
      final cy = size.height * 0.5 + halo.startPosition.dy + drift.dy + py;
      final radius = halo.size / 2;

      final rect = Rect.fromCircle(center: Offset(cx, cy), radius: radius);
      final paint = Paint()
        ..shader = RadialGradient(
          colors: [
            halo.color.withValues(alpha: 0.55),
            halo.color.withValues(alpha: 0.18),
            halo.color.withValues(alpha: 0.0),
          ],
          stops: const [0.0, 0.55, 1.0],
        ).createShader(rect)
        ..maskFilter = animationsEnabled
            ? MaskFilter.blur(BlurStyle.normal, blurSigma)
            : null;
      canvas.drawCircle(Offset(cx, cy), radius, paint);
    }
  }

  void _paintGrid(Canvas canvas) {
    final grid = config.grid!;
    final paint = Paint()
      ..color = grid.color.withValues(alpha: 0.22)
      ..strokeWidth = 0.6;

    for (double x = 0; x <= size.width; x += grid.cellSize) {
      canvas.drawLine(Offset(x, 0), Offset(x, size.height), paint);
    }
    for (double y = 0; y <= size.height; y += grid.cellSize) {
      canvas.drawLine(Offset(0, y), Offset(size.width, y), paint);
    }

    // Подсветка грида вокруг курсора.
    if (grid.cursorGlowIntensity > 0 && animationsEnabled) {
      final cx = mouse.dx * size.width;
      final cy = mouse.dy * size.height;
      final glowPaint = Paint()
        ..shader = RadialGradient(
          colors: [
            grid.color.withValues(alpha: 0.35 * grid.cursorGlowIntensity),
            grid.color.withValues(alpha: 0.0),
          ],
          stops: const [0.0, 1.0],
        ).createShader(Rect.fromCircle(
          center: Offset(cx, cy),
          radius: grid.cursorGlowRadius,
        ));
      canvas.drawCircle(Offset(cx, cy), grid.cursorGlowRadius, glowPaint);
    }
  }

  void _paintRings(Canvas canvas) {
    final rings = config.rings!;
    final center = Offset(size.width * 0.5, size.height * 0.55);
    // Наклон к курсору — псевдо-3D.
    final tiltDx = (mouse.dx - 0.5) * 0.25;
    final tiltDy = (mouse.dy - 0.5) * 0.25;
    final baseAngle = animationsEnabled ? time * rings.speed : 0.0;

    for (int i = 0; i < rings.count; i++) {
      final radius = (size.width * rings.baseRadius) + i * (size.width * rings.radiusStep);
      final angle = baseAngle + i * 0.7;
      final ringPaint = Paint()
        ..color = rings.color.withValues(alpha: 0.5 - i * 0.1)
        ..style = PaintingStyle.stroke
        ..strokeWidth = rings.thickness;

      // Эллипс с наклоном (перспектива) + лёгкий поворот.
      final tilt = rings.tilt + tiltDy * 0.3;
      final ry = radius * (1.0 - tilt.clamp(0.0, 0.85));
      final rx = radius * (1.0 + tiltDx * 0.15);

      canvas.save();
      canvas.translate(center.dx, center.dy);
      canvas.rotate(angle);
      canvas.drawOval(Rect.fromCenter(center: Offset.zero, width: rx * 2, height: ry * 2), ringPaint);
      canvas.restore();
    }
  }

  void _paintGodRays(Canvas canvas) {
    // Мягкие диагональные лучи из верхнего левого угла, медленно дрейфующие.
    final cx = size.width * 0.18;
    final cy = size.height * 0.1;
    final rayCount = 5;
    final baseAngle = animationsEnabled ? time * 0.05 : 0.0;
    for (int i = 0; i < rayCount; i++) {
      final angle = 0.6 + i * 0.12 + baseAngle + (mouse.dx - 0.5) * 0.1;
      final length = size.width * 1.4;
      final dx = math.cos(angle) * length;
      final dy = math.sin(angle) * length;
      final paint = Paint()
        ..shader = LinearGradient(
          begin: Alignment(0, 0),
          end: Alignment(dx / size.width, dy / size.height),
          colors: [
            const Color(0xFFFFFFFF).withValues(alpha: 0.18),
            const Color(0xFFFFF6FA).withValues(alpha: 0.0),
          ],
        ).createShader(Rect.fromPoints(Offset(cx, cy), Offset(cx + dx, cy + dy)));
      final path = Path()
        ..moveTo(cx, cy)
        ..lineTo(cx + dx + 40, cy + dy - 40)
        ..lineTo(cx + dx + 40, cy + dy + 40)
        ..close();
      canvas.drawPath(path, paint..style = PaintingStyle.fill);
    }
  }

  void _paintSparkles(Canvas canvas) {
    final spec = config.sparkles!;
    for (final s in sparkles) {
      final t = (time * s.speed + s.phase) % (math.pi * 2);
      final twinkle = (math.sin(t) + 1) / 2; // 0..1
      final alpha = 0.2 + 0.8 * twinkle;
      // Лёгкий параллакс sparkles.
      final px = (mouse.dx - 0.5) * size.width * 0.05;
      final py = (mouse.dy - 0.5) * size.height * 0.05;
      final cx = s.x * size.width + px;
      final cy = s.y * size.height + py;
      final paint = Paint()
        ..color = spec.color.withValues(alpha: alpha * 0.9)
        ..style = PaintingStyle.fill;
      // Крестик-блик.
      final r = s.size * (0.6 + twinkle * 0.6);
      canvas.drawCircle(Offset(cx, cy), r, paint);
      if (twinkle > 0.7) {
        final linePaint = Paint()
          ..color = spec.color.withValues(alpha: alpha * 0.5)
          ..strokeWidth = 0.5;
        canvas.drawLine(Offset(cx - r * 3, cy), Offset(cx + r * 3, cy), linePaint);
        canvas.drawLine(Offset(cx, cy - r * 3), Offset(cx, cy + r * 3), linePaint);
      }
    }
  }

  void _paintCursorHalo(Canvas canvas) {
    final cx = mouse.dx * size.width;
    final cy = mouse.dy * size.height;
    final paint = Paint()
      ..shader = RadialGradient(
        colors: [
          config.cursorHaloColor!.withValues(alpha: 0.28),
          config.cursorHaloColor!.withValues(alpha: 0.0),
        ],
        stops: const [0.0, 1.0],
      ).createShader(Rect.fromCircle(
        center: Offset(cx, cy),
        radius: config.cursorHaloRadius,
      ));
    canvas.drawCircle(Offset(cx, cy), config.cursorHaloRadius, paint);
  }

  void _paintBloom(Canvas canvas) {
    // Мягкое виньеточное свечение по периметру.
    final rect = Rect.fromLTWH(0, 0, size.width, size.height);
    final paint = Paint()
      ..shader = RadialGradient(
        center: Alignment.center,
        colors: [
          Colors.transparent,
          const Color(0xFFFFFFFF).withValues(alpha: 0.0),
          const Color(0xFFFFF6FA).withValues(alpha: 0.12 * config.bloomIntensity),
        ],
        stops: const [0.0, 0.6, 1.0],
      ).createShader(rect);
    canvas.drawRect(rect, paint);
  }

  @override
  bool shouldRepaint(covariant _LightBackgroundPainter old) {
    return old.time != time ||
        old.mouse != mouse ||
        old.config != config ||
        old.blurStrength != blurStrength ||
        old.animationsEnabled != animationsEnabled;
  }
}
