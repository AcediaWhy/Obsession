import '../../core/errors/result.dart';
import '../entities/hosts_config.dart';

/// Репозиторий для управления hosts-файлом и ИИ-обходом.
abstract class IHostsRepository {
  Future<Result<String>> readHosts();
  Future<Result<bool>> isInstalled(AiProvider provider);
  Future<Result<HostsConfig>> checkStatus(AiProvider provider);
  Future<Result<void>> install(AiProvider provider);
  Future<Result<void>> uninstall();
}
