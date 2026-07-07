import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../repositories/i_autostart_repository.dart';

class CheckAutostartUseCase implements UseCase<bool, NoParams> {
  final IAutostartRepository repository;
  const CheckAutostartUseCase(this.repository);

  @override
  Future<Result<bool>> call(NoParams params) async {
    final enabled = await repository.isEnabled();
    return Success(enabled);
  }
}

class EnableAutostartUseCase implements UseCase<void, NoParams> {
  final IAutostartRepository repository;
  const EnableAutostartUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.enable();
}

class DisableAutostartUseCase implements UseCase<void, NoParams> {
  final IAutostartRepository repository;
  const DisableAutostartUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.disable();
}

class ToggleAutostartUseCase implements UseCase<bool, NoParams> {
  final IAutostartRepository repository;
  const ToggleAutostartUseCase(this.repository);

  @override
  Future<Result<bool>> call(NoParams params) => repository.toggle();
}
