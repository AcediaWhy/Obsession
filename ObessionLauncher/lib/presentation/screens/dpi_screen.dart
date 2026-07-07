import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../l10n/app_localizations.dart';
import '../providers/dependency_providers.dart';
import '../providers/dpi_provider.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/glass_panel.dart';
import '../widgets/glow_widgets.dart';
import '../widgets/log_panel.dart';
import '../widgets/neon_power_button.dart';

class DpiScreen extends ConsumerWidget {
  const DpiScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final dpiState = ref.watch(dpiProvider);
    final dpiNotifier = ref.read(dpiProvider.notifier);
    final settings = ref.watch(settingsProvider);
    final categories = ref.watch(processRepositoryProvider).getCategories();

    // Показываем SnackBar при ошибке запуска/аварийном завершении.
    ref.listen<DpiState>(dpiProvider, (prev, next) {
      if (next.error.isNotEmpty && (prev == null || prev.error != next.error)) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(next.error),
            behavior: SnackBarBehavior.floating,
            action: SnackBarAction(
              label: AppLocalizations.of(context).close,
              onPressed: () => dpiNotifier.clearError(),
            ),
          ),
        );
      }
    });

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            AnimatedHeader(
              title: AppLocalizations.of(context).dpiTitle,
              subtitle: AppLocalizations.of(context).dpiSubtitle,
            ),
            _StatusPill(isActive: dpiState.isActive),
          ],
        ),
        const SizedBox(height: 24),
        Expanded(
          child: LayoutBuilder(
            builder: (context, constraints) {
              // Адаптивная ширина левой панели: 360 на широких, 280 на узких.
              final panelWidth = constraints.maxWidth < 600 ? 280.0 : 360.0;
              return Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  ConstrainedBox(
                    constraints: BoxConstraints(
                      maxWidth: panelWidth,
                      minWidth: 240,
                    ),
                child: GlassPanel(
                  padding: const EdgeInsets.all(20),
                  child: SingleChildScrollView(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          AppLocalizations.of(context).categories,
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w600,
                            color: AppTheme.textMutedOf(context),
                            letterSpacing: 1.2,
                          ),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          AppLocalizations.of(context).canSelectMultiple,
                          style: TextStyle(fontSize: 10, color: AppTheme.textMutedOf(context)),
                        ),
                        const SizedBox(height: 16),
                        Wrap(
                          spacing: 8,
                          runSpacing: 8,
                          children: categories
                              .map((cat) => _CategoryChip(
                                    category: cat,
                                    isSelected: dpiState.selectedCategories.contains(cat),
                                    onTap: () => dpiNotifier.toggleCategory(cat),
                                    accent: settings.accentColor,
                                    glow: settings.glowIntensity,
                                    disabled: dpiState.isActive,
                                  ))
                              .toList(),
                        ),
                        const SizedBox(height: 24),
                        if (dpiState.selectedCategories.isNotEmpty) ...[
                          Wrap(
                            alignment: WrapAlignment.spaceBetween,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            runSpacing: 8,
                            children: [
                              Text(
                                AppLocalizations.of(context).config,
                                style: TextStyle(
                                  fontSize: 12,
                                  fontWeight: FontWeight.w600,
                                  color: AppTheme.textMutedOf(context),
                                  letterSpacing: 1.2,
                                ),
                              ),
                              if (!dpiState.isActive && !dpiState.isTesting)
                                Row(
                                  mainAxisSize: MainAxisSize.min,
                                  children: [
                                    GlowButton(
                                      label: AppLocalizations.of(context).autoSelect,
                                      icon: Icons.auto_fix_high,
                                      color: settings.accentColor,
                                      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                                      onTap: () => dpiNotifier.autoConfigure(startAfter: true),
                                    ),
                                    const SizedBox(width: 8),
                                    TextButton.icon(
                                      onPressed: () {
                                        final cat = dpiState.selectedCategories.first;
                                        dpiNotifier.testAllConfigs(cat);
                                      },
                                      icon: const Icon(Icons.science_outlined, size: 14),
                                      label: Text(AppLocalizations.of(context).test, style: const TextStyle(fontSize: 11)),
                                      style: TextButton.styleFrom(
                                        foregroundColor: settings.accentColor,
                                      ),
                                    ),
                                  ],
                                ),
                            ],
                          ),
                          const SizedBox(height: 8),
                          ...dpiState.selectedCategories.map((cat) => _ConfigDropdown(
                                category: cat,
                                selected: dpiNotifier.getSelectedConfig(cat),
                                accent: settings.accentColor,
                                glow: settings.glowIntensity,
                                testResult: dpiState.testResults[dpiNotifier.getSelectedConfig(cat)],
                                onChanged: dpiState.isActive
                                    ? null
                                    : (conf) {
                                        if (conf != null) {
                                          dpiNotifier.setConfig(cat, conf);
                                        }
                                      },
                              )),
                          if (dpiState.isTesting) ...[
                            const SizedBox(height: 16),
                            LinearProgressIndicator(
                              value: dpiState.testingTotal > 0
                                  ? dpiState.testingCurrent / dpiState.testingTotal
                                  : 0,
                              backgroundColor: Colors.white.withValues(alpha: 0.05),
                              color: settings.accentColor,
                            ),
                            const SizedBox(height: 8),
                            Text(
                              dpiState.testingConfig.endsWith('.conf')
                                  ? AppLocalizations.of(context).testingConfig(
                                      dpiState.testingCurrent,
                                      dpiState.testingTotal,
                                      dpiState.testingConfig,
                                    )
                                  : AppLocalizations.of(context).testingCategory(
                                      dpiState.testingCurrent,
                                      dpiState.testingTotal,
                                      dpiState.testingConfig,
                                    ),
                              style: TextStyle(fontSize: 11, color: AppTheme.textSecondaryOf(context)),
                            ),
                          ],
                          if (dpiState.testResults.isNotEmpty && !dpiState.isTesting) ...[
                            const SizedBox(height: 16),
                            ...dpiState.testResults.entries.map((e) => _TestResultRow(
                                  confFile: e.key,
                                  isWorking: e.value,
                                  accent: settings.accentColor,
                                  onTap: () => dpiNotifier.setConfig(
                                    dpiState.selectedCategories.first,
                                    e.key,
                                  ),
                                )),
                          ],
                        ],
                      ],
                    ),
                 ),
               ),
              ),
              const SizedBox(width: 24),
              Expanded(
                child: Center(
                  child: NeonPowerButton(
                    isActive: dpiState.isActive,
                    isLoading: dpiState.isTransitioning,
                    onTap: dpiState.isTransitioning
                        ? null
                        : () async {
                            if (dpiState.isActive) {
                              await dpiNotifier.stop();
                            } else {
                              await dpiNotifier.start();
                            }
                          },
                  ),
                ),
              ),
            ],
          );
            },
          ),
        ),
        const SizedBox(height: 16),
        const SizedBox(
          height: 180,
          child: LogPanel(),
        ),
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
            isActive ? AppLocalizations.of(context).active : AppLocalizations.of(context).inactive,
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w500,
              color: AppTheme.textPrimaryOf(context),
            ),
          ),
        ],
      ),
    );
  }
}

class _CategoryChip extends StatelessWidget {
  final String category;
  final bool isSelected;
  final bool disabled;
  final VoidCallback onTap;
  final Color accent;
  final double glow;

  const _CategoryChip({
    required this.category,
    required this.isSelected,
    required this.disabled,
    required this.onTap,
    required this.accent,
    required this.glow,
  });

  String get label => switch (category) {
        'discord' => 'Discord',
        'youtube_twitch' => 'YouTube / Twitch',
        'gaming' => 'Gaming',
        'universal' => 'Universal',
        _ => category,
      };

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.transparent,
      child: InkWell(
        onTap: disabled ? null : onTap,
        borderRadius: BorderRadius.circular(10),
        child: AnimatedContainer(
          duration: 200.ms,
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
          decoration: BoxDecoration(
            color: isSelected ? accent.withValues(alpha: 0.15) : Colors.transparent,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(
              color: isSelected ? accent.withValues(alpha: 0.4) : Colors.white.withValues(alpha: 0.08),
            ),
            boxShadow: isSelected
                ? [
                    BoxShadow(
                      color: accent.withValues(alpha: glow * 0.3),
                      blurRadius: 8,
                    ),
                  ]
                : null,
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(
                  color: isSelected ? accent : AppTheme.textMutedOf(context).withValues(alpha: 0.4),
                  borderRadius: BorderRadius.circular(2),
                ),
              ),
              const SizedBox(width: 8),
              Text(
                label,
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

class _ConfigDropdown extends ConsumerWidget {
  final String category;
  final String selected;
  final Color accent;
  final double glow;
  final bool? testResult;
  final ValueChanged<String?>? onChanged;

  const _ConfigDropdown({
    required this.category,
    required this.selected,
    required this.accent,
    required this.glow,
    this.testResult,
    this.onChanged,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final configs = ref.watch(processRepositoryProvider).getConfigsForCategory(category);
    final resultColor = testResult == true
        ? AppTheme.success
        : testResult == false
            ? AppTheme.error
            : null;

    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Row(
        children: [
          if (resultColor != null)
            Container(
              width: 6,
              height: 6,
              margin: const EdgeInsets.only(right: 8),
              decoration: BoxDecoration(
                color: resultColor,
                shape: BoxShape.circle,
                boxShadow: [
                  BoxShadow(color: resultColor.withValues(alpha: glow * 0.5), blurRadius: 6),
                ],
              ),
            ),
          Expanded(
            child: Container(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
              decoration: BoxDecoration(
                color: Colors.white.withValues(alpha: 0.03),
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: Colors.white.withValues(alpha: 0.08)),
              ),
              child: DropdownButton<String>(
                value: selected.isEmpty ? null : selected,
                items: configs
                    .map((c) => DropdownMenuItem(
                          value: c,
                          child: Text(
                            c.replaceAll('.conf', ''),
                            style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context)),
                          ),
                        ))
                    .toList(),
                onChanged: onChanged == null
                    ? null
                    : (v) {
                        if (v != null) onChanged!(v);
                      },
                underline: const SizedBox(),
                isExpanded: true,
                icon: Icon(Icons.expand_more, size: 16, color: accent),
                style: TextStyle(fontSize: 12, color: AppTheme.textPrimaryOf(context)),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _TestResultRow extends StatelessWidget {
  final String confFile;
  final bool isWorking;
  final Color accent;
  final VoidCallback onTap;

  const _TestResultRow({
    required this.confFile,
    required this.isWorking,
    required this.accent,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final color = isWorking ? AppTheme.success : AppTheme.error;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(6),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Row(
          children: [
            Icon(
              isWorking ? Icons.check_circle : Icons.cancel,
              size: 14,
              color: color,
            ),
            const SizedBox(width: 8),
            Text(
              confFile.replaceAll('.conf', ''),
              style: TextStyle(fontSize: 11, color: AppTheme.textSecondaryOf(context)),
            ),
            const Spacer(),
            Text(
              isWorking ? AppLocalizations.of(context).working : AppLocalizations.of(context).notWorking,
              style: TextStyle(fontSize: 10, color: color, fontWeight: FontWeight.w600),
            ),
          ],
        ),
      ),
    );
  }
}
