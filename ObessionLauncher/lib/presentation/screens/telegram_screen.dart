import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../l10n/app_localizations.dart';
import '../providers/proxy_provider.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/glass_panel.dart';
import '../widgets/glow_widgets.dart';
import '../widgets/neon_power_button.dart';

class TelegramScreen extends ConsumerWidget {
  const TelegramScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tgState = ref.watch(proxyProvider);
    final tgNotifier = ref.read(proxyProvider.notifier);
    final settings = ref.watch(settingsProvider);

    // Показываем SnackBar при ошибке запуска прокси.
    ref.listen<ProxyState>(proxyProvider, (prev, next) {
      if (next.error.isNotEmpty && (prev == null || prev.error != next.error)) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(next.error),
            backgroundColor: AppTheme.error,
            behavior: SnackBarBehavior.floating,
          ),
        );
        tgNotifier.clearError();
      }
    });

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            AnimatedHeader(
              title: AppLocalizations.of(context).telegramTitle,
              subtitle: AppLocalizations.of(context).telegramSubtitle,
            ),
            _StatusPill(isActive: tgState.isActive),
          ],
        ),
        const SizedBox(height: 24),
        if (!tgState.isAvailable) ...[
          GlassPanel(
            padding: const EdgeInsets.all(20),
            child: Row(
              children: [
                Icon(Icons.warning_amber_rounded, color: AppTheme.warning, size: 28),
                SizedBox(width: 16),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                    Text(
                      AppLocalizations.of(context).tgProxyNotFoundTitle,
                      style: TextStyle(
                          fontSize: 14,
                          fontWeight: FontWeight.w600,
                          color: AppTheme.warning,
                        ),
                      ),
                      SizedBox(height: 4),
                      Text(
                        AppLocalizations.of(context).tgProxyNotFoundMessage,
                        style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context)),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ] else ...[
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  flex: 2,
                  child: GlassPanel(
                    padding: const EdgeInsets.all(20),
                    child: SingleChildScrollView(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            AppLocalizations.of(context).proxySettings,
                            style: TextStyle(
                              fontSize: 11,
                              fontWeight: FontWeight.w600,
                              color: AppTheme.textMutedOf(context),
                              letterSpacing: 1.2,
                            ),
                          ),
                          const SizedBox(height: 16),
                          _SettingField(
                            label: AppLocalizations.of(context).port,
                            value: tgState.config.port.toString(),
                            icon: Icons.router_rounded,
                            accent: settings.accentColor,
                          ),
                          const SizedBox(height: 12),
                          _SettingField(
                            label: AppLocalizations.of(context).fakeTlsDomainOptional,
                            value: tgState.config.fakeTlsDomain.isEmpty
                                ? AppLocalizations.of(context).notSet
                                : tgState.config.fakeTlsDomain,
                            icon: Icons.lock_outline,
                            accent: settings.accentColor,
                            isDim: tgState.config.fakeTlsDomain.isEmpty,
                          ),
                          const SizedBox(height: 24),
                          if (tgState.proxyLink.isNotEmpty) ...[
                            Text(
                              AppLocalizations.of(context).connectionLink,
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: FontWeight.w600,
                                color: AppTheme.textMutedOf(context),
                                letterSpacing: 1.2,
                              ),
                            ),
                            const SizedBox(height: 12),
                            Container(
                              padding: const EdgeInsets.all(12),
                              decoration: BoxDecoration(
                                color: Colors.white.withValues(alpha: 0.03),
                                borderRadius: BorderRadius.circular(8),
                                border: Border.all(color: Colors.white.withValues(alpha: 0.08)),
                              ),
                              child: SelectableText(
                                tgState.proxyLink,
                                style: TextStyle(
                                  fontSize: 11,
                                  color: settings.accentColor,
                                  fontFamily: 'monospace',
                                ),
                              ),
                            ),
                            const SizedBox(height: 12),
                              Row(
                                children: [
                                  Expanded(
                                    child:                                     GlowButton(
                                      label: AppLocalizations.of(context).openInTelegram,
                                      icon: Icons.open_in_new_rounded,
                                      color: settings.accentColor,
                                      fullWidth: true,
                                      onTap: tgState.isActive
                                          ? () => tgNotifier.openInTelegram()
                                          : null,
                                    ),
                                  ),
                                  const SizedBox(width: 8),
                                  Expanded(
                                    child:                                     GlowButton(
                                      label: AppLocalizations.of(context).copy,
                                      icon: Icons.copy_rounded,
                                      color: AppTheme.textSecondaryOf(context),
                                      isOutline: true,
                                      fullWidth: true,
                                      onTap: tgState.isActive
                                          ? () async {
                                              await tgNotifier.copyLink();
                                              if (!context.mounted) return;
                                              // Успех копирования не меняет state.error,
                                              // поэтому подтверждаем явно. Ошибки придут через listener.
                                              if (ref.read(proxyProvider).error.isEmpty) {
                                                ScaffoldMessenger.of(context).showSnackBar(
                                                  SnackBar(
                                                    content: Text(AppLocalizations.of(context).copyLink),
                                                    behavior: SnackBarBehavior.floating,
                                                  ),
                                                );
                                              }
                                            }
                                          : null,
                                    ),
                                  ),
                                ],
                              ),
                          ],
                          if (!tgState.isActive) ...[
                            const SizedBox(height: 24),
                            Text(
                              AppLocalizations.of(context).instructions,
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: FontWeight.w600,
                                color: AppTheme.textMutedOf(context),
                                letterSpacing: 1.2,
                              ),
                            ),
                            const SizedBox(height: 8),
                            Text(
                              AppLocalizations.of(context).telegramInstructions,
                              style: TextStyle(
                                fontSize: 12,
                                color: AppTheme.textSecondaryOf(context),
                                height: 1.6,
                              ),
                            ),
                          ],
                        ],
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 16),
                Expanded(
                  flex: 3,
                  child: Center(
                    child: NeonPowerButton(
                      isActive: tgState.isActive,
                      isLoading: tgState.isTransitioning,
                      onTap: () async {
                        if (tgState.isTransitioning) return;
                        if (tgState.isActive) {
                          await tgNotifier.stop();
                        } else {
                          await tgNotifier.start();
                        }
                      },
                    ),
                  ),
                ),
              ],
            ),
          ),
        ],
      ],
    );
  }
}

class _StatusPill extends ConsumerWidget {
  final bool isActive;
  const _StatusPill({required this.isActive});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final color = isActive ? AppTheme.success : AppTheme.error;

    return GlassPanel(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
      borderRadius: 30,
      glowColor: isActive ? color : null,
      glowRadius: 12,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          AnimatedStatusIndicator(
            isActive: isActive,
            size: 9,
            color: color,
          ),
          const SizedBox(width: 10),
          Text(
            isActive ? AppLocalizations.of(context).activeShort : AppLocalizations.of(context).inactiveShort,
            style: TextStyle(fontSize: 13, fontWeight: FontWeight.w500, color: AppTheme.textPrimaryOf(context)),
          ),
        ],
      ),
    );
  }
}

class _SettingField extends StatelessWidget {
  final String label;
  final String value;
  final IconData icon;
  final Color accent;
  final bool isDim;

  const _SettingField({
    required this.label,
    required this.value,
    required this.icon,
    required this.accent,
    this.isDim = false,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(icon, size: 16, color: accent),
        const SizedBox(width: 12),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(label, style: TextStyle(fontSize: 10, color: AppTheme.textMutedOf(context))),
              Text(
                value,
                style: TextStyle(
                  fontSize: 13,
                  color: isDim ? AppTheme.textMutedOf(context) : AppTheme.textPrimaryOf(context),
                  fontWeight: FontWeight.w500,
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}


