use crate::{emit_progress, machine_handoff, machine_worker, upgrade, user_cleanup, InstallingGuard, NamedMutexGuard, INSTALL_OPERATION_MUTEX};
use tauri::AppHandle;

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
}

#[tauri::command]
pub(crate) async fn uninstall_execute(app: AppHandle, options: user_cleanup::CleanupOptions) -> Result<Outcome, String> {
    if !uninstall_mode() || cfg!(debug_assertions) {
        return Err("Удаление доступно только из установленного uninstall.exe.".into());
    }
    let _operation = NamedMutexGuard::acquire(INSTALL_OPERATION_MUTEX)?;
    let _busy = InstallingGuard::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        // Resolve and validate all user targets before removing machine state.
        let plan = user_cleanup::plan(options)?;
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
        emit_progress(&app, 100, "finish");
        Ok(Outcome { leftovers, restart_required: true })
    }).await.map_err(|e| e.to_string())?
}
