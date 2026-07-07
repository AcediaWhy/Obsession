import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import '../../l10n/app_localizations.dart';
import '../providers/settings_provider.dart';
import '../providers/tray_provider.dart';
import '../theme/app_theme.dart';

/// Кастомные оконные контролы.
class WindowControls extends ConsumerWidget {
  const WindowControls({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final iconColor = isLight ? theme!.textSecondary : AppTheme.textSecondary;

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _WindowButton(
          icon: Icons.minimize_rounded,
          tooltip: AppLocalizations.of(context).minimize,
          accent: settings.accentColor,
          glow: settings.glowIntensity,
          isLight: isLight,
          iconColor: iconColor,
          onPressed: () => windowManager.minimize(),
        ),
        _MaximizeButton(
          accent: settings.accentColor,
          glow: settings.glowIntensity,
          isLight: isLight,
          iconColor: iconColor,
        ),
        _WindowButton(
          icon: Icons.close_rounded,
          tooltip: AppLocalizations.of(context).close,
          accent: AppTheme.error,
          glow: settings.glowIntensity,
          isLight: isLight,
          iconColor: iconColor,
          isClose: true,
          onPressed: () async {
            final trayState = ref.read(trayProvider);
            if (settings.minimizeToTray && trayState.isInitialized) {
              // Трей инициализирован — скрываем окно, иконка останется в трее.
              await windowManager.hide();
            } else {
              // close() триггерит onWindowClose у WindowListener'ов
              // (HomeScreen), который остановит DPI-процессы.
              await windowManager.close();
            }
          },
        ),
      ],
    );
  }
}

class _WindowButton extends StatefulWidget {
  final IconData icon;
  final String tooltip;
  final VoidCallback onPressed;
  final Color accent;
  final double glow;
  final bool isClose;
  final bool isLight;
  final Color iconColor;

  const _WindowButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    required this.accent,
    required this.glow,
    this.isClose = false,
    required this.isLight,
    required this.iconColor,
  });

  @override
  State<_WindowButton> createState() => _WindowButtonState();
}

class _WindowButtonState extends State<_WindowButton> {
  bool _isHovered = false;

  @override
  Widget build(BuildContext context) {
    final hoverColor = widget.isClose
        ? AppTheme.error
        : widget.accent;
    return Tooltip(
      message: widget.tooltip,
      waitDuration: const Duration(milliseconds: 500),
      child: MouseRegion(
        onEnter: (_) => setState(() => _isHovered = true),
        onExit: (_) => setState(() => _isHovered = false),
        child: GestureDetector(
          onTap: widget.onPressed,
          child: AnimatedContainer(
            duration: 150.ms,
            width: 40,
            height: 32,
            decoration: BoxDecoration(
              color: _isHovered
                  ? (widget.isLight
                      ? hoverColor.withValues(alpha: 0.14)
                      : hoverColor.withValues(alpha: 0.15))
                  : Colors.transparent,
              borderRadius: BorderRadius.circular(6),
              boxShadow: _isHovered && !widget.isLight
                  ? [
                      BoxShadow(
                        color: hoverColor.withValues(alpha: widget.glow * 0.4),
                        blurRadius: 8,
                      ),
                    ]
                  : null,
            ),
            child: Icon(
              widget.icon,
              size: 16,
              color: _isHovered ? hoverColor : widget.iconColor,
            ),
          ),
        ),
      ),
    );
  }
}

class _MaximizeButton extends StatefulWidget {
  final Color accent;
  final double glow;
  final bool isLight;
  final Color iconColor;

  const _MaximizeButton({
    required this.accent,
    required this.glow,
    required this.isLight,
    required this.iconColor,
  });

  @override
  State<_MaximizeButton> createState() => _MaximizeButtonState();
}

class _MaximizeButtonState extends State<_MaximizeButton> {
  bool _isMaximized = false;

  @override
  void initState() {
    super.initState();
    _updateState();
  }

  Future<void> _updateState() async {
    final value = await windowManager.isMaximized();
    if (mounted && value != _isMaximized) {
      setState(() => _isMaximized = value);
    }
  }

  @override
  Widget build(BuildContext context) {
    return _WindowButton(
      icon: Icons.crop_square_rounded,
      tooltip: _isMaximized
          ? AppLocalizations.of(context).restore
          : AppLocalizations.of(context).maximize,
      accent: widget.accent,
      glow: widget.glow,
      isLight: widget.isLight,
      iconColor: widget.iconColor,
      onPressed: () async {
        if (await windowManager.isMaximized()) {
          await windowManager.unmaximize();
        } else {
          await windowManager.maximize();
        }
        await _updateState();
      },
    );
  }
}
