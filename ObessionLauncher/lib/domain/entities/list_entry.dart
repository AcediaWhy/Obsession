/// Запись в списке доменов/IP.
class ListEntry {
  /// Стабильный идентификатор записи в пределах сессии редактора.
  ///
  /// Нужен, чтобы результаты валидации и состояние строк UI привязывались
  /// к конкретной записи, а не к её позиции в списке. Позиционные ключи
  /// (индексы) рассинхронизируются при удалении/вставке/сортировке — это
  /// приводило к «съезжающему» тексту и неверным иконкам валидации.
  final int id;
  final String value;
  final String? comment;
  final bool isComment;
  final bool isBlank;

  const ListEntry({
    required this.value,
    this.comment,
    this.isComment = false,
    this.isBlank = false,
    int? id,
  }) : id = id ?? -1;

  static int _seq = 0;

  /// Выдаёт новую запись с уникальным [id] (для только что созданных строк).
  static int _nextId() => --_seq;

  factory ListEntry.domain(String domain, {String? comment, int? id}) => ListEntry(
        value: domain,
        comment: comment,
        id: id ?? _nextId(),
      );

  factory ListEntry.ip(String ip, {String? comment, int? id}) => ListEntry(
        value: ip,
        comment: comment,
        id: id ?? _nextId(),
      );

  factory ListEntry.comment(String text) => ListEntry(
        value: text,
        isComment: true,
        id: _nextId(),
      );

  factory ListEntry.blank() => ListEntry(
        value: '',
        isBlank: true,
        id: _nextId(),
      );

  /// Гарантирует, что у записи есть валидный [id]; выдаёт новый, если его ещё нет.
  ListEntry ensureId() => id == -1 ? withNewId() : this;

  ListEntry withNewId() => copyWith(id: _nextId());

  ListEntry copyWith({
    String? value,
    String? comment,
    bool? isComment,
    bool? isBlank,
    int? id,
  }) =>
      ListEntry(
        value: value ?? this.value,
        comment: comment ?? this.comment,
        isComment: isComment ?? this.isComment,
        isBlank: isBlank ?? this.isBlank,
        id: id ?? this.id,
      );

  @override
  String toString() {
    if (isBlank) return '';
    if (isComment) return '# $value';
    if (comment != null && comment!.isNotEmpty) return '$value # $comment';
    return value;
  }
}
