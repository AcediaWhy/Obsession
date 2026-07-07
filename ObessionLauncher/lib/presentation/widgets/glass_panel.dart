import 'dart:ui';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/entities/app_settings.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';

/// Glassmorphism-панель с неоновой окантовкой, hover-эффектом и entrance-анимацией.
///
/// Адаптируется под яркость темы:
/// - Тёмные темы: тёмный полупрозрачный glass + неоновый glow на hover.
/// - Светлые темы: полупрозрачный белый glass + пастельный edge-bloom на hover.
/// - Seraphim: pearlescent-режим — переливающийся жемчужный градиент-сweep
///   поверх белой панели (iridescence на акцентах — отдельным виджетом).
class GlassPanel extends ConsumerStatefulWidget {
  final Widget child;
  final EdgeInsets? padding;
  final EdgeInsets? margin;
  final double? borderRadius;
  final Color? borderColor;
  final Color? glowColor;
  final double? glowRadius;
  final VoidCallback? onTap;
  final bool animateEntrance;
  /// Принудительно включить pearlescent-перелив (для Seraphim hero-карточек).
  final bool pearlescent;

  const GlassPanel({
    super.key,
    required this.child,
    this.padding,
    this.margin,
    this.borderRadius,
    this.borderColor,
    this.glowColor,
    this.glowRadius,
    this.onTap,
    this.animateEntrance = true,
    this.pearlescent = false,
  });

  @override
  ConsumerState<GlassPanel> createState() => _GlassPanelState();
}

class _GlassPanelState extends ConsumerState<GlassPanel> {
  bool _hovered = false;
  bool _pressed = false;

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final br = widget.borderRadius ?? (theme?.borderRadius ?? 20.0);
    final blur = settings.animationsEnabled ? settings.blurStrength : 0.0;
    final glowColor = widget.glowColor ?? settings.accentColor;
    final hasHover = widget.onTap != null;
    final usePearlescent = widget.pearlescent || (theme?.iridescenceIntensity ?? 0) > 0.3;

    Widget panel = Container(
      margin: widget.margin,
      child: MouseRegion(
        onEnter: hasHover ? (_) => setState(() => _hovered = true) : null,
        onExit: hasHover ? (_) => setState(() => _hovered = false) : null,
        child: GestureDetector(
          onTap: widget.onTap,
          onTapDown: hasHover ? (_) => setState(() => _pressed = true) : null,
          onTapUp: hasHover ? (_) => setState(() => _pressed = false) : null,
          onTapCancel: hasHover ? () => setState(() => _pressed = false) : null,
          child: AnimatedContainer(
            duration: 200.ms,
            curve: Curves.easeOutCubic,
            transform: Matrix4.diagonal3Values(
              _pressed ? 0.99 : _hovered ? 1.01 : 1.0,
              _pressed ? 0.99 : _hovered ? 1.01 : 1.0,
              1.0,
            ),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(br),
              child: blur > 0
                  ? BackdropFilter(
                      filter: ImageFilter.blur(sigmaX: blur, sigmaY: blur),
                      child: _innerContainer(br, glowColor, settings, isLight, theme, usePearlescent),
                    )
                  : _innerContainer(br, glowColor, settings, isLight, theme, usePearlescent),
            ),
          ),
        ),
      ),
    );

    if (widget.animateEntrance && settings.animationsEnabled) {
      panel = panel
          .animate()
          .fadeIn(duration: 350.ms, curve: Curves.easeOut)
          .slideY(begin: 0.03, duration: 350.ms, curve: Curves.easeOut);
    }

    return panel;
  }

  Widget _innerContainer(
    double br,
    Color glowColor,
    AppSettings settings,
    bool isLight,
    ObsessionTheme? theme,
    bool pearlescent,
  ) {
    if (isLight) {
      return _lightContainer(br, glowColor, settings, theme, pearlescent);
    }
    return _darkContainer(br, glowColor, settings);
  }

  /// Тёмный glass (космический неон) — оригинальный стиль.
  Widget _darkContainer(double br, Color glowColor, AppSettings settings) {
    return Container(
      padding: widget.padding ?? const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: Colors.white.withValues(alpha: _hovered ? 0.06 : 0.04),
        borderRadius: BorderRadius.circular(br),
        border: Border.all(
          color: _hovered
              ? glowColor.withValues(alpha: 0.25)
              : widget.borderColor ?? Colors.white.withValues(alpha: 0.08),
          width: _hovered ? 1.5 : 1.0,
        ),
        boxShadow: [
          if (widget.glowColor != null || _hovered)
            BoxShadow(
              color: glowColor.withValues(alpha: settings.glowIntensity * (_hovered ? 0.6 : 0.5)),
              blurRadius: _hovered ? (widget.glowRadius ?? 20) + 6 : (widget.glowRadius ?? 20),
              spreadRadius: _hovered ? 1 : 0,
            ),
        ],
      ),
      child: widget.child,
    );
  }

  /// Светлый glass: полупрозрачный белый + пастельный edge-bloom.
  /// Pearlescent-режим добавляет переливающийся жемчужный sweep-градиент.
  Widget _lightContainer(
    double br,
    Color glowColor,
    AppSettings settings,
    ObsessionTheme? theme,
    bool pearlescent,
  ) {
    final cardColor = theme?.cardColor ?? const Color(0xFFFFFFFF);
    final cardOpacity = _hovered
        ? (theme?.cardOpacityHover ?? 0.72)
        : (theme?.cardOpacity ?? 0.55);
    final edgeBloom = theme?.edgeBloomColor ?? glowColor;
    final borderColor = widget.borderColor ??
        (theme?.cardBorderColor ?? const Color(0xFFFFFFFF)).withValues(alpha: 0.7);

    final decoration = BoxDecoration(
      color: cardColor.withValues(alpha: cardOpacity),
      borderRadius: BorderRadius.circular(br),
      border: Border.all(
        color: _hovered ? edgeBloom.withValues(alpha: 0.6) : borderColor,
        width: _hovered ? 1.5 : 1.0,
      ),
      boxShadow: [
        // Мягкая светлая тень — «парящая» панель.
        BoxShadow(
          color: Colors.black.withValues(alpha: 0.04 + (_hovered ? 0.02 : 0)),
          blurRadius: 18,
          offset: const Offset(0, 6),
        ),
        // Пастельный edge-bloom на hover.
        if (_hovered)
          BoxShadow(
            color: edgeBloom.withValues(alpha: settings.glowIntensity * 0.35),
            blurRadius: (widget.glowRadius ?? 22) + 6,
            spreadRadius: 1,
          ),
      ],
    );

    if (!pearlescent) {
      return Container(
        padding: widget.padding ?? const EdgeInsets.all(16),
        decoration: decoration,
        child: widget.child,
      );
    }

    // Pearlescent: переливающийся жемчужный sweep под контентом.
    return _PearlescentSweep(
      borderRadius: br,
      tints: theme?.iridescenceTints ?? const [Color(0xFFFFFFFF), Color(0xFFF8C4D8), Color(0xFFD8EEFF)],
      intensity: theme?.iridescenceIntensity ?? 0.5,
      decoration: decoration,
      padding: widget.padding ?? const EdgeInsets.all(16),
      child: widget.child,
    );
  }
}

/// Переливающийся жемчужный sweep-градиент (градиент-вариант перламутра
/// для обычных панелей; iridescence-шейдер применяется отдельно на акцентах).
class _PearlescentSweep extends StatefulWidget {
  final double borderRadius;
  final List<Color> tints;
  final double intensity;
  final BoxDecoration decoration;
  final Widget child;
  final EdgeInsets padding;

  const _PearlescentSweep({
    required this.borderRadius,
    required this.tints,
    required this.intensity,
    required this.decoration,
    required this.padding,
    required this.child,
  });

  @override
  State<_PearlescentSweep> createState() => _PearlescentSweepState();
}

class _PearlescentSweepState extends State<_PearlescentSweep>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(seconds: 12),
    )..repeat();
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: _controller,
      builder: (context, child) {
        final t = _controller.value;
        // Медленный sweep угла градиента.
        final angle = t * 2 * 3.14159265;
        final begin = Alignment(math.cos(angle), math.sin(angle));
        final end = Alignment(-begin.x, -begin.y);
        final sweepColors = [
          for (int i = 0; i < widget.tints.length; i++)
            widget.tints[i].withValues(alpha: 0.18 * widget.intensity),
        ];
        return DecoratedBox(
          decoration: widget.decoration.copyWith(
            gradient: LinearGradient(begin: begin, end: end, colors: sweepColors),
          ),
          child: Padding(padding: widget.padding, child: child),
        );
      },
      child: widget.child,
    );
  }
}
