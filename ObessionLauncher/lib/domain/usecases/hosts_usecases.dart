import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../entities/hosts_config.dart';
import '../repositories/i_hosts_repository.dart';

class CheckHostsStatusUseCase implements UseCase<HostsConfig, AiProvider> {
  final IHostsRepository repository;
  const CheckHostsStatusUseCase(this.repository);

  @override
  Future<Result<HostsConfig>> call(AiProvider provider) =>
      repository.checkStatus(provider);
}

class InstallHostsUseCase implements UseCase<void, AiProvider> {
  final IHostsRepository repository;
  const InstallHostsUseCase(this.repository);

  @override
  Future<Result<void>> call(AiProvider provider) => repository.install(provider);
}

class UninstallHostsUseCase implements UseCase<void, NoParams> {
  final IHostsRepository repository;
  const UninstallHostsUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.uninstall();
}
