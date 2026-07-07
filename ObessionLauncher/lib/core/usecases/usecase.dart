import '../errors/result.dart';

/// Базовый интерфейс для Use Case.
abstract class UseCase<T, Params> {
  Future<Result<T>> call(Params params);
}

/// Для use case без параметров.
class NoParams {
  const NoParams();
}
