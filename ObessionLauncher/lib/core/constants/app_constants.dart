/// Глобальные константы приложения.
class AppConstants {
  AppConstants._();

  static const String appName = 'Obsession';
  static const String appVersion = '1.0.0';
  static const String appDataFolder = 'Obsession';

  static const String winwsExe = 'winws.exe';
  static const String tgProxyExe = 'TgWsProxy_windows.exe';

  static const String hostsPath = r'C:\Windows\System32\drivers\etc\hosts';

  static const int defaultProxyPort = 1443;
  static const int maxLogHistory = 200;

  static const String malwHostsUrl =
      'https://raw.githubusercontent.com/ImMALWARE/dns.malw.link/refs/heads/master/hosts';
  static const String geohideHostsUrl =
      'https://github.com/Internet-Helper/GeoHideDNS/raw/refs/heads/main/hosts/hosts';
  static const String additionalHostsUrl =
      'https://raw.githubusercontent.com/AvenCores/Goida-AI-Unlocker/refs/heads/main/additional_hosts.json';

  // Автообновление single EXE
  static const String githubOwner = 'VlarpSu';
  static const String githubRepo = 'Obsession';
  static const String githubApiLatestRelease =
      'https://api.github.com/repos/VlarpSu/Obsession/releases/latest';
  static const String singleExeAssetName = 'Obsession.exe';
}
