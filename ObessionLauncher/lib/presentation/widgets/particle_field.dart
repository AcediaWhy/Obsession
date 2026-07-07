import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../providers/settings_provider.dart';

/// Поле частиц, притягиваемых чёрной дырой.
class ParticleField extends ConsumerStatefulWidget {
  const ParticleField({super.key});

  @override
  ConsumerState<ParticleField> createState() => _ParticleFieldState();
}

class _ParticleFieldState extends ConsumerState<ParticleField>
    with SingleTickerProviderStateMixin {
  late final Ticker _ticker;
  final List<Particle> _particles = [];
  final Random _rng = Random();
  double _lastTime = 0;
  Duration _lastFrame = Duration.zero;
  Size _size = Size.zero;
  int _frame = 0;

  @override
  void initState() {
    super.initState();
    _ticker = Ticker((elapsed) {
      // Ограничиваем частицы до 30 FPS.
      if (elapsed - _lastFrame < const Duration(milliseconds: 33)) return;
      _lastFrame = elapsed;
      final dt = (elapsed.inMilliseconds - _lastTime * 1000) / 1000.0;
      _lastTime = elapsed.inMilliseconds / 1000.0;
      if (dt > 0) {
        _update(dt);
      }
    });
    _ticker.start();
  }

  @override
  void dispose() {
    _ticker.dispose();
    super.dispose();
  }

  void _update(double dt) {
    final settings = ref.read(settingsProvider);
    if (!settings.blackHoleEnabled || !settings.animationsEnabled) return;

    final size = _size;
    if (size.isEmpty) return;

    final center = Offset(size.width / 2, size.height / 2);
    final blackHoleRadius = size.shortestSide * settings.blackHoleRadius;
    final targetCount = (200 + settings.particleDensity * 600).round();

    // Spawn new particles
    final spawnRate = (targetCount * 0.3).clamp(1.0, 20.0);
    final spawnCount = (spawnRate * dt).floor();
    for (var i = 0; i < spawnCount; i++) {
      if (_particles.length < targetCount) {
        _particles.add(_spawnParticle(size, center, blackHoleRadius));
      }
    }

    // Update particles
    for (var i = _particles.length - 1; i >= 0; i--) {
      final p = _particles[i];
      final toCenter = center - p.position;
      final dist = toCenter.distance;
      final dir = dist > 0 ? toCenter / dist : Offset.zero;

      // Gravity-like acceleration
      final gravity = 80.0 + settings.blackHoleRadius * 200.0;
      p.velocity += dir * gravity * dt;

      // Orbital tangential force for spiral effect
      final tangent = Offset(-dir.dy, dir.dx);
      p.velocity += tangent * 30.0 * dt;

      // Damping
      p.velocity *= 0.999;

      p.position += p.velocity * dt;

      // Fade as approaching black hole
      final fadeStart = blackHoleRadius * 4.0;
      if (dist < fadeStart) {
        p.opacity *= (dist / fadeStart).clamp(0.0, 1.0);
      }

      // Remove if consumed by black hole or invisible
      if (dist < blackHoleRadius * 1.2 || p.opacity < 0.01) {
        _particles.removeAt(i);
      }
    }

    _frame++;
    if (mounted) setState(() {});
  }

  Particle _spawnParticle(Size size, Offset center, double blackHoleRadius) {
    // Spawn from edges of screen
    final edge = _rng.nextInt(4);
    late Offset pos;
    switch (edge) {
      case 0:
        pos = Offset(_rng.nextDouble() * size.width, -10);
      case 1:
        pos = Offset(size.width + 10, _rng.nextDouble() * size.height);
      case 2:
        pos = Offset(_rng.nextDouble() * size.width, size.height + 10);
      default:
        pos = Offset(-10, _rng.nextDouble() * size.height);
    }

    final toCenter = center - pos;
    final dist = toCenter.distance;
    final dir = toCenter / dist;

    // Initial velocity slightly toward center with randomness
    final speed = 10.0 + _rng.nextDouble() * 40.0;
    final velocity = dir * speed + Offset(
      (_rng.nextDouble() - 0.5) * 30.0,
      (_rng.nextDouble() - 0.5) * 30.0,
    );

    return Particle(
      position: pos,
      velocity: velocity,
      size: 1.0 + _rng.nextDouble() * 2.5,
      opacity: 0.3 + _rng.nextDouble() * 0.7,
      colorVariation: _rng.nextDouble(),
    );
  }

  @override
  Widget build(BuildContext context) {
    _size = MediaQuery.sizeOf(context);
    final settings = ref.watch(settingsProvider);

    if (!settings.blackHoleEnabled || !settings.animationsEnabled) {
      return const SizedBox.shrink();
    }

    return CustomPaint(
      size: _size,
      painter: _ParticlePainter(
        particles: _particles,
        accentColor: settings.accentColor,
        blackHoleRadius: _size.shortestSide * settings.blackHoleRadius,
        center: Offset(_size.width / 2, _size.height / 2),
        frame: _frame,
      ),
    );
  }
}

class Particle {
  Offset position;
  Offset velocity;
  double size;
  double opacity;
  double colorVariation;

  Particle({
    required this.position,
    required this.velocity,
    required this.size,
    required this.opacity,
    required this.colorVariation,
  });
}

class _ParticlePainter extends CustomPainter {
  final List<Particle> particles;
  final Color accentColor;
  final double blackHoleRadius;
  final Offset center;
  final int frame;

  _ParticlePainter({
    required this.particles,
    required this.accentColor,
    required this.blackHoleRadius,
    required this.center,
    required this.frame,
  });

  @override
  void paint(Canvas canvas, Size size) {
    for (final p in particles) {
      final dist = (center - p.position).distance;
      final color = Color.lerp(
        const Color(0xFFFFFFFF),
        accentColor,
        p.colorVariation * 0.7,
      )!;

      final paint = Paint()
        ..color = color.withValues(alpha: p.opacity * (dist / (blackHoleRadius * 5.0)).clamp(0.2, 1.0))
        ..style = PaintingStyle.fill;

      canvas.drawCircle(p.position, p.size, paint);
    }
  }

  @override
  bool shouldRepaint(covariant _ParticlePainter oldDelegate) =>
      oldDelegate.frame != frame ||
      oldDelegate.accentColor != accentColor ||
      oldDelegate.blackHoleRadius != blackHoleRadius ||
      oldDelegate.center != center;
}
