import 'package:flutter/material.dart';
import 'package:flutter_animate/flutter_animate.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../l10n/app_localizations.dart';
import '../providers/settings_provider.dart';
import '../theme/app_theme.dart';
import '../widgets/animated_background.dart';
import '../widgets/glow_widgets.dart';

class OnboardingScreen extends ConsumerStatefulWidget {
  const OnboardingScreen({super.key});

  @override
  ConsumerState<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends ConsumerState<OnboardingScreen> {
  final PageController _controller = PageController();
  int _currentPage = 0;

  List<_OnboardingPage> _pages(BuildContext context) {
    final l = AppLocalizations.of(context);
    return [
      _OnboardingPage(
        icon: Icons.shield_moon,
        title: l.appName,
        description: l.onboardingIntro,
      ),
      _OnboardingPage(
        icon: Icons.bolt,
        title: l.dpiTitle,
        description: l.onboardingDpi,
      ),
      _OnboardingPage(
        icon: Icons.smart_toy,
        title: l.aiTitle,
        description: l.onboardingAi,
      ),
      _OnboardingPage(
        icon: Icons.send,
        title: l.telegramTitle,
        description: l.onboardingTelegram,
      ),
      _OnboardingPage(
        icon: Icons.account_tree,
        title: l.profilesTitle,
        description: l.onboardingProfiles,
      ),
      _OnboardingPage(
        icon: Icons.palette,
        title: l.onboardingStyleTitle,
        description: l.onboardingStyle,
      ),
    ];
  }

  void _next() {
    if (_currentPage < _pages(context).length - 1) {
      _controller.nextPage(
        duration: const Duration(milliseconds: 400),
        curve: Curves.easeOutCubic,
      );
    } else {
      _finish();
    }
  }

  void _finish() {
    final settings = ref.read(settingsProvider);
    ref.read(settingsProvider.notifier).update(
      settings.copyWith(hasCompletedOnboarding: true),
    );
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final accent = settings.accentColor;
    final pages = _pages(context);

    return Scaffold(
      body: Stack(
        children: [
          const AnimatedBackground(),
          SafeArea(
            child: Column(
              children: [
                Expanded(
                  child: PageView.builder(
                    controller: _controller,
                    itemCount: pages.length,
                    onPageChanged: (index) => setState(() => _currentPage = index),
                    itemBuilder: (context, index) => _buildPage(pages[index], accent),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.all(24),
                  child: Row(
                    children: [
                      Row(
                        children: List.generate(pages.length, (index) {
                          return AnimatedContainer(
                            duration: 200.ms,
                            margin: const EdgeInsets.only(right: 8),
                            width: index == _currentPage ? 24 : 8,
                            height: 8,
                            decoration: BoxDecoration(
                              color: index == _currentPage
                                  ? accent
                                  : AppTheme.textMutedOf(context).withValues(alpha: 0.4),
                              borderRadius: BorderRadius.circular(4),
                            ),
                          );
                        }),
                      ),
                      const Spacer(),
                      if (_currentPage < pages.length - 1)
                        GlowButton(
                          label: AppLocalizations.of(context).skip,
                          color: AppTheme.textSecondaryOf(context),
                          isOutline: true,
                          onTap: _finish,
                        ),
                      const SizedBox(width: 12),
                      GlowButton(
                        label: _currentPage == pages.length - 1
                            ? AppLocalizations.of(context).getStarted
                            : AppLocalizations.of(context).next,
                        icon: _currentPage == pages.length - 1 ? Icons.rocket_launch : Icons.arrow_forward,
                        color: accent,
                        onTap: _next,
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildPage(_OnboardingPage page, Color accent) {
    return Padding(
      padding: const EdgeInsets.all(40),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Container(
            width: 120,
            height: 120,
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.topLeft,
                end: Alignment.bottomRight,
                colors: [accent, accent.withValues(alpha: 0.6)],
              ),
              borderRadius: BorderRadius.circular(30),
              boxShadow: [
                BoxShadow(
                  color: accent.withValues(alpha: 0.4),
                  blurRadius: 30,
                  spreadRadius: 4,
                ),
              ],
            ),
            child: Icon(page.icon, size: 56, color: Colors.white),
          ).animate().scale(duration: 500.ms, curve: Curves.easeOutBack),
          const SizedBox(height: 40),
          Text(
            page.title,
            style: TextStyle(
              fontSize: 32,
              fontWeight: FontWeight.bold,
              color: AppTheme.textPrimaryOf(context),
            ),
          ).animate().fadeIn(duration: 400.ms).slideY(begin: 0.1),
          const SizedBox(height: 16),
          Text(
            page.description,
            textAlign: TextAlign.center,
            style: TextStyle(
              fontSize: 15,
              color: AppTheme.textSecondaryOf(context),
              height: 1.6,
            ),
          ).animate(delay: 100.ms).fadeIn(duration: 400.ms).slideY(begin: 0.1),
        ],
      ),
    );
  }
}

class _OnboardingPage {
  final IconData icon;
  final String title;
  final String description;

  const _OnboardingPage({
    required this.icon,
    required this.title,
    required this.description,
  });
}
