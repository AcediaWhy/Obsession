// Оболочка «Obsession Setup»: medium-integrity UI и native per-machine worker.
// Release никогда не извлекает и не запускает вложенный current-user NSIS.
// Старый transactional flow остаётся только debug simulation до завершения
// native update/uninstall transaction.

mod machine_handoff;
mod payload_compression;
pub mod machine_worker;
mod upgrade;
mod uninstall;
mod user_cleanup;

use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, ERROR_ALREADY_EXISTS, HANDLE,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
};

// Debug simulation сохраняет прежний интерфейс run_install, но не запускает
// никакой внешний payload.
static PAYLOAD: &[u8] = &[];

// Пока NSIS работает, прервать установку безопасно нельзя: блокируем закрытие
// окна (фронт дополнительно прячет ✕).
static INSTALLING: AtomicBool = AtomicBool::new(false);
static PROGRESS_SEQUENCE: AtomicU32 = AtomicU32::new(0);

const SETUP_INSTANCE_MUTEX: &str = "Local\\com.vlarpsu.obsession.setup.instance.v1";
const INSTALL_OPERATION_MUTEX: &str = "Local\\com.vlarpsu.obsession.setup.install.v1";
pub const UNINSTALL_SWITCH: &str = "--uninstall";
const OWNER_MARKER: &str = ".obsession-install-owner";
const OWNER_ID: &str = "com.vlarpsu.obsession";
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
const MAX_INSTALL_PATH_UTF16: usize = 240;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, serde::Serialize)]
struct Progress {
    sequence: u32,
    pct: u32,
    stage: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum InstallerErrorCode {
    Busy,
    UacCancelled,
    DowngradeBlocked,
    PreflightFailed,
    InstallFailed,
    RollbackRestored,
    RollbackIncomplete,
    LaunchFailed,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallerFailure {
    code: InstallerErrorCode,
    retryable: bool,
    message_code: &'static str,
    log_path: Option<String>,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum InstallerOutcomeMode {
    Install,
    Update,
    Repair,
}

#[derive(Debug, serde::Serialize)]
struct InstallerOutcome {
    dir: String,
    mode: InstallerOutcomeMode,
    version: String,
}

struct NamedMutexGuard {
    handle: HANDLE,
}

// Windows kernel handles are process-wide rather than thread-affine. Moving
// sole ownership of this handle into Tauri's async command future is safe.
unsafe impl Send for NamedMutexGuard {}

impl NamedMutexGuard {
    fn acquire(name: &str) -> Result<Self, String> {
        let mut wide: Vec<u16> = name.encode_utf16().collect();
        wide.push(0);

        // SAFETY: Win32 documents the thread-local last-error slot. Clearing it
        // prevents an unrelated earlier ERROR_ALREADY_EXISTS from being reused
        // when CreateMutexW creates a brand-new object.
        unsafe { SetLastError(0) };
        // SAFETY: `wide` is a live, null-terminated UTF-16 buffer. A non-null
        // handle is owned by this guard and closed in Drop.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "Не удалось создать системную блокировку инсталлера: {}",
                std::io::Error::last_os_error()
            ));
        }

        // GetLastError must be read immediately after CreateMutexW.
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already_exists {
            // SAFETY: CreateMutexW returned a valid handle even when the named
            // object already existed.
            unsafe { CloseHandle(handle) };
            return Err("Установка Obsession уже запущена.".into());
        }

        Ok(Self { handle })
    }
}

impl Drop for NamedMutexGuard {
    fn drop(&mut self) {
        // SAFETY: this guard is the sole owner of the handle.
        unsafe { CloseHandle(self.handle) };
    }
}

struct InstallingGuard;

impl InstallingGuard {
    fn acquire() -> Result<Self, String> {
        INSTALLING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Установка Obsession уже выполняется.".to_string())?;
        Ok(Self)
    }
}

impl Drop for InstallingGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::SeqCst);
    }
}

fn emit_progress(app: &AppHandle, pct: u32, stage: &'static str) {
    let sequence = PROGRESS_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit(
        "setup-progress",
        Progress {
            sequence,
            pct,
            stage,
        },
    );
}

fn installer_failure(error: String, preferred: InstallerErrorCode) -> InstallerFailure {
    let normalized = error.to_lowercase();
    let (code, retryable, message_code) = if normalized.contains("отменён пользователем")
        || normalized.contains("operation was canceled by the user")
    {
        (
            InstallerErrorCode::UacCancelled,
            true,
            "installer.error.uac_cancelled",
        )
    } else if normalized.contains("более новая версия")
        || normalized.contains("понижен") && normalized.contains("заблокирован")
    {
        (
            InstallerErrorCode::DowngradeBlocked,
            false,
            "installer.error.downgrade_blocked",
        )
    } else if normalized.contains("recovery")
        || normalized.contains("rollback remains pending")
        || normalized.contains("rollback также завершился ошибкой")
        || normalized.contains("восстановление не завершено")
        || normalized.contains("возврат backup также не удался")
    {
        (
            InstallerErrorCode::RollbackIncomplete,
            false,
            "installer.error.rollback_incomplete",
        )
    } else if normalized.contains("предыдущая версия восстановлена") {
        (
            InstallerErrorCode::RollbackRestored,
            true,
            "installer.error.rollback_restored",
        )
    } else if normalized.contains("уже запущена")
        || normalized.contains("уже выполняется")
        || normalized.contains("already running")
    {
        (InstallerErrorCode::Busy, true, "installer.error.busy")
    } else {
        match preferred {
            InstallerErrorCode::PreflightFailed => (
                InstallerErrorCode::PreflightFailed,
                true,
                "installer.error.preflight_failed",
            ),
            InstallerErrorCode::LaunchFailed => (
                InstallerErrorCode::LaunchFailed,
                true,
                "installer.error.launch_failed",
            ),
            _ => (
                InstallerErrorCode::InstallFailed,
                true,
                "installer.error.install_failed",
            ),
        }
    };

    let _ = upgrade::record_preflight_error(error);
    InstallerFailure {
        code,
        retryable,
        message_code,
        log_path: Some(
            upgrade::persistent_log_path()
                .to_string_lossy()
                .into_owned(),
        ),
    }
}

fn outcome_mode(mode: upgrade::InstallMode) -> Result<InstallerOutcomeMode, InstallerFailure> {
    match mode {
        upgrade::InstallMode::Install => Ok(InstallerOutcomeMode::Install),
        upgrade::InstallMode::Update => Ok(InstallerOutcomeMode::Update),
        upgrade::InstallMode::Repair => Ok(InstallerOutcomeMode::Repair),
        upgrade::InstallMode::Blocked => Err(installer_failure(
            "Понижение до версии этого setup заблокировано более новой установкой.".into(),
            InstallerErrorCode::DowngradeBlocked,
        )),
    }
}

fn show_fallback_error(error: String) {
    let _ = upgrade::record_preflight_error(error);
    let message = format!(
        "Не удалось завершить безопасную установку. Технические подробности сохранены в локальном журнале:\n{}",
        upgrade::persistent_log_path().display()
    );
    let mut text: Vec<u16> = message.encode_utf16().collect();
    text.push(0);
    let mut title: Vec<u16> = "Obsession Setup".encode_utf16().collect();
    title.push(0);
    // SAFETY: оба буфера живы, завершаются NUL и не изменяются во время вызова.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

fn show_fallback_info(message: &str) {
    let mut text: Vec<u16> = message.encode_utf16().collect();
    text.push(0);
    let mut title: Vec<u16> = "Obsession Setup".encode_utf16().collect();
    title.push(0);
    // SAFETY: both buffers are NUL-terminated and live for the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        )
    };
}

fn path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_string();
    }
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value.to_lowercase()
}

fn path_is_same_or_child(candidate: &str, root: &str) -> bool {
    candidate == root
        || candidate
            .strip_prefix(root)
            .is_some_and(|tail| tail.starts_with('\\'))
}

fn resolve_existing_prefix(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path.to_path_buf();
    let mut missing = Vec::<OsString>::new();

    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            "Не удалось определить существующий родитель каталога установки.".to_string()
        })?;
        missing.push(name.to_os_string());
        if !existing.pop() {
            return Err("Не удалось определить каталог установки.".into());
        }
    }

    let mut resolved = fs::canonicalize(&existing)
        .map_err(|e| format!("Не удалось проверить путь установки: {e}"))?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn reject_reparse_points(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if !current.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|e| format!("Не удалось проверить {}: {e}", current.display()))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!(
                "Путь установки проходит через ссылку или junction: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn has_invalid_windows_component(path: &Path) -> bool {
    path.components().any(|component| {
        let std::path::Component::Normal(value) = component else {
            return false;
        };
        let value = value.to_string_lossy();
        if value.is_empty()
            || value.ends_with([' ', '.'])
            || value
                .chars()
                .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        {
            return true;
        }

        let stem = value
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
    })
}

fn reject_protected_path(path: &Path) -> Result<(), String> {
    let candidate = path_key(path);

    let forbidden_trees = [
        "WINDIR",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "TEMP",
        "TMP",
    ];
    for variable in forbidden_trees {
        let Some(root) = std::env::var_os(variable) else {
            continue;
        };
        let resolved =
            resolve_existing_prefix(Path::new(&root)).unwrap_or_else(|_| PathBuf::from(root));
        let root = path_key(&resolved);
        if path_is_same_or_child(&candidate, &root) {
            return Err(format!(
                "Нельзя устанавливать Obsession в защищённый системный каталог: {}",
                path.display()
            ));
        }
    }

    // Descendants of these locations are valid (the default lives below
    // LOCALAPPDATA), but the profile/data roots themselves and their ancestors
    // are far too broad to be an application directory.
    for variable in ["USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
        let Some(root) = std::env::var_os(variable) else {
            continue;
        };
        let resolved =
            resolve_existing_prefix(Path::new(&root)).unwrap_or_else(|_| PathBuf::from(root));
        let root = path_key(&resolved);
        if candidate == root || path_is_same_or_child(&root, &candidate) {
            return Err(format!(
                "Выбран слишком широкий каталог установки: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_directory_ownership(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        return Err("Путь установки указывает не на каталог.".into());
    }

    let mut entries =
        fs::read_dir(path).map_err(|e| format!("Не удалось прочитать каталог установки: {e}"))?;
    if entries.next().is_none() {
        return Ok(());
    }

    let owned =
        fs::read_to_string(path.join(OWNER_MARKER)).is_ok_and(|value| value.trim() == OWNER_ID);
    let legacy_install =
        path.join("Obsession.exe").is_file() && path.join("uninstall.exe").is_file();
    if owned || legacy_install {
        return Ok(());
    }

    Err(format!(
        "Каталог {} не пуст и не принадлежит Obsession. Выберите пустую папку, чтобы не перезаписать чужие файлы.",
        path.display()
    ))
}

fn validate_install_dir(raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(|c| c == '\0' || c == '\r' || c == '\n') {
        return Err("Укажите корректный путь установки.".into());
    }

    let path = PathBuf::from(raw);
    let mut prefix = path.components();
    let local_drive = matches!(
        prefix.next(),
        Some(std::path::Component::Prefix(value))
            if matches!(value.kind(), std::path::Prefix::Disk(_))
    ) && matches!(prefix.next(), Some(std::path::Component::RootDir));
    if !path.is_absolute()
        || !local_drive
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(
            "Путь установки должен находиться на локальном диске и не содержать . или ...".into(),
        );
    }
    if has_invalid_windows_component(&path) {
        return Err("Путь установки содержит недопустимое имя или символ.".into());
    }
    if path.as_os_str().encode_wide().count() > MAX_INSTALL_PATH_UTF16 {
        return Err("Путь установки слишком длинный.".into());
    }

    let resolved = resolve_existing_prefix(&path)?;
    let root = resolved
        .ancestors()
        .last()
        .ok_or_else(|| "Не удалось определить корень пути установки.".to_string())?;
    if path_key(&resolved) == path_key(root) {
        return Err("Нельзя устанавливать Obsession в корень диска.".into());
    }

    reject_reparse_points(&path)?;
    reject_protected_path(&resolved)?;
    validate_directory_ownership(&path)?;
    Ok(path)
}

#[tauri::command]
fn installer_snapshot(app: AppHandle) -> Result<upgrade::InstallerSnapshot, InstallerFailure> {
    let mut snapshot = upgrade::installer_snapshot(&app.package_info().version.to_string())
        .map_err(|error| installer_failure(error, InstallerErrorCode::PreflightFailed))?;
    let install_root = machine_worker::machine_install_root()
        .map_err(|error| installer_failure(error, InstallerErrorCode::PreflightFailed))?;
    if install_root.exists() && snapshot.mode == upgrade::InstallMode::Install {
        snapshot.mode = upgrade::InstallMode::Repair;
    }
    snapshot.dir = install_root.to_string_lossy().into_owned();
    Ok(snapshot)
}

#[tauri::command]
async fn install(
    app: AppHandle,
    dir: String,
    desktop: bool,
    start_menu: bool,
) -> Result<InstallerOutcome, InstallerFailure> {
    let _installing = InstallingGuard::acquire()
        .map_err(|error| installer_failure(error, InstallerErrorCode::Busy))?;
    let _operation_lock = NamedMutexGuard::acquire(INSTALL_OPERATION_MUTEX)
        .map_err(|error| installer_failure(error, InstallerErrorCode::Busy))?;
    let current_version = app.package_info().version.to_string();
    if !cfg!(debug_assertions) {
        let install_root = machine_worker::machine_install_root()
            .map_err(|error| installer_failure(error, InstallerErrorCode::PreflightFailed))?;
        if path_key(Path::new(&dir)) != path_key(&install_root) {
            return Err(installer_failure(
                "Путь установки устарел: Obsession устанавливается только в Program Files.".into(),
                InstallerErrorCode::PreflightFailed,
            ));
        }
        let snapshot = upgrade::installer_snapshot(&current_version)
            .map_err(|error| installer_failure(error, InstallerErrorCode::PreflightFailed))?;
        let mode = outcome_mode(snapshot.mode)?;
        let result_dir = install_root.to_string_lossy().into_owned();
        let installed_version = current_version.clone();
        tauri::async_runtime::spawn_blocking(move || {
            emit_progress(&app, 2, "prepare");
            machine_handoff::provision_machine_runtime(desktop, start_menu, |pct, stage| {
                emit_progress(&app, pct, stage);
            })?;
            emit_progress(&app, 94, "shortcuts");
            upgrade::finalize_machine_user_state(&install_root, &installed_version)?;
            emit_progress(&app, 100, "finish");
            Ok(())
        })
        .await
        .map_err(|error| {
            installer_failure(
                format!("Внутренняя ошибка защищённой установки: {error}"),
                InstallerErrorCode::InstallFailed,
            )
        })?
        .map_err(|error| installer_failure(error, InstallerErrorCode::InstallFailed))?;
        return Ok(InstallerOutcome {
            dir: result_dir,
            mode,
            version: current_version,
        });
    }
    let plan = upgrade::prepare_plan(&dir, &current_version)
        .map_err(|error| installer_failure(error, InstallerErrorCode::PreflightFailed))?;
    let mode = outcome_mode(plan.mode)?;
    // Блокирующая работа (fs, ожидание ребёнка) — строго через
    // tauri::async_runtime (см. историю с паникой tokio::spawn в этом репо).
    tauri::async_runtime::spawn_blocking(move || {
        upgrade::run_install(&app, plan, desktop, start_menu, PAYLOAD)
    })
    .await
    .map_err(|error| {
        installer_failure(
            format!("Внутренняя ошибка инсталлера: {error}"),
            InstallerErrorCode::InstallFailed,
        )
    })?
    .map_err(|error| installer_failure(error, InstallerErrorCode::InstallFailed))?;
    Ok(InstallerOutcome {
        dir,
        mode,
        version: current_version,
    })
}

#[tauri::command]
fn launch_app(app: AppHandle, dir: String) -> Result<(), InstallerFailure> {
    let exe = Path::new(&dir).join("Obsession.exe");
    if !exe.exists() {
        return Err(installer_failure(
            format!("Не найден {}", exe.display()),
            InstallerErrorCode::LaunchFailed,
        ));
    }
    // The UI must always launch at the caller's medium integrity. Do not turn
    // ERROR_ELEVATION_REQUIRED into a UAC prompt for an executable selected
    // from an install path: privileged work belongs to the future service.
    Command::new(&exe)
        .current_dir(&dir)
        .spawn()
        .map_err(|error| {
            installer_failure(
                format!("Не удалось запустить Obsession: {error}"),
                InstallerErrorCode::LaunchFailed,
            )
        })?;
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
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
        (
            HKEY_CURRENT_USER,
            format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
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

/// Нет WebView2: используем тот же native per-machine worker без WebView/NSIS.
/// Временный current-user payload никогда не запускается как fallback.
pub fn run_fallback() {
    let _self_image_lock = match machine_handoff::lock_current_setup_image() {
        Ok(lock) => lock,
        Err(error) => {
            show_fallback_error(error);
            return;
        }
    };
    let _instance_guard = match NamedMutexGuard::acquire(SETUP_INSTANCE_MUTEX) {
        Ok(guard) => guard,
        Err(error) => {
            show_fallback_error(error);
            return;
        }
    };
    if cfg!(debug_assertions) {
        show_fallback_error("Защищённый fallback доступен только в release setup.".into());
        return;
    }
    if let Err(error) = upgrade::recover_pending_transaction(None) {
        show_fallback_error(format!(
            "Не удалось восстановить предыдущую установку перед fallback: {error}"
        ));
        return;
    }
    let install_root = match machine_worker::machine_install_root() {
        Ok(path) => path,
        Err(error) => {
            show_fallback_error(error);
            return;
        }
    };
    if let Err(error) = machine_handoff::provision_machine_runtime(true, true, |_, _| {}) {
        show_fallback_error(error);
        return;
    }
    if let Err(error) =
        upgrade::finalize_machine_user_state(&install_root, env!("CARGO_PKG_VERSION"))
    {
        show_fallback_error(error);
        return;
    }
    show_fallback_info(
        "Obsession установлена в Program Files. Для запуска интерфейса установите Microsoft Edge WebView2 Runtime и затем откройте Obsession из меню «Пуск».",
    );
}

pub fn run_uninstall() -> bool {
    let Some(options) = uninstall::fallback_choices() else { return true; };
    let _operation = match NamedMutexGuard::acquire(INSTALL_OPERATION_MUTEX) {
        Ok(guard) => guard,
        Err(error) => { show_fallback_error(error); return false; }
    };
    let cleanup = match user_cleanup::plan(options) {
        Ok(plan) => plan,
        Err(error) => { show_fallback_error(error); return false; }
    };
    let _self_image_lock = match machine_handoff::lock_current_setup_image() {
        Ok(lock) => lock,
        Err(error) => {
            show_fallback_error(error);
            return false;
        }
    };
    if cfg!(debug_assertions) {
        show_fallback_error(
            "Native machine uninstall is available only in a release build.".into(),
        );
        return false;
    }
    let install_root = match machine_worker::machine_install_root() {
        Ok(path) => path,
        Err(error) => {
            show_fallback_error(error);
            return false;
        }
    };
    if let Err(error) = upgrade::prepare_graphical_uninstall(&install_root) {
        show_fallback_error(error);
        return false;
    }
    if let Err(error) = machine_handoff::uninstall_machine_runtime(|_, _| {}) {
        show_fallback_error(error);
        return false;
    }
    if let Err(error) = upgrade::cleanup_machine_user_state(&install_root) {
        show_fallback_error(format!(
            "Obsession removed its machine components, but current-user cleanup failed: {error}"
        ));
        return false;
    }
    let leftovers = user_cleanup::execute(cleanup);
    if !leftovers.is_empty() {
        show_fallback_error(format!("Программа удалена, но часть данных осталась:\n{}", leftovers.join("\n")));
        return false;
    }
    show_fallback_info("Obsession удалена. Заблокированные файлы будут окончательно удалены Windows после следующей перезагрузки.");
    true
}

pub fn run() {
    let _self_image_lock = match machine_handoff::lock_current_setup_image() {
        Ok(lock) => lock,
        Err(error) => {
            eprintln!("obsession-setup: {error}");
            return;
        }
    };
    let _instance_guard = match NamedMutexGuard::acquire(SETUP_INSTANCE_MUTEX) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("obsession-setup: {error}");
            return;
        }
    };

    tauri::Builder::default()
        .on_window_event(|_window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if INSTALLING.load(Ordering::SeqCst) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            uninstall::uninstall_mode,
            uninstall::uninstall_preview,
            uninstall::uninstall_execute,
            installer_snapshot,
            install,
            launch_app,
            close_setup,
            minimize_setup
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска Obsession Setup");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::current_dir()
                .expect("current dir")
                .join("target")
                .join("installer-safety-tests")
                .join(format!("{label}-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn accepts_empty_absolute_directory() {
        let dir = TestDir::new("empty");
        assert_eq!(
            validate_install_dir(dir.0.to_str().unwrap()).unwrap(),
            dir.0
        );
    }

    #[test]
    fn rejects_non_empty_foreign_directory() {
        let dir = TestDir::new("foreign");
        fs::write(dir.0.join("notes.txt"), "user data").unwrap();
        let error = validate_install_dir(dir.0.to_str().unwrap()).unwrap_err();
        assert!(error.contains("не принадлежит Obsession"));
    }

    #[test]
    fn accepts_owned_and_legacy_installations() {
        let owned = TestDir::new("owned");
        fs::write(owned.0.join(OWNER_MARKER), format!("{OWNER_ID}\n")).unwrap();
        fs::write(owned.0.join("resource.dat"), "old").unwrap();
        assert!(validate_install_dir(owned.0.to_str().unwrap()).is_ok());

        let legacy = TestDir::new("legacy");
        fs::write(legacy.0.join("Obsession.exe"), "stub").unwrap();
        fs::write(legacy.0.join("uninstall.exe"), "stub").unwrap();
        assert!(validate_install_dir(legacy.0.to_str().unwrap()).is_ok());
    }

    #[test]
    fn rejects_forged_owner_marker() {
        let dir = TestDir::new("forged-marker");
        fs::write(dir.0.join(OWNER_MARKER), "another.product").unwrap();
        assert!(validate_install_dir(dir.0.to_str().unwrap()).is_err());
    }

    #[test]
    fn rejects_relative_root_and_reserved_paths() {
        assert!(validate_install_dir("relative\\Obsession").is_err());
        assert!(validate_install_dir(r"C:\").is_err());
        assert!(validate_install_dir(r"C:\\safe\\CON\\Obsession").is_err());
        assert!(validate_install_dir(r"C:\\safe\\..\\Obsession").is_err());
        assert!(validate_install_dir(r"\\server\share\Obsession").is_err());
    }

    #[test]
    fn named_mutex_rejects_a_second_owner() {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            "Local\\com.vlarpsu.obsession.setup.test.{}.{}",
            std::process::id(),
            id
        );
        let first = NamedMutexGuard::acquire(&name).unwrap();
        assert!(NamedMutexGuard::acquire(&name).is_err());
        drop(first);
        assert!(NamedMutexGuard::acquire(&name).is_ok());
    }
}
