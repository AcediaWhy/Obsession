use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDOK, MB_ICONINFORMATION, MB_OKCANCEL};

/// Called before installing any Obsession files, while setup is still medium
/// integrity. Only Microsoft's signed bootstrapper is allowed to execute.
pub(crate) fn install() -> Result<(), String> {
    let text: Vec<u16> = "Для окна Obsession нужен Microsoft WebView2 Runtime.\n\nСкачать и установить его с сайта Microsoft? Понадобится интернет; установка может занять несколько минут.\n\nДо завершения этого шага файлы Obsession не изменятся."
        .encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "Obsession · WebView2".encode_utf16().chain(Some(0)).collect();
    // SAFETY: both strings are NUL terminated and alive for the modal call.
    if unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OKCANCEL | MB_ICONINFORMATION) } != IDOK {
        return Err("Установка WebView2 отменена. Obsession не установлена.".into());
    }
    let mut system_dir = [0u16; 32768];
    // SAFETY: the writable buffer length is passed exactly.
    let length = unsafe { GetSystemDirectoryW(system_dir.as_mut_ptr(), system_dir.len() as u32) } as usize;
    if length == 0 || length >= system_dir.len() { return Err("Не удалось найти системный PowerShell.".into()); }
    let powershell = PathBuf::from(String::from_utf16_lossy(&system_dir[..length]))
        .join(r"WindowsPowerShell\v1.0\powershell.exe");
    let result = Command::new(powershell)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", include_str!("webview_bootstrap.ps1")])
        .creation_flags(super::CREATE_NO_WINDOW)
        .output().map_err(|error| format!("Не удалось запустить установку WebView2: {error}"))?;
    if !result.status.success() {
        return Err(format!("WebView2 не установлен: {}", String::from_utf8_lossy(&result.stderr)));
    }
    if !super::webview2_present() {
        return Err("WebView2 пока недоступен. Если Microsoft запросила перезагрузку, перезагрузите Windows и повторите установку.".into());
    }
    Ok(())
}
