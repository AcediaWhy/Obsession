/// Базовый класс для всех ошибок приложения.
abstract class FailureBase {
  final String message;
  final Object? error;
  final StackTrace? stackTrace;

  const FailureBase(this.message, {this.error, this.stackTrace});

  @override
  String toString() => message;
}

/// Нет прав администратора.
class AdminFailure extends FailureBase {
  const AdminFailure({super.error, super.stackTrace})
      : super('Требуются права администратора');
}

/// Ошибка работы с процессом winws / TgWsProxy.
class ProcessFailure extends FailureBase {
  const ProcessFailure(super.message, {super.error, super.stackTrace});
}

/// Ошибка работы с файлами (hosts, конфиги, бэкапы).
class FileFailure extends FailureBase {
  const FileFailure(super.message, {super.error, super.stackTrace});
}

/// Ошибка сети (загрузка hosts, обновления).
class NetworkFailure extends FailureBase {
  const NetworkFailure(super.message, {super.error, super.stackTrace});
}

/// Ошибка обновления приложения.
class UpdateFailure extends FailureBase {
  const UpdateFailure(super.message, {super.error, super.stackTrace});
}

/// Ошибка настроек / профилей.
class SettingsFailure extends FailureBase {
  const SettingsFailure(super.message, {super.error, super.stackTrace});
}

/// Ошибка валидации (списки, порты, домены).
class ValidationFailure extends FailureBase {
  const ValidationFailure(super.message, {super.error, super.stackTrace});
}

/// Неизвестная ошибка.
class UnknownFailure extends FailureBase {
  const UnknownFailure({super.error, super.stackTrace})
      : super('Неизвестная ошибка');
}
