/// Информация о запущенном DPI-процессе.
class ProcessInfo {
  final int pid;
  final String category;
  final String configFile;
  final DateTime startedAt;

  const ProcessInfo({
    required this.pid,
    required this.category,
    required this.configFile,
    required this.startedAt,
  });

  ProcessInfo copyWith({
    int? pid,
    String? category,
    String? configFile,
    DateTime? startedAt,
  }) =>
      ProcessInfo(
        pid: pid ?? this.pid,
        category: category ?? this.category,
        configFile: configFile ?? this.configFile,
        startedAt: startedAt ?? this.startedAt,
      );
}
