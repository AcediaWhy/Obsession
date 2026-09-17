; Safety hooks for the Obsession NSIS payload.
;
; Tauri includes this file after utils.nsh and invokes the four NSIS_HOOK_*
; macros from its standard install/uninstall sections. The stock template still
; owns file copying, registry entries, shortcuts and removal of known files.

!define OBSESSION_OWNER_ID "com.vlarpsu.obsession"
!define OBSESSION_OWNER_MARKER ".obsession-install-owner"
!define OBSESSION_INSTALL_MUTEX "Local\com.vlarpsu.obsession.nsis.install.v1"

; Tauri's stock macro identifies processes only by filename. Obsession performs
; an exact executable-path check in installer-safety.ps1 instead, so an unrelated
; process named Obsession.exe is never terminated by this installer.
!macroundef CheckIfAppIsRunning
!macro CheckIfAppIsRunning executableName productName
!macroend

!macro OBSESSION_PREPARE_SAFETY_HELPER
  InitPluginsDir
  ; Tauri copies hooks.nsh to target/<profile>/nsis/<arch> before invoking
  ; makensis. Walk back to src-tauri so the helper is embedded from source.
  File /oname=$PLUGINSDIR\obsession-installer-safety.ps1 "${__FILEDIR__}\..\..\..\..\nsis\installer-safety.ps1"
  System::Call 'kernel32::SetEnvironmentVariable(t "OBSESSION_INSTALL_DIR", t "$INSTDIR") i.r0'
!macroend

; Runs one static action from the embedded helper and leaves its exit code in $0
; and captured output in $1. The install path is inherited through the process
; environment, avoiding command-line interpolation of a user-controlled path.
!macro OBSESSION_RUN_SAFETY_ACTION action
  System::Call 'kernel32::SetEnvironmentVariable(t "OBSESSION_SETUP_ACTION", t "${action}") i.r0'
  nsExec::ExecToStack 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\obsession-installer-safety.ps1"'
  Pop $0
  Pop $1
  System::Call 'kernel32::SetEnvironmentVariable(t "OBSESSION_SETUP_ACTION", p 0) i.r2'
!macroend

!macro OBSESSION_ABORT message
  DetailPrint "Obsession: ${message}"
  DetailPrint "$1"
  IfSilent +2
    MessageBox MB_OK|MB_ICONSTOP "${message}$\r$\n$\r$\n$1"
  Abort "${message}"
!macroend

!macro OBSESSION_ACQUIRE_INSTALL_LOCK
  System::Call 'kernel32::CreateMutex(p 0, i 0, t "${OBSESSION_INSTALL_MUTEX}") p.r8 ?e'
  Pop $9
  ${If} $8 P= 0
    StrCpy $1 "CreateMutex failed"
    !insertmacro OBSESSION_ABORT "Не удалось создать блокировку установки."
  ${ElseIf} $9 = 183
    System::Call 'kernel32::CloseHandle(p r8)'
    StrCpy $1 "Другой установщик уже удерживает системную блокировку."
    !insertmacro OBSESSION_ABORT "Установка Obsession уже запущена."
  ${EndIf}
!macroend

!macro OBSESSION_WRITE_OWNER_MARKER
  ClearErrors
  FileOpen $0 "$INSTDIR\${OBSESSION_OWNER_MARKER}" w
  ${IfNot} ${Errors}
    FileWrite $0 "${OBSESSION_OWNER_ID}$\r$\n"
    FileClose $0
    SetFileAttributes "$INSTDIR\${OBSESSION_OWNER_MARKER}" HIDDEN
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Obsession: проверка безопасного каталога установки…"
  !insertmacro OBSESSION_PREPARE_SAFETY_HELPER
  !insertmacro OBSESSION_RUN_SAFETY_ACTION "ValidateInstallDir"
  ${If} $0 <> 0
    !insertmacro OBSESSION_ABORT "Небезопасный каталог установки. Выберите пустую папку для Obsession."
  ${EndIf}

  !insertmacro OBSESSION_ACQUIRE_INSTALL_LOCK

  DetailPrint "Obsession: остановка установленного приложения…"
  !insertmacro OBSESSION_RUN_SAFETY_ACTION "StopOwnedApplication"
  ${If} $0 <> 0
    System::Call 'kernel32::CloseHandle(p r8)'
    !insertmacro OBSESSION_ABORT "Не удалось безопасно остановить Obsession. Закройте приложение вручную."
  ${EndIf}

  ; winws/Obsession Telegram Proxy живут в %APPDATA%\Obsession\bin и переживают падение
  ; приложения. Не фатально: установка новой версии их файлов не трогает
  ; (перезаливкой ведает версионный гейт в paths.rs), но осиротевший winws
  ; продолжит фильтровать трафик — об этом надо сказать вслух.
  !insertmacro OBSESSION_RUN_SAFETY_ACTION "StopOwnedRuntime"
  ${If} $0 <> 0
    DetailPrint "Obsession: $1"
  ${EndIf}

  ; Never recursively remove $INSTDIR here. The standard Tauri template updates
  ; known files in place; transactional cleanup/rollback belongs to Installer PR 2.
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro OBSESSION_WRITE_OWNER_MARKER
  DetailPrint "Obsession: установка завершена."
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro OBSESSION_PREPARE_SAFETY_HELPER
  !insertmacro OBSESSION_RUN_SAFETY_ACTION "ValidateInstallDir"
  ${If} $0 <> 0
    !insertmacro OBSESSION_ABORT "Каталог установки не подтверждён как принадлежащий Obsession."
  ${EndIf}

  !insertmacro OBSESSION_ACQUIRE_INSTALL_LOCK
  !insertmacro OBSESSION_RUN_SAFETY_ACTION "StopOwnedApplication"
  ${If} $0 <> 0
    System::Call 'kernel32::CloseHandle(p r8)'
    !insertmacro OBSESSION_ABORT "Не удалось безопасно остановить Obsession. Закройте приложение вручную."
  ${EndIf}

  ; Критично именно при удалении: winws мог пережить падение приложения, и
  ; после сноса бинарников его будет нечем остановить — драйвер WinDivert
  ; продолжит перехватывать трафик машины до перезагрузки. Проверка ведётся по
  ; точному пути в %APPDATA%\Obsession\bin, поэтому ЧУЖОЙ winws.exe (стоковый
  ; Zapret рядом) не пострадает. Не Abort: удаление должно завершиться в любом
  ; случае, иначе пользователь останется и с процессом, и с установкой.
  !insertmacro OBSESSION_RUN_SAFETY_ACTION "StopOwnedRuntime"
  ${If} $0 <> 0
    DetailPrint "Obsession: $1"
    IfSilent +2
      MessageBox MB_OK|MB_ICONEXCLAMATION "Удаление продолжится, но процессы обхода остановить не удалось.$\r$\n$\r$\n$1$\r$\n$\r$\nПерезагрузите компьютер, чтобы выгрузить драйвер WinDivert."
  ${EndIf}

  ; Runtime firewall rules are generation-scoped (for example
  ; "Obsession TgWsProxy 1443 gen7"). The helper removes only this exact product
  ; namespace at its current integrity. It must never elevate a script unpacked
  ; below $PLUGINSDIR; the future per-machine uninstaller owns elevated cleanup.
  !insertmacro OBSESSION_RUN_SAFETY_ACTION "CleanupFirewall"
  ${If} $0 <> 0
    DetailPrint "Obsession: не удалось удалить правила брандмауэра: $1"
  ${EndIf}

  ${If} $UpdateMode = 1
    ; Legacy installations did not have an owner marker. Preserve/create it
    ; across the old uninstaller so the new payload can safely recognize stale
    ; product files without trusting an arbitrary non-empty directory.
    !insertmacro OBSESSION_WRITE_OWNER_MARKER
  ${Else}
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Obsession"
    Delete "$INSTDIR\${OBSESSION_OWNER_MARKER}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
