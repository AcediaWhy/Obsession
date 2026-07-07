import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/entities/profile.dart';
import '../../l10n/app_localizations.dart';
import '../providers/profile_provider.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/glass_panel.dart';
import '../widgets/glow_widgets.dart';
import '../widgets/obsession_dialog.dart';
import '../widgets/shimmer_skeleton.dart';

class ProfileScreen extends ConsumerStatefulWidget {
  const ProfileScreen({super.key});

  @override
  ConsumerState<ProfileScreen> createState() => _ProfileScreenState();
}

class _ProfileScreenState extends ConsumerState<ProfileScreen> {
  @override
  void initState() {
    super.initState();
    Future.microtask(() => ref.read(profileProvider.notifier).load());
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(profileProvider);
    final notifier = ref.read(profileProvider.notifier);
    final settings = ref.watch(settingsProvider);

    // Показываем ошибки операций с профилем.
    ref.listen(profileProvider, (prev, next) {
      final err = next.error;
      if (err != null && err.isNotEmpty && err != prev?.error) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(err),
            backgroundColor: AppTheme.error,
            behavior: SnackBarBehavior.floating,
          ),
        );
        notifier.clearError();
      }
    });

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children: [
            AnimatedHeader(
              title: AppLocalizations.of(context).profilesTitle,
              subtitle: AppLocalizations.of(context).profilesSubtitle,
            ),
            GlowIconButton(
              icon: Icons.add,
              color: settings.accentColor,
              tooltip: AppLocalizations.of(context).newProfile,
              onTap: () => _showCreateDialog(context, notifier),
            ),
          ],
        ),
        const SizedBox(height: 24),
        if (state.isLoading)
          const Expanded(
            child: Padding(
              padding: EdgeInsets.only(top: 24),
              child: Column(
                children: [
                  ShimmerCard(),
                  ShimmerCard(),
                  ShimmerCard(),
                ],
              ),
            ),
          )
        else
          Expanded(
            child: state.profiles.isEmpty
                ? Center(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Icon(Icons.account_tree_outlined, size: 48, color: AppTheme.textMutedOf(context).withValues(alpha: 0.5)),
                        const SizedBox(height: 16),
                        Text(
                          AppLocalizations.of(context).noProfiles,
                          style: TextStyle(fontSize: 14, color: AppTheme.textMutedOf(context)),
                          textAlign: TextAlign.center,
                        ),
                      ],
                    ),
                  )
                : ListView.builder(
                    itemCount: state.profiles.length,
                    itemBuilder: (context, index) {
                      final profile = state.profiles[index];
                      final isActive = profile.id == state.activeProfile?.id;
                      return Padding(
                        padding: const EdgeInsets.only(bottom: 12),
                        child: _ProfileCard(
                          profile: profile,
                          isActive: isActive,
                          accent: settings.accentColor,
                          onActivate: () async {
                            await notifier.switchProfile(profile.id);
                          },
                          onDelete: () => _confirmDelete(context, notifier, profile),
                        ),
                      );
                    },
                  ),
          ),
      ],
    );
  }

  Future<void> _showCreateDialog(BuildContext context, ProfileNotifier notifier) async {
    final name = await showObsessionInputDialog(
      context: context,
      title: AppLocalizations.of(context).newProfile,
      icon: Icons.account_tree,
      hintText: AppLocalizations.of(context).profileName,
      confirmLabel: AppLocalizations.of(context).create,
    );
    if (name != null && name.isNotEmpty) {
      await notifier.create(name);
    }
  }

  Future<void> _confirmDelete(
    BuildContext context,
    ProfileNotifier notifier,
    Profile profile,
  ) async {
    final ok = await showObsessionDialog<bool>(
      context: context,
      title: AppLocalizations.of(context).deleteProfileTitle,
      message: AppLocalizations.of(context).deleteProfileMessage(profile.name),
      icon: Icons.delete_outline,
      iconColor: AppTheme.error,
      actions: [
        ObsessionDialogAction<bool>(
          label: AppLocalizations.of(context).delete,
          value: true,
          color: AppTheme.error,
          isPrimary: true,
        ),
        ObsessionDialogAction<bool>(
          label: AppLocalizations.of(context).cancel,
          value: false,
        ),
      ],
    );
    if (ok == true) {
      await notifier.delete(profile.id);
    }
  }
}

class _ProfileCard extends StatelessWidget {
  final Profile profile;
  final bool isActive;
  final Color accent;
  final VoidCallback onActivate;
  final VoidCallback onDelete;

  const _ProfileCard({
    required this.profile,
    required this.isActive,
    required this.accent,
    required this.onActivate,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    return GlassPanel(
      glowColor: isActive ? accent : null,
      glowRadius: 12,
      child: Row(
        children: [
          AnimatedStatusIndicator(
            isActive: isActive,
            size: 11,
            color: isActive ? accent : AppTheme.textMutedOf(context),
          ),
          const SizedBox(width: 16),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  profile.name,
                  style: TextStyle(
                    fontSize: 16,
                    fontWeight: FontWeight.w600,
                    color: AppTheme.textPrimaryOf(context),
                  ),
                ),
                Text(
                  AppLocalizations.of(context)
                      .profileCategoriesWithProvider(profile.dpiConfigs.length, profile.hostsProvider.label),
                  style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context)),
                ),
              ],
            ),
          ),
          if (!isActive)
            TextButton(
              onPressed: onActivate,
              child: Text(AppLocalizations.of(context).activate, style: TextStyle(color: accent)),
            ),
          IconButton(
            icon: const Icon(Icons.delete_outline, color: AppTheme.error, size: 20),
            onPressed: onDelete,
          ),
        ],
      ),
    );
  }
}

