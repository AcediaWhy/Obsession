import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';

/// Анимированная кнопка с glow-эффектом.
///
/// Светлые темы: primary — pill с розово-лавандовым градиентом,
/// secondary (isOutline) — white-glass с голубым outline.
/// Тёмные темы: оригинальный неоновый стиль.
class GlowButton extends ConsumerStatefulWidget {
  final String label;
  final IconData? icon;
  final VoidCallback? onTap;
  final Color? color;
  final bool isOutline;
  final bool fullWidth;
  final EdgeInsets padding;

  const GlowButton({
    super.key,
    required this.label,
    this.icon,
    this.onTap,
    this.color,
    this.isOutline = false,
    this.fullWidth = false,
    this.padding = const EdgeInsets.symmetric(vertical: 12, horizontal: 16),
  });

  @override
  ConsumerState<GlowButton> createState() => _GlowButtonState();
}

class _GlowButtonState extends ConsumerState<GlowButton> {
  bool _hovered = false;
  bool _pressed = false;

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final color = widget.color ?? settings.accentColor;
    final glow = settings.glowIntensity;
    final disabled = widget.onTap == null;
    final radius = (theme?.borderRadius ?? 12.0) * 0.9;

    if (isLight) {
      return _lightButton(color, glow, disabled, radius, theme);
    }
    return _darkButton(color, glow, disabled, radius);
  }

  Widget _darkButton(Color color, double glow, bool disabled, double radius) {
    return GestureDetector(
      onTapDown: (_) => setState(() => _pressed = true),
      onTapUp: (_) => setState(() => _pressed = false),
      onTapCancel: () => setState(() => _pressed = false),
      onTap: widget.onTap,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: AnimatedContainer(
          duration: 150.ms,
          curve: Curves.easeOutCubic,
          transform: Matrix4.diagonal3Values(
            _pressed ? 0.96 : _hovered ? 1.03 : 1.0,
            _pressed ? 0.96 : _hovered ? 1.03 : 1.0,
            1.0,
          ),
          padding: widget.padding,
          decoration: BoxDecoration(
            color: widget.isOutline
                ? Colors.transparent
                : color.withValues(alpha: disabled ? 0.08 : 0.15),
            borderRadius: BorderRadius.circular(radius),
            border: Border.all(
              color: color.withValues(alpha: disabled ? 0.2 : widget.isOutline ? 0.5 : 0.4),
              width: 1.5,
            ),
            boxShadow: disabled
                ? null
                : [
                    BoxShadow(
                      color: color.withValues(alpha: glow * (_hovered ? 0.5 : 0.25)),
                      blurRadius: _hovered ? 20 : 12,
                      spreadRadius: _hovered ? 2 : 0,
                    ),
                  ],
          ),
          child: _content(color, disabled),
        ),
      ),
    );
  }

  Widget _lightButton(Color color, double glow, bool disabled, double radius, ObsessionTheme? theme) {
    // Primary: pill с розово-лавандовым градиентом. Secondary: white-glass + outline.
    final isPrimary = !widget.isOutline;
    final accent = color;
    final secondary = theme?.secondaryAccent ?? const Color(0xFFD9C6FF);
    final labelColor = isPrimary
        ? (disabled ? Colors.white.withValues(alpha: 0.5) : Colors.white)
        : (disabled ? accent.withValues(alpha: 0.4) : accent);

    return GestureDetector(
      onTapDown: (_) => setState(() => _pressed = true),
      onTapUp: (_) => setState(() => _pressed = false),
      onTapCancel: () => setState(() => _pressed = false),
      onTap: widget.onTap,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: AnimatedContainer(
          duration: 150.ms,
          curve: Curves.easeOutCubic,
          transform: Matrix4.diagonal3Values(
            _pressed ? 0.97 : _hovered ? 1.02 : 1.0,
            _pressed ? 0.97 : _hovered ? 1.02 : 1.0,
            1.0,
          ),
          padding: widget.padding,
          decoration: BoxDecoration(
            color: isPrimary
                ? null
                : Colors.white.withValues(alpha: disabled ? 0.4 : (_hovered ? 0.8 : 0.65)),
            gradient: isPrimary
                ? LinearGradient(
                    begin: Alignment.topLeft,
                    end: Alignment.bottomRight,
                    colors: disabled
                        ? [accent.withValues(alpha: 0.4), secondary.withValues(alpha: 0.4)]
                        : [accent, secondary],
                  )
                : null,
            borderRadius: BorderRadius.circular(radius * 1.6), // pill-ish
            border: Border.all(
              color: isPrimary
                  ? Colors.white.withValues(alpha: disabled ? 0.2 : 0.5)
                  : accent.withValues(alpha: disabled ? 0.2 : 0.5),
              width: 1.2,
            ),
            boxShadow: disabled
                ? null
                : [
                    BoxShadow(
                      color: accent.withValues(alpha: glow * (_hovered ? 0.4 : 0.18)),
                      blurRadius: _hovered ? 18 : 10,
                      spreadRadius: _hovered ? 1 : 0,
                    ),
                    if (isPrimary)
                      BoxShadow(
                        color: Colors.black.withValues(alpha: 0.06),
                        blurRadius: 8,
                        offset: const Offset(0, 3),
                      ),
                  ],
          ),
          child: _content(labelColor, disabled),
        ),
      ),
    );
  }

  Widget _content(Color color, bool disabled) {
    return Row(
      mainAxisSize: widget.fullWidth ? MainAxisSize.max : MainAxisSize.min,
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        if (widget.icon != null) ...[
          Icon(widget.icon, size: 18, color: disabled ? color.withValues(alpha: 0.5) : color),
          const SizedBox(width: 8),
        ],
        Text(
          widget.label,
          style: TextStyle(
            fontSize: 13,
            fontWeight: FontWeight.w600,
            color: disabled ? color.withValues(alpha: 0.5) : color,
          ),
        ),
      ],
    );
  }
}

/// Анимированная кнопка с иконкой.
class GlowIconButton extends ConsumerStatefulWidget {
  final IconData icon;
  final VoidCallback? onTap;
  final Color? color;
  final double size;
  final String? tooltip;

  const GlowIconButton({
    super.key,
    required this.icon,
    this.onTap,
    this.color,
    this.size = 40,
    this.tooltip,
  });

  @override
  ConsumerState<GlowIconButton> createState() => _GlowIconButtonState();
}

class _GlowIconButtonState extends ConsumerState<GlowIconButton> {
  bool _hovered = false;
  bool _pressed = false;

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final color = widget.color ?? settings.accentColor;
    final glow = settings.glowIntensity;
    final disabled = widget.onTap == null;

    Widget button = GestureDetector(
      onTapDown: (_) => setState(() => _pressed = true),
      onTapUp: (_) => setState(() => _pressed = false),
      onTapCancel: () => setState(() => _pressed = false),
      onTap: widget.onTap,
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: AnimatedContainer(
          duration: 150.ms,
          width: widget.size,
          height: widget.size,
          transform: Matrix4.diagonal3Values(
            _pressed ? 0.95 : 1.0,
            _pressed ? 0.95 : 1.0,
            1.0,
          ),
          decoration: BoxDecoration(
            color: isLight
                ? Colors.white.withValues(alpha: _hovered ? 0.8 : 0.0)
                : (_hovered ? color.withValues(alpha: 0.15) : Colors.transparent),
            borderRadius: BorderRadius.circular(isLight ? widget.size * 0.35 : 10),
            boxShadow: _hovered && !disabled
                ? [
                    if (isLight)
                      BoxShadow(
                        color: Colors.black.withValues(alpha: 0.05),
                        blurRadius: 8,
                        offset: const Offset(0, 2),
                      ),
                    BoxShadow(
                      color: color.withValues(alpha: glow * (isLight ? 0.25 : 0.4)),
                      blurRadius: 12,
                    ),
                  ]
                : null,
            border: isLight && _hovered
                ? Border.all(color: color.withValues(alpha: 0.4), width: 1.0)
                : null,
          ),
          child: Icon(
            widget.icon,
            size: 20,
            color: disabled
                ? color.withValues(alpha: 0.4)
                : (_hovered ? color : color.withValues(alpha: isLight ? 0.75 : 0.7)),
          ),
        ),
      ),
    );

    if (widget.tooltip != null) {
      button = Tooltip(
        message: widget.tooltip!,
        waitDuration: const Duration(milliseconds: 500),
        child: button,
      );
    }

    return Semantics(
      button: true,
      enabled: !disabled,
      label: widget.tooltip,
      child: button,
    );
  }
}

/// Пульсирующий индикатор статуса.
class AnimatedStatusIndicator extends StatelessWidget {
  final bool isActive;
  final double size;
  final Color? color;

  const AnimatedStatusIndicator({
    super.key,
    required this.isActive,
    this.size = 10,
    this.color,
  });

  @override
  Widget build(BuildContext context) {
    final color = this.color ?? (isActive ? const Color(0xFF34D399) : const Color(0xFFFB7185));

    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: color,
        shape: BoxShape.circle,
        boxShadow: isActive
            ? [
                BoxShadow(
                  color: color.withValues(alpha: 0.6),
                  blurRadius: size * 1.5,
                  spreadRadius: size * 0.3,
                ),
              ]
            : null,
      ),
    )
        .animate(onPlay: (c) => c.repeat(reverse: true))
        .scale(begin: const Offset(1, 1), end: const Offset(1.4, 1.4), duration: 1000.ms)
        .fade(begin: 0.7, end: 1.0, duration: 1000.ms);
  }
}

/// Анимированный текстовый заголовок с fade + slide.
/// Светлые темы: Nunito + тёмный текст. Тёмные темы: Inter + светлый текст.
class AnimatedHeader extends ConsumerWidget {
  final String title;
  final String? subtitle;

  const AnimatedHeader({super.key, required this.title, this.subtitle});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final titleColor = theme?.textPrimary ?? (isLight ? const Color(0xFF5C6073) : const Color(0xFFF8FAFC));
    final subtitleColor =
        theme?.textSecondary ?? (isLight ? const Color(0xFF8A8FA3) : const Color(0xFF94A3B8));

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title,
          style: TextStyle(
            fontSize: 28,
            fontWeight: FontWeight.bold,
            color: titleColor,
            fontFamily: isLight ? theme?.headingFont : null,
          ),
        ).animate().fadeIn(duration: 300.ms).slideY(begin: -0.05),
        if (subtitle != null) ...[
          const SizedBox(height: 4),
          Text(
            subtitle!,
            style: TextStyle(fontSize: 13, color: subtitleColor, fontFamily: isLight ? theme?.bodyFont : null),
          ).animate(delay: 100.ms).fadeIn(duration: 300.ms).slideY(begin: -0.03),
        ],
      ],
    );
  }
}

