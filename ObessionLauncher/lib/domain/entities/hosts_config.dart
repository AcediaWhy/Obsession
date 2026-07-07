/// Провайдер hosts для ИИ-обхода.
enum AiProvider {
  malw,
  geohide;

  String get label => switch (this) {
        malw => 'dns.malw.link',
        geohide => 'GeoHide',
      };

  String get hostsUrl => switch (this) {
        malw =>
            'https://raw.githubusercontent.com/ImMALWARE/dns.malw.link/refs/heads/master/hosts',
        geohide =>
            'https://github.com/Internet-Helper/GeoHideDNS/raw/refs/heads/main/hosts/hosts',
      };

  String get marker => switch (this) {
        malw => 'dns.malw.link',
        geohide => 'dns.geohide.ru',
      };
}

/// Статус установки ИИ-обхода.
enum HostsStatus { installed, outdated, notInstalled, offline }

/// Информация об установленном ИИ-обходе.
class HostsConfig {
  final AiProvider provider;
  final HostsStatus status;
  final String localVersion;
  final String remoteVersion;

  const HostsConfig({
    required this.provider,
    required this.status,
    this.localVersion = '',
    this.remoteVersion = '',
  });

  HostsConfig copyWith({
    AiProvider? provider,
    HostsStatus? status,
    String? localVersion,
    String? remoteVersion,
  }) =>
      HostsConfig(
        provider: provider ?? this.provider,
        status: status ?? this.status,
        localVersion: localVersion ?? this.localVersion,
        remoteVersion: remoteVersion ?? this.remoteVersion,
      );
}
