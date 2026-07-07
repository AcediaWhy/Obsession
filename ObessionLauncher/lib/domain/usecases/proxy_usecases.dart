import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../entities/proxy_config.dart';
import '../repositories/i_proxy_repository.dart';

class CheckProxyAvailableUseCase implements UseCase<bool, NoParams> {
  final IProxyRepository repository;
  const CheckProxyAvailableUseCase(this.repository);

  @override
  Future<Result<bool>> call(NoParams params) async {
    final available = await repository.isAvailable();
    return Success(available);
  }
}

class StartProxyUseCase implements UseCase<void, ProxyConfig> {
  final IProxyRepository repository;
  const StartProxyUseCase(this.repository);

  @override
  Future<Result<void>> call(ProxyConfig config) => repository.start(config);
}

class StopProxyUseCase implements UseCase<void, NoParams> {
  final IProxyRepository repository;
  const StopProxyUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.stop();
}

class GetProxyLinkUseCase implements UseCase<String, NoParams> {
  final IProxyRepository repository;
  const GetProxyLinkUseCase(this.repository);

  @override
  Future<Result<String>> call(NoParams params) => repository.getProxyLink();
}

class OpenProxyInTelegramUseCase implements UseCase<void, NoParams> {
  final IProxyRepository repository;
  const OpenProxyInTelegramUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.openInTelegram();
}

class CopyProxyLinkUseCase implements UseCase<void, NoParams> {
  final IProxyRepository repository;
  const CopyProxyLinkUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.copyLink();
}
