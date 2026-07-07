import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../repositories/i_admin_repository.dart';

class CheckAdminUseCase implements UseCase<bool, NoParams> {
  final IAdminRepository repository;
  const CheckAdminUseCase(this.repository);

  @override
  Future<Result<bool>> call(NoParams params) async {
    final isAdmin = await repository.isAdmin();
    return Success(isAdmin);
  }
}

class RelaunchAsAdminUseCase implements UseCase<void, NoParams> {
  final IAdminRepository repository;
  const RelaunchAsAdminUseCase(this.repository);

  @override
  Future<Result<void>> call(NoParams params) => repository.relaunchAsAdmin();
}
