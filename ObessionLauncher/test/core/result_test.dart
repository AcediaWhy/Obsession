import 'package:flutter_test/flutter_test.dart';
import 'package:obsession/core/errors/failures.dart';
import 'package:obsession/core/errors/result.dart';

void main() {
  group('Result', () {
    test('Success holds value and reports isSuccess', () {
      const result = Success<int>(42);
      expect(result.isSuccess, isTrue);
      expect(result.isFailure, isFalse);
      expect(result.valueOrNull, 42);
      expect(result.failureOrNull, isNull);
    });

    test('Failure holds failure and reports isFailure', () {
      const failure = ProcessFailure('test error');
      const result = Failure<int>(failure);
      expect(result.isSuccess, isFalse);
      expect(result.isFailure, isTrue);
      expect(result.valueOrNull, isNull);
      expect(result.failureOrNull, isNotNull);
      expect(result.failureOrNull!.failure.message, 'test error');
    });

    test('when dispatches success branch', () {
      const result = Success<String>('ok');
      final value = result.when(
        success: (v) => 'got: $v',
        failure: (f) => 'error: ${f.message}',
      );
      expect(value, 'got: ok');
    });

    test('when dispatches failure branch', () {
      const result = Failure<String>(NetworkFailure('no network'));
      final value = result.when(
        success: (v) => 'got: $v',
        failure: (f) => 'error: ${f.message}',
      );
      expect(value, 'error: no network');
    });

    test('Failure preserves typed failure', () {
      const result = Failure<bool>(AdminFailure());
      expect(result.when(
        success: (_) => 'success',
        failure: (f) => f.runtimeType.toString(),
      ), 'AdminFailure');
    });
  });
}
