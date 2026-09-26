# Paper uninstaller

The installed `uninstall.exe --uninstall` now opens a dedicated paper-style UI.
Windows Apps & Features already points at this entry. Install/repair with the
new setup first to replace an older installed uninstaller. The downloaded setup
is not a standalone remover: the elevated worker accepts only the exact
protected installed uninstaller.

## Choices and scope

- Settings/profiles: current-user data in LocalAppData/vlarpsu/Obsession,
  RoamingAppData/Obsession, and the legacy LocalAppData/Obsession directory.
- Cache/logs: known cache entries in those directories plus Local/Roaming
  `com.vlarpsu.obsession` and `com.vlarpsu.obsession.setup` WebView data.
- Temporary: only `obsession-installer-safety-PID-HEX-ATTEMPT.ps1` files created
  by the setup's TempArtifacts helper, not all Temp or arbitrary prefix matches.
- Retired `bin` directories in the three data roots are removed regardless of
  the keep-settings choice. Machine files, service, protected runtime state,
  owned shortcuts and owned current-user registry pointers are always removed.

No other users' profiles, shared WebView2 runtime, manually saved downloads,
Windows event history, or arbitrary user exports are scanned/deleted. This is
not a forensic eraser. Locked application files are listed in the result;
machine-owned locked files use the existing reboot-deletion mechanism.

## Очистка кэша окна после закрытия

Если выбран кэш, профили `com.vlarpsu.obsession.setup` в Local/Roaming AppData
не удаляются при работающем WebView. После успешного машинного удаления экран
сообщает о незавершённой очистке и предлагает закрыть окно. `run_return`
завершает WebView, после чего нативная часть удаляет только эти два профиля.
Она повторяет проверку путей и очистку до 20 раз с интервалом 500 мс. Чужие
процессы не завершаются. Если файлы всё ещё заняты, появляется нативное
предупреждение; автоматическое удаление этих остатков после перезагрузки
не обещается. При аварийном завершении процесса отложенная очистка не выполнится.

Настройки и остальные выбранные данные очищаются по исходному плану до
экрана завершения. Если кэш не выбран, отложенная очистка не назначается.

Проверка исправления: 77 Rust-тестов пройдены (1 пакетный тест пропущен),
включая занятой файл, повтор после освобождения, данные закрытия WebView,
соседние файлы и выбор категорий; 5 frontend-тестов и production build пройдены.
Живое удаление на компьютере разработчика для этой проверки не запускалось.

## Safety

The UI sends three booleans, never deletion paths. Known folders are resolved
for the interactive user via Windows Shell; user-data cleanup stays outside the
elevated worker. Reparse-point ancestry is rejected. File enumeration is bounded,
files are removed individually, and directories are removed only when empty.
New or locked entries are reported rather than hidden by recursive deletion.
Data deletion is permanent and requires the explicit confirmation screen.

Before machine teardown the installed app is stopped by the existing exact-path
helper. The authenticated service's hosts snapshot is checked; active managed
hosts changes are uninstalled through its preserving merge operation. If this
cannot be confirmed, teardown stops and the UI asks for repair instead of
discarding recovery data. Missing/unavailable services require repair first.

Without WebView2, a native Yes/No/Cancel prompt offers full cleanup or retaining
settings. No selects retaining settings; Cancel performs no removal.

## Verification

74 Rust tests passed (one package test intentionally ignored in the normal suite).
The six new cleanup tests cover narrow temporary names, cache classification,
switch combinations, exact fixture boundaries, new-file reporting and refusal
of relative/escaped paths. Frontend production build passed. The safe browser
preview (`?preview=uninstall`) was exercised through choice, confirmation,
progress and completion; it never invokes uninstall commands.

Live elevated uninstall/reboot testing was not performed on the user's machine.
Before distributing broadly, validate install → keep settings → reinstall →
full uninstall in a disposable Windows VM, including UAC cancellation, missing
service, WebView locks and externally edited hosts.
