import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../l10n/app_localizations.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import 'glow_widgets.dart';

/// Унифицированный диалог в стиле приложения.
class ObsessionDialog extends ConsumerWidget {
  final String title;
  final String? message;
  final Widget? content;
  final IconData? icon;
  final Color? iconColor;
  final List<Widget> actions;

  const ObsessionDialog({
    super.key,
    required this.title,
    this.message,
    this.content,
    this.icon,
    this.iconColor,
    required this.actions,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final accent = settings.accentColor;
    final glow = settings.glowIntensity;

    return BackdropFilter(
      filter: ImageFilter.blur(sigmaX: 8, sigmaY: 8),
      child: Dialog(
        backgroundColor: Colors.transparent,
        insetPadding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
        child: Container(
          decoration: BoxDecoration(
            color: AppTheme.surfaceOf(context).withValues(alpha: 0.92),
            borderRadius: BorderRadius.circular(20),
            border: Border.all(color: accent.withValues(alpha: 0.3), width: 1.5),
            boxShadow: [
              BoxShadow(
                color: accent.withValues(alpha: glow * 0.3),
                blurRadius: 24,
                spreadRadius: 2,
              ),
            ],
          ),
          child: ClipRRect(
            borderRadius: BorderRadius.circular(20),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Container(
                  padding: const EdgeInsets.fromLTRB(24, 24, 24, 16),
                  decoration: BoxDecoration(
                    gradient: LinearGradient(
                      begin: Alignment.topLeft,
                      end: Alignment.bottomRight,
                      colors: [
                        accent.withValues(alpha: 0.1),
                        Colors.transparent,
                      ],
                    ),
                  ),
                  child: Row(
                    children: [
                      if (icon != null) ...[
                        Container(
                          width: 40,
                          height: 40,
                          decoration: BoxDecoration(
                            color: (iconColor ?? accent).withValues(alpha: 0.15),
                            borderRadius: BorderRadius.circular(12),
                            boxShadow: [
                              BoxShadow(
                                color: (iconColor ?? accent).withValues(alpha: glow * 0.3),
                                blurRadius: 12,
                              ),
                            ],
                          ),
                          child: Icon(icon, color: iconColor ?? accent, size: 22),
                        ).animate().scale(duration: 300.ms, curve: Curves.easeOutBack),
                        const SizedBox(width: 16),
                      ],
                      Expanded(
                        child: Text(
                          title,
                          style: TextStyle(
                            fontSize: 18,
                            fontWeight: FontWeight.bold,
                            color: AppTheme.textPrimaryOf(context),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
                if (message != null || content != null)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(24, 0, 24, 24),
                    child: content ??
                        Text(
                          message!,
                          style: TextStyle(
                            fontSize: 13,
                            color: AppTheme.textSecondaryOf(context),
                            height: 1.5,
                          ),
                        ),
                  ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(24, 0, 24, 24),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: actions,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Удобная функция для показа ObsessionDialog.
Future<T?> showObsessionDialog<T>({
  required BuildContext context,
  required String title,
  String? message,
  Widget? content,
  IconData? icon,
  Color? iconColor,
  required List<ObsessionDialogAction> actions,
}) {
  return showDialog<T>(
    context: context,
    barrierColor: Colors.black.withValues(alpha: 0.5),
    builder: (ctx) => ObsessionDialog(
      title: title,
      message: message,
      content: content,
      icon: icon,
      iconColor: iconColor,
      actions: actions
          .map((a) => a.isPrimary
              ? GlowButton(
                  label: a.label,
                  icon: a.icon,
                  color: a.color ?? AppTheme.success,
                  onTap: () => Navigator.pop<T>(ctx, a.value),
                )
              : GlowButton(
                  label: a.label,
                  icon: a.icon,
                  color: a.color ?? AppTheme.textSecondaryOf(context),
                  isOutline: true,
                  onTap: () => Navigator.pop<T>(ctx, a.value),
                ))
          .toList(),
    ),
  );
}

class ObsessionDialogAction<T> {
  final String label;
  final IconData? icon;
  final T? value;
  final Color? color;
  final bool isPrimary;

  const ObsessionDialogAction({
    required this.label,
    this.icon,
    this.value,
    this.color,
    this.isPrimary = false,
  });
}

/// Диалог с текстовым вводом в стиле приложения.
Future<String?> showObsessionInputDialog({
  required BuildContext context,
  required String title,
  String? message,
  IconData? icon,
  Color? iconColor,
  required String hintText,
  String? initialValue,
  required String confirmLabel,
  String? cancelLabel,
}) {
  return showDialog<String>(
    context: context,
    barrierColor: Colors.black.withValues(alpha: 0.5),
    builder: (ctx) => _ObsessionInputDialog(
      title: title,
      message: message,
      icon: icon,
      iconColor: iconColor,
      hintText: hintText,
      initialValue: initialValue,
      confirmLabel: confirmLabel,
      cancelLabel: cancelLabel,
    ),
  );
}

class _ObsessionInputDialog extends ConsumerStatefulWidget {
  final String title;
  final String? message;
  final IconData? icon;
  final Color? iconColor;
  final String hintText;
  final String? initialValue;
  final String confirmLabel;
  final String? cancelLabel;

  const _ObsessionInputDialog({
    required this.title,
    this.message,
    this.icon,
    this.iconColor,
    required this.hintText,
    this.initialValue,
    required this.confirmLabel,
    this.cancelLabel,
  });

  @override
  ConsumerState<_ObsessionInputDialog> createState() => _ObsessionInputDialogState();
}

class _ObsessionInputDialogState extends ConsumerState<_ObsessionInputDialog> {
  late final TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.initialValue);
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return ObsessionDialog(
      title: widget.title,
      message: widget.message,
      icon: widget.icon,
      iconColor: widget.iconColor,
      content: TextField(
        controller: _controller,
        autofocus: true,
        style: TextStyle(color: AppTheme.textPrimaryOf(context)),
        decoration: InputDecoration(
          hintText: widget.hintText,
          hintStyle: TextStyle(color: AppTheme.textMutedOf(context)),
          border: InputBorder.none,
        ),
      ),
      actions: [
        GlowButton(
          label: widget.cancelLabel ?? l.cancel,
          color: AppTheme.textSecondaryOf(context),
          isOutline: true,
          onTap: () => Navigator.pop<String>(context, null),
        ),
        GlowButton(
          label: widget.confirmLabel,
          icon: Icons.check,
          onTap: () => Navigator.pop<String>(context, _controller.text),
        ),
      ],
    );
  }
}
