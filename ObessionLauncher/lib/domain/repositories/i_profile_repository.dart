import '../../core/errors/result.dart';
import '../entities/profile.dart';

/// Репозиторий для управления профилями.
abstract class IProfileRepository {
  Future<Result<List<Profile>>> getProfiles();
  Future<Result<Profile?>> getActiveProfile();
  Future<Result<void>> saveProfile(Profile profile);
  Future<Result<void>> deleteProfile(String id);
  Future<Result<void>> setActiveProfile(String id);
  Future<Result<Profile>> createProfile(String name);
}
