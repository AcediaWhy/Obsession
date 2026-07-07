import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../l10n/app_localizations.dart';
import '../providers/settings_provider.dart';
import '../providers/update_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/glow_widgets.dart';
import '../widgets/obsession_dialog.dart';

class UpdateDialog extends ConsumerWidget {
  const UpdateDialog({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(updateProvider);
    final accent = ref.watch(settingsProvider).accentColor;

    return ObsessionDialog(
      title: AppLocalizations.of(context).updateAvailable,
      content: SizedBox(
        width: 400,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              AppLocalizations.of(context).versionWithValue(state.info?.version ?? ''),
              style: TextStyle(
                fontSize: 16,
                fontWeight: FontWeight.bold,
                color: accent,
              ),
            ),
            const SizedBox(height: 12),
            if (state.info?.publishedAt != null)
              Text(
                AppLocalizations.of(context).versionDate(
                  state.info!.publishedAt!.toLocal().toString().split(' ').first,
                ),
                style: TextStyle(
                  fontSize: 12,
                  color: AppTheme.textMutedOf(context),
                ),
              ),
            const SizedBox(height: 12),
            Container(
              constraints: const BoxConstraints(maxHeight: 200),
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: AppTheme.insetSurfaceOf(context),
                borderRadius: BorderRadius.circular(12),
              ),
              child: SingleChildScrollView(
                child: Text(
                  state.info?.releaseNotes ?? AppLocalizations.of(context).noDescription,
                  style: TextStyle(
                    fontSize: 13,
                    color: AppTheme.textSecondaryOf(context),
                    height: 1.5,
                  ),
                ),
              ),
            ),
            if (state.status == UpdateStatus.downloading) ...[
              const SizedBox(height: 16),
              LinearProgressIndicator(
                value: state.progress > 0 ? state.progress : null,
                backgroundColor: AppTheme.insetSurfaceOf(context),
                valueColor: AlwaysStoppedAnimation<Color>(accent),
                borderRadius: BorderRadius.circular(4),
              ),
              const SizedBox(height: 8),
              Text(
                AppLocalizations.of(context).downloadingUpdate,
                style: TextStyle(fontSize: 12, color: AppTheme.textMutedOf(context)),
              ),
            ],
            if (state.status == UpdateStatus.readyToInstall && state.fileHash != null) ...[
              const SizedBox(height: 16),
              Text(
                AppLocalizations.of(context).hashHint,
                style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context)),
              ),
              const SizedBox(height: 8),
              Container(
                padding: const EdgeInsets.all(10),
                decoration: BoxDecoration(
                  color: AppTheme.insetSurfaceOf(context),
                  borderRadius: BorderRadius.circular(8),
                ),
                child: SelectableText(
                  state.fileHash!,
                  style: TextStyle(
                    fontSize: 11,
                    color: AppTheme.textMutedOf(context),
                    fontFamily: 'monospace',
                  ),
                ),
              ),
            ],
            if (state.error != null) ...[
              const SizedBox(height: 12),
              Text(
                AppLocalizations.of(context).errorWithMessage(state.error!),
                style: const TextStyle(fontSize: 12, color: AppTheme.error),
              ),
            ],
          ],
        ),
      ),
      actions: [
        GlowButton(
          label: AppLocalizations.of(context).later,
          color: AppTheme.textSecondaryOf(context),
          isOutline: true,
          onTap: () {
            ref.read(updateProvider.notifier).dismiss();
            Navigator.of(context).pop();
          },
        ),
        if (state.status == UpdateStatus.available)
          GlowButton(
            label: AppLocalizations.of(context).download,
            icon: Icons.download,
            color: accent,
            onTap: () => ref.read(updateProvider.notifier).download(),
          ),
        if (state.status == UpdateStatus.readyToInstall) ...[
          GlowButton(
            label: AppLocalizations.of(context).folder,
            icon: Icons.folder_open,
            color: AppTheme.textSecondaryOf(context),
            isOutline: true,
            onTap: () => ref.read(updateProvider.notifier).openFolder(),
          ),
          const SizedBox(width: 8),
          GlowButton(
            label: AppLocalizations.of(context).runInstaller,
            icon: Icons.install_desktop,
            color: accent,
            onTap: () async {
              final ok = await ref.read(updateProvider.notifier).runInstaller();
              if (ok && context.mounted) {
                Navigator.of(context).pop();
              }
            },
          ),
        ],
      ],
    );
  }
}
