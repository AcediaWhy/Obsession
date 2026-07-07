import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:google_fonts/google_fonts.dart';

import '../../domain/entities/log_entry.dart';
import '../../l10n/app_localizations.dart';
import '../providers/log_provider.dart';
import '../theme/app_theme.dart';
import 'glass_panel.dart';

/// Лог-панель с цветовой подсветкой.
class LogPanel extends ConsumerWidget {
  const LogPanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final logs = ref.watch(logHistoryProvider);
    final logRepo = ref.watch(logRepositoryRiverpodProvider);

    return GlassPanel(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.terminal, size: 14, color: AppTheme.textMutedOf(context)),
              const SizedBox(width: 6),
              Text(
                AppLocalizations.of(context).logTitle,
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  color: AppTheme.textMutedOf(context),
                  letterSpacing: 1.2,
                ),
              ),
              const Spacer(),
              if (logs.isNotEmpty)
                GestureDetector(
                  onTap: () => logRepo.clear(),
                  child: Icon(Icons.cleaning_services_outlined,
                      size: 14, color: AppTheme.textMutedOf(context)),
                ),
            ],
          ),
          const SizedBox(height: 8),
          Expanded(
            child: logs.isEmpty
                ? Center(
                    child: Text(
                      AppLocalizations.of(context).waitingToStart,
                      style: TextStyle(
                        fontSize: 12,
                        color: AppTheme.textMutedOf(context),
                        fontStyle: FontStyle.italic,
                      ),
                    ),
                  )
                : ListView.builder(
                    reverse: true,
                    itemCount: logs.length,
                    itemBuilder: (context, i) {
                      final idx = logs.length - 1 - i;
                      final entry = logs[idx];
                      final color = switch (entry.level) {
                        LogLevel.error => AppTheme.error,
                        LogLevel.warning => AppTheme.warning,
                        LogLevel.success => AppTheme.success,
                        LogLevel.info => AppTheme.textSecondaryOf(context),
                      };
                      final time = '${entry.timestamp.hour.toString().padLeft(2, '0')}:'
                          '${entry.timestamp.minute.toString().padLeft(2, '0')}:'
                          '${entry.timestamp.second.toString().padLeft(2, '0')}';
                      return Padding(
                        padding: const EdgeInsets.only(bottom: 2),
                        child: Text.rich(
                          TextSpan(
                            children: [
                              TextSpan(
                                text: '[$time] ',
                                style: GoogleFonts.jetBrainsMono(
                                  fontSize: 11,
                                  color: AppTheme.textMutedOf(context),
                                ),
                              ),
                              TextSpan(
                                text: entry.message,
                                style: GoogleFonts.jetBrainsMono(
                                  fontSize: 11,
                                  color: color,
                                ),
                              ),
                            ],
                          ),
                        ),
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}
