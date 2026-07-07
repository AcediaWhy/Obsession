// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appName => 'Obsession';

  @override
  String get dpiTitle => 'DPI bypass';

  @override
  String get dpiSubtitle => 'Manage bypass blocking via WinDivert.';

  @override
  String get categories => 'Categories';

  @override
  String get canSelectMultiple => 'Multiple can be selected';

  @override
  String get config => 'Config';

  @override
  String get autoSelect => 'Auto-select';

  @override
  String get test => 'Test';

  @override
  String testingConfig(int current, int total, String config) {
    return 'Testing $current/$total: $config';
  }

  @override
  String testingCategory(int current, int total, String config) {
    return 'Auto-selecting category $current/$total: $config';
  }

  @override
  String get start => 'START';

  @override
  String get stop => 'Stop';

  @override
  String get active => 'ACTIVE';

  @override
  String get inactive => 'INACTIVE';

  @override
  String get activeShort => 'Active';

  @override
  String get inactiveShort => 'Stopped';

  @override
  String get protectionActive => 'Protection active';

  @override
  String get protectionStopped => 'Stopped';

  @override
  String get working => 'Working';

  @override
  String get notWorking => 'Not working';

  @override
  String get settings => 'Settings';

  @override
  String get settingsSubtitle => 'Customize appearance and behavior.';

  @override
  String get appearance => 'Appearance';

  @override
  String get themePreset => 'Theme preset';

  @override
  String get themeGroupDark => 'Dark';

  @override
  String get themeGroupLight => 'Light';

  @override
  String get backgroundMode => 'Background mode';

  @override
  String get backgroundModeAuto => 'Auto (from theme)';

  @override
  String get accentColor => 'Accent color';

  @override
  String get glowIntensity => 'Glow intensity';

  @override
  String get blurStrength => 'Glass blur strength';

  @override
  String get backgroundSpeed => 'Background animation speed';

  @override
  String get particleDensity => 'Particle density';

  @override
  String get animationsEnabled => 'Animations enabled';

  @override
  String get blackHoleEnabled => 'Black hole background';

  @override
  String get blackHole => 'Black hole';

  @override
  String get blackHoleRadius => 'Black hole radius';

  @override
  String get blackHoleDiskBrightness => 'Accretion disk brightness';

  @override
  String get blackHoleLensIntensity => 'Gravitational lensing intensity';

  @override
  String get behavior => 'Behavior';

  @override
  String get minimizeToTray => 'Minimize to tray on close';

  @override
  String get autostart => 'Autostart on system boot';

  @override
  String get resetSettings => 'Reset to defaults';

  @override
  String get resetConfirmTitle => 'Reset settings?';

  @override
  String get resetConfirmMessage =>
      'All customization settings will be reset to defaults.';

  @override
  String get cancel => 'Cancel';

  @override
  String get reset => 'Reset';

  @override
  String get yes => 'Yes';

  @override
  String get no => 'No';

  @override
  String get minimize => 'Minimize';

  @override
  String get maximize => 'Maximize';

  @override
  String get restore => 'Restore';

  @override
  String get close => 'Close';

  @override
  String get language => 'Language';

  @override
  String get russian => 'Russian';

  @override
  String get english => 'English';

  @override
  String get liveLog => 'Live log';

  @override
  String get logTitle => 'LOG';

  @override
  String get waitingToStart => 'Waiting to start...';

  @override
  String get statistics => 'Statistics';

  @override
  String get status => 'Status';

  @override
  String get dpiBypass => 'DPI bypass';

  @override
  String get telegramProxy => 'Telegram proxy';

  @override
  String get hosts => 'Hosts';

  @override
  String get activeProcesses => 'Active processes';

  @override
  String activeProcessesCount(int count) {
    return '$count processes';
  }

  @override
  String get selectedConfigs => 'Selected configs';

  @override
  String get version => 'Version';

  @override
  String versionWithValue(String version) {
    return 'Version: $version';
  }

  @override
  String get uptime => 'Uptime';

  @override
  String get noActiveProcesses => 'No active processes';

  @override
  String get selectedConfigsTitle => 'SELECTED CONFIGS';

  @override
  String get restart => 'Restart';

  @override
  String get configFolder => 'Config folder';

  @override
  String get singularityStatus => 'SINGULARITY STATUS';

  @override
  String get installed => 'Installed';

  @override
  String get notInstalled => 'Not installed';

  @override
  String get logEmpty => 'Log is empty';

  @override
  String get menu => 'MENU';

  @override
  String get aiTitle => 'AI bypass';

  @override
  String get aiSubtitle => 'Unlock ChatGPT, Claude, Gemini via hosts file.';

  @override
  String get provider => 'PROVIDER';

  @override
  String get install => 'Install';

  @override
  String get update => 'Update';

  @override
  String get uninstall => 'Uninstall';

  @override
  String get check => 'Check';

  @override
  String get installedUpToDate => 'Installed, up to date';

  @override
  String get installedOutdated => 'Installed, but outdated';

  @override
  String get aiNotInstalled => 'Not installed';

  @override
  String get uninstallAiTitle => 'Remove AI bypass?';

  @override
  String get uninstallAiMessage =>
      'The hosts file will be restored to its default value.';

  @override
  String get telegramTitle => 'Telegram bypass';

  @override
  String get telegramSubtitle =>
      'MTProto WebSocket bridge to unblock Telegram Desktop.';

  @override
  String get tgProxyNotFoundTitle => 'TgWsProxy.exe not found';

  @override
  String get tgProxyNotFoundMessage =>
      'Place TgWsProxy_windows.exe in the bin/ folder and restart the app.';

  @override
  String get proxySettings => 'PROXY SETTINGS';

  @override
  String get port => 'Port';

  @override
  String get fakeTlsDomainOptional => 'Fake TLS domain (opt.)';

  @override
  String get notSet => 'not set';

  @override
  String get connectionLink => 'CONNECTION LINK';

  @override
  String get openInTelegram => 'Open in Telegram';

  @override
  String get copy => 'Copy';

  @override
  String get instructions => 'INSTRUCTIONS';

  @override
  String get telegramInstructions =>
      '1. Press START to launch the proxy\n2. Press «Open in Telegram» or copy the link\n3. In Telegram: Settings → Advanced → Connection type → Proxy';

  @override
  String get listsTitle => 'List editor';

  @override
  String get listsSubtitle => 'Edit domains and IPs for bypassing blocks.';

  @override
  String get importListDialogTitle => 'Import list';

  @override
  String get exportListDialogTitle => 'Export list';

  @override
  String get selectListHint => 'Select a list';

  @override
  String entriesCount(int count) {
    return '$count entries';
  }

  @override
  String get domain => 'Domain';

  @override
  String get ip => 'IP';

  @override
  String get comment => 'Comment';

  @override
  String get save => 'Save';

  @override
  String get clearListTitle => 'Clear the list?';

  @override
  String get clearListMessage =>
      'All entries will be deleted. This action cannot be undone.';

  @override
  String get clear => 'Clear';

  @override
  String get removeDuplicates => 'Remove duplicates';

  @override
  String get listEntryHint => 'Domain or IP';

  @override
  String get commentHint => 'Comment';

  @override
  String get profilesTitle => 'Profiles';

  @override
  String get profilesSubtitle => 'Saved sets of settings.';

  @override
  String get profile => 'Profile';

  @override
  String get newProfile => 'New profile';

  @override
  String get profileName => 'Profile name';

  @override
  String get create => 'Create';

  @override
  String get delete => 'Delete';

  @override
  String get deleteProfileTitle => 'Delete profile?';

  @override
  String deleteProfileMessage(String name) {
    return 'Profile \"$name\" will be permanently deleted.';
  }

  @override
  String get activate => 'Activate';

  @override
  String profileCategoriesWithProvider(int count, String provider) {
    return '$count categories · $provider';
  }

  @override
  String get noProfiles => 'No saved profiles';

  @override
  String get copyLink => 'Copy link';

  @override
  String get proxyPort => 'Proxy port';

  @override
  String portWithValue(int port) {
    return 'Port $port';
  }

  @override
  String get fakeTlsDomain => 'Fake TLS domain';

  @override
  String get updateAvailable => 'Update available';

  @override
  String versionDate(String date) {
    return 'Published: $date';
  }

  @override
  String get noDescription => 'No description';

  @override
  String get downloadingUpdate => 'Downloading update...';

  @override
  String get download => 'Download';

  @override
  String get later => 'Later';

  @override
  String get folder => 'Folder';

  @override
  String get runInstaller => 'Run installer';

  @override
  String get fileHash => 'SHA256';

  @override
  String get hashHint =>
      'File downloaded. Compare SHA256 with the GitHub release before running.';

  @override
  String errorWithMessage(String error) {
    return 'Error: $error';
  }

  @override
  String get onboardingTitle => 'Welcome to Obsession';

  @override
  String get onboardingSubtitle =>
      'DPI and blocking bypass with a cosmic interface. Press below to begin.';

  @override
  String get onboardingContinue => 'Continue';

  @override
  String get onboardingIntro =>
      'A cosmic launcher for bypassing blocks. DPI, AI and Telegram — all in one window.';

  @override
  String get onboardingDpi =>
      'Auto-select WinDivert configurations for Discord, YouTube, games and other services.';

  @override
  String get onboardingAi =>
      'Unlock ChatGPT, Claude and Gemini by updating the hosts file.';

  @override
  String get onboardingTelegram =>
      'Local MTProto WebSocket proxy to unblock Telegram Desktop.';

  @override
  String get onboardingProfiles =>
      'Save settings and switch between them with one click.';

  @override
  String get onboardingStyleTitle => 'Customize your style';

  @override
  String get onboardingStyle =>
      'Accent color, glow, black hole and animations to match your mood.';

  @override
  String get skip => 'Skip';

  @override
  String get next => 'Next';

  @override
  String get getStarted => 'Get started';

  @override
  String get open => 'Open';

  @override
  String get availability => 'Availability';

  @override
  String latencyMs(int ms) {
    return '$ms ms';
  }

  @override
  String get unreachable => 'unreachable';

  @override
  String get checking => 'checking...';

  @override
  String get emergencyStop => 'Emergency stop';

  @override
  String get emergencyStopTitle => 'Stop everything?';

  @override
  String get emergencyStopMessage =>
      'DPI, Telegram proxy and hosts rules will be stopped and removed. Continue?';

  @override
  String get saveLogToFile => 'Save log to file';

  @override
  String logSavedTo(String path) {
    return 'Log saved: $path';
  }

  @override
  String get logSaveFailed => 'Failed to save log';

  @override
  String get autoDetected => 'Auto-detected';

  @override
  String get autoDetectedSubtitle =>
      'Domains that Zapret automatically identified as blocked';

  @override
  String get noAutoDetected =>
      'Nothing detected yet. The list builds automatically as you use bypass.';

  @override
  String get clearAutoDetected => 'Clear lists';

  @override
  String get autoDetectedCleared => 'Auto-detected lists cleared';

  @override
  String domainsCount(int count) {
    return '$count domains';
  }

  @override
  String get orphanDetectedTitle => 'Orphaned processes detected';

  @override
  String orphanDetectedMessage(int count) {
    return 'Found $count winws.exe process(es) from a previous launch (possibly after a crash). They may interfere with bypass startup. Stop them?\n\nWarning: this will stop ALL winws.exe processes in the system, including those launched by other DPI tools.';
  }

  @override
  String get orphanDetectedClear => 'Stop';

  @override
  String get orphanDetectedIgnore => 'Ignore';

  @override
  String get orphanDetectedNone => 'No orphaned processes detected.';

  @override
  String get orphanDetectedCleared => 'Orphaned processes stopped.';

  @override
  String get orphanDetectedCleanFailed => 'Failed to stop orphaned processes.';
}
