/// Конфигурация DPI-обхода для одной категории.
class DpiConfig {
  final String category;
  final String configFile;

  const DpiConfig({
    required this.category,
    required this.configFile,
  });

  DpiConfig copyWith({
    String? category,
    String? configFile,
  }) =>
      DpiConfig(
        category: category ?? this.category,
        configFile: configFile ?? this.configFile,
      );

  @override
  String toString() => 'DpiConfig($category: $configFile)';
}
