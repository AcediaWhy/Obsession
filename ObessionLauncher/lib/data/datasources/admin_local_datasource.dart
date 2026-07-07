import 'dart:io' as io;

/// Проверка и запрос прав администратора.
class AdminLocalDataSource {
  AdminLocalDataSource();

  /// Надёжная проверка elevated-токена через whoami.
  /// В отличие от `net session` не зависит от сервиса LanmanServer.
  Future<bool> isAdmin() async {
    try {
      final result = await io.Process.run(
        'powershell',
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          // Проверяем членство в роли Administrator у текущего токена.
          // Возвращаем результат через код выхода (exit 0/1).
          '\$id=[System.Security.Principal.WindowsIdentity]::GetCurrent();'
          '\$p=New-Object System.Security.Principal.WindowsPrincipal(\$id);'
          'if(\$p.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)){exit 0}else{exit 1}',
        ],
        stdoutEncoding: null,
        stderrEncoding: null,
      ).timeout(const Duration(seconds: 15));
      return result.exitCode == 0;
    } catch (_) {
      // Fallback на net session если PowerShell недоступен.
      try {
        final result = await io.Process.run(
          'net',
          ['session'],
          stdoutEncoding: null,
          stderrEncoding: null,
        ).timeout(const Duration(seconds: 10));
        return result.exitCode == 0;
      } catch (_) {
        return false;
      }
    }
  }

  /// Перезапуск приложения с правами администратора.
  /// В debug-режиме (когда манифест не применяется) используется как fallback.
  Future<bool> relaunchAsAdmin() async {
    try {
      final exePath = io.Platform.executable;
      final safeExePath = exePath.replaceAll("'", "''");
      final result = await io.Process.run(
        'powershell',
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          "try { \$p = Start-Process -FilePath '$safeExePath' -Verb RunAs -PassThru -ErrorAction Stop; "
              "if (\$p) { exit 0 } else { exit 1 } } catch { exit 2 }",
        ],
      ).timeout(const Duration(seconds: 60));
      // 0 = успех, 2 = UAC отклонён/ошибка, 1 = не запущено.
      return result.exitCode == 0;
    } catch (_) {
      return false;
    }
  }
}
