import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'l10n/app_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'core/constants/app_constants.dart';
import 'presentation/providers/autostart_provider.dart';
import 'presentation/providers/hosts_provider.dart';
import 'presentation/providers/proxy_provider.dart';
import 'presentation/providers/settings_provider.dart';
import 'presentation/providers/tray_provider.dart';
import 'presentation/screens/home_screen.dart';
import 'presentation/screens/onboarding_screen.dart';
import 'presentation/theme/app_theme.dart';
import 'presentation/widgets/crt_overlay.dart';


class ObsessionApp extends ConsumerStatefulWidget {
  const ObsessionApp({super.key});

  @override
  ConsumerState<ObsessionApp> createState() => _ObsessionAppState();
}

class _ObsessionAppState extends ConsumerState<ObsessionApp> {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      final settings = ref.read(settingsProvider);
      ref.read(hostsProvider.notifier).refreshStatus();
      ref.read(proxyProvider.notifier).checkAvailability();
      ref.read(autostartProvider.notifier);
      ref.read(trayProvider.notifier).init(locale: settings.locale);
    });
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(settingsProvider);
    final baseTheme = AppTheme.themeFrom(settings);

    return MaterialApp(
      title: AppConstants.appName,
      debugShowCheckedModeBanner: false,
      theme: baseTheme,
      locale: Locale(settings.locale),
      localizationsDelegates: const [
        AppLocalizations.delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      supportedLocales: const [
        Locale('ru'),
        Locale('en'),
      ],
      home: settings.hasCompletedOnboarding ? const HomeScreen() : const OnboardingScreen(),
      builder: (context, child) => CrtOverlay(child: child!),
    );
  }
}
