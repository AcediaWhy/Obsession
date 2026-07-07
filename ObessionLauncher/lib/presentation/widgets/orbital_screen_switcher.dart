import 'package:flutter/material.dart';

import '../providers/app_state_provider.dart';
import '../screens/ai_screen.dart';
import '../screens/dpi_screen.dart';
import '../screens/list_editor_screen.dart';
import '../screens/profile_screen.dart';
import '../screens/settings_screen.dart';
import '../screens/telegram_screen.dart';

/// Переключатель вкладок с плавной анимацией.
///
/// Использует [AnimatedSwitcher] с кастомным transitionBuilder:
/// исходящий экран уходит влево с затуханием,
/// входящий выезжает справа с появлением.
class OrbitalScreenSwitcher extends StatelessWidget {
  final AppTab activeTab;

  const OrbitalScreenSwitcher({super.key, required this.activeTab});

  @override
  Widget build(BuildContext context) {
    return AnimatedSwitcher(
      duration: const Duration(milliseconds: 450),
      reverseDuration: const Duration(milliseconds: 350),
      switchInCurve: Curves.easeOutCubic,
      switchOutCurve: Curves.easeInCubic,
      transitionBuilder: (child, animation) {
        return FadeTransition(
          opacity: animation,
          child: SlideTransition(
            position: Tween<Offset>(
              begin: const Offset(0.08, 0.0),
              end: Offset.zero,
            ).animate(animation),
            child: ScaleTransition(
              scale: Tween<double>(begin: 0.985, end: 1.0).animate(animation),
              child: child,
            ),
          ),
        );
      },
      child: _buildScreen(activeTab),
    );
  }

  Widget _buildScreen(AppTab tab) {
    return switch (tab) {
      AppTab.dpi => const DpiScreen(key: ValueKey('dpi')),
      AppTab.ai => const AiScreen(key: ValueKey('ai')),
      AppTab.telegram => const TelegramScreen(key: ValueKey('telegram')),
      AppTab.lists => const ListEditorScreen(key: ValueKey('lists')),
      AppTab.profiles => const ProfileScreen(key: ValueKey('profiles')),
      AppTab.settings => const SettingsScreen(key: ValueKey('settings')),
    };
  }
}
