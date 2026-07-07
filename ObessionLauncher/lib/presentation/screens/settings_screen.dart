import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/entities/app_theme_preset.dart';
import '../../l10n/app_localizations.dart';
import '../providers/autostart_provider.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import '../theme/app_theme_preset.dart';
import '../widgets/glass_panel.dart';
import '../widgets/glow_widgets.dart';
import '../widgets/obsession_dialog.dart';

class SettingsScreen extends ConsumerStatefulWidget {
  const SettingsScreen({super.key});

  @override
  ConsumerState<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends ConsumerState<SettingsScreen> {
  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final notifier = ref.read(settingsProvider.notifier);

    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          AnimatedHeader(
            title: AppLocalizations.of(context).settings,
            subtitle: AppLocalizations.of(context).settingsSubtitle,
          ),
          const SizedBox(height: 24),
          _SectionTitle(title: AppLocalizations.of(context).appearance),
          const SizedBox(height: 12),
          GlassPanel(
            padding: const EdgeInsets.all(20),
            child: Column(
              children: [
                _PresetRow(
                  current: settings.themePreset,
                  onSelect: (preset) {
                    if (preset == AppThemePreset.custom) {
                      notifier.update(settings.copyWith(
                        themePreset: preset,
                        clearBackgroundMode: true,
                      ));
                      return;
                    }
                    final data = preset.data;
                    notifier.update(settings.copyWith(
                      themePreset: preset,
                      accentColor: data.accentColor,
                      glowIntensity: data.glowIntensity,
                      particleDensity: data.particleDensity,
                      blackHoleRadius: data.blackHoleRadius,
                      blackHoleDiskBrightness: data.blackHoleDiskBrightness,
                      blackHoleLensIntensity: data.blackHoleLensIntensity,
                      clearBackgroundMode: true,
                    ));
                  },
                ),
                const _Divider(),
                _BackgroundModeRow(
                  current: settings.backgroundMode,
                  isLightTheme: settings.themePreset.isLight,
                  onSelect: (mode) {
                    if (mode == null) {
                      notifier.update(settings.copyWith(clearBackgroundMode: true));
                    } else {
                      notifier.update(settings.copyWith(backgroundMode: mode));
                    }
                  },
                ),
                const _Divider(),
                _LanguageRow(
                  current: settings.locale,
                  onSelect: (locale) => notifier.update(settings.copyWith(locale: locale)),
                ),
                const _Divider(),
                _ColorRow(
                  label: AppLocalizations.of(context).accentColor,
                  current: settings.accentColor,
                  onPick: (c) => notifier.update(settings.copyWith(accentColor: c)),
                ),
                const _Divider(),
                _SliderRow(
                  label: AppLocalizations.of(context).glowIntensity,
                  value: settings.glowIntensity,
                  min: 0,
                  max: 1,
                  divisions: 20,
                  displayValue: '${(settings.glowIntensity * 100).round()}%',
                  onChanged: (v) => notifier.update(settings.copyWith(glowIntensity: v)),
                ),
                const _Divider(),
                _SliderRow(
                  label: AppLocalizations.of(context).blurStrength,
                  value: settings.blurStrength,
                  min: 0,
                  max: 40,
                  divisions: 40,
                  displayValue: settings.blurStrength.toStringAsFixed(0),
                  onChanged: (v) => notifier.update(settings.copyWith(blurStrength: v)),
                ),
                const _Divider(),
                _SliderRow(
                  label: AppLocalizations.of(context).backgroundSpeed,
                  value: settings.backgroundBlobSpeed,
                  min: 0,
                  max: 2,
                  divisions: 20,
                  displayValue: '${settings.backgroundBlobSpeed.toStringAsFixed(1)}x',
                  onChanged: (v) => notifier.update(settings.copyWith(backgroundBlobSpeed: v)),
                ),
                const _Divider(),
                _SliderRow(
                  label: AppLocalizations.of(context).particleDensity,
                  value: settings.particleDensity,
                  min: 0,
                  max: 1,
                  divisions: 20,
                  displayValue: '${(settings.particleDensity * 100).round()}%',
                  onChanged: (v) => notifier.update(settings.copyWith(particleDensity: v)),
                ),
                const _Divider(),
                _ToggleRow(
                  label: AppLocalizations.of(context).animationsEnabled,
                  value: settings.animationsEnabled,
                  onChanged: (v) => notifier.update(settings.copyWith(animationsEnabled: v)),
                  accent: settings.accentColor,
                ),
                const _Divider(),
                _ToggleRow(
                  label: AppLocalizations.of(context).blackHoleEnabled,
                  value: settings.blackHoleEnabled,
                  onChanged: (v) => notifier.update(settings.copyWith(blackHoleEnabled: v)),
                  accent: settings.accentColor,
                ),
              ],
            ),
          ).animate().fadeIn(duration: 400.ms).slideY(begin: 0.1),
          const SizedBox(height: 24),
          _SectionTitle(title: AppLocalizations.of(context).blackHole),
          const SizedBox(height: 12),
          GlassPanel(
            padding: const EdgeInsets.all(20),
            child: Column(
              children: [
                _SliderRow(
                  label: AppLocalizations.of(context).blackHoleRadius,
                  value: settings.blackHoleRadius,
                  min: 0.05,
                  max: 0.4,
                  divisions: 35,
                  displayValue: '${(settings.blackHoleRadius * 100).round()}%',
                  onChanged: (v) => notifier.update(settings.copyWith(blackHoleRadius: v)),
                ),
                const _Divider(),
                _SliderRow(
                  label: AppLocalizations.of(context).blackHoleDiskBrightness,
                  value: settings.blackHoleDiskBrightness,
                  min: 0,
                  max: 2,
                  divisions: 40,
                  displayValue: '${(settings.blackHoleDiskBrightness * 100).round()}%',
                  onChanged: (v) => notifier.update(settings.copyWith(blackHoleDiskBrightness: v)),
                ),
                const _Divider(),
                _SliderRow(
                  label: AppLocalizations.of(context).blackHoleLensIntensity,
                  value: settings.blackHoleLensIntensity,
                  min: 0,
                  max: 1.5,
                  divisions: 30,
                  displayValue: '${(settings.blackHoleLensIntensity * 100).round()}%',
                  onChanged: (v) => notifier.update(settings.copyWith(blackHoleLensIntensity: v)),
                ),
              ],
            ),
          ).animate().fadeIn(duration: 400.ms, delay: 50.ms).slideY(begin: 0.1),
          const SizedBox(height: 24),
          _SectionTitle(title: AppLocalizations.of(context).behavior),
          const SizedBox(height: 12),
          GlassPanel(
            padding: const EdgeInsets.all(20),
            child: Column(
              children: [
                _ToggleRow(
                  label: AppLocalizations.of(context).minimizeToTray,
                  value: settings.minimizeToTray,
                  onChanged: (v) => notifier.update(settings.copyWith(minimizeToTray: v)),
                  accent: settings.accentColor,
                ),
                const _Divider(),
                const _AutostartToggle(),
              ],
            ),
          ).animate().fadeIn(duration: 400.ms, delay: 100.ms).slideY(begin: 0.1),
          const SizedBox(height: 24),
          Center(
            child: TextButton.icon(
              onPressed: () => _confirmReset(context, notifier),
              icon: Icon(Icons.restart_alt, size: 16, color: AppTheme.textMutedOf(context)),
              label: Text(AppLocalizations.of(context).resetSettings,
                  style: TextStyle(fontSize: 12, color: AppTheme.textMutedOf(context))),
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _confirmReset(BuildContext context, SettingsNotifier notifier) async {
    final ok = await showObsessionDialog<bool>(
      context: context,
      title: AppLocalizations.of(context).resetConfirmTitle,
      message: AppLocalizations.of(context).resetConfirmMessage,
      icon: Icons.restart_alt,
      iconColor: AppTheme.warning,
      actions: [
        ObsessionDialogAction<bool>(label: AppLocalizations.of(context).cancel, value: false),
        ObsessionDialogAction<bool>(
          label: AppLocalizations.of(context).reset,
          icon: Icons.restart_alt,
          value: true,
          color: AppTheme.warning,
          isPrimary: true,
        ),
      ],
    );
    if (ok == true) {
      await notifier.reset();
    }
  }
}

class _PresetRow extends StatelessWidget {
  final AppThemePreset current;
  final ValueChanged<AppThemePreset> onSelect;

  const _PresetRow({required this.current, required this.onSelect});

  static const _darkPresets = [
    AppThemePreset.obsession,
    AppThemePreset.obsidian,
    AppThemePreset.terminal,
    AppThemePreset.eclipse,
  ];
  static const _lightPresets = [
    AppThemePreset.auroraMist,
    AppThemePreset.candyTerminal,
    AppThemePreset.seraphim,
  ];

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          l.themePreset,
          style: TextStyle(fontSize: 13, color: AppTheme.textSecondaryOf(context)),
        ),
        const SizedBox(height: 10),
        Text(
          l.themeGroupDark,
          style: TextStyle(
            fontSize: 10,
            fontWeight: FontWeight.w700,
            color: AppTheme.textMutedOf(context),
            letterSpacing: 1.2,
          ),
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 10,
          runSpacing: 10,
          children: _darkPresets.map((p) => _presetChip(context, p)).toList(),
        ),
        const SizedBox(height: 14),
        Text(
          l.themeGroupLight,
          style: TextStyle(
            fontSize: 10,
            fontWeight: FontWeight.w700,
            color: AppTheme.textMutedOf(context),
            letterSpacing: 1.2,
          ),
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 10,
          runSpacing: 10,
          children: [
            ..._lightPresets.map((p) => _presetChip(context, p)),
            _presetChip(context, AppThemePreset.custom),
          ],
        ),
      ],
    );
  }

  Widget _presetChip(BuildContext context, AppThemePreset preset) {
    final data = preset.data;
    final isSelected = current == preset;
    final swatches = <Color>[
      data.accentColor,
      data.secondaryAccent,
      if (preset.isLight) ...(data.iridescenceTints.take(3).toList()),
    ];
    return GestureDetector(
      onTap: () => onSelect(preset),
      child: AnimatedContainer(
        duration: 200.ms,
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        decoration: BoxDecoration(
          color: isSelected
              ? data.accentColor.withValues(alpha: 0.15)
              : AppTheme.insetSurfaceOf(context),
          borderRadius: BorderRadius.circular(data.sharpAngles ? 2 : 10),
          border: Border.all(
            color: isSelected
                ? data.accentColor.withValues(alpha: 0.6)
                : AppTheme.hairlineOf(context),
            width: 1.5,
          ),
          boxShadow: isSelected
              ? [
                  BoxShadow(
                    color: data.accentColor.withValues(alpha: data.glowIntensity * 0.4),
                    blurRadius: 12,
                    spreadRadius: 1,
                  ),
                ]
              : null,
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            // Мини-превью палитры.
            Container(
              width: 28,
              height: 14,
              decoration: BoxDecoration(
                borderRadius: BorderRadius.circular(4),
                gradient: LinearGradient(
                  colors: swatches.isNotEmpty
                      ? swatches
                      : [data.accentColor, data.accentColor],
                ),
                border: Border.all(color: AppTheme.hairlineOf(context), width: 0.5),
              ),
            ),
            const SizedBox(width: 8),
            Text(
              preset.label,
              style: TextStyle(
                fontSize: 12,
                fontWeight: isSelected ? FontWeight.w600 : FontWeight.w500,
                color: isSelected ? AppTheme.textPrimaryOf(context) : AppTheme.textSecondaryOf(context),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Селектор режима фона: «Авто (из темы)» + ручной оверрайд.
/// Показывает релевантный набор: тёмные режимы для тёмных тем, светлые для светлых.
class _BackgroundModeRow extends StatelessWidget {
  final BackgroundMode? current;
  final bool isLightTheme;
  final ValueChanged<BackgroundMode?> onSelect;

  const _BackgroundModeRow({
    required this.current,
    required this.isLightTheme,
    required this.onSelect,
  });

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    final accent = Theme.of(context).colorScheme.primary;
    // Релевантный набор режимов под яркость темы + «none».
    final modes = <BackgroundMode>[
      if (isLightTheme) ...[
        BackgroundMode.auroraDrift,
        BackgroundMode.candyGrid,
        BackgroundMode.seraphimHalo,
      ] else ...[
        BackgroundMode.blackHole,
      ],
      BackgroundMode.none,
    ];

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          l.backgroundMode,
          style: TextStyle(fontSize: 13, color: AppTheme.textSecondaryOf(context)),
        ),
        const SizedBox(height: 10),
        Wrap(
          spacing: 10,
          runSpacing: 10,
          children: [
            // «Авто (из темы)» — null оверрайд.
            _modeChip(
              context,
              label: l.backgroundModeAuto,
              isSelected: current == null,
              accent: accent,
              onTap: () => onSelect(null),
            ),
            ...modes.map((m) => _modeChip(
                  context,
                  label: m.label,
                  isSelected: current == m,
                  accent: accent,
                  onTap: () => onSelect(m),
                )),
          ],
        ),
      ],
    );
  }

  Widget _modeChip(
    BuildContext context, {
    required String label,
    required bool isSelected,
    required Color accent,
    required VoidCallback onTap,
  }) {
    return GestureDetector(
      onTap: onTap,
      child: AnimatedContainer(
        duration: 200.ms,
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        decoration: BoxDecoration(
          color: isSelected
              ? accent.withValues(alpha: 0.15)
              : AppTheme.insetSurfaceOf(context),
          borderRadius: BorderRadius.circular(10),
          border: Border.all(
            color: isSelected ? accent.withValues(alpha: 0.6) : AppTheme.hairlineOf(context),
            width: 1.5,
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12,
            fontWeight: isSelected ? FontWeight.w600 : FontWeight.w500,
            color: isSelected ? AppTheme.textPrimaryOf(context) : AppTheme.textSecondaryOf(context),
          ),
        ),
      ),
    );
  }
}

class _LanguageRow extends StatelessWidget {
  final String current;
  final ValueChanged<String> onSelect;

  const _LanguageRow({required this.current, required this.onSelect});

  static const _locales = ['ru', 'en'];

  @override
  Widget build(BuildContext context) {
    final l = AppLocalizations.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          l.language,
          style: TextStyle(fontSize: 13, color: AppTheme.textSecondaryOf(context)),
        ),
        const SizedBox(height: 12),
        Row(
          children: _locales.map((code) {
            final isSelected = current == code;
            final label = code == 'ru' ? l.russian : l.english;
            return Padding(
              padding: const EdgeInsets.only(right: 10),
              child: GestureDetector(
                onTap: () => onSelect(code),
                child: AnimatedContainer(
                  duration: 200.ms,
                  padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                  decoration: BoxDecoration(
                    color: isSelected
                        ? AppTheme.textPrimaryOf(context).withValues(alpha: 0.1)
                        : AppTheme.insetSurfaceOf(context),
                    borderRadius: BorderRadius.circular(10),
                    border: Border.all(
                      color: isSelected
                          ? AppTheme.textPrimaryOf(context).withValues(alpha: 0.4)
                          : AppTheme.hairlineOf(context),
                      width: 1.5,
                    ),
                  ),
                  child: Text(
                    label,
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: isSelected ? FontWeight.w600 : FontWeight.w500,
                      color: isSelected ? AppTheme.textPrimaryOf(context) : AppTheme.textSecondaryOf(context),
                    ),
                  ),
                ),
              ),
            );
          }).toList(),
        ),
      ],
    );
  }
}

class _AutostartToggle extends ConsumerWidget {
  const _AutostartToggle();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final autostart = ref.watch(autostartProvider);
    final notifier = ref.read(autostartProvider.notifier);
    final settings = ref.watch(settingsProvider);

    return Row(
      children: [
        Expanded(
            child: Text(AppLocalizations.of(context).autostart,
                style: TextStyle(fontSize: 13, color: AppTheme.textSecondaryOf(context)))),
        Switch(
          value: autostart.isEnabled,
          onChanged: (v) => notifier.toggle(),
          activeThumbColor: settings.accentColor,
        ),
      ],
    );
  }
}

class _SectionTitle extends StatelessWidget {
  final String title;
  const _SectionTitle({required this.title});

  @override
  Widget build(BuildContext context) {
    return Text(
      title.toUpperCase(),
      style: TextStyle(
        fontSize: 11,
        fontWeight: FontWeight.w600,
        color: AppTheme.textMutedOf(context),
        letterSpacing: 1.2,
      ),
    );
  }
}

class _Divider extends StatelessWidget {
  const _Divider();
  @override
  Widget build(BuildContext context) => Container(
      height: 1,
      color: AppTheme.hairlineOf(context),
      margin: const EdgeInsets.symmetric(vertical: 12));
}

class _ColorRow extends StatelessWidget {
  final String label;
  final Color current;
  final ValueChanged<Color> onPick;

  const _ColorRow({required this.label, required this.current, required this.onPick});

  static const _presets = [
    Color(0xFF6366F1),
    Color(0xFF8B5CF6),
    Color(0xFF3B82F6),
    Color(0xFF34D399),
    Color(0xFFF59E0B),
    Color(0xFFEF4444),
    Color(0xFFEC4899),
    Color(0xFF06B6D4),
  ];

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(child: Text(label, style: TextStyle(fontSize: 13, color: AppTheme.textSecondaryOf(context)))),
        Row(
          children: _presets.map((c) {
            final isSelected = current.toARGB32() == c.toARGB32();
            return Padding(
              padding: const EdgeInsets.only(left: 6),
              child: GestureDetector(
                onTap: () => onPick(c),
                child: AnimatedContainer(
                  duration: 200.ms,
                  width: 24,
                  height: 24,
                  decoration: BoxDecoration(
                    color: c,
                    shape: BoxShape.circle,
                    border: Border.all(
                      color: isSelected ? Colors.white : Colors.transparent,
                      width: 2,
                    ),
                    boxShadow:
                        isSelected ? [BoxShadow(color: c.withValues(alpha: 0.6), blurRadius: 8)] : null,
                  ),
                ),
              ),
            );
          }).toList(),
        ),
      ],
    );
  }
}

class _SliderRow extends ConsumerWidget {
  final String label;
  final double value;
  final double min;
  final double max;
  final int divisions;
  final String displayValue;
  final ValueChanged<double> onChanged;

  const _SliderRow({
    required this.label,
    required this.value,
    required this.min,
    required this.max,
    required this.divisions,
    required this.displayValue,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            Text(label, style: TextStyle(fontSize: 13, color: AppTheme.textSecondaryOf(context))),
            Text(displayValue,
                style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: settings.accentColor)),
          ],
        ),
        SliderTheme(
          data: SliderTheme.of(context).copyWith(
            activeTrackColor: settings.accentColor,
            thumbColor: settings.accentColor,
            overlayColor: settings.accentColor.withValues(alpha: 0.2),
            trackHeight: 3,
          ),
          child: Slider(
            value: value,
            min: min,
            max: max,
            divisions: divisions,
            onChanged: onChanged,
          ),
        ),
      ],
    );
  }
}

class _ToggleRow extends StatelessWidget {
  final String label;
  final bool value;
  final ValueChanged<bool> onChanged;
  final Color accent;

  const _ToggleRow({required this.label, required this.value, required this.onChanged, required this.accent});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(child: Text(label, style: TextStyle(fontSize: 13, color: AppTheme.textSecondaryOf(context)))),
        Switch(
          value: value,
          onChanged: onChanged,
          activeThumbColor: accent,
        ),
      ],
    );
  }
}
