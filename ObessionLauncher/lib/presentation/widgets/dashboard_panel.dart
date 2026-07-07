import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/datasources/ping_local_datasource.dart';
import '../../domain/entities/hosts_config.dart';
import '../../domain/entities/log_entry.dart';
import '../../l10n/app_localizations.dart';
import '../providers/dpi_provider.dart';
import '../providers/hosts_provider.dart';
import '../providers/log_provider.dart';
import '../providers/ping_provider.dart';
import '../providers/proxy_provider.dart';
import '../providers/settings_provider.dart';
import '../providers/update_provider.dart';
import '../theme/app_theme.dart';
import 'glass_panel.dart';

/// Правая информационная панель в стиле Obsession.
/// Показывает статус сервисов, статистику и live log.
/// Адаптируется под яркость темы через [ObsessionTheme].
class DashboardPanel extends ConsumerWidget {
  const DashboardPanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final accent = settings.accentColor;
    final dpiState = ref.watch(dpiProvider);
    final proxyState = ref.watch(proxyProvider);
    final hostsState = ref.watch(hostsProvider);
    final logs = ref.watch(logHistoryProvider);
    final updateState = ref.watch(updateProvider);
    final pingState = ref.watch(pingMonitorProvider);
    final hostsInstalled = hostsState.status == HostsStatus.installed ||
        hostsState.status == HostsStatus.outdated;

    return GlassPanel(
      padding: const EdgeInsets.all(20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _Header(accent: accent, theme: theme),
          const SizedBox(height: 24),
          _SectionTitle(title: AppLocalizations.of(context).status, accent: accent, theme: theme),
          const SizedBox(height: 12),
          _StatusCard(
            label: AppLocalizations.of(context).dpiBypass,
            isActive: dpiState.isActive,
            detail: dpiState.isActive
                ? AppLocalizations.of(context).activeProcessesCount(dpiState.activeProcesses.length)
                : AppLocalizations.of(context).inactiveShort,
            accent: accent,
            theme: theme,
          ),
          const SizedBox(height: 10),
          _StatusCard(
            label: AppLocalizations.of(context).telegramProxy,
            isActive: proxyState.isActive,
            detail: proxyState.isActive
                ? AppLocalizations.of(context).portWithValue(proxyState.config.port)
                : AppLocalizations.of(context).inactiveShort,
            accent: accent,
            theme: theme,
          ),
          const SizedBox(height: 10),
          _StatusCard(
            label: AppLocalizations.of(context).hosts,
            isActive: hostsInstalled,
            detail: hostsInstalled
                ? AppLocalizations.of(context).installed
                : AppLocalizations.of(context).notInstalled,
            accent: accent,
            theme: theme,
          ),
          const SizedBox(height: 24),
          _SectionTitle(title: AppLocalizations.of(context).statistics, accent: accent, theme: theme),
          const SizedBox(height: 12),
          _StatRow(label: AppLocalizations.of(context).activeProcesses, value: '${dpiState.activeProcesses.length}', theme: theme),
          _StatRow(label: AppLocalizations.of(context).selectedConfigs, value: '${dpiState.selectedCategories.length}', theme: theme),
          _StatRow(label: AppLocalizations.of(context).version, value: updateState.info?.version ?? '1.0.0', theme: theme),
          const SizedBox(height: 24),
          _SectionTitle(title: AppLocalizations.of(context).availability, accent: accent, theme: theme),
          const SizedBox(height: 12),
          _PingRow(
            label: 'Discord',
            result: pingState.results['discord.com'],
            isChecking: pingState.isMonitoring && pingState.results['discord.com'] == null,
            accent: accent,
            theme: theme,
          ),
          const SizedBox(height: 10),
          _PingRow(
            label: 'YouTube',
            result: pingState.results['youtube.com'],
            isChecking: pingState.isMonitoring && pingState.results['youtube.com'] == null,
            accent: accent,
            theme: theme,
          ),
          const SizedBox(height: 24),
          _SectionTitle(title: AppLocalizations.of(context).liveLog, accent: accent, theme: theme),
          const SizedBox(height: 12),
          Expanded(
            child: _LogView(logs: logs, accent: accent, theme: theme),
          ),
        ],
      ),
    ).animate().fadeIn(duration: 500.ms).slideX(begin: 0.1);
  }
}

class _Header extends StatelessWidget {
  final Color accent;
  final ObsessionTheme? theme;
  const _Header({required this.accent, this.theme});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(
            color: accent,
            shape: BoxShape.circle,
            boxShadow: [
              BoxShadow(
                color: accent.withValues(alpha: 0.6),
                blurRadius: 10,
                spreadRadius: 1,
              ),
            ],
          ),
        ),
        const SizedBox(width: 10),
        Text(
          AppLocalizations.of(context).singularityStatus,
          style: TextStyle(
            fontSize: 11,
            fontWeight: FontWeight.w700,
            color: theme?.textMuted ?? AppTheme.textMuted,
            letterSpacing: 2,
          ),
        ),
      ],
    );
  }
}

class _SectionTitle extends StatelessWidget {
  final String title;
  final Color accent;
  final ObsessionTheme? theme;
  const _SectionTitle({required this.title, required this.accent, this.theme});

  @override
  Widget build(BuildContext context) {
    return Text(
      title.toUpperCase(),
      style: TextStyle(
        fontSize: 10,
        fontWeight: FontWeight.w700,
        color: accent.withValues(alpha: 0.7),
        letterSpacing: 1.5,
      ),
    );
  }
}

class _StatusCard extends StatelessWidget {
  final String label;
  final bool isActive;
  final String detail;
  final Color accent;
  final ObsessionTheme? theme;

  const _StatusCard({
    required this.label,
    required this.isActive,
    required this.detail,
    required this.accent,
    this.theme,
  });

  @override
  Widget build(BuildContext context) {
    final statusColor = isActive ? AppTheme.success : AppTheme.error;
    final inset = theme?.insetSurfaceColor ?? AppTheme.bgSurface.withValues(alpha: 0.4);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: inset,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(
          color: statusColor.withValues(alpha: 0.25),
          width: 1,
        ),
      ),
      child: Row(
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(
              color: statusColor,
              shape: BoxShape.circle,
              boxShadow: isActive
                  ? [
                      BoxShadow(
                        color: statusColor.withValues(alpha: 0.6),
                        blurRadius: 8,
                        spreadRadius: 1,
                      ),
                    ]
                  : null,
            ),
          )
              .animate(onPlay: (c) => c.repeat(reverse: true))
              .scale(begin: const Offset(0.85, 0.85), end: const Offset(1.15, 1.15), duration: 1200.ms),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: theme?.textPrimary ?? AppTheme.textPrimary,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  detail,
                  style: TextStyle(
                    fontSize: 10,
                    color: theme?.textMuted ?? AppTheme.textMuted,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _StatRow extends StatelessWidget {
  final String label;
  final String value;
  final ObsessionTheme? theme;

  const _StatRow({required this.label, required this.value, this.theme});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Text(
            label,
            style: TextStyle(
              fontSize: 11,
              color: theme?.textSecondary ?? AppTheme.textSecondary,
            ),
          ),
          Text(
            value,
            style: TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              color: theme?.textPrimary ?? AppTheme.textPrimary,
            ),
          ),
        ],
      ),
    );
  }
}

/// Строка доступности сервиса: пульсирующий индикатор + latency.
class _PingRow extends StatelessWidget {
  final String label;
  final PingResult? result;
  final bool isChecking;
  final Color accent;
  final ObsessionTheme? theme;

  const _PingRow({
    required this.label,
    required this.result,
    required this.isChecking,
    required this.accent,
    this.theme,
  });

  @override
  Widget build(BuildContext context) {
    final isReachable = result?.isReachable ?? false;
    final statusColor = isReachable
        ? AppTheme.success
        : isChecking
            ? (theme?.textMuted ?? AppTheme.textMuted)
            : AppTheme.error;

    final String detail;
    final ping = result;
    if (ping == null) {
      detail = AppLocalizations.of(context).checking;
    } else if (ping.isReachable) {
      detail = AppLocalizations.of(context).latencyMs(ping.latencyMs);
    } else {
      detail = AppLocalizations.of(context).unreachable;
    }

    final inset = theme?.insetSurfaceColor ?? AppTheme.bgSurface.withValues(alpha: 0.4);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: inset,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(
          color: statusColor.withValues(alpha: 0.25),
          width: 1,
        ),
      ),
      child: Row(
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(
              color: statusColor,
              shape: BoxShape.circle,
              boxShadow: isReachable
                  ? [
                      BoxShadow(
                        color: statusColor.withValues(alpha: 0.6),
                        blurRadius: 8,
                        spreadRadius: 1,
                      ),
                    ]
                  : null,
            ),
          )
              .animate(onPlay: (c) => c.repeat(reverse: true))
              .scale(begin: const Offset(0.85, 0.85), end: const Offset(1.15, 1.15), duration: 1200.ms),
          const SizedBox(width: 12),
          Expanded(
            child: Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Text(
                  label,
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: theme?.textPrimary ?? AppTheme.textPrimary,
                  ),
                ),
                Text(
                  detail,
                  style: TextStyle(
                    fontSize: 11,
                    color: theme?.textMuted ?? AppTheme.textMuted,
                    fontFamily: 'JetBrains Mono',
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _LogView extends StatelessWidget {
  final List<LogEntry> logs;
  final Color accent;
  final ObsessionTheme? theme;

  const _LogView({required this.logs, required this.accent, this.theme});

  @override
  Widget build(BuildContext context) {
    final recent = logs.length > 50 ? logs.sublist(logs.length - 50) : logs;
    final inset = theme?.insetSurfaceColor ?? AppTheme.bgDeep.withValues(alpha: 0.6);

    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: inset,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(
          color: theme?.hairlineBorder ?? Colors.white.withValues(alpha: 0.06),
        ),
      ),
      child: recent.isEmpty
          ? Center(
              child: Text(
                AppLocalizations.of(context).logEmpty,
                style: TextStyle(
                  fontSize: 11,
                  color: theme?.textMuted ?? AppTheme.textMuted,
                ),
              ),
            )
          : ListView.builder(
              reverse: true,
              itemCount: recent.length,
              padding: EdgeInsets.zero,
              itemBuilder: (context, index) {
                final entry = recent[recent.length - 1 - index];
                return _LogLine(entry: entry, accent: accent, theme: theme);
              },
            ),
    );
  }
}

class _LogLine extends StatelessWidget {
  final LogEntry entry;
  final Color accent;
  final ObsessionTheme? theme;

  const _LogLine({required this.entry, required this.accent, this.theme});

  @override
  Widget build(BuildContext context) {
    final color = switch (entry.level) {
      LogLevel.success => AppTheme.success,
      LogLevel.error => AppTheme.error,
      LogLevel.warning => AppTheme.warning,
      LogLevel.info => (theme?.textMuted ?? AppTheme.textMuted),
    };

    final h = entry.timestamp.hour.toString().padLeft(2, '0');
    final m = entry.timestamp.minute.toString().padLeft(2, '0');
    final s = entry.timestamp.second.toString().padLeft(2, '0');
    final time = '$h:$m:$s';

    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: RichText(
        text: TextSpan(
          children: [
            TextSpan(
              text: '[$time] ',
              style: TextStyle(
                fontSize: 9,
                color: theme?.textMuted ?? AppTheme.textMuted,
                fontFamily: 'JetBrains Mono',
              ),
            ),
            TextSpan(
              text: entry.message,
              style: TextStyle(
                fontSize: 10,
                color: color,
                fontFamily: 'JetBrains Mono',
              ),
            ),
          ],
        ),
      ),
    );
  }
}
