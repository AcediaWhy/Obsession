import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/entities/hosts_config.dart';
import '../../l10n/app_localizations.dart';
import '../providers/hosts_provider.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/glass_panel.dart';
import '../widgets/glow_widgets.dart';
import '../widgets/obsession_dialog.dart';

class AiScreen extends ConsumerStatefulWidget {
  const AiScreen({super.key});

  @override
  ConsumerState<AiScreen> createState() => _AiScreenState();
}

class _AiScreenState extends ConsumerState<AiScreen> {
  ProviderSubscription<HostsState>? _hostsSub;

  @override
  void initState() {
    super.initState();
    // BUG-12: показываем ошибки установки/удаления пользователю.
    // Подписку сохраняем и закрываем в dispose — иначе она живёт до конца
    // ProviderScope и ссылается на этот State после удаления виджета.
    _hostsSub = ref.listenManual(hostsProvider, (previous, next) {
      if (next.error != null && next.error != previous?.error) {
        _showError(next.error!);
        ref.read(hostsProvider.notifier).clearError();
      }
    });
  }

  @override
  void dispose() {
    _hostsSub?.close();
    super.dispose();
  }

  void _showError(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        backgroundColor: AppTheme.error,
        behavior: SnackBarBehavior.floating,
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final aiState = ref.watch(hostsProvider);
    final aiNotifier = ref.read(hostsProvider.notifier);
    final settings = ref.watch(settingsProvider);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        AnimatedHeader(
          title: AppLocalizations.of(context).aiTitle,
          subtitle: AppLocalizations.of(context).aiSubtitle,
        ),
        const SizedBox(height: 24),
        GlassPanel(
          padding: const EdgeInsets.all(20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                AppLocalizations.of(context).provider.toUpperCase(),
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: AppTheme.textMutedOf(context),
                  letterSpacing: 1.2,
                ),
              ),
              const SizedBox(height: 12),
              Row(
                children: AiProvider.values.map((p) {
                  final isSelected = aiState.provider == p;
                  return Expanded(
                    child: Padding(
                      padding: const EdgeInsets.only(right: 8),
                      child: _ProviderCard(
                        provider: p,
                        isSelected: isSelected,
                        onTap: aiState.isBusy ? null : () => aiNotifier.setProvider(p),
                        accent: settings.accentColor,
                        glow: settings.glowIntensity,
                      ),
                    ),
                  );
                }).toList(),
              ),
            ],
          ),
        ).animate().fadeIn(duration: 400.ms).slideY(begin: 0.1),
        const SizedBox(height: 16),
        GlassPanel(
          padding: const EdgeInsets.all(20),
          child: Row(
            children: [
              _StatusIcon(status: aiState.status, isBusy: aiState.isBusy),
              const SizedBox(width: 16),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      _statusLabel(context, aiState.status),
                      style: TextStyle(
                        fontSize: 16,
                        fontWeight: FontWeight.w600,
                        color: AppTheme.textPrimaryOf(context),
                      ),
                    ),
                    if (aiState.localVersion.isNotEmpty)
                      Text(
                        AppLocalizations.of(context).versionWithValue(aiState.localVersion),
                        style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context)),
                      ),
                  ],
                ),
              ),
              if (aiState.isBusy)
                SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(strokeWidth: 2, color: AppTheme.textSecondaryOf(context)),
                ),
            ],
          ),
        ),
        const Spacer(),
        Row(
          children: [
              Expanded(
                child:                 GlowButton(
                  label: aiState.status == HostsStatus.notInstalled
                      ? AppLocalizations.of(context).install
                      : AppLocalizations.of(context).update,
                  icon: Icons.download_rounded,
                  color: settings.accentColor,
                  fullWidth: true,
                  onTap: aiState.isBusy
                      ? null
                      : () async {
                          await aiNotifier.install();
                        },
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                child:                 GlowButton(
                  label: AppLocalizations.of(context).uninstall,
                  icon: Icons.delete_outline_rounded,
                  color: AppTheme.error,
                  isOutline: true,
                  fullWidth: true,
                  onTap: aiState.isBusy || aiState.status == HostsStatus.notInstalled
                      ? null
                      : () async {
                          await _confirmUninstall(context, aiNotifier);
                        },
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                child:                 GlowButton(
                  label: AppLocalizations.of(context).check,
                  icon: Icons.refresh_rounded,
                  color: AppTheme.textSecondaryOf(context),
                  isOutline: true,
                  fullWidth: true,
                  onTap: aiState.isBusy ? null : () => aiNotifier.refreshStatus(),
                ),
              ),
          ],
        ).animate().fadeIn(duration: 400.ms, delay: 200.ms).slideY(begin: 0.2),
      ],
    );
  }

  String _statusLabel(BuildContext context, HostsStatus status) => switch (status) {
        HostsStatus.installed => AppLocalizations.of(context).installedUpToDate,
        HostsStatus.outdated => AppLocalizations.of(context).installedOutdated,
        HostsStatus.notInstalled => AppLocalizations.of(context).aiNotInstalled,
        HostsStatus.offline => AppLocalizations.of(context).installedUpToDate,
      };

  Future<void> _confirmUninstall(BuildContext context, HostsNotifier notifier) async {
    final ok = await showObsessionDialog<bool>(
      context: context,
      title: AppLocalizations.of(context).uninstallAiTitle,
      message: AppLocalizations.of(context).uninstallAiMessage,
      icon: Icons.delete_outline,
      iconColor: AppTheme.error,
      actions: [
        ObsessionDialogAction<bool>(label: AppLocalizations.of(context).cancel, value: false),
        ObsessionDialogAction<bool>(
          label: AppLocalizations.of(context).uninstall,
          icon: Icons.delete,
          value: true,
          color: AppTheme.error,
          isPrimary: true,
        ),
      ],
    );
    if (ok == true) await notifier.uninstall();
  }
}

class _ProviderCard extends StatelessWidget {
  final AiProvider provider;
  final bool isSelected;
  final VoidCallback? onTap;
  final Color accent;
  final double glow;

  const _ProviderCard({
    required this.provider,
    required this.isSelected,
    required this.onTap,
    required this.accent,
    required this.glow,
  });

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: AnimatedContainer(
          duration: 200.ms,
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            color: isSelected ? accent.withValues(alpha: 0.12) : Colors.transparent,
            borderRadius: BorderRadius.circular(12),
            border: Border.all(
              color: isSelected ? accent.withValues(alpha: 0.4) : Colors.white.withValues(alpha: 0.08),
            ),
            boxShadow: isSelected
                ? [BoxShadow(color: accent.withValues(alpha: glow * 0.3), blurRadius: 12)]
                : null,
          ),
          child: Column(
            children: [
              Icon(
                provider == AiProvider.geohide ? Icons.public : Icons.dns_rounded,
                size: 24,
                color: isSelected ? accent : AppTheme.textSecondaryOf(context),
              ),
              const SizedBox(height: 8),
              Text(
                provider.label,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: isSelected ? FontWeight.w600 : FontWeight.w500,
                  color: isSelected ? accent : AppTheme.textSecondaryOf(context),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _StatusIcon extends StatelessWidget {
  final HostsStatus status;
  final bool isBusy;
  const _StatusIcon({required this.status, required this.isBusy});

  @override
  Widget build(BuildContext context) {
    if (isBusy) return const SizedBox(width: 24, height: 24, child: CircularProgressIndicator(strokeWidth: 2));
    final (color, icon) = switch (status) {
      HostsStatus.installed => (AppTheme.success, Icons.check_circle_rounded),
      HostsStatus.outdated => (AppTheme.warning, Icons.update_rounded),
      HostsStatus.notInstalled => (AppTheme.error, Icons.cancel_rounded),
      HostsStatus.offline => (AppTheme.textMutedOf(context), Icons.wifi_off_rounded),
    };
    return Icon(icon, color: color, size: 28);
  }
}


