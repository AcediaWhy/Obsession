/// Конфигурация Telegram-прокси.
class ProxyConfig {
  final int port;
  final String? secret;
  final String fakeTlsDomain;
  final String proxyLink;

  const ProxyConfig({
    required this.port,
    this.secret,
    this.fakeTlsDomain = '',
    this.proxyLink = '',
  });

  factory ProxyConfig.defaults() => const ProxyConfig(
        port: 1443,
        fakeTlsDomain: '',
        proxyLink: '',
      );

  ProxyConfig copyWith({
    int? port,
    String? secret,
    String? fakeTlsDomain,
    String? proxyLink,
  }) =>
      ProxyConfig(
        port: port ?? this.port,
        secret: secret ?? this.secret,
        fakeTlsDomain: fakeTlsDomain ?? this.fakeTlsDomain,
        proxyLink: proxyLink ?? this.proxyLink,
      );
}
