import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../providers/settings_provider.dart';

/// Shimmer-эффект для загрузочных скелетонов.
class ShimmerSkeleton extends ConsumerWidget {
  final double width;
  final double height;
  final double borderRadius;

  const ShimmerSkeleton({
    super.key,
    required this.width,
    required this.height,
    this.borderRadius = 12,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(settingsProvider);
    final accent = settings.accentColor;

    return Container(
      width: width,
      height: height,
      decoration: BoxDecoration(
        color: Colors.white.withValues(alpha: 0.04),
        borderRadius: BorderRadius.circular(borderRadius),
      ),
    )
        .animate(onPlay: (c) => c.repeat())
        .shimmer(
          duration: 1200.ms,
          color: accent.withValues(alpha: 0.15),
        );
  }
}

/// Загрузочный скелетон для списка.
class ShimmerList extends StatelessWidget {
  final int itemCount;
  const ShimmerList({super.key, this.itemCount = 6});

  @override
  Widget build(BuildContext context) {
    return ListView.builder(
      itemCount: itemCount,
      physics: const NeverScrollableScrollPhysics(),
      itemBuilder: (context, index) => Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: Row(
          children: [
            const ShimmerSkeleton(width: 40, height: 40, borderRadius: 10),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  ShimmerSkeleton(width: double.infinity, height: 14, borderRadius: 6),
                  const SizedBox(height: 8),
                  ShimmerSkeleton(width: 120, height: 10, borderRadius: 6),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// Загрузочный скелетон для карточки.
class ShimmerCard extends StatelessWidget {
  const ShimmerCard({super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: ShimmerSkeleton(
        width: double.infinity,
        height: 80,
        borderRadius: 20,
      ),
    );
  }
}

/// Загрузочный скелетон для панели настроек.
class ShimmerSettingsPanel extends StatelessWidget {
  const ShimmerSettingsPanel({super.key});

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        ShimmerSkeleton(width: 120, height: 14, borderRadius: 6),
        const SizedBox(height: 16),
        ShimmerSkeleton(width: double.infinity, height: 48, borderRadius: 12),
        const SizedBox(height: 12),
        ShimmerSkeleton(width: double.infinity, height: 48, borderRadius: 12),
        const SizedBox(height: 12),
        ShimmerSkeleton(width: double.infinity, height: 48, borderRadius: 12),
      ],
    );
  }
}
