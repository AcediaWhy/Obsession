import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/entities/list_entry.dart';
import '../../l10n/app_localizations.dart';
import '../providers/dependency_providers.dart';
import '../providers/list_provider.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/glass_panel.dart';
import '../widgets/glow_widgets.dart';
import '../widgets/obsession_dialog.dart';
import '../widgets/shimmer_skeleton.dart';

class ListEditorScreen extends ConsumerStatefulWidget {
  const ListEditorScreen({super.key});

  @override
  ConsumerState<ListEditorScreen> createState() => _ListEditorScreenState();
}

class _ListEditorScreenState extends ConsumerState<ListEditorScreen> {
  @override
  void initState() {
    super.initState();
    Future.microtask(() => ref.read(listEditorProvider.notifier).loadNames());
  }

  Future<void> _importFile() async {
    final result = await FilePicker.platform.pickFiles(
      type: FileType.any,
      allowMultiple: false,
      dialogTitle: AppLocalizations.of(context).importListDialogTitle,
    );
    if (result == null || result.files.single.path == null) return;

    await ref.read(listEditorProvider.notifier).importFromFile(result.files.single.path!);
  }

  Future<void> _exportFile() async {
    final name = ref.read(listEditorProvider).selectedList;
    if (name == null) return;

    final output = await FilePicker.platform.saveFile(
      dialogTitle: AppLocalizations.of(context).exportListDialogTitle,
      fileName: '$name.txt',
      type: FileType.custom,
      allowedExtensions: ['txt'],
    );
    if (output == null) return;

    await ref.read(listEditorProvider.notifier).exportToFile(output);
  }

  Future<void> _confirmClear() async {
    final ok = await showObsessionDialog<bool>(
      context: context,
      title: AppLocalizations.of(context).clearListTitle,
      message: AppLocalizations.of(context).clearListMessage,
      icon: Icons.cleaning_services,
      iconColor: AppTheme.warning,
      actions: [
        ObsessionDialogAction<bool>(label: AppLocalizations.of(context).cancel, value: false),
        ObsessionDialogAction<bool>(
          label: AppLocalizations.of(context).clear,
          icon: Icons.delete_sweep,
          value: true,
          color: AppTheme.error,
          isPrimary: true,
        ),
      ],
    );
    if (ok == true) {
      ref.read(listEditorProvider.notifier).clearEntries();
    }
  }

  Future<void> _showAutoDetected() async {
    final paths = ref.read(pathsDataSourceProvider);
    final l = AppLocalizations.of(context);
    final autohosts = paths.readAutohosts();
    final totalDomains = autohosts.values.fold(0, (sum, list) => sum + list.length);

    await showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: AppTheme.surfaceOf(context),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        title: Row(
          children: [
            Icon(Icons.auto_awesome, color: AppTheme.success, size: 24),
            const SizedBox(width: 12),
            Text(l.autoDetected, style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600, color: AppTheme.textPrimaryOf(context))),
            const Spacer(),
            if (totalDomains > 0)
              Text(l.domainsCount(totalDomains),
                  style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context))),
          ],
        ),
        content: SizedBox(
          width: double.maxFinite,
          child: autohosts.isEmpty || totalDomains == 0
              ? Padding(
                  padding: const EdgeInsets.symmetric(vertical: 32),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.search_off, size: 48, color: AppTheme.textMutedOf(context).withValues(alpha: 0.5)),
                      const SizedBox(height: 16),
                      Text(l.noAutoDetected,
                          style: TextStyle(fontSize: 13, color: AppTheme.textMutedOf(context)),
                          textAlign: TextAlign.center),
                    ],
                  ),
                )
              : SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(l.autoDetectedSubtitle,
                          style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context))),
                      const SizedBox(height: 16),
                      ...autohosts.entries.expand((e) {
                        return [
                          Padding(
                            padding: const EdgeInsets.only(top: 12, bottom: 6),
                            child: Text(
                              '${e.key} (${e.value.length})',
                              style: TextStyle(
                                fontSize: 13,
                                fontWeight: FontWeight.w600,
                                color: AppTheme.success,
                              ),
                            ),
                          ),
                          ...e.value.take(50).map((domain) => Padding(
                                padding: const EdgeInsets.only(left: 12, bottom: 2),
                                child: Text(domain,
                                    style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context))),
                              )),
                          if (e.value.length > 50)
                            Padding(
                              padding: const EdgeInsets.only(left: 12, top: 4),
                              child: Text('+ ${e.value.length - 50}...',
                                  style: TextStyle(fontSize: 11, color: AppTheme.textMutedOf(context))),
                            ),
                        ];
                      }),
                    ],
                  ),
                ),
        ),
        actions: [
          if (totalDomains > 0)
            TextButton(
              onPressed: () async {
                await paths.clearAutohosts();
                if (mounted && ctx.mounted) {
                  Navigator.of(ctx).pop();
                  ScaffoldMessenger.of(context).showSnackBar(
                    SnackBar(
                      content: Text(l.autoDetectedCleared),
                      behavior: SnackBarBehavior.floating,
                    ),
                  );
                }
              },
              child: Text(l.clearAutoDetected, style: TextStyle(color: AppTheme.error)),
            ),
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(),
            child: Text(l.close, style: TextStyle(color: AppTheme.textSecondaryOf(context))),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(listEditorProvider);
    final notifier = ref.read(listEditorProvider.notifier);
    final settings = ref.watch(settingsProvider);

    ref.listen(listEditorProvider, (prev, next) {
      final err = next.error;
      if (err != null && err.isNotEmpty && err != prev?.error) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(err),
            behavior: SnackBarBehavior.floating,
            backgroundColor: AppTheme.error,
          ),
        );
        notifier.clearError();
      }
    });

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        AnimatedHeader(
          title: AppLocalizations.of(context).listsTitle,
          subtitle: AppLocalizations.of(context).listsSubtitle,
        ),
        const SizedBox(height: 24),
        GlassPanel(
          padding: const EdgeInsets.all(16),
          child: DropdownButton<String>(
            value: state.selectedList,
            isExpanded: true,
            hint: Text(AppLocalizations.of(context).selectListHint, style: TextStyle(color: AppTheme.textSecondaryOf(context))),
            dropdownColor: AppTheme.surfaceOf(context),
            items: state.listNames
                .map((name) => DropdownMenuItem(
                      value: name,
                      child: Text(name, style: TextStyle(color: AppTheme.textPrimaryOf(context))),
                    ))
                .toList(),
            onChanged: (name) {
              if (name != null) notifier.selectList(name);
            },
          ),
        ).animate().fadeIn(duration: 400.ms).slideY(begin: 0.1),
        const SizedBox(height: 16),
        if (state.isLoading)
          const Expanded(
            child: Padding(
              padding: EdgeInsets.only(top: 24),
              child: ShimmerSettingsPanel(),
            ),
          )
        else if (state.selectedList != null)
          Expanded(
            child: GlassPanel(
              padding: const EdgeInsets.all(16),
              child: Column(
                children: [
                  Row(
                    children: [
                      GlowIconButton(
                        icon: Icons.file_upload_outlined,
                        color: settings.accentColor,
                        tooltip: AppLocalizations.of(context).importListDialogTitle,
                        onTap: _importFile,
                      ),
                      const SizedBox(width: 8),
                      GlowIconButton(
                        icon: Icons.file_download_outlined,
                        color: settings.accentColor,
                        tooltip: AppLocalizations.of(context).exportListDialogTitle,
                        onTap: _exportFile,
                      ),
                      const SizedBox(width: 8),
                      GlowIconButton(
                        icon: Icons.content_paste_go,
                        color: AppTheme.warning,
                        tooltip: AppLocalizations.of(context).clear,
                        onTap: _confirmClear,
                      ),
                      const SizedBox(width: 8),
                      GlowIconButton(
                        icon: Icons.merge_type,
                        color: AppTheme.textSecondaryOf(context),
                        tooltip: AppLocalizations.of(context).removeDuplicates,
                        onTap: () => notifier.removeDuplicates(),
                      ),
                      const SizedBox(width: 8),
                      GlowIconButton(
                        icon: Icons.auto_awesome,
                        color: AppTheme.success,
                        tooltip: AppLocalizations.of(context).autoDetected,
                        onTap: _showAutoDetected,
                      ),
                      const Spacer(),
                      Text(
                        AppLocalizations.of(context).entriesCount(
                          state.entries.where((e) => !e.isBlank && !e.isComment).length,
                        ),
                        style: TextStyle(fontSize: 12, color: AppTheme.textSecondaryOf(context)),
                      ),
                    ],
                  ).animate().fadeIn(duration: 300.ms),
                  const SizedBox(height: 12),
                  Expanded(
                    child: ListView.builder(
                      itemCount: state.entries.length,
                      itemBuilder: (context, index) {
                        final entry = state.entries[index];
                        final isValid = state.validationResults[entry.id] ?? true;
                        return Padding(
                          key: ValueKey(entry.id),
                          padding: const EdgeInsets.only(bottom: 8),
                          child: _ListEntryRow(
                            key: ValueKey('row_${entry.id}'),
                            entry: entry,
                            isValid: isValid,
                            accent: settings.accentColor,
                            onChanged: (e) => notifier.updateEntry(index, e),
                            onDelete: () => notifier.removeEntry(index),
                            onToggleComment: () => notifier.updateEntry(
                              index,
                              entry.isComment
                                  ? entry.copyWith(
                                      value: entry.value.isEmpty ? 'example.com' : entry.value,
                                      isComment: false,
                                    )
                                  : entry.copyWith(isComment: true),
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                        Expanded(
                        child: GlowButton(
                          label: AppLocalizations.of(context).domain,
                          icon: Icons.add,
                          color: settings.accentColor,
                          fullWidth: true,
                          onTap: () => notifier.addEntry(ListEntry.domain('example.com')),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: GlowButton(
                          label: AppLocalizations.of(context).ip,
                          icon: Icons.add,
                          color: AppTheme.textSecondaryOf(context),
                          isOutline: true,
                          fullWidth: true,
                          onTap: () => notifier.addIp(),
                        ),
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: GlowButton(
                          label: AppLocalizations.of(context).comment,
                          icon: Icons.comment,
                          color: AppTheme.textSecondaryOf(context),
                          isOutline: true,
                          fullWidth: true,
                          onTap: () => notifier.addComment(),
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                      flex: 2,
                      child: GlowButton(
                        label: AppLocalizations.of(context).save,
                        icon: Icons.save,
                        color: AppTheme.success,
                        fullWidth: true,
                        onTap: () async {
                          await notifier.save();
                        },
                      ),
                    ),
                    ],
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _ListEntryRow extends StatelessWidget {
  final ListEntry entry;
  final bool isValid;
  final Color accent;
  final ValueChanged<ListEntry> onChanged;
  final VoidCallback onDelete;
  final VoidCallback onToggleComment;

  const _ListEntryRow({
    super.key,
    required this.entry,
    required this.isValid,
    required this.accent,
    required this.onChanged,
    required this.onDelete,
    required this.onToggleComment,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        color: entry.isComment
            ? Colors.white.withValues(alpha: 0.02)
            : Colors.white.withValues(alpha: 0.04),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(
          color: entry.isComment
              ? Colors.white.withValues(alpha: 0.05)
              : !isValid
                  ? AppTheme.error.withValues(alpha: 0.4)
                  : Colors.white.withValues(alpha: 0.08),
        ),
      ),
      child: Row(
        children: [
          Icon(
            entry.isComment
                ? Icons.comment
                : isValid
                    ? Icons.check_circle
                    : Icons.error,
            color: entry.isComment
                ? AppTheme.textMutedOf(context)
                : isValid
                    ? AppTheme.success
                    : AppTheme.error,
            size: 16,
          ),
          const SizedBox(width: 8),
          Expanded(
            child: TextFormField(
              // Ключ привязан к id записи + флагу комментария, чтобы при
              // переключении «домен/комментарий» поле пересоздавалось с новым
              // initialValue, а при удалении соседних строк состояние не
              // «съезжало» на другую запись.
              key: ValueKey('field_${entry.id}_${entry.isComment}'),
              initialValue: entry.value,
              enabled: !entry.isComment,
              style: TextStyle(
                fontSize: 13,
                color: entry.isComment ? AppTheme.textMutedOf(context) : AppTheme.textPrimaryOf(context),
                fontStyle: entry.isComment ? FontStyle.italic : null,
              ),
                decoration: InputDecoration(
                hintText: entry.isComment
                    ? AppLocalizations.of(context).commentHint
                    : AppLocalizations.of(context).listEntryHint,
                hintStyle: TextStyle(color: AppTheme.textMutedOf(context)),
                border: InputBorder.none,
                contentPadding: EdgeInsets.zero,
              ),
              onChanged: (value) => onChanged(entry.copyWith(value: value)),
            ),
          ),
          GlowIconButton(
            icon: entry.isComment ? Icons.code : Icons.comment,
            color: AppTheme.textSecondaryOf(context),
            size: 32,
            onTap: onToggleComment,
          ),
          GlowIconButton(
            icon: Icons.delete_outline,
            color: AppTheme.error,
            size: 32,
            onTap: onDelete,
          ),
        ],
      ),
    );
  }
}
