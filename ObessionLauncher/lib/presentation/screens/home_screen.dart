import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import '../../domain/entities/app_settings.dart';
import '../../domain/entities/app_theme_preset.dart';
import '../../l10n/app_localizations.dart';
import '../providers/app_state_provider.dart';
import '../providers/dependency_providers.dart';
import '../providers/settings_provider.dart';
import '../providers/update_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/animated_background.dart';
import '../widgets/black_hole_background.dart';
import '../widgets/custom_title_bar.dart';
import '../widgets/dashboard_panel.dart';
import '../widgets/gravitational_grid.dart';
import '../widgets/light_background.dart';
import '../widgets/obsession_dialog.dart';
import '../widgets/orbital_screen_switcher.dart';
import '../widgets/particle_field.dart';
import '../widgets/update_dialog.dart';


class HomeScreen extends ConsumerStatefulWidget {
  const HomeScreen({super.key});

  @override
  ConsumerState<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends ConsumerState<HomeScreen> with WindowListener {
  @override
  void initState() {
    super.initState();
    windowManager.setPreventClose(true);
    windowManager.addListener(this);
    _checkUpdateAfterBuild();
    _checkOrphanedProcesses();
    // Динамический цвет нативного окна под текущую тему.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final settings = ref.read(settingsProvider);
      windowManager.setBackgroundColor(AppTheme.windowBackgroundColor(settings));
    });
  }

  /// При старте обнаруживает orphan-процессы winws.exe от предыдущего запуска
  /// (после краша/Task Manager) и предлагает пользователю их очистить.
  ///
  /// Orphan-процессы мешают новому запуску: winws падает с "a copy of winws
  /// is already running with the same filter". Раньше приложение решало это
  /// автоматическим `taskkill /IM winws.exe` при каждом stopAll (что убивало
  /// и сторонние экземпляры других DPI-инструментов). Теперь kill только своих
  /// PID, а orphan — через явный диалог с предупреждением.
  void _checkOrphanedProcesses() {
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      await Future.delayed(const Duration(milliseconds: 500));
      if (!mounted) return;
      final l = AppLocalizations.of(context);
      final processDataSource = ref.read(processDataSourceProvider);
      final orphans = await processDataSource.detectOrphanedWinws();
      if (!mounted || orphans.isEmpty) return;
      final shouldClean = await showObsessionDialog<bool>(
        context: context,
        title: l.orphanDetectedTitle,
        message: l.orphanDetectedMessage(orphans.length),
        icon: Icons.warning_amber_rounded,
        iconColor: AppTheme.warning,
        actions: [
          ObsessionDialogAction<bool>(
            label: l.orphanDetectedIgnore,
            value: false,
            isPrimary: false,
          ),
          ObsessionDialogAction<bool>(
            label: l.orphanDetectedClear,
            value: true,
            isPrimary: true,
            color: AppTheme.warning,
          ),
        ],
      );
      if (!mounted || shouldClean != true) return;
      await processDataSource.emergencyKillAllByName();
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(l.orphanDetectedCleared),
          behavior: SnackBarBehavior.floating,
        ),
      );
    });
  }

  @override
  void dispose() {
    windowManager.removeListener(this);
    super.dispose();
  }

  @override
  void onWindowClose() async {
    // Единый координатор завершения: останавливает DPI + прокси + логгер,
    // затем уничтожает окно. Идемпотентен (защита от двойного срабатывания).
    // Фикс прежней утечки: раньше останавливался только DPI, прокси и логгер
    // оставались висеть с захваченными ресурсами.
    await ref.read(shutdownCoordinatorProvider).shutdown();
  }

  /// Разрешает и рендерит фон по эффективному режиму (оверрайд или дефолт пресета).
  Widget _buildBackground(AppSettings settings) {
    final mode = settings.effectiveBackgroundMode;
    switch (mode) {
      case BackgroundMode.blackHole:
        if (!settings.blackHoleEnabled) {
          return const RepaintBoundary(child: AnimatedBackground());
        }
        // Каждый слой фона изолирован RepaintBoundary: анимированные слои
        // (чёрная дыра, сетка, частицы) перерисовываются каждый кадр и не
        // тянут за собой перерисовку статичного UI поверх.
        return Stack(
          children: const [
            RepaintBoundary(child: BlackHoleBackground()),
            RepaintBoundary(child: GravitationalGrid()),
            RepaintBoundary(child: ParticleField()),
          ],
        );
      case BackgroundMode.auroraDrift:
      case BackgroundMode.candyGrid:
      case BackgroundMode.seraphimHalo:
        final preset = AppTheme.resolvePreset(settings);
        final config = preset.lightBackground ?? settings.themePreset.data.lightBackground;
        if (config == null) return const SizedBox.shrink();
        return RepaintBoundary(
          child: LightBackground(
            config: config,
            animationsEnabled: settings.animationsEnabled,
            blobSpeed: settings.backgroundBlobSpeed,
            blurStrength: settings.blurStrength,
          ),
        );
      case BackgroundMode.none:
        final preset = AppTheme.resolvePreset(settings);
        if (preset.isLight) {
          final grad = preset.lightBackground?.baseGradient ?? const [Color(0xFFFFFCFF)];
          return Container(decoration: BoxDecoration(gradient: LinearGradient(
            begin: Alignment.topCenter,
            end: Alignment.bottomCenter,
            colors: grad,
          )));
        }
        return const RepaintBoundary(child: AnimatedBackground());
    }
  }

  void _checkUpdateAfterBuild() {
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      await Future.delayed(const Duration(seconds: 2));
      if (!mounted) return;
      await ref.read(updateProvider.notifier).check();
      final state = ref.read(updateProvider);
      if (!mounted) return;
      if (state.status == UpdateStatus.available) {
        showDialog(
          context: context,
          barrierDismissible: false,
          builder: (_) => const UpdateDialog(),
        );
      } else if (state.status == UpdateStatus.error) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(state.error ?? ''),
            behavior: SnackBarBehavior.floating,
          ),
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final activeTab = ref.watch(activeTabProvider);
    final settings = ref.watch(settingsProvider);
    final size = MediaQuery.sizeOf(context);
    final showDashboard = size.width >= 1100;

    // Обновляем цвет нативного окна при смене темы/пресета.
    ref.listen(settingsProvider, (prev, next) {
      final prevBg = AppTheme.windowBackgroundColor(prev ?? next);
      final nextBg = AppTheme.windowBackgroundColor(next);
      if (prevBg.toARGB32() != nextBg.toARGB32()) {
        windowManager.setBackgroundColor(nextBg);
      }
    });

    return Scaffold(
      body: Stack(
        children: [
          // Фон целиком в своём RepaintBoundary — не влияет на перерисовку UI.
          RepaintBoundary(child: _buildBackground(settings)),
          Positioned(
            top: 40,
            left: 0,
            right: 0,
            bottom: 0,
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _NavRail(activeTab: activeTab).animate().fadeIn(duration: 400.ms).slideX(begin: -0.1),
                Expanded(
                  child: Padding(
                    padding: const EdgeInsets.only(left: 24, right: 24, bottom: 24),
                    child: OrbitalScreenSwitcher(activeTab: activeTab),
                  ),
                ),
                if (showDashboard)
                  const SizedBox(
                    width: 300,
                    child: Padding(
                      padding: EdgeInsets.only(right: 24, bottom: 24),
                      child: DashboardPanel(),
                    ),
                  ),
              ],
            ),
          ),
          const Positioned(
            top: 0,
            left: 0,
            right: 0,
            height: 40,
            child: CustomTitleBar(),
          ),
        ],
      ),
    );
  }
}

class _NavRail extends ConsumerWidget {
  final AppTab activeTab;

  const _NavRail({required this.activeTab});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final titleColor = theme?.textPrimary ?? AppTheme.textPrimary;
    final subtitleColor = theme?.textSecondary ?? AppTheme.textSecondary;
    final mutedColor = theme?.textMuted ?? AppTheme.textMuted;

    return Container(
      width: 220,
      padding: const EdgeInsets.only(top: 48, left: 16, right: 16, bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 40,
                height: 40,
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(12),
                  gradient: const LinearGradient(
                    begin: Alignment.topLeft,
                    end: Alignment.bottomRight,
                    colors: [Color(0xFF6366F1), Color(0xFF8B5CF6)],
                  ),
                ),
                clipBehavior: Clip.antiAlias,
                child: Image.asset(
                  'assets/icons/app_icon.png',
                  fit: BoxFit.cover,
                ),
              ),
              const SizedBox(width: 12),
              Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Obsession',
                    style: TextStyle(
                      fontSize: 18,
                      fontWeight: FontWeight.bold,
                      color: titleColor,
                      fontFamily: isLight ? theme?.headingFont : null,
                    ),
                  ),
                  Text(
                    'Launcher',
                    style: TextStyle(
                      fontSize: 12,
                      color: subtitleColor,
                      height: 1,
                      fontFamily: isLight ? theme?.bodyFont : null,
                    ),
                  ),
                ],
              ),
            ],
          ),
          const SizedBox(height: 32),
          Text(
            AppLocalizations.of(context).menu,
            style: TextStyle(
              fontSize: 11,
              fontWeight: FontWeight.w600,
              color: mutedColor,
              letterSpacing: 1.2,
            ),
          ),
          const SizedBox(height: 8),
          _NavItem(
            icon: Icons.bolt,
            label: AppLocalizations.of(context).dpiTitle,
            isActive: activeTab == AppTab.dpi,
            onTap: () => ref.read(activeTabProvider.notifier).state = AppTab.dpi,
          ),
          _NavItem(
            icon: Icons.smart_toy,
            label: AppLocalizations.of(context).aiTitle,
            isActive: activeTab == AppTab.ai,
            onTap: () => ref.read(activeTabProvider.notifier).state = AppTab.ai,
          ),
          _NavItem(
            icon: Icons.send,
            label: AppLocalizations.of(context).telegramTitle,
            isActive: activeTab == AppTab.telegram,
            onTap: () => ref.read(activeTabProvider.notifier).state = AppTab.telegram,
          ),
          _NavItem(
            icon: Icons.settings,
            label: AppLocalizations.of(context).settings,
            isActive: activeTab == AppTab.settings,
            onTap: () => ref.read(activeTabProvider.notifier).state = AppTab.settings,
          ),
          _NavItem(
            icon: Icons.list_alt,
            label: AppLocalizations.of(context).listsTitle,
            isActive: activeTab == AppTab.lists,
            onTap: () => ref.read(activeTabProvider.notifier).state = AppTab.lists,
          ),
          _NavItem(
            icon: Icons.account_tree,
            label: AppLocalizations.of(context).profilesTitle,
            isActive: activeTab == AppTab.profiles,
            onTap: () => ref.read(activeTabProvider.notifier).state = AppTab.profiles,
          ),
          const Spacer(),
          Padding(
            padding: const EdgeInsets.only(left: 8),
            child: Text(
              'made by VlarpSu',
              style: TextStyle(
                fontSize: 11,
                color: mutedColor,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _NavItem extends ConsumerStatefulWidget {
  final IconData icon;
  final String label;
  final bool isActive;
  final VoidCallback onTap;

  const _NavItem({
    required this.icon,
    required this.label,
    required this.isActive,
    required this.onTap,
  });

  @override
  ConsumerState<_NavItem> createState() => _NavItemState();
}

class _NavItemState extends ConsumerState<_NavItem> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final theme = Theme.of(context).extension<ObsessionTheme>();
    final isLight = theme?.isLight ?? false;
    final accent = settings.accentColor;
    final glow = settings.glowIntensity;
    final hovered = _hovered;
    final active = widget.isActive;

    final inactiveColor = theme?.textSecondary ?? AppTheme.textSecondary;
    final color = active || hovered ? accent : inactiveColor;

    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          onTap: widget.onTap,
          child: AnimatedContainer(
            duration: 200.ms,
            curve: Curves.easeOutCubic,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            decoration: BoxDecoration(
              color: active
                  ? accent.withValues(alpha: isLight ? 0.12 : 0.15)
                  : hovered
                      ? accent.withValues(alpha: isLight ? 0.08 : 0.06)
                      : Colors.transparent,
              borderRadius: BorderRadius.circular(12),
              border: Border.all(
                color: active
                    ? accent.withValues(alpha: isLight ? 0.4 : 0.3)
                    : hovered
                        ? accent.withValues(alpha: isLight ? 0.3 : 0.2)
                        : Colors.transparent,
              ),
              boxShadow: [
                if (active && !isLight)
                  BoxShadow(
                    color: accent.withValues(alpha: glow * 0.4),
                    blurRadius: 12,
                  ),
                if (hovered && !active && !isLight)
                  BoxShadow(
                    color: accent.withValues(alpha: glow * 0.2),
                    blurRadius: 10,
                  ),
                if (active && isLight)
                  BoxShadow(
                    color: accent.withValues(alpha: glow * 0.2),
                    blurRadius: 10,
                  ),
              ],
            ),
            child: Row(
              children: [
                AnimatedContainer(
                  duration: 200.ms,
                  transform: Matrix4.diagonal3Values(
                    hovered ? 1.15 : 1.0,
                    hovered ? 1.15 : 1.0,
                    1.0,
                  ),
                  child: Icon(widget.icon, size: 18, color: color),
                ),
                const SizedBox(width: 10),
                Text(
                  widget.label,
                  style: TextStyle(
                    fontSize: 13,
                    fontWeight: active ? FontWeight.w600 : FontWeight.w500,
                    color: color,
                    fontFamily: isLight ? theme?.bodyFont : null,
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
