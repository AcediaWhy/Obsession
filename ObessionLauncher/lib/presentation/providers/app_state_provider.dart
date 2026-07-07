import 'package:flutter_riverpod/flutter_riverpod.dart';

enum AppTab { dpi, ai, telegram, lists, profiles, settings }

final activeTabProvider = StateProvider<AppTab>((ref) => AppTab.dpi);

final adminStatusProvider = FutureProvider<bool>((ref) async {
  return true; // Логика проверки прав выполняется в main.dart до запуска UI.
});
