import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../l10n/app_localizations.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import 'iridescence.dart';

/// Большая круглая кнопка с неоновым glow (тёмные темы) или
/// «фарфоровым» capsule-переливом (светлые темы, Seraphim — с iridescence).
class NeonPowerButton extends ConsumerWidget {
  final bool isActive;
  final bool isLoading;
  final FutureOr<void> Function()? onTap;

  const NeonPowerButton({
    super.key,
    required this.isActive,
    this.isLoading = false,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final l = AppLocalizations.of(context);
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final color = isActive ? AppTheme.success : settings.accentColor;
    final glowIntensity = settings.glowIntensity;
    final animationsEnabled = settings.animationsEnabled;

    if (isLight) {
      return _LightPowerButton(
        isActive: isActive,
        isLoading: isLoading,
        onTap: onTap,
        color: color,
        glowIntensity: glowIntensity,
        animationsEnabled: animationsEnabled,
        l: l,
        theme: theme,
      );
    }

    return _DarkPowerButton(
      isActive: isActive,
      isLoading: isLoading,
      onTap: onTap,
      color: color,
      glowIntensity: glowIntensity,
      animationsEnabled: animationsEnabled,
      l: l,
    );
  }
}

/// Тёмный вариант — оригинальный неоновый.
class _DarkPowerButton extends StatelessWidget {
  final bool isActive;
  final bool isLoading;
  final FutureOr<void> Function()? onTap;
  final Color color;
  final double glowIntensity;
  final bool animationsEnabled;
  final AppLocalizations l;

  const _DarkPowerButton({
    required this.isActive,
    required this.isLoading,
    required this.onTap,
    required this.color,
    required this.glowIntensity,
    required this.animationsEnabled,
    required this.l,
  });

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: isActive ? l.stop : l.start,
      child: GestureDetector(
        onTap: isLoading || onTap == null ? null : () => onTap!(),
        child: Stack(
          alignment: Alignment.center,
          children: [
            if (animationsEnabled)
              Container(
                width: 200,
                height: 200,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: color.withValues(alpha: 0.08),
                ),
              )
                  .animate(onPlay: (c) => c.repeat(reverse: true))
                  .scale(
                    begin: const Offset(1, 1),
                    end: const Offset(1.15, 1.15),
                    duration: 2000.ms,
                    curve: Curves.easeInOut,
                  )
                  .fade(begin: 0.5, end: 0.2, duration: 2000.ms),
            Container(
              width: 180,
              height: 180,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                gradient: LinearGradient(
                  begin: Alignment.topLeft,
                  end: Alignment.bottomRight,
                  colors: [
                    color.withValues(alpha: 0.6),
                    color.withValues(alpha: 0.2),
                  ],
                ),
                boxShadow: [
                  BoxShadow(
                    color: color.withValues(alpha: glowIntensity),
                    blurRadius: 30,
                    spreadRadius: 2,
                  ),
                ],
              ),
            ),
            Container(
              width: 160,
              height: 160,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: AppTheme.bgSurfaceAlt.withValues(alpha: 0.8),
                border: Border.all(
                  color: color.withValues(alpha: 0.5),
                  width: 2,
                ),
              ),
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  if (isLoading)
                    SizedBox(
                      width: 48,
                      height: 48,
                      child: CircularProgressIndicator(
                        strokeWidth: 3,
                        color: color,
                      ),
                    )
                  else ...[
                    Icon(
                      isActive ? Icons.stop_rounded : Icons.power_settings_new_rounded,
                      size: 48,
                      color: color,
                    )
                        .animate(target: isActive ? 1 : 0)
                        .rotate(begin: 0.0, end: 0.5, duration: 400.ms),
                    const SizedBox(height: 6),
                    Text(
                      isActive ? l.stop : l.start,
                      style: TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.bold,
                        color: color,
                        letterSpacing: 2,
                        shadows: [
                          Shadow(
                            color: color.withValues(alpha: 0.4),
                            blurRadius: 8,
                          ),
                        ],
                      ),
                    ),
                  ],
                ],
              ),
            )
                .animate(target: isActive ? 1 : 0)
                .shimmer(duration: 2000.ms, color: color.withValues(alpha: 0.3)),
          ],
        ),
      ),
    );
  }
}

/// Светлый вариант — «фарфоровый» capsule с мягким bloom.
/// Для Seraphim — iridescence-перелив на самой кнопке.
class _LightPowerButton extends StatefulWidget {
  final bool isActive;
  final bool isLoading;
  final FutureOr<void> Function()? onTap;
  final Color color;
  final double glowIntensity;
  final bool animationsEnabled;
  final AppLocalizations l;
  final ObsessionTheme? theme;

  const _LightPowerButton({
    required this.isActive,
    required this.isLoading,
    required this.onTap,
    required this.color,
    required this.glowIntensity,
    required this.animationsEnabled,
    required this.l,
    required this.theme,
  });

  @override
  State<_LightPowerButton> createState() => _LightPowerButtonState();
}

class _LightPowerButtonState extends State<_LightPowerButton> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final color = widget.color;
    final textColor = widget.isActive ? AppTheme.success : color;

    Widget button = Semantics(
      button: true,
      label: widget.isActive ? widget.l.stop : widget.l.start,
      child: GestureDetector(
        onTap: widget.isLoading || widget.onTap == null ? null : () => widget.onTap!(),
        child: MouseRegion(
          onEnter: (_) => setState(() => _hovered = true),
          onExit: (_) => setState(() => _hovered = false),
          child: Stack(
            alignment: Alignment.center,
            children: [
              // Внешнее мягкое halo-кольцо (bloom).
              if (widget.animationsEnabled)
                Container(
                  width: 210,
                  height: 210,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: color.withValues(alpha: 0.10),
                  ),
                )
                    .animate(onPlay: (c) => c.repeat(reverse: true))
                    .scale(
                      begin: const Offset(1, 1),
                      end: const Offset(1.12, 1.12),
                      duration: 2400.ms,
                      curve: Curves.easeInOut,
                    )
                    .fade(begin: 0.4, end: 0.15, duration: 2400.ms),
              // Кольцо-ореол (aureola) — пастельный.
              Container(
                width: 184,
                height: 184,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  gradient: RadialGradient(
                    colors: [
                      color.withValues(alpha: 0.0),
                      color.withValues(alpha: 0.18),
                      color.withValues(alpha: 0.0),
                    ],
                    stops: const [0.55, 0.8, 1.0],
                  ),
                ),
              ),
              // Фарфоровая капсула.
              _porcelainCapsule(textColor),
            ],
          ),
        ),
      ),
    );

    return button;
  }

  Widget _porcelainCapsule(Color textColor) {
    final color = widget.color;
    final useIridescence =
        widget.theme != null && shouldUseIridescence(widget.theme!.preset);
    final capsule = Container(
      width: 160,
      height: 160,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        color: Colors.white.withValues(alpha: 0.85),
        border: Border.all(
          color: color.withValues(alpha: _hovered ? 0.7 : 0.45),
          width: 2,
        ),
        boxShadow: [
          BoxShadow(
            color: color.withValues(alpha: widget.glowIntensity * (_hovered ? 0.5 : 0.3)),
            blurRadius: _hovered ? 34 : 26,
            spreadRadius: 2,
          ),
          BoxShadow(
            color: Colors.black.withValues(alpha: 0.06),
            blurRadius: 12,
            offset: const Offset(0, 4),
          ),
        ],
      ),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          if (widget.isLoading)
            SizedBox(
              width: 48,
              height: 48,
              child: CircularProgressIndicator(
                strokeWidth: 3,
                color: color,
              ),
            )
          else ...[
            Icon(
              widget.isActive ? Icons.stop_rounded : Icons.power_settings_new_rounded,
              size: 48,
              color: textColor,
            )
                .animate(target: widget.isActive ? 1 : 0)
                .rotate(begin: 0.0, end: 0.5, duration: 400.ms),
            const SizedBox(height: 6),
            Text(
              widget.isActive ? widget.l.stop : widget.l.start,
              style: TextStyle(
                fontSize: 14,
                fontWeight: FontWeight.bold,
                color: textColor,
                letterSpacing: 2,
                fontFamily: widget.theme?.headingFont,
              ),
            ),
          ],
        ],
      ),
    );

    // Seraphim: iridescence-перелив поверх фарфоровой капсулы.
    if (useIridescence && widget.theme != null) {
      return Iridescence(
        intensity: widget.theme!.iridescenceIntensity * 0.6,
        tints: widget.theme!.iridescenceTints,
        speed: 0.8,
        highlight: _hovered ? 1.0 : 0.3,
        borderRadius: 80,
        child: capsule,
      )
          .animate(target: widget.isActive ? 1 : 0)
          .shimmer(duration: 2400.ms, color: color.withValues(alpha: 0.25));
    }

    return capsule
        .animate(target: widget.isActive ? 1 : 0)
        .shimmer(duration: 2400.ms, color: color.withValues(alpha: 0.2));
  }
}
