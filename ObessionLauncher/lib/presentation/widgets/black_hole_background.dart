import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/entities/app_settings.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';

/// Фон с чёрной дырой: GLSL-шейдер + fallback CustomPainter.
class BlackHoleBackground extends ConsumerStatefulWidget {
  const BlackHoleBackground({super.key});

  @override
  ConsumerState<BlackHoleBackground> createState() => _BlackHoleBackgroundState();
}

class _BlackHoleBackgroundState extends ConsumerState<BlackHoleBackground>
    with SingleTickerProviderStateMixin {
  late final Ticker _ticker;
  Duration _elapsed = Duration.zero;
  FragmentProgram? _program;
  bool _loaded = false;
  Duration _lastFrame = Duration.zero;

  Offset _mouse = Offset.zero;
  Duration _lastMouseUpdate = Duration.zero;

  @override
  void initState() {
    super.initState();
    _loadShader();
    _ticker = Ticker((elapsed) {
      _elapsed = elapsed;
      if (!_loaded || !ref.read(settingsProvider).animationsEnabled) return;
      // Ограничиваем фон до 30 FPS для снижения нагрузки на GPU.
      if (elapsed - _lastFrame >= const Duration(milliseconds: 33)) {
        _lastFrame = elapsed;
        setState(() {});
      }
    });
    _ticker.start();
  }

  Future<void> _loadShader() async {
    try {
      _program = await FragmentProgram.fromAsset('shaders/black_hole.frag');
      if (mounted) setState(() => _loaded = true);
    } catch (_) {
      // Fallback активируется автоматически.
    }
  }

  @override
  void dispose() {
    _ticker.dispose();
    _program = null;
    super.dispose();
  }

  void _onMouseMove(PointerEvent event) {
    // Throttle mouse-move до 30 FPS, чтобы не перерисовывать на каждый пиксель.
    final now = _elapsed;
    if (now - _lastMouseUpdate < const Duration(milliseconds: 33)) return;
    _lastMouseUpdate = now;
    _mouse = event.position;
    // Не вызываем setState — mouse обновится при следующем тике _ticker.
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);

    return Listener(
      onPointerMove: _onMouseMove,
      onPointerHover: _onMouseMove,
      behavior: HitTestBehavior.translucent,
      child: SizedBox.expand(
        child: !_loaded || _program == null
            ? _BlackHolePainterWidget(
                settings: settings,
                elapsed: _elapsed,
                mouse: _mouse,
              )
            : _buildShader(settings),
      ),
    );
  }

  Widget _buildShader(AppSettings settings) {
    final size = MediaQuery.sizeOf(context);
    final shader = _program!.fragmentShader();
    final accent = settings.accentColor;

    shader.setFloat(0, size.width);
    shader.setFloat(1, size.height);
    shader.setFloat(2, _elapsed.inMilliseconds / 1000.0);
    shader.setFloat(3, 0.3 + settings.glowIntensity * 0.2); // uIntensity
    shader.setFloat(4, settings.backgroundBlobSpeed); // uSpeed
    shader.setFloat(5, accent.r);
    shader.setFloat(6, accent.g);
    shader.setFloat(7, accent.b);
    shader.setFloat(8, settings.blackHoleRadius); // uRadius
    shader.setFloat(9, settings.blackHoleDiskBrightness); // uDiskBrightness
    shader.setFloat(10, settings.blackHoleLensIntensity); // uLensIntensity
    shader.setFloat(11, _mouse.dx); // uMouse.x
    shader.setFloat(12, _mouse.dy); // uMouse.y

    return CustomPaint(
      size: size,
      painter: _ShaderPainter(shader),
    );
  }
}

class _ShaderPainter extends CustomPainter {
  final FragmentShader shader;

  _ShaderPainter(this.shader);

  @override
  void paint(Canvas canvas, Size size) {
    canvas.drawRect(
      Rect.fromLTWH(0, 0, size.width, size.height),
      Paint()..shader = shader,
    );
  }

  @override
  bool shouldRepaint(covariant _ShaderPainter oldDelegate) =>
      oldDelegate.shader != shader;
}

/// Fallback CustomPainter для чёрной дыры.
class _BlackHolePainterWidget extends StatelessWidget {
  final AppSettings settings;
  final Duration elapsed;
  final Offset mouse;

  const _BlackHolePainterWidget({
    required this.settings,
    required this.elapsed,
    required this.mouse,
  });

  @override
  Widget build(BuildContext context) {
    return CustomPaint(
      size: MediaQuery.sizeOf(context),
      painter: _BlackHolePainter(
        settings: settings,
        elapsed: elapsed,
        mouse: mouse,
      ),
    );
  }
}

class _BlackHolePainter extends CustomPainter {
  final AppSettings settings;
  final Duration elapsed;
  final Offset mouse;

  _BlackHolePainter({
    required this.settings,
    required this.elapsed,
    required this.mouse,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final center = Offset(size.width / 2, size.height / 2);
    final radius = size.shortestSide * settings.blackHoleRadius;

    // Starfield background
    final bgRect = Rect.fromLTWH(0, 0, size.width, size.height);
    canvas.drawRect(
      bgRect,
      Paint()..color = AppTheme.bgDeep,
    );

    // Mouse lens offset
    final mouseOffset = Offset(
      (mouse.dx - size.width / 2) * 0.05 * settings.blackHoleLensIntensity,
      (mouse.dy - size.height / 2) * 0.05 * settings.blackHoleLensIntensity,
    );
    final lensCenter = center + mouseOffset;

    final accent = settings.accentColor;

    // Accretion disk glow — цвета вытекают из акцента пресета, а не
    // захардкоженного оранжевого огня (раньше Obsidian/Terminal/Eclipse
    // выглядели одинаково).
    final diskPaint = Paint()
      ..shader = RadialGradient(
        colors: [
          Colors.white.withValues(alpha: 0.8 * settings.blackHoleDiskBrightness),
          accent.withValues(alpha: 0.6 * settings.blackHoleDiskBrightness),
          HSLColor.fromColor(accent).withHue((HSLColor.fromColor(accent).hue - 20) % 360).toColor().withValues(alpha: 0.4 * settings.blackHoleDiskBrightness),
          accent.withValues(alpha: 0.25 * settings.blackHoleDiskBrightness),
          Colors.transparent,
        ],
        stops: const [0.0, 0.2, 0.4, 0.7, 1.0],
      ).createShader(Rect.fromCircle(center: lensCenter, radius: radius * 3.5))
      ..maskFilter = MaskFilter.blur(BlurStyle.normal, radius * 0.3);

    canvas.drawCircle(lensCenter, radius * 3.5, diskPaint);

    // Photon ring
    final photonPaint = Paint()
      ..color = Colors.white.withValues(alpha: 0.7 * settings.blackHoleDiskBrightness)
      ..style = PaintingStyle.stroke
      ..strokeWidth = radius * 0.08
      ..maskFilter = MaskFilter.blur(BlurStyle.normal, radius * 0.15);

    canvas.drawCircle(lensCenter, radius * 1.5, photonPaint);

    // Event horizon
    final horizonPaint = Paint()
      ..color = Colors.black
      ..style = PaintingStyle.fill
      ..maskFilter = MaskFilter.blur(BlurStyle.normal, radius * 0.05);

    canvas.drawCircle(lensCenter, radius, horizonPaint);

    // Vignette
    final vignettePaint = Paint()
      ..shader = RadialGradient(
        colors: [Colors.transparent, Colors.black.withValues(alpha: 0.4)],
        stops: const [0.6, 1.0],
      ).createShader(bgRect);
    canvas.drawRect(bgRect, vignettePaint);
  }

  @override
  bool shouldRepaint(covariant _BlackHolePainter oldDelegate) =>
      oldDelegate.elapsed != elapsed ||
      oldDelegate.mouse != mouse ||
      oldDelegate.settings != settings;
}
