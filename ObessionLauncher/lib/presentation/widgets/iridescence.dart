import 'dart:ui' as ui;

import 'package:flutter/material.dart';

import '../../domain/entities/app_theme_preset.dart';
/// Перламутровая (iridescence) обёртка для акцентных элементов Seraphim.
/// Применяет thin-film GLSL-шейдер ([iridescence.frag]) поверх дочернего
/// виджета через [ShaderMask], создавая переливающийся nacre-эффект.
///
/// Используется на сигнатурных элементах (power button, hero-карточки),
/// а не на всех панелях — обычные панели получают градиент-перелив в GlassPanel.
class Iridescence extends StatefulWidget {
  final Widget child;
  /// 0..1 — сила перелива (обычно берётся из preset.iridescenceIntensity).
  final double intensity;
  /// Пастельные tints перелива (5 цветов).
  final List<Color> tints;
  /// Скорость дрейфа.
  final double speed;
  /// Подсветка hover (0..1) — усиливает перелив.
  final double highlight;
  /// Радиус скругления маски (для соответствия форме дочернего виджета).
  final double borderRadius;

  const Iridescence({
    super.key,
    required this.child,
    required this.intensity,
    required this.tints,
    this.speed = 1.0,
    this.highlight = 0.0,
    this.borderRadius = 0.0,
  });

  @override
  State<Iridescence> createState() => _IridescenceState();
}

class _IridescenceState extends State<Iridescence>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller;
  ui.FragmentProgram? _program;
  bool _loaded = false;
  bool _failed = false;
  Offset _mouse = const Offset(0.5, 0.5);
  Offset _targetMouse = const Offset(0.5, 0.5);

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(seconds: 1),
    )..repeat();
    _loadShader();
  }

  Future<void> _loadShader() async {
    try {
      _program = await ui.FragmentProgram.fromAsset('shaders/iridescence.frag');
      if (mounted) setState(() => _loaded = true);
    } catch (_) {
      // Шейдер недоступен — fallback на градиентную маску.
      if (mounted) setState(() => _failed = true);
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Color _c(int i) => widget.tints[i % widget.tints.length];

  double _r(Color c) => c.r * 255.0;
  double _g(Color c) => c.g * 255.0;
  double _b(Color c) => c.b * 255.0;

  @override
  Widget build(BuildContext context) {
    if (_failed || !_loaded) {
      // Fallback: мягкий градиент-перелив без шейдера.
      return _gradientFallback();
    }
    return AnimatedBuilder(
      animation: _controller,
      builder: (context, child) {
        return MouseRegion(
          onHover: (e) {
            final box = context.findRenderObject() as RenderBox?;
            if (box == null) return;
            final size = box.size;
            setState(() {
              _targetMouse = Offset(
                (e.localPosition.dx / size.width).clamp(0.0, 1.0),
                (e.localPosition.dy / size.height).clamp(0.0, 1.0),
              );
            });
          },
          child: ShaderMask(
            shaderCallback: (bounds) {
              // Плавное преследование курсора.
              _mouse = Offset(
                _mouse.dx + (_targetMouse.dx - _mouse.dx) * 0.15,
                _mouse.dy + (_targetMouse.dy - _mouse.dy) * 0.15,
              );
              final fragment = _program!.fragmentShader();
              fragment
                ..setFloat(0, bounds.width)
                ..setFloat(1, bounds.height)
                ..setFloat(2, _controller.value * 60)
                ..setFloat(3, widget.intensity)
                ..setFloat(4, widget.speed)
                ..setFloat(5, _r(_c(0)))
                ..setFloat(6, _g(_c(0)))
                ..setFloat(7, _b(_c(0)))
                ..setFloat(8, _r(_c(1)))
                ..setFloat(9, _g(_c(1)))
                ..setFloat(10, _b(_c(1)))
                ..setFloat(11, _r(_c(2)))
                ..setFloat(12, _g(_c(2)))
                ..setFloat(13, _b(_c(2)))
                ..setFloat(14, _r(_c(3)))
                ..setFloat(15, _g(_c(3)))
                ..setFloat(16, _b(_c(3)))
                ..setFloat(17, _r(_c(4)))
                ..setFloat(18, _g(_c(4)))
                ..setFloat(19, _b(_c(4)))
                ..setFloat(20, _mouse.dx)
                ..setFloat(21, _mouse.dy)
                ..setFloat(22, widget.highlight);
              return fragment;
            },
            blendMode: BlendMode.srcOver,
            child: child,
          ),
        );
      },
      child: ClipRRect(
        borderRadius: BorderRadius.circular(widget.borderRadius),
        child: widget.child,
      ),
    );
  }

  Widget _gradientFallback() {
    // Мягкий анимированный multi-stop градиент как запасной перелив.
    return AnimatedBuilder(
      animation: _controller,
      builder: (context, child) {
        final t = _controller.value;
        final colors = <Color>[
          for (int i = 0; i < widget.tints.length; i++)
            Color.lerp(widget.tints[i], widget.tints[(i + 1) % widget.tints.length], t)!,
        ];
        return DecoratedBox(
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(widget.borderRadius),
            gradient: LinearGradient(
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
              colors: colors,
            ),
          ),
          child: child,
        );
      },
      child: widget.child,
    );
  }
}

/// Утилита: проверить, нужно ли применять iridescence (для пресета Seraphim).
bool shouldUseIridescence(AppThemeData preset) =>
    preset.iridescenceIntensity > 0 && preset.iridescenceTints.isNotEmpty;
