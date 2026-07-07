//! Проверка прав администратора и перезапуск через UAC.
//! Порт из `admin_local_datasource.dart` + логики в `main.dart`.

/// True, если текущий процесс запущен с повышенными правами.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    true
}

/// Перезапускает приложение с правами администратора через UAC.
/// Возвращает true, если пользователь принял UAC и elevated-инстанс запущен.
/// Блокирует до ответа пользователя на запрос UAC.
/// (В debug-сборке не вызывается — отсюда `allow(dead_code)`.)
#[cfg(windows)]
#[allow(dead_code)]
pub fn relaunch_as_admin() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    // Экранируем одинарные кавычки для PowerShell single-quoted строки.
    let exe_str = exe.display().to_string().replace('\'', "''");
    let cmd = format!("Start-Process -FilePath '{exe_str}' -Verb RunAs");
    match crate::util::std_command("powershell")
        .args(["-NoProfile", "-Command", &cmd])
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
pub fn relaunch_as_admin() -> bool {
    false
}
