import 'package:flutter_test/flutter_test.dart';
import 'package:obsession/domain/entities/list_entry.dart';
import 'package:obsession/domain/usecases/list_usecases.dart';

void main() {
  group('ValidateListEntryUseCase', () {
    const useCase = ValidateListEntryUseCase();

    test('validates simple domains', () async {
      final result = await useCase(ListEntry(value: 'example.com'));
      expect(result.valueOrNull, isTrue);
    });

    test('validates subdomains', () async {
      final result = await useCase(ListEntry(value: 'sub.domain.example.com'));
      expect(result.valueOrNull, isTrue);
    });

    test('validates IPv4 addresses', () async {
      final result = await useCase(ListEntry(value: '192.168.1.1'));
      expect(result.valueOrNull, isTrue);
    });

    test('rejects invalid domain with spaces', () async {
      final result = await useCase(ListEntry(value: 'not a domain'));
      expect(result.valueOrNull, isFalse);
    });

    test('rejects empty value', () async {
      final result = await useCase(ListEntry(value: ''));
      expect(result.valueOrNull, isFalse);
    });

    test('accepts comments as always valid', () async {
      final result = await useCase(ListEntry.comment('any comment here'));
      expect(result.valueOrNull, isTrue);
    });

    test('accepts blank entries as valid', () async {
      final result = await useCase(ListEntry.blank());
      expect(result.valueOrNull, isTrue);
    });
  });

  group('ListEntry', () {
    test('blank entry is blank', () {
      expect(ListEntry.blank().isBlank, isTrue);
    });

    test('comment entry is comment', () {
      expect(ListEntry.comment('note').isComment, isTrue);
    });

    test('domain entry is not blank nor comment', () {
      final entry = ListEntry(value: 'example.com');
      expect(entry.isBlank, isFalse);
      expect(entry.isComment, isFalse);
    });

    test('copyWith preserves comment', () {
      final entry = ListEntry(value: 'example.com', comment: 'note');
      final updated = entry.copyWith(value: 'new.com');
      expect(updated.value, 'new.com');
      expect(updated.comment, 'note');
    });
  });
}
