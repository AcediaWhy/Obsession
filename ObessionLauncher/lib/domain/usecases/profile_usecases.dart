import '../../core/errors/result.dart';
import '../../core/usecases/usecase.dart';
import '../entities/profile.dart';
import '../repositories/i_profile_repository.dart';

class GetProfilesUseCase implements UseCase<List<Profile>, NoParams> {
  final IProfileRepository repository;
  const GetProfilesUseCase(this.repository);

  @override
  Future<Result<List<Profile>>> call(NoParams params) => repository.getProfiles();
}

class GetActiveProfileUseCase implements UseCase<Profile?, NoParams> {
  final IProfileRepository repository;
  const GetActiveProfileUseCase(this.repository);

  @override
  Future<Result<Profile?>> call(NoParams params) => repository.getActiveProfile();
}

class CreateProfileUseCase implements UseCase<Profile, String> {
  final IProfileRepository repository;
  const CreateProfileUseCase(this.repository);

  @override
  Future<Result<Profile>> call(String name) => repository.createProfile(name);
}

class SaveProfileUseCase implements UseCase<void, Profile> {
  final IProfileRepository repository;
  const SaveProfileUseCase(this.repository);

  @override
  Future<Result<void>> call(Profile profile) => repository.saveProfile(profile);
}

class DeleteProfileUseCase implements UseCase<void, String> {
  final IProfileRepository repository;
  const DeleteProfileUseCase(this.repository);

  @override
  Future<Result<void>> call(String id) => repository.deleteProfile(id);
}

class SwitchProfileUseCase implements UseCase<void, String> {
  final IProfileRepository repository;
  const SwitchProfileUseCase(this.repository);

  @override
  Future<Result<void>> call(String id) => repository.setActiveProfile(id);
}
