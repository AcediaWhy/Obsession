// Оболочка «Obsession Setup»: фирменный UI поверх штатного NSIS-движка Tauri.
//
// Схема: в бинарь вшит готовый Obsession_<v>_x64-setup.exe (include_bytes!),
// команда install() достаёт его во %TEMP% и запускает тихо (/S /D=<путь>) —
// вся логика апгрейда (taskkill старой версии, зачистка каталога, реестр,
// деинсталлятор, WebView2) остаётся у проверенного NSIS-шаблона и hooks.nsh.
// Оболочка лишь рисует прогресс (полл размера каталога) и правит ярлыки.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use std::os::windows::process::CommandExt;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

// В release payload обязан лежать в payload/payload.exe (кладёт
// scripts/build-setup.mjs ДО cargo). В debug — пустой стаб: установку
// подменяет мок, чтобы крутить дизайн в `tauri dev` без полного билда.
#[cfg(not(debug_assertions))]
static PAYLOAD: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/payload/payload.exe"));
#[cfg(debug_assertions)]
static PAYLOAD: &[u8] = &[];

// Пока NSIS работает, прервать установку безопасно нельзя: блокируем закрытие
// окна (фронт дополнительно прячет ✕).
static INSTALLING: AtomicBool = AtomicBool::new(false);

// Грубая оценка распакованного объёма для процентов (бинарь + resources).
// Точность не важна: полл clamp'ится в 5..95, добивается до 100 по exit code.
const EXPECTED_INSTALL_BYTES: u64 = 40_000_000;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, serde::Serialize)]
struct Progress {
    pct: u32,
    stage: &'static str,
}

fn emit_progress(app: &AppHandle, pct: u32, stage: &'static str) {
    let _ = app.emit("setup-progress", Progress { pct, stage });
}

fn payload_temp_path() -> PathBuf {
    std::env::temp_dir().join(format!("obsession-setup-payload-{}.exe", std::process::id()))
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(p) {
        for entry in rd.flatten() {
            if let Ok(md) = entry.metadata() {
                if md.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += md.len();
                }
            }
        }
    }
    total
}

#[tauri::command]
fn default_dir() -> String {
    // Тот же дефолт, что у NSIS в currentUser-режиме: $LOCALAPPDATA\Obsession.
    std::env::var("LOCALAPPDATA")
        .map(|p| format!("{p}\\Obsession"))
        .unwrap_or_else(|_| "C:\\Obsession".into())
}

#[tauri::command]
async fn install(app: AppHandle, dir: String, desktop: bool, start_menu: bool) -> Result<(), String> {
    INSTALLING.store(true, Ordering::SeqCst);
    // Блокирующая работа (fs, ожидание ребёнка) — строго через
    // tauri::async_runtime (см. историю с паникой tokio::spawn в этом репо).
    let res = tauri::async_runtime::spawn_blocking(move || do_install(&app, &dir, desktop, start_menu))
        .await
        .map_err(|e| format!("Внутренняя ошибка инсталлера: {e}"))
        .and_then(|r| r);
    INSTALLING.store(false, Ordering::SeqCst);
    res
}

fn do_install(app: &AppHandle, dir: &str, desktop: bool, start_menu: bool) -> Result<(), String> {
    emit_progress(app, 2, "prepare");

    // Мок для dev-итераций дизайна: payload в debug-сборке не вшит.
    if cfg!(debug_assertions) {
        for pct in (4..=96).step_by(4) {
            std::thread::sleep(Duration::from_millis(90));
            emit_progress(app, pct, "install");
        }
        std::thread::sleep(Duration::from_millis(250));
        emit_progress(app, 100, "finish");
        return Ok(());
    }

    let tmp = payload_temp_path();
    std::fs::write(&tmp, PAYLOAD)
        .map_err(|e| format!("Не удалось распаковать инсталлятор во временную папку: {e}"))?;

    // Тихий NSIS. Аргументы СЫРЫЕ (raw_arg): NSIS не понимает кавычек вокруг
    // /D= — стандартный квотинг std::process сломал бы пути с пробелами.
    // /D= обязан идти последним.
    emit_progress(app, 5, "install");
    let mut child = Command::new(&tmp)
        .raw_arg("/S")
        .raw_arg(format!("/D={dir}"))
        .spawn()
        .map_err(|e| format!("Не удалось запустить установку: {e}"))?;

    // Прогресс — рост целевого каталога против ожидаемого объёма.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let done = dir_size(Path::new(dir));
                let pct = 5 + (done.saturating_mul(90) / EXPECTED_INSTALL_BYTES).min(90) as u32;
                emit_progress(app, pct, "install");
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("Ошибка ожидания инсталлятора: {e}"));
            }
        }
    };
    let _ = std::fs::remove_file(&tmp);

    if !status.success() {
        return Err(format!(
            "Инсталлятор завершился с кодом {}. Закройте Obsession и попробуйте ещё раз.",
            status.code().unwrap_or(-1)
        ));
    }

    emit_progress(app, 97, "finish");

    // NSIS в тихом режиме создаёт ОБА ярлыка — снятые галки подчищаем сами.
    // Desktop берём через известные папки (может быть перенесён OneDrive).
    if !desktop {
        if let Some(d) = dirs::desktop_dir() {
            let _ = std::fs::remove_file(d.join("Obsession.lnk"));
        }
    }
    if !start_menu {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let _ = std::fs::remove_file(
                Path::new(&appdata).join(r"Microsoft\Windows\Start Menu\Programs\Obsession.lnk"),
            );
        }
    }

    emit_progress(app, 100, "finish");
    Ok(())
}

#[tauri::command]
fn launch_app(app: AppHandle, dir: String) -> Result<(), String> {
    let exe = Path::new(&dir).join("Obsession.exe");
    if !exe.exists() {
        return Err(format!("Не найден {}", exe.display()));
    }
    // UAC поднимает сам Obsession (admin::relaunch_as_admin в основном
    // приложении) — обычного spawn достаточно. Ошибка 740 (нужна элевация,
    // если у exe когда-нибудь появится манифест requireAdministrator) —
    // фолбэк через `cmd /C start` = ShellExecute с UAC-промптом.
    let spawned = Command::new(&exe).current_dir(&dir).spawn();
    if let Err(e) = spawned {
        if e.raw_os_error() == Some(740) {
            Command::new("cmd")
                .args(["/C", "start", "", exe.to_string_lossy().as_ref()])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| format!("Не удалось запустить Obsession: {e}"))?;
        } else {
            return Err(format!("Не удалось запустить Obsession: {e}"));
        }
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn close_setup(app: AppHandle) {
    if !INSTALLING.load(Ordering::SeqCst) {
        app.exit(0);
    }
}

#[tauri::command]
fn minimize_setup(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
}

/// Есть ли WebView2 Runtime (по реестру EdgeUpdate). Без него наше окно не
/// отрисуется — уходим в run_fallback(). Форс для теста ветки:
/// OBSESSION_SETUP_FORCE_FALLBACK=1.
pub fn webview2_present() -> bool {
    if std::env::var("OBSESSION_SETUP_FORCE_FALLBACK").is_ok_and(|v| v == "1") {
        return false;
    }
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let candidates = [
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{GUID}")),
        (HKEY_LOCAL_MACHINE, format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{GUID}")),
        (HKEY_CURRENT_USER, format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{GUID}")),
    ];
    for (hive, path) in candidates {
        if let Ok(key) = RegKey::predef(hive).open_subkey(&path) {
            if let Ok(pv) = key.get_value::<String, _>("pv") {
                if !pv.is_empty() && pv != "0.0.0.0" {
                    return true;
                }
            }
        }
    }
    false
}

/// Нет WebView2: достаём payload и запускаем ВИДИМЫЙ стоковый NSIS — он сам
/// скачает рантайм. Временный exe не удаляем (им пользуется живой процесс);
/// %TEMP% приберёт система.
pub fn run_fallback() {
    if PAYLOAD.is_empty() {
        eprintln!("obsession-setup: dev-сборка без payload — фолбэк недоступен");
        return;
    }
    let tmp = payload_temp_path();
    if std::fs::write(&tmp, PAYLOAD).is_ok() {
        let _ = Command::new(&tmp).spawn();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|_window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if INSTALLING.load(Ordering::SeqCst) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            default_dir,
            install,
            launch_app,
            close_setup,
            minimize_setup
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска Obsession Setup");
}
