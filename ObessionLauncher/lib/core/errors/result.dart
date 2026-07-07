import 'failures.dart';

/// Результат операции: либо успех с данными, либо ошибка.
sealed class Result<T> {
  const Result();

  bool get isSuccess => this is Success<T>;
  bool get isFailure => this is Failure<T>;

  T? get valueOrNull {
    return switch (this) {
      Success<T>(value: final v) => v,
      Failure<T>() => null,
    };
  }

  Failure<T>? get failureOrNull {
    return switch (this) {
      Success<T>() => null,
      Failure<T>(failure: final f) => Failure<T>(f),
    };
  }

  R when<R>({
    required R Function(T value) success,
    required R Function(FailureBase failure) failure,
  }) {
    return switch (this) {
      Success<T>(value: final v) => success(v),
      Failure<T>(failure: final f) => failure(f),
    };
  }
}

final class Success<T> extends Result<T> {
  final T value;
  const Success(this.value);
}

final class Failure<T> extends Result<T> {
  final FailureBase failure;
  const Failure(this.failure);
}
