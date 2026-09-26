use crate::{emit_progress, machine_handoff, machine_worker, upgrade, user_cleanup, InstallingGuard, NamedMutexGuard, INSTALL_OPERATION_MUTEX};
use tauri::AppHandle;
use std::{path::PathBuf, sync::Mutex};

static DEFERRED_CACHE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

pub(crate) fn finish_after_window() {
    let roots = std::mem::take(&mut *DEFERRED_CACHE.lock().unwrap_or_else(|e| e.into_inner()));
    if roots.is_empty() { return; }
    let mut errors = Vec::new();
    for attempt in 0..20 {
        if attempt > 0 { std::thread::sleep(std::time::Duration::from_millis(500)); }
        errors = user_cleanup::finish_setup_cache(&roots);
        if errors.is_empty() { return; }
    }
    // Нативное сообщение не создаёт новый WebView и не блокирует его профиль.
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_ICONWARNING};
    let message = format!("Не удалось полностью удалить кэш окна Obsession. Файлы могут быть заняты другим процессом.\n\n{}", errors.iter().take(8).cloned().collect::<Vec<_>>().join("\n"));
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "Удаление Obsession".encode_utf16().chain(Some(0)).collect();
    // Буферы завершены NUL и существуют до закрытия диалога.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING); }
}

pub(crate) fn fallback_choices() -> Option<user_cleanup::CleanupOptions> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_YESNOCANCEL, MB_ICONWARNING, MB_DEFBUTTON2, IDYES, IDNO};
    let text: Vec<u16> = "Удалить Obsession?\n\nДа — удалить программу и все её пользовательские данные.\nНет — удалить программу, сохранив настройки (кэш и временные файлы убрать).\nОтмена — ничего не удалять.\n\nДанные не попадут в корзину. Чужие профили и общие компоненты Windows не затрагиваются."
        .encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "Удаление Obsession".encode_utf16().chain(Some(0)).collect();
    let answer = unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_YESNOCANCEL | MB_ICONWARNING | MB_DEFBUTTON2) };
    match answer {
        IDYES | IDNO => Some(user_cleanup::CleanupOptions { settings: answer == IDYES, cache: true, temporary: true }),
        _ => None,
    }
}

#[tauri::command]
pub(crate) fn uninstall_mode() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    args.len() == 2 && args[1] == crate::UNINSTALL_SWITCH
}

#[tauri::command]
pub(crate) fn uninstall_preview() -> Result<Vec<user_cleanup::CleanupLocation>, String> {
    user_cleanup::locations()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Outcome {
    leftovers: Vec<String>,
    restart_required: bool,
    cleanup_after_close: bool,
}

#[tauri::command]
pub(crate) async fn uninstall_execute(app: AppHandle, options: user_cleanup::CleanupOptions) -> Result<Outcome, String> {
    if !uninstall_mode() || cfg!(debug_assertions) {
        return Err("Удаление доступно только из установленного uninstall.exe.".into());
    }
    let _operation = NamedMutexGuard::acquire(INSTALL_OPERATION_MUTEX)?;
    let _busy = InstallingGuard::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        // Проверяем цели до удаления службы; кэш собственного окна откладываем.
        let (plan, deferred) = user_cleanup::defer_setup_cache(user_cleanup::plan(options)?, options.cache)?;
        let root = machine_worker::machine_install_root()?;
        emit_progress(&app, 5, "prepare");
        upgrade::prepare_graphical_uninstall(&root)?;
        machine_handoff::uninstall_machine_runtime(|pct, stage| {
            emit_progress(&app, 10 + pct * 7 / 10, stage);
        })?;
        let mut leftovers = Vec::new();
        if let Err(error) = upgrade::cleanup_machine_user_state(&root) {
            leftovers.push(format!("Ярлыки / автозагрузка: {error}"));
        }
        emit_progress(&app, 85, "cleanup");
        leftovers.extend(user_cleanup::execute(plan));
        let cleanup_after_close = !deferred.is_empty();
        *DEFERRED_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = deferred;
        emit_progress(&app, 100, "finish");
        Ok(Outcome { leftovers, restart_required: true, cleanup_after_close })
    }).await.map_err(|e| e.to_string())?
}
