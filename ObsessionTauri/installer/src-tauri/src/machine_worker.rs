//! Native, narrowly-scoped per-machine setup worker.
//!
//! This mode is dispatched before WebView/Tauri startup. It accepts no caller
//! paths, service names or command text: every machine-wide target is derived
//! from Windows Known Folders and fixed product constants in this module.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use obsession_runtime_service::dpi_materializer::ProtectedDataLayout;
use obsession_runtime_service::protected_layout::ProtectedLayout;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER,
    ERROR_NOT_ALL_ASSIGNED, ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST,
    ERROR_SERVICE_NOT_ACTIVE, ERROR_SUCCESS, HANDLE, HLOCAL, LUID, S_FALSE, S_OK,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetSecurityInfo, SDDL_REVISION_1,
    SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
    GetTokenInformation, LookupPrivilegeValueW, TokenElevation, DACL_SECURITY_INFORMATION,
    LUID_AND_ATTRIBUTES, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, SE_PRIVILEGE_ENABLED, SE_RESTORE_NAME,
    SE_TAKE_OWNERSHIP_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION, TOKEN_PRIVILEGES,
    TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, GetFileInformationByHandle, MoveFileExW,
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    MOVEFILE_DELAY_UNTIL_REBOOT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, OPEN_EXISTING,
    READ_CONTROL, WRITE_DAC, WRITE_OWNER,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, CreateServiceW, DeleteService,
    OpenSCManagerW, OpenServiceW, QueryServiceConfigW, QueryServiceStatusEx, StartServiceW,
    QUERY_SERVICE_CONFIGW, SC_HANDLE, SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE,
    SC_STATUS_PROCESS_INFO, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG, SERVICE_CONTROL_STOP,
    SERVICE_ERROR_NORMAL, SERVICE_KERNEL_DRIVER, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS,
    SERVICE_RUNNING, SERVICE_START, SERVICE_START_PENDING, SERVICE_STATUS,
    SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_PROCESS, SERVICE_STOP, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, SERVICE_WIN32_OWN_PROCESS,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_ProgramData, FOLDERID_ProgramFiles, FOLDERID_PublicDesktop,
    IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT, SLGP_RAWPATH,
};
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE};
use winreg::RegKey;

pub(crate) const INTERNAL_SWITCH: &str = "--obsession-machine-worker-v1";
pub(crate) const PROVISION_ACTION: &str = "provision";
pub(crate) const UNINSTALL_ACTION: &str = "uninstall";
const PRODUCT_DIRECTORY: &str = "Obsession";
const RUNTIME_DIRECTORY: &str = "Runtime";
const SERVICE_DIRECTORY: &str = "runtime";
const SERVICE_FILE_NAME: &str = "Obsession.Runtime.exe";
const MANIFEST_FILE_NAME: &str = "runtime-manifest.json";
const SERVICE_MANIFEST_PATH: &str = "runtime/Obsession.Runtime.exe";
const SERVICE_NAME: PCWSTR = w!("ObsessionRuntime");
const SERVICE_DISPLAY_NAME: PCWSTR = w!("Obsession Runtime");
const LOCAL_SYSTEM_ACCOUNT: PCWSTR = w!("LocalSystem");
const WINDIVERT_SERVICE_NAME: PCWSTR = w!("WinDivert");
const WINDIVERT_DRIVER_RELATIVE_PATH: &str = "bin/WinDivert64.sys";
const STATE_DIRECTORY_SDDL: PCWSTR =
    w!("O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;GRGX;;;BU)");

const MACHINE_PAYLOAD_MAGIC: &[u8; 8] = b"OBSMACH1";
const MACHINE_PAYLOAD_PRODUCT_ID: &str = "com.vlarpsu.obsession";
const MACHINE_MAIN_BINARY: &str = "obsession.exe";
const MACHINE_UNINSTALL_BINARY: &str = "uninstall.exe";
const MACHINE_RUNTIME_MANIFEST: &str = "runtime/runtime-manifest.json";
const MACHINE_RESULT_PREFIX: &str = "setup-result-";
const MACHINE_UPDATE_JOURNAL_FILES: [&str; 2] = ["machine-update-a.json", "machine-update-b.json"];
const MACHINE_UPDATE_STAGE_PREFIX: &str = ".Obsession.updating-";
const MACHINE_UPDATE_BACKUP_PREFIX: &str = ".Obsession.backup-";
const MACHINE_OPERATION_MUTEX_NAME: PCWSTR = w!("Global\\ObsessionMachineProvisionV1");
const MACHINE_SHORTCUT_NAME: &str = "Obsession.lnk";
const MACHINE_SHORTCUT_DESCRIPTION: PCWSTR = w!("Obsession");
const MAX_SHORTCUT_TARGET_UTF16: usize = 32_768;
const MACHINE_UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Obsession";
const MACHINE_PAYLOAD_HEADER_BYTES: usize = 16;
const MAX_MACHINE_PAYLOAD_BYTES: usize = 512 * 1024 * 1024;
const MAX_MACHINE_PAYLOAD_FILES: usize = 4096;
const MAX_MACHINE_PAYLOAD_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_MACHINE_PAYLOAD_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SERVICE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MACHINE_JOURNAL_BYTES: u64 = 16 * 1024;
const MAX_MACHINE_RESULT_SCAN: usize = 4096;
const MAX_RETAINED_MACHINE_RESULTS: usize = 32;
const MAX_MACHINE_RESULT_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const SERVICE_TRANSITION_TIMEOUT: Duration = Duration::from_secs(30);
const MACHINE_UPDATE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
const MACHINE_UPDATE_CLEANUP_RETRY_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(not(debug_assertions))]
static MACHINE_PAYLOAD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/payload/machine-payload.bin"
));

#[cfg(debug_assertions)]
static MACHINE_PAYLOAD: &[u8] = &[];

/// Returns `None` for a normal setup launch. Once the exact internal switch is
/// present in argv[1], malformed worker arguments fail closed instead of
/// falling through to the graphical installer.
pub fn dispatch_from_environment() -> Option<Result<(), String>> {
    let arguments: Vec<OsString> = std::env::args_os().collect();
    dispatch_arguments(&arguments)
}

fn dispatch_arguments(arguments: &[OsString]) -> Option<Result<(), String>> {
    if arguments.get(1).map(OsString::as_os_str) != Some(OsStr::new(INTERNAL_SWITCH)) {
        return None;
    }
    if arguments.len() < 4 {
        return Some(Err(
            "invalid internal worker arguments; expected a fixed action and request id".into(),
        ));
    }
    let Some(request_id) = arguments.get(3).and_then(|value| value.to_str()) else {
        return Some(Err("invalid machine worker request id".into()));
    };
    if !valid_machine_request_id(request_id) {
        return Some(Err("invalid machine worker request id".into()));
    }
    match arguments.get(2).map(OsString::as_os_str) {
        Some(action) if action == OsStr::new(PROVISION_ACTION) && arguments.len() == 6 => {
            let options = match MachineShortcutOptions::parse(&arguments[4], &arguments[5]) {
                Ok(options) => options,
                Err(error) => return Some(Err(error)),
            };
            Some(provision_machine_runtime(request_id, options))
        }
        Some(action) if action == OsStr::new(UNINSTALL_ACTION) && arguments.len() == 4 => {
            Some(uninstall_machine_runtime(request_id))
        }
        _ => Some(Err(
            "invalid internal worker arguments; expected a fixed action and bounded shortcut flags"
                .into(),
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MachineShortcutOptions {
    desktop: bool,
    start_menu: bool,
}

impl MachineShortcutOptions {
    fn parse(desktop: &OsStr, start_menu: &OsStr) -> Result<Self, String> {
        fn flag(value: &OsStr) -> Option<bool> {
            match value.to_str() {
                Some("0") => Some(false),
                Some("1") => Some(true),
                _ => None,
            }
        }

        Ok(Self {
            desktop: flag(desktop).ok_or_else(|| {
                "invalid machine worker desktop shortcut flag; expected 0 or 1".to_string()
            })?,
            start_menu: flag(start_menu).ok_or_else(|| {
                "invalid machine worker Start Menu shortcut flag; expected 0 or 1".to_string()
            })?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MachinePaths {
    program_files: PathBuf,
    install_root: PathBuf,
    service_binary: PathBuf,
    manifest: PathBuf,
    program_data: PathBuf,
    product_data: PathBuf,
    runtime_data: PathBuf,
}

impl MachinePaths {
    fn discover() -> Result<Self, String> {
        let program_files = known_folder(&FOLDERID_ProgramFiles, "Program Files")?;
        let program_data = known_folder(&FOLDERID_ProgramData, "ProgramData")?;
        Ok(Self::from_roots(program_files, program_data))
    }

    fn from_roots(program_files: PathBuf, program_data: PathBuf) -> Self {
        let install_root = program_files.join(PRODUCT_DIRECTORY);
        let service_root = install_root.join(SERVICE_DIRECTORY);
        let product_data = program_data.join(PRODUCT_DIRECTORY);
        let runtime_data = product_data.join(RUNTIME_DIRECTORY);
        Self {
            program_files,
            install_root,
            service_binary: service_root.join(SERVICE_FILE_NAME),
            manifest: service_root.join(MANIFEST_FILE_NAME),
            program_data,
            product_data,
            runtime_data,
        }
    }
}

pub(crate) fn machine_install_root() -> Result<PathBuf, String> {
    Ok(MachinePaths::discover()?.install_root)
}

pub(crate) fn machine_result_path(request_id: &str) -> Result<PathBuf, String> {
    if !valid_machine_request_id(request_id) {
        return Err("invalid machine worker request id".into());
    }
    Ok(MachinePaths::discover()?
        .runtime_data
        .join(format!("{MACHINE_RESULT_PREFIX}{request_id}.json")))
}

fn valid_machine_request_id(value: &str) -> bool {
    value.len() == 32
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn cleanup_stale_machine_results(runtime_data: &Path, keep_request_id: &str) -> Result<(), String> {
    let mut candidates = Vec::new();
    let mut scanned = 0usize;
    for entry in fs::read_dir(runtime_data)
        .map_err(|error| format!("could not inspect machine result directory: {error}"))?
    {
        scanned = scanned
            .checked_add(1)
            .ok_or_else(|| "machine result scan overflowed".to_string())?;
        if scanned > MAX_MACHINE_RESULT_SCAN {
            return Err("machine result directory exceeds its bounded scan limit".into());
        }
        let entry = entry.map_err(|error| format!("could not inspect machine result: {error}"))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(request_id) = name
            .strip_prefix(MACHINE_RESULT_PREFIX)
            .and_then(|value| value.strip_suffix(".json"))
        else {
            continue;
        };
        if !valid_machine_request_id(request_id) || request_id == keep_request_id {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || !metadata.is_file() {
            return Err(format!(
                "machine result cleanup found an unsafe entry: {}",
                path.display()
            ));
        }
        candidates.push((
            metadata.modified().unwrap_or(UNIX_EPOCH),
            name.to_owned(),
            path,
        ));
    }

    candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    let now = SystemTime::now();
    for (index, (modified, _, path)) in candidates.into_iter().enumerate() {
        let expired = now
            .duration_since(modified)
            .is_ok_and(|age| age >= MAX_MACHINE_RESULT_AGE);
        if index < MAX_RETAINED_MACHINE_RESULTS && !expired {
            continue;
        }
        fs::remove_file(&path).map_err(|error| {
            format!("could not remove stale result {}: {error}", path.display())
        })?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MachineUpdatePhase {
    Prepared,
    Committing,
    Swapped,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MachineUpdateJournal {
    schema_version: u32,
    sequence: u64,
    transaction_id: String,
    phase: MachineUpdatePhase,
    service_existed: bool,
}

impl MachineUpdateJournal {
    fn new(transaction_id: &str, service_existed: bool) -> Self {
        Self {
            schema_version: 1,
            sequence: 0,
            transaction_id: transaction_id.to_owned(),
            phase: MachineUpdatePhase::Prepared,
            service_existed,
        }
    }

    fn write_phase(
        &mut self,
        paths: &MachinePaths,
        phase: MachineUpdatePhase,
    ) -> Result<(), String> {
        let mut next = self.clone();
        next.sequence = next
            .sequence
            .checked_add(1)
            .ok_or_else(|| "machine update journal sequence overflowed".to_string())?;
        next.phase = phase;
        let slots = machine_update_journal_paths(paths);
        let slot = next.sequence as usize % slots.len();
        if slots[slot].exists() {
            reject_reparse_point(&slots[slot])?;
        }
        let bytes = serde_json::to_vec(&next)
            .map_err(|error| format!("could not serialize machine update journal: {error}"))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&slots[slot])
            .map_err(|error| format!("could not open machine update journal: {error}"))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("could not persist machine update journal: {error}"))?;
        *self = next;
        Ok(())
    }

    fn clear(&self, paths: &MachinePaths) -> Result<(), String> {
        for path in machine_update_journal_paths(paths) {
            if path.exists() {
                reject_reparse_point(&path)?;
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "could not remove machine update journal {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MachineUpdateCheckpoint {
    Prepared,
    ServiceStopped,
    TargetBackedUp,
    TargetSwapped,
    SwappedJournaled,
    PayloadVerified,
    ServiceActivated,
    Committed,
}

trait MachineUpdateService {
    fn stop_before_swap(&mut self) -> Result<(), String>;
    fn activate_new(&mut self) -> Result<(), String>;
    fn stop_before_rollback(&mut self) -> Result<(), String>;
    fn restore_after_rollback(&mut self) -> Result<(), String>;
}

struct NativeMachineUpdateService<'a> {
    paths: &'a MachinePaths,
    service_existed: bool,
}

impl MachineUpdateService for NativeMachineUpdateService<'_> {
    fn stop_before_swap(&mut self) -> Result<(), String> {
        stop_runtime_service_if_present()?;
        stop_owned_windivert_driver(&machine_windivert_driver_path(self.paths))
    }

    fn activate_new(&mut self) -> Result<(), String> {
        configure_runtime_service(&self.paths.service_binary)
    }

    fn stop_before_rollback(&mut self) -> Result<(), String> {
        stop_runtime_service_if_present()?;
        stop_owned_windivert_driver(&machine_windivert_driver_path(self.paths))
    }

    fn restore_after_rollback(&mut self) -> Result<(), String> {
        if self.service_existed {
            configure_runtime_service(&self.paths.service_binary)
        } else {
            delete_runtime_service_if_present()
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum MachineReportState {
    Running,
    Succeeded,
    Failed,
}

#[derive(Serialize)]
struct MachineWorkerReport<'a> {
    schema_version: u32,
    request_id: &'a str,
    sequence: u32,
    pct: u32,
    stage: &'a str,
    state: MachineReportState,
    error: Option<String>,
}

struct MachineResultWriter<'a> {
    request_id: &'a str,
    sequence: u32,
    file: File,
}

impl<'a> MachineResultWriter<'a> {
    fn create(path: &Path, request_id: &'a str) -> Result<Self, String> {
        if path.exists() {
            reject_reparse_point(path)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("could not create machine worker result: {error}"))?;
        Ok(Self {
            request_id,
            sequence: 0,
            file,
        })
    }

    fn write(
        &mut self,
        pct: u32,
        stage: &str,
        state: MachineReportState,
        error: Option<&str>,
    ) -> Result<(), String> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| "machine worker result sequence overflowed".to_string())?;
        let error = error.map(|value| value.chars().take(2048).collect());
        let bytes = serde_json::to_vec(&MachineWorkerReport {
            schema_version: 1,
            request_id: self.request_id,
            sequence: self.sequence,
            pct,
            stage,
            state,
            error,
        })
        .map_err(|error| format!("could not serialize machine worker result: {error}"))?;
        self.file
            .set_len(0)
            .and_then(|()| self.file.seek(SeekFrom::Start(0)).map(|_| ()))
            .and_then(|()| self.file.write_all(&bytes))
            .and_then(|()| self.file.sync_data())
            .map_err(|error| format!("could not persist machine worker result: {error}"))
    }

    fn running(&mut self, pct: u32, stage: &str) -> Result<(), String> {
        self.write(pct, stage, MachineReportState::Running, None)
    }

    fn succeeded(&mut self) -> Result<(), String> {
        self.write(90, "verify", MachineReportState::Succeeded, None)
    }

    fn failed(&mut self, error: &str) -> Result<(), String> {
        self.write(100, "finish", MachineReportState::Failed, Some(error))
    }
}

fn provision_machine_runtime(
    request_id: &str,
    shortcut_options: MachineShortcutOptions,
) -> Result<(), String> {
    ensure_elevated()?;
    let _machine_operation = MachineOperationMutex::acquire()?;
    enable_privilege(SE_TAKE_OWNERSHIP_NAME, "SeTakeOwnershipPrivilege")?;
    enable_privilege(SE_RESTORE_NAME, "SeRestorePrivilege")?;

    let paths = MachinePaths::discover()?;
    let _state_directories = provision_state_layout(&paths)?;
    let result_path = paths
        .runtime_data
        .join(format!("{MACHINE_RESULT_PREFIX}{request_id}.json"));
    let mut report = MachineResultWriter::create(&result_path, request_id)?;
    report.running(10, "prepare")?;
    let result: Result<(), String> = (|| {
        let setup_image = current_setup_image_identity()?;
        cleanup_stale_machine_results(&paths.runtime_data, request_id)?;
        recover_pending_machine_update(&paths)?;
        report.running(25, "install")?;
        install_machine_payload_if_missing(
            &paths,
            MACHINE_PAYLOAD,
            request_id,
            Some(&setup_image),
        )?;
        report.running(70, "verify")?;
        verify_install_layout(&paths)?;
        report.running(88, "install")?;
        configure_runtime_service(&paths.service_binary)?;
        register_machine_uninstall(&paths, expected_machine_payload_version())?;
        sync_machine_shortcuts(&paths, shortcut_options, request_id)
    })();
    match result {
        Ok(()) => {
            report.succeeded()?;
            Ok(())
        }
        Err(error) => {
            if let Err(report_error) = report.failed(&error) {
                return Err(format!("{error}; additionally, {report_error}"));
            }
            Err(error)
        }
    }
}

fn uninstall_machine_runtime(request_id: &str) -> Result<(), String> {
    ensure_elevated()?;
    let _machine_operation = MachineOperationMutex::acquire()?;
    enable_privilege(SE_TAKE_OWNERSHIP_NAME, "SeTakeOwnershipPrivilege")?;
    enable_privilege(SE_RESTORE_NAME, "SeRestorePrivilege")?;

    let paths = MachinePaths::discover()?;
    let setup_image = current_setup_image_identity()?;
    verify_running_machine_uninstaller(&paths, &setup_image)?;
    let _state_directories = provision_state_layout(&paths)?;
    let result_path = paths
        .runtime_data
        .join(format!("{MACHINE_RESULT_PREFIX}{request_id}.json"));
    let mut report = MachineResultWriter::create(&result_path, request_id)?;
    report.running(10, "prepare")?;
    let result: Result<(), String> = (|| {
        cleanup_stale_machine_results(&paths.runtime_data, request_id)?;
        recover_pending_machine_update(&paths)?;
        let payload = parse_machine_payload(MACHINE_PAYLOAD)?;
        verify_exact_machine_install(&payload, &paths.install_root, Some(&setup_image))?;
        report.running(35, "install")?;
        delete_runtime_service_if_present()?;
        stop_owned_windivert_driver(&machine_windivert_driver_path(&paths))?;
        report.running(60, "verify")?;
        sync_machine_shortcuts(
            &paths,
            MachineShortcutOptions {
                desktop: false,
                start_menu: false,
            },
            request_id,
        )?;
        unregister_machine_uninstall()?;
        remove_machine_installation(&paths, &result_path)?;
        report.running(88, "finish")?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            report.succeeded()?;
            Ok(())
        }
        Err(error) => {
            if let Err(report_error) = report.failed(&error) {
                return Err(format!("{error}; additionally, {report_error}"));
            }
            Err(error)
        }
    }
}

fn verify_running_machine_uninstaller(
    paths: &MachinePaths,
    image: &SetupImageIdentity,
) -> Result<(), String> {
    let expected = paths.install_root.join(MACHINE_UNINSTALL_BINARY);
    let actual = fs::canonicalize(&image.path)
        .map_err(|error| format!("could not canonicalize running uninstaller: {error}"))?;
    let expected = fs::canonicalize(&expected)
        .map_err(|error| format!("could not canonicalize protected uninstaller: {error}"))?;
    if path_key(&actual) != path_key(&expected) {
        return Err(
            "machine uninstall is allowed only from the protected installed uninstaller".into(),
        );
    }
    verify_machine_uninstaller(&paths.install_root, image)
}

fn remove_machine_installation(paths: &MachinePaths, result_path: &Path) -> Result<(), String> {
    let program_files_preserve = [
        MACHINE_MAIN_BINARY.to_owned(),
        MACHINE_UNINSTALL_BINARY.to_owned(),
    ];
    remove_owned_machine_tree(
        &paths.install_root,
        &paths.install_root,
        &program_files_preserve,
    )?;
    let result_relative = result_path
        .strip_prefix(&paths.product_data)
        .map_err(|_| "machine result escaped the protected product data root".to_string())?;
    let product_data_preserve = [payload_path_key(result_relative)];
    remove_owned_machine_tree(
        &paths.product_data,
        &paths.product_data,
        &product_data_preserve,
    )
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MachineTreeRemovalPlan {
    delete_now: Vec<PathBuf>,
    delete_after_reboot: Vec<PathBuf>,
    directories_after_reboot: Vec<PathBuf>,
}

fn plan_owned_machine_tree_removal(
    root: &Path,
    expected_root: &Path,
    preserve_until_reboot: &[String],
) -> Result<MachineTreeRemovalPlan, String> {
    if path_key(root) != path_key(expected_root) {
        return Err("refusing to remove an unexpected machine-owned root".into());
    }
    if !root.exists() {
        return Ok(MachineTreeRemovalPlan::default());
    }
    reject_reparse_tree(root)?;

    let mut plan = MachineTreeRemovalPlan::default();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        plan.directories_after_reboot.push(directory.clone());
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("could not inspect {}: {error}", directory.display()))?
        {
            let entry =
                entry.map_err(|error| format!("could not inspect uninstall entry: {error}"))?;
            let path = entry.path();
            reject_reparse_point(&path)?;
            if entry
                .file_type()
                .map_err(|error| format!("could not inspect {}: {error}", path.display()))?
                .is_dir()
            {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| "uninstall entry escaped its machine-owned root".to_string())?;
                if preserve_until_reboot
                    .iter()
                    .any(|candidate| payload_path_key(relative) == candidate.as_str())
                {
                    plan.delete_after_reboot.push(path);
                } else {
                    plan.delete_now.push(path);
                }
            }
            if plan.delete_now.len()
                + plan.delete_after_reboot.len()
                + plan.directories_after_reboot.len()
                > MAX_MACHINE_PAYLOAD_FILES * 32
            {
                return Err("machine-owned uninstall tree exceeds its bounded entry limit".into());
            }
        }
    }

    plan.delete_now.sort_by_key(|path| path_key(path));
    plan.delete_after_reboot.sort_by_key(|path| path_key(path));
    plan.directories_after_reboot.sort_by(|left, right| {
        right
            .components()
            .count()
            .cmp(&left.components().count())
            .then_with(|| path_key(left).cmp(&path_key(right)))
    });
    Ok(plan)
}

fn remove_owned_machine_tree(
    root: &Path,
    expected_root: &Path,
    preserve_until_reboot: &[String],
) -> Result<(), String> {
    let plan = plan_owned_machine_tree_removal(root, expected_root, preserve_until_reboot)?;
    for file in plan.delete_now {
        if let Err(error) = fs::remove_file(&file) {
            if error.kind() == std::io::ErrorKind::NotFound {
                continue;
            }
            schedule_delete_after_reboot(&file)?;
        }
    }
    for file in plan.delete_after_reboot {
        schedule_delete_after_reboot(&file)?;
    }
    for directory in plan.directories_after_reboot {
        match fs::remove_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => schedule_delete_after_reboot(&directory)?,
        }
    }
    Ok(())
}

fn schedule_delete_after_reboot(path: &Path) -> Result<(), String> {
    let wide = wide_path(path)?;
    // SAFETY: the source is a live NUL-terminated path. A null destination with
    // DELAY_UNTIL_REBOOT is the documented native self-delete mechanism.
    unsafe {
        MoveFileExW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    }
    .map_err(|error| {
        format!(
            "could not schedule {} for reboot cleanup: {error}",
            path.display()
        )
    })
}

fn known_folder(folder: *const windows::core::GUID, label: &str) -> Result<PathBuf, String> {
    // SAFETY: `folder` points to a process-lifetime Windows KNOWNFOLDERID.
    let raw = unsafe { SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None) }
        .map_err(|error| format!("could not resolve {label}: {error}"))?;
    // SAFETY: SHGetKnownFolderPath returned a NUL-terminated CoTaskMem string.
    let decoded = unsafe { raw.to_string() };
    // SAFETY: the buffer belongs to the COM task allocator regardless of UTF-16
    // decoding success and must be released exactly once.
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    let decoded = decoded.map_err(|error| format!("invalid {label} path: {error}"))?;
    if decoded.is_empty() {
        return Err(format!("Windows returned an empty {label} path"));
    }
    Ok(PathBuf::from(decoded))
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if status == S_OK || status == S_FALSE {
            Ok(Self)
        } else {
            Err(format!(
                "could not initialize COM for machine shortcuts: {}",
                windows::core::Error::from_hresult(status)
            ))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn validate_machine_shortcut_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {label}: {error}"))?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(format!(
            "{label} is missing, not a directory, or a reparse point: {}",
            path.display()
        ));
    }
    Ok(())
}

fn shortcut_target(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect shortcut {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(format!(
            "shortcut is not a regular non-reparse file: {}",
            path.display()
        ));
    }

    let shortcut_wide = wide_path(path)?;
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| format!("could not create shortcut reader: {error}"))?;
    let persistent: IPersistFile = link
        .cast()
        .map_err(|error| format!("could not access shortcut persistence: {error}"))?;
    unsafe { persistent.Load(PCWSTR(shortcut_wide.as_ptr()), STGM_READ) }
        .map_err(|error| format!("could not load shortcut {}: {error}", path.display()))?;

    let mut target = vec![0u16; MAX_SHORTCUT_TARGET_UTF16];
    unsafe { link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32) }
        .map_err(|error| format!("could not read shortcut target {}: {error}", path.display()))?;
    let length = target
        .iter()
        .position(|unit| *unit == 0)
        .ok_or_else(|| format!("shortcut target is not NUL terminated: {}", path.display()))?;
    if length == 0 {
        return Err(format!("shortcut has no target: {}", path.display()));
    }
    Ok(PathBuf::from(OsString::from_wide(&target[..length])))
}

fn preflight_enabled_machine_shortcut(path: &Path, target: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let existing = shortcut_target(path)?;
            if path_key(&existing) != path_key(target) {
                return Err(format!(
                    "refusing to overwrite an unowned shortcut: {}",
                    path.display()
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "could not inspect shortcut slot {}: {error}",
            path.display()
        )),
    }
}

fn create_owned_machine_shortcut(
    path: &Path,
    target: &Path,
    working_directory: &Path,
    request_id: &str,
) -> Result<(), String> {
    preflight_enabled_machine_shortcut(path, target)?;
    let temporary = path.with_file_name(format!(".Obsession-{request_id}.tmp.lnk"));
    match fs::symlink_metadata(&temporary) {
        Ok(_) => {
            return Err(format!(
                "machine shortcut temporary path already exists: {}",
                temporary.display()
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not inspect machine shortcut temporary path: {error}"
            ))
        }
    }

    let result = (|| {
        let target_wide = wide_path(target)?;
        let working_wide = wide_path(working_directory)?;
        let temporary_wide = wide_path(&temporary)?;
        {
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
                    .map_err(|error| format!("could not create machine shortcut: {error}"))?;
            unsafe { link.SetPath(PCWSTR(target_wide.as_ptr())) }
                .map_err(|error| format!("could not set machine shortcut target: {error}"))?;
            unsafe { link.SetWorkingDirectory(PCWSTR(working_wide.as_ptr())) }.map_err(
                |error| format!("could not set machine shortcut working directory: {error}"),
            )?;
            unsafe { link.SetIconLocation(PCWSTR(target_wide.as_ptr()), 0) }
                .map_err(|error| format!("could not set machine shortcut icon: {error}"))?;
            unsafe { link.SetDescription(MACHINE_SHORTCUT_DESCRIPTION) }
                .map_err(|error| format!("could not mark machine shortcut ownership: {error}"))?;
            let persistent: IPersistFile = link.cast().map_err(|error| {
                format!("could not access machine shortcut persistence: {error}")
            })?;
            unsafe { persistent.Save(PCWSTR(temporary_wide.as_ptr()), true) }
                .map_err(|error| format!("could not save machine shortcut: {error}"))?;
        }

        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temporary)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("could not flush machine shortcut: {error}"))?;
        let staged_target = shortcut_target(&temporary)?;
        if path_key(&staged_target) != path_key(target) {
            return Err("machine shortcut readback target did not match".into());
        }

        let destination_wide = wide_path(path)?;
        unsafe {
            MoveFileExW(
                PCWSTR(temporary_wide.as_ptr()),
                PCWSTR(destination_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|error| format!("could not commit machine shortcut: {error}"))?;
        let committed_target = shortcut_target(path)?;
        if path_key(&committed_target) != path_key(target) {
            return Err("committed machine shortcut readback target did not match".into());
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_owned_machine_shortcut(path: &Path, target: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "could not inspect machine shortcut {}: {error}",
                path.display()
            ))
        }
        Ok(_) => {}
    }

    let Ok(existing_target) = shortcut_target(path) else {
        // An unreadable, malformed, non-file or reparse-backed entry cannot be
        // proven to belong to Obsession and must be preserved.
        return Ok(());
    };
    if path_key(&existing_target) != path_key(target) {
        return Ok(());
    }
    fs::remove_file(path).map_err(|error| {
        format!(
            "could not remove owned shortcut {}: {error}",
            path.display()
        )
    })
}

fn sync_machine_shortcut_state(
    path: &Path,
    target: &Path,
    working_directory: &Path,
    enabled: bool,
    request_id: &str,
) -> Result<(), String> {
    if enabled {
        create_owned_machine_shortcut(path, target, working_directory, request_id)
    } else {
        remove_owned_machine_shortcut(path, target)
    }
}

fn sync_machine_shortcuts(
    paths: &MachinePaths,
    options: MachineShortcutOptions,
    request_id: &str,
) -> Result<(), String> {
    let target = paths.install_root.join(MACHINE_MAIN_BINARY);
    reject_reparse_point(&paths.install_root)?;
    reject_reparse_point(&target)?;

    let common_programs = known_folder(&FOLDERID_CommonPrograms, "Common Programs")?;
    let public_desktop = known_folder(&FOLDERID_PublicDesktop, "Public Desktop")?;
    validate_machine_shortcut_directory(&common_programs, "Common Programs")?;
    validate_machine_shortcut_directory(&public_desktop, "Public Desktop")?;

    let start_menu = common_programs.join(MACHINE_SHORTCUT_NAME);
    let desktop = public_desktop.join(MACHINE_SHORTCUT_NAME);
    let _com = ComApartment::initialize()?;
    if options.start_menu {
        preflight_enabled_machine_shortcut(&start_menu, &target)?;
    }
    if options.desktop {
        preflight_enabled_machine_shortcut(&desktop, &target)?;
    }

    sync_machine_shortcut_state(
        &start_menu,
        &target,
        &paths.install_root,
        options.start_menu,
        request_id,
    )?;
    sync_machine_shortcut_state(
        &desktop,
        &target,
        &paths.install_root,
        options.desktop,
        request_id,
    )
}

struct KernelHandle(HANDLE);

impl Drop for KernelHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: this guard is the sole owner of the kernel handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct MachineOperationMutex(HANDLE);

impl MachineOperationMutex {
    fn acquire() -> Result<Self, String> {
        unsafe { SetLastError(ERROR_SUCCESS) };
        // SAFETY: the name is a fixed process-lifetime UTF-16 string and the
        // newly-created mutex is requested with initial ownership.
        let handle = unsafe { CreateMutexW(None, true, MACHINE_OPERATION_MUTEX_NAME) }
            .map_err(|error| format!("could not create machine operation mutex: {error}"))?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: this handle was returned by CreateMutexW and is not owned
            // by this process when the named object already existed.
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("another elevated Obsession machine operation is already running".into());
        }
        Ok(Self(handle))
    }
}

impl Drop for MachineOperationMutex {
    fn drop(&mut self) {
        // SAFETY: this guard owns the mutex and releases ownership before
        // closing the kernel handle.
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

fn process_token(
    access: windows::Win32::Security::TOKEN_ACCESS_MASK,
) -> Result<KernelHandle, String> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo process handle is valid and `token` is writable.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) }
        .map_err(|error| format!("could not open the setup process token: {error}"))?;
    Ok(KernelHandle(token))
}

fn ensure_elevated() -> Result<(), String> {
    let token = process_token(TOKEN_QUERY)?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the output buffer is correctly sized for TokenElevation and
    // remains live for the duration of the call.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    }
    .map_err(|error| format!("could not query setup elevation: {error}"))?;
    if returned < size_of::<TOKEN_ELEVATION>() as u32 || elevation.TokenIsElevated == 0 {
        return Err("machine worker requires an elevated administrator token".into());
    }
    Ok(())
}

fn enable_privilege(name: PCWSTR, label: &str) -> Result<(), String> {
    let token = process_token(TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES)?;
    let mut luid = LUID::default();
    // SAFETY: `name` is a fixed NUL-terminated privilege name and `luid` is
    // writable.
    unsafe { LookupPrivilegeValueW(PCWSTR::null(), name, &mut luid) }
        .map_err(|error| format!("could not resolve {label}: {error}"))?;
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // AdjustTokenPrivileges reports partial assignment through last-error even
    // when its BOOL result is successful, so start from a known value.
    unsafe { SetLastError(ERROR_SUCCESS) };
    // SAFETY: the fixed-size TOKEN_PRIVILEGES buffer is valid for this call.
    unsafe { AdjustTokenPrivileges(token.0, false, Some(&privileges), 0, None, None) }
        .map_err(|error| format!("could not enable {label}: {error}"))?;
    if unsafe { GetLastError() } == ERROR_NOT_ALL_ASSIGNED {
        return Err(format!("the elevated token does not contain {label}"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachinePayloadManifest {
    schema_version: u32,
    product_id: String,
    version: String,
    files: Vec<MachinePayloadRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachinePayloadRecord {
    path: String,
    size: u64,
    sha256: String,
}

struct ParsedMachinePayload<'a> {
    files: Vec<ParsedMachineFile<'a>>,
}

struct ParsedMachineFile<'a> {
    relative: PathBuf,
    bytes: &'a [u8],
    sha256: String,
}

#[derive(Clone, Debug)]
struct SetupImageIdentity {
    path: PathBuf,
    size: u64,
    sha256: String,
}

fn setup_image_identity(path: &Path) -> Result<SetupImageIdentity, String> {
    reject_reparse_point(path)?;
    let metadata =
        fs::metadata(path).map_err(|error| format!("could not inspect setup image: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MACHINE_PAYLOAD_FILE_BYTES
    {
        return Err("setup image is missing or outside its size bound".into());
    }
    Ok(SetupImageIdentity {
        path: path.to_path_buf(),
        size: metadata.len(),
        sha256: sha256_file(path, MAX_MACHINE_PAYLOAD_FILE_BYTES)?,
    })
}

fn current_setup_image_identity() -> Result<SetupImageIdentity, String> {
    let path = std::env::current_exe()
        .map_err(|error| format!("could not resolve current setup image: {error}"))?;
    setup_image_identity(&path)
}

fn materialize_machine_uninstaller(root: &Path, image: &SetupImageIdentity) -> Result<(), String> {
    let destination = root.join(MACHINE_UNINSTALL_BINARY);
    let mut source = File::open(&image.path)
        .map_err(|error| format!("could not open setup image for uninstall copy: {error}"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| format!("could not create protected uninstaller: {error}"))?;
    let copied = std::io::copy(&mut source, &mut output)
        .and_then(|bytes| output.sync_all().map(|()| bytes))
        .map_err(|error| format!("could not persist protected uninstaller: {error}"))?;
    if copied != image.size {
        return Err("protected uninstaller copy changed size during materialization".into());
    }
    verify_machine_uninstaller(root, image)
}

fn verify_machine_uninstaller(root: &Path, image: &SetupImageIdentity) -> Result<(), String> {
    let path = root.join(MACHINE_UNINSTALL_BINARY);
    reject_reparse_point(&path)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("could not inspect protected uninstaller: {error}"))?;
    if !metadata.is_file()
        || metadata.len() != image.size
        || sha256_file(&path, MAX_MACHINE_PAYLOAD_FILE_BYTES)? != image.sha256
    {
        return Err("protected uninstaller does not match the running setup image".into());
    }
    Ok(())
}

fn expected_machine_payload_version() -> &'static str {
    option_env!("OBSESSION_MACHINE_PAYLOAD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn parse_machine_payload(bytes: &[u8]) -> Result<ParsedMachinePayload<'_>, String> {
    if bytes.len() < MACHINE_PAYLOAD_HEADER_BYTES || bytes.len() > MAX_MACHINE_PAYLOAD_BYTES {
        return Err("machine payload is missing or outside its size bound".into());
    }
    if &bytes[..MACHINE_PAYLOAD_MAGIC.len()] != MACHINE_PAYLOAD_MAGIC {
        return Err("machine payload has an invalid magic value".into());
    }
    let manifest_length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let header_file_count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if manifest_length == 0
        || manifest_length > MAX_MACHINE_PAYLOAD_MANIFEST_BYTES
        || header_file_count == 0
        || header_file_count > MAX_MACHINE_PAYLOAD_FILES
    {
        return Err("machine payload header exceeds its bounds".into());
    }
    let manifest_end = MACHINE_PAYLOAD_HEADER_BYTES
        .checked_add(manifest_length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| "machine payload manifest is truncated".to_string())?;
    let manifest: MachinePayloadManifest =
        serde_json::from_slice(&bytes[MACHINE_PAYLOAD_HEADER_BYTES..manifest_end])
            .map_err(|error| format!("could not parse machine payload manifest: {error}"))?;
    if manifest.schema_version != 1
        || manifest.product_id != MACHINE_PAYLOAD_PRODUCT_ID
        || manifest.version != expected_machine_payload_version()
        || manifest.files.len() != header_file_count
    {
        return Err("machine payload identity does not match this setup binary".into());
    }

    let mut cursor = manifest_end;
    let mut seen = BTreeSet::new();
    let mut parsed = Vec::with_capacity(manifest.files.len());
    for record in manifest.files {
        let relative = safe_machine_relative(&record.path)
            .ok_or_else(|| format!("unsafe machine payload path: {}", record.path))?;
        let key = payload_path_key(&relative);
        if !seen.insert(key) {
            return Err(format!("duplicate machine payload path: {}", record.path));
        }
        if record.size > MAX_MACHINE_PAYLOAD_FILE_BYTES
            || record.sha256.len() != 64
            || !record.sha256.as_bytes().iter().all(u8::is_ascii_hexdigit)
        {
            return Err(format!("invalid machine payload record: {}", record.path));
        }
        let size = usize::try_from(record.size)
            .map_err(|_| format!("machine payload file is too large: {}", record.path))?;
        let end = cursor
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| format!("truncated machine payload file: {}", record.path))?;
        let file_bytes = &bytes[cursor..end];
        let actual = sha256_bytes(file_bytes);
        if !actual.eq_ignore_ascii_case(&record.sha256) {
            return Err(format!("machine payload hash mismatch: {}", record.path));
        }
        parsed.push(ParsedMachineFile {
            relative,
            bytes: file_bytes,
            sha256: record.sha256.to_ascii_lowercase(),
        });
        cursor = end;
    }
    if cursor != bytes.len() {
        return Err("machine payload contains unauthenticated trailing bytes".into());
    }
    for required in [
        MACHINE_MAIN_BINARY,
        SERVICE_MANIFEST_PATH,
        MACHINE_RUNTIME_MANIFEST,
    ] {
        if !seen.contains(&required.to_ascii_lowercase()) {
            return Err(format!(
                "machine payload is missing required file: {required}"
            ));
        }
    }
    Ok(ParsedMachinePayload { files: parsed })
}

fn safe_machine_relative(value: &str) -> Option<PathBuf> {
    if value.is_empty()
        || value.len() > 240
        || !value.is_ascii()
        || value.contains(['\\', '\0', ':'])
        || value.starts_with('/')
        || value.ends_with('/')
        || value.bytes().any(|byte| byte < 0x20)
    {
        return None;
    }
    let path = PathBuf::from(value);
    if !path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
        || path.components().any(|component| {
            let Component::Normal(value) = component else {
                return true;
            };
            let value = value.to_string_lossy();
            if value.is_empty()
                || value.ends_with([' ', '.'])
                || value
                    .chars()
                    .any(|character| matches!(character, '<' | '>' | '"' | '|' | '?' | '*'))
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
                    .is_some_and(|number| {
                        matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                    })
        })
    {
        return None;
    }
    Some(path)
}

fn payload_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn install_machine_payload_if_missing(
    paths: &MachinePaths,
    payload_bytes: &[u8],
    transaction_id: &str,
    uninstaller: Option<&SetupImageIdentity>,
) -> Result<(), String> {
    let payload = parse_machine_payload(payload_bytes)?;
    if paths.install_root.exists() {
        if uninstaller.is_some() {
            reject_registered_machine_downgrade(expected_machine_payload_version())?;
        }
        if verify_exact_machine_install(&payload, &paths.install_root, uninstaller).is_ok() {
            return Ok(());
        }
        reject_reparse_tree(&paths.install_root)?;
        verify_install_layout(paths).map_err(|error| {
            format!(
                "existing protected installation is not safe to update transactionally: {error}"
            )
        })?;
        let service_existed = verify_runtime_service_before_update(&paths.service_binary)?;
        let mut service = NativeMachineUpdateService {
            paths,
            service_existed,
        };
        return transactional_update_machine_payload(
            paths,
            &payload,
            transaction_id,
            service_existed,
            uninstaller,
            &mut service,
            |_| Ok(()),
        );
    }

    install_fresh_machine_payload(paths, &payload, uninstaller)
}

fn install_fresh_machine_payload(
    paths: &MachinePaths,
    payload: &ParsedMachinePayload<'_>,
    uninstaller: Option<&SetupImageIdentity>,
) -> Result<(), String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stage = paths.program_files.join(format!(
        ".Obsession.installing-{}-{nonce:x}",
        std::process::id()
    ));
    let descriptor = LocalSecurityDescriptor::state_directory()?;
    let stage_handle = create_new_protected_directory(&stage, &descriptor)?;
    let prepare_result = (|| {
        materialize_machine_payload(payload, &stage)?;
        if let Some(image) = uninstaller {
            materialize_machine_uninstaller(&stage, image)?;
        }
        verify_exact_machine_install(payload, &stage, uninstaller)?;
        verify_service_manifest_entry(
            &stage.join(SERVICE_DIRECTORY).join(MANIFEST_FILE_NAME),
            &stage.join(SERVICE_DIRECTORY).join(SERVICE_FILE_NAME),
        )
    })();
    if let Err(error) = prepare_result {
        drop(stage_handle);
        cleanup_failed_machine_directory(&paths.program_files, &stage);
        return Err(error);
    }
    drop(stage_handle);
    fs::rename(&stage, &paths.install_root).map_err(|error| {
        cleanup_failed_machine_directory(&paths.program_files, &stage);
        format!(
            "could not commit machine installation {} -> {}: {error}",
            stage.display(),
            paths.install_root.display()
        )
    })?;
    if let Err(error) = verify_exact_machine_install(payload, &paths.install_root, uninstaller) {
        cleanup_failed_machine_directory(&paths.program_files, &paths.install_root);
        return Err(format!(
            "committed machine payload failed verification: {error}"
        ));
    }
    Ok(())
}

fn transactional_update_machine_payload<S, F>(
    paths: &MachinePaths,
    payload: &ParsedMachinePayload<'_>,
    transaction_id: &str,
    service_existed: bool,
    uninstaller: Option<&SetupImageIdentity>,
    service: &mut S,
    mut checkpoint: F,
) -> Result<(), String>
where
    S: MachineUpdateService,
    F: FnMut(MachineUpdateCheckpoint) -> Result<(), String>,
{
    if !valid_machine_request_id(transaction_id) {
        return Err("invalid machine update transaction id".into());
    }
    let (stage, backup) = machine_update_paths(paths, transaction_id);
    if stage.exists() || backup.exists() {
        return Err("machine update staging paths unexpectedly already exist".into());
    }

    let descriptor = LocalSecurityDescriptor::state_directory()?;
    let stage_handle = create_machine_update_stage(&stage, &descriptor)?;
    let prepare_result = (|| {
        materialize_machine_payload(payload, &stage)?;
        if let Some(image) = uninstaller {
            materialize_machine_uninstaller(&stage, image)?;
        }
        verify_exact_machine_install(payload, &stage, uninstaller)?;
        verify_service_manifest_entry(
            &stage.join(SERVICE_DIRECTORY).join(MANIFEST_FILE_NAME),
            &stage.join(SERVICE_DIRECTORY).join(SERVICE_FILE_NAME),
        )
    })();
    drop(stage_handle);
    if let Err(error) = prepare_result {
        let _ = cleanup_machine_update_directory(paths, &stage, transaction_id, true);
        return Err(error);
    }

    let mut journal = MachineUpdateJournal::new(transaction_id, service_existed);
    if let Err(error) = journal.write_phase(paths, MachineUpdatePhase::Prepared) {
        let _ = cleanup_machine_update_directory(paths, &stage, transaction_id, true);
        return Err(error);
    }

    let operation = (|| {
        checkpoint(MachineUpdateCheckpoint::Prepared)?;
        service.stop_before_swap()?;
        checkpoint(MachineUpdateCheckpoint::ServiceStopped)?;
        journal.write_phase(paths, MachineUpdatePhase::Committing)?;
        durable_rename(&paths.install_root, &backup)?;
        checkpoint(MachineUpdateCheckpoint::TargetBackedUp)?;
        durable_rename(&stage, &paths.install_root)?;
        checkpoint(MachineUpdateCheckpoint::TargetSwapped)?;
        journal.write_phase(paths, MachineUpdatePhase::Swapped)?;
        checkpoint(MachineUpdateCheckpoint::SwappedJournaled)?;
        verify_exact_machine_install(payload, &paths.install_root, uninstaller)?;
        verify_install_layout(paths)?;
        checkpoint(MachineUpdateCheckpoint::PayloadVerified)?;
        service.activate_new()?;
        checkpoint(MachineUpdateCheckpoint::ServiceActivated)?;
        journal.write_phase(paths, MachineUpdatePhase::Committed)?;
        checkpoint(MachineUpdateCheckpoint::Committed)
    })();

    if let Err(error) = operation {
        if journal.phase == MachineUpdatePhase::Committed {
            return Err(format!(
                "machine update committed, but finalization was interrupted: {error}"
            ));
        }
        return match rollback_machine_update(paths, &journal, service) {
            Ok(()) => Err(format!("machine update rolled back safely: {error}")),
            Err(rollback_error) => Err(format!(
                "machine update failed: {error}; rollback remains pending: {rollback_error}"
            )),
        };
    }

    cleanup_machine_update_directory(paths, &backup, transaction_id, false).map_err(|error| {
        format!("machine update committed, but backup cleanup remains pending: {error}")
    })?;
    journal.clear(paths).map_err(|error| {
        format!("machine update committed, but journal cleanup remains pending: {error}")
    })
}

fn rollback_machine_update<S: MachineUpdateService>(
    paths: &MachinePaths,
    journal: &MachineUpdateJournal,
    service: &mut S,
) -> Result<(), String> {
    validate_machine_update_journal(journal)?;
    service.stop_before_rollback()?;
    let (stage, backup) = machine_update_paths(paths, &journal.transaction_id);
    let target_exists = paths.install_root.exists();
    let stage_exists = stage.exists();
    let backup_exists = backup.exists();

    if backup_exists {
        if target_exists {
            if stage_exists {
                cleanup_machine_update_directory(paths, &stage, &journal.transaction_id, true)?;
            }
            durable_rename(&paths.install_root, &stage)?;
        }
        durable_rename(&backup, &paths.install_root)?;
        cleanup_machine_update_directory(paths, &stage, &journal.transaction_id, true)?;
    } else {
        if journal.phase == MachineUpdatePhase::Swapped && !stage_exists {
            return Err("machine update backup is missing after the target swap".into());
        }
        if !target_exists {
            return Err("machine update lost both the target and its backup".into());
        }
        if stage_exists {
            cleanup_machine_update_directory(paths, &stage, &journal.transaction_id, true)?;
        }
    }

    verify_install_layout(paths)
        .map_err(|error| format!("restored machine installation failed verification: {error}"))?;
    service.restore_after_rollback()?;
    journal.clear(paths)
}

fn recover_pending_machine_update(paths: &MachinePaths) -> Result<(), String> {
    let Some(journal) = load_machine_update_journal(paths)? else {
        return Ok(());
    };
    let mut service = NativeMachineUpdateService {
        paths,
        service_existed: journal.service_existed,
    };
    recover_machine_update_with_service(paths, &journal, &mut service)
}

fn recover_machine_update_with_service<S: MachineUpdateService>(
    paths: &MachinePaths,
    journal: &MachineUpdateJournal,
    service: &mut S,
) -> Result<(), String> {
    validate_machine_update_journal(journal)?;
    let (stage, backup) = machine_update_paths(paths, &journal.transaction_id);
    if journal.phase == MachineUpdatePhase::Committed {
        verify_install_layout(paths).map_err(|error| {
            format!("committed machine update failed recovery verification: {error}")
        })?;
        // A WinDivert kernel image can outlive every user-mode DPI process and
        // keep the old driver file mapped after the install root was renamed to
        // the transaction backup. Quiesce the protected runtime and its owned
        // driver before retrying committed cleanup, then restore the new service
        // even when cleanup still reports an unrelated lock.
        service.stop_before_swap()?;
        let cleanup_result = (|| {
            cleanup_machine_update_directory(paths, &stage, &journal.transaction_id, true)?;
            cleanup_machine_update_directory(paths, &backup, &journal.transaction_id, false)
        })();
        let activation_result = service.activate_new();
        match (cleanup_result, activation_result) {
            (Err(cleanup_error), Err(activation_error)) => {
                return Err(format!(
                    "{cleanup_error}; additionally, could not restore ObsessionRuntime: {activation_error}"
                ));
            }
            (Err(cleanup_error), Ok(())) => return Err(cleanup_error),
            (Ok(()), Err(activation_error)) => return Err(activation_error),
            (Ok(()), Ok(())) => {}
        }
        return journal.clear(paths);
    }
    rollback_machine_update(paths, journal, service)
}

fn machine_update_journal_paths(paths: &MachinePaths) -> [PathBuf; 2] {
    MACHINE_UPDATE_JOURNAL_FILES.map(|name| paths.runtime_data.join(name))
}

fn load_machine_update_journal(
    paths: &MachinePaths,
) -> Result<Option<MachineUpdateJournal>, String> {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for (index, path) in machine_update_journal_paths(paths).into_iter().enumerate() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                invalid.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_MACHINE_JOURNAL_BYTES
        {
            invalid.push(format!("{}: invalid journal file", path.display()));
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                invalid.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        match serde_json::from_slice::<MachineUpdateJournal>(&bytes) {
            Ok(journal) if validate_machine_update_journal(&journal).is_ok() => {
                valid.push((index, journal));
            }
            Ok(_) => invalid.push(format!("{}: invalid journal identity", path.display())),
            Err(error) => invalid.push(format!("{}: {error}", path.display())),
        }
    }

    if valid.is_empty() {
        return if invalid.is_empty() {
            Ok(None)
        } else {
            Err(format!(
                "machine update journal is corrupted: {}",
                invalid.join("; ")
            ))
        };
    }
    if valid.len() == 2 {
        let (_, first) = &valid[0];
        let (_, second) = &valid[1];
        if first.transaction_id != second.transaction_id || first.sequence == second.sequence {
            return Err("machine update journal slots conflict".into());
        }
    }
    valid.sort_by_key(|(_, journal)| journal.sequence);
    Ok(valid.pop().map(|(_, journal)| journal))
}

fn validate_machine_update_journal(journal: &MachineUpdateJournal) -> Result<(), String> {
    if journal.schema_version != 1
        || journal.sequence == 0
        || !valid_machine_request_id(&journal.transaction_id)
    {
        return Err("machine update journal has an invalid identity or schema".into());
    }
    Ok(())
}

fn machine_update_paths(paths: &MachinePaths, transaction_id: &str) -> (PathBuf, PathBuf) {
    (
        paths
            .program_files
            .join(format!("{MACHINE_UPDATE_STAGE_PREFIX}{transaction_id}")),
        paths
            .program_files
            .join(format!("{MACHINE_UPDATE_BACKUP_PREFIX}{transaction_id}")),
    )
}

fn durable_rename(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.exists() || destination.exists() {
        return Err(format!(
            "unsafe machine directory rename state: {} -> {}",
            source.display(),
            destination.display()
        ));
    }
    reject_reparse_tree(source)?;
    let source_wide = wide_path(source)?;
    let destination_wide = wide_path(destination)?;
    // SAFETY: both paths are live NUL-terminated buffers and the destination
    // was checked absent. WRITE_THROUGH asks Windows to flush the rename.
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| {
        format!(
            "could not durably rename {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

fn cleanup_machine_update_directory(
    paths: &MachinePaths,
    candidate: &Path,
    transaction_id: &str,
    stage: bool,
) -> Result<(), String> {
    let (expected_stage, expected_backup) = machine_update_paths(paths, transaction_id);
    let expected = if stage {
        &expected_stage
    } else {
        &expected_backup
    };
    if path_key(candidate) != path_key(expected) {
        return Err("refusing to clean an unexpected machine update directory".into());
    }
    if !candidate.exists() {
        return Ok(());
    }
    let deadline = Instant::now() + MACHINE_UPDATE_CLEANUP_TIMEOUT;
    loop {
        // Keep the original fail-closed reparse validation on every attempt:
        // retries must never turn a transient file lock into path traversal.
        reject_reparse_tree(candidate)?;
        match fs::remove_dir_all(candidate) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) if retryable_machine_cleanup_error(&error) && Instant::now() < deadline => {
                std::thread::sleep(MACHINE_UPDATE_CLEANUP_RETRY_INTERVAL);
            }
            Err(error) if should_defer_machine_backup_cleanup(stage, &error) => {
                // The new payload is already committed and verified. A kernel
                // image can remain mapped until reboot even after SCM reports
                // SERVICE_STOPPED, so a locked *backup* is post-commit hygiene,
                // not grounds to report the installation as rolled back. Reuse
                // the bounded uninstall planner to remove everything possible
                // now and schedule only the remaining owned paths for reboot.
                remove_owned_machine_tree(candidate, candidate, &[]).map_err(|defer_error| {
                    format!(
                        "could not clean machine update directory {}: {error}; \
                         additionally, reboot cleanup could not be scheduled: {defer_error}",
                        candidate.display()
                    )
                })?;
                return Ok(());
            }
            Err(error) => {
                return Err(format!(
                    "could not clean machine update directory {}: {error}",
                    candidate.display()
                ));
            }
        }
    }
}

fn retryable_machine_cleanup_error(error: &std::io::Error) -> bool {
    // Windows can report a still-mapped executable or kernel image as access
    // denied, sharing violation, lock violation, or directory-not-empty while
    // the Service Control Manager finishes releasing the image section.
    matches!(error.raw_os_error(), Some(5 | 32 | 33 | 145))
        || matches!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
        )
}

fn should_defer_machine_backup_cleanup(stage: bool, error: &std::io::Error) -> bool {
    !stage && retryable_machine_cleanup_error(error)
}

fn create_machine_update_stage(
    path: &Path,
    descriptor: &LocalSecurityDescriptor,
) -> Result<KernelHandle, String> {
    #[cfg(test)]
    {
        let _ = descriptor;
        fs::create_dir(path)
            .map_err(|error| format!("could not create test machine staging directory: {error}"))?;
        Ok(KernelHandle(HANDLE::default()))
    }
    #[cfg(not(test))]
    {
        create_new_protected_directory(path, descriptor)
    }
}

fn create_new_protected_directory(
    path: &Path,
    descriptor: &LocalSecurityDescriptor,
) -> Result<KernelHandle, String> {
    let wide = wide_path(path)?;
    let attributes = descriptor.attributes();
    // SAFETY: the path and descriptor-backed security attributes remain live.
    unsafe { CreateDirectoryW(PCWSTR(wide.as_ptr()), Some(&attributes)) }
        .map_err(|error| format!("could not create machine staging directory: {error}"))?;
    ensure_protected_directory(path, descriptor)
}

fn materialize_machine_payload(
    payload: &ParsedMachinePayload<'_>,
    root: &Path,
) -> Result<(), String> {
    if !root.is_dir()
        || fs::read_dir(root)
            .map_err(|error| format!("could not inspect machine staging directory: {error}"))?
            .next()
            .is_some()
    {
        return Err("machine staging directory is missing or not empty".into());
    }
    for file in &payload.files {
        ensure_payload_parent(root, &file.relative)?;
        let destination = root.join(&file.relative);
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|error| format!("could not create {}: {error}", destination.display()))?;
        output
            .write_all(file.bytes)
            .and_then(|()| output.sync_all())
            .map_err(|error| format!("could not write {}: {error}", destination.display()))?;
    }
    Ok(())
}

fn ensure_payload_parent(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    let Some(parent) = relative.parent() else {
        return Ok(());
    };
    for component in parent.components() {
        let Component::Normal(component) = component else {
            return Err("machine payload contains an invalid parent component".into());
        };
        current.push(component);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("could not create {}: {error}", current.display())),
        }
        reject_reparse_directory(&current)?;
    }
    Ok(())
}

fn verify_materialized_machine_payload(
    payload: &ParsedMachinePayload<'_>,
    root: &Path,
) -> Result<(), String> {
    for file in &payload.files {
        let path = root.join(&file.relative);
        reject_reparse_point(&path)?;
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
        if !metadata.is_file() || metadata.len() != file.bytes.len() as u64 {
            return Err(format!(
                "machine payload file size mismatch: {}",
                path.display()
            ));
        }
        let actual = sha256_file(&path, MAX_MACHINE_PAYLOAD_FILE_BYTES)?;
        if actual != file.sha256 {
            return Err(format!(
                "machine payload file hash mismatch: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
fn verify_exact_machine_payload(
    payload: &ParsedMachinePayload<'_>,
    root: &Path,
) -> Result<(), String> {
    verify_exact_machine_install(payload, root, None)
}

fn verify_exact_machine_install(
    payload: &ParsedMachinePayload<'_>,
    root: &Path,
    uninstaller: Option<&SetupImageIdentity>,
) -> Result<(), String> {
    verify_materialized_machine_payload(payload, root)?;
    if let Some(image) = uninstaller {
        verify_machine_uninstaller(root, image)?;
    }
    reject_reparse_tree(root)?;

    let mut expected = BTreeMap::new();
    for file in &payload.files {
        expected.insert(payload_path_key(&file.relative), false);
        let mut parent = file.relative.parent();
        while let Some(relative) = parent {
            if relative.as_os_str().is_empty() {
                break;
            }
            expected.insert(payload_path_key(relative), true);
            parent = relative.parent();
        }
    }
    if uninstaller.is_some() {
        expected.insert(MACHINE_UNINSTALL_BINARY.to_owned(), false);
    }

    let max_entries = MAX_MACHINE_PAYLOAD_FILES
        .checked_mul(16)
        .ok_or_else(|| "machine payload entry bound overflowed".to_string())?;
    let mut actual = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("could not inspect {}: {error}", directory.display()))?
        {
            let entry = entry
                .map_err(|error| format!("could not inspect machine payload entry: {error}"))?;
            let path = entry.path();
            reject_reparse_point(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "machine payload entry escaped its root".to_string())?;
            let is_directory = entry
                .file_type()
                .map_err(|error| format!("could not inspect {}: {error}", path.display()))?
                .is_dir();
            let key = payload_path_key(relative);
            if expected.get(&key) != Some(&is_directory) {
                return Err(format!(
                    "machine payload contains an unexpected entry: {}",
                    path.display()
                ));
            }
            if actual.insert(key, is_directory).is_some() || actual.len() > max_entries {
                return Err("machine payload tree exceeds its authenticated bounds".into());
            }
            if is_directory {
                pending.push(path);
            }
        }
    }
    if actual != expected {
        return Err("machine payload tree is incomplete".into());
    }
    Ok(())
}

fn cleanup_failed_machine_directory(program_files: &Path, candidate: &Path) {
    let exact_target = candidate == program_files.join(PRODUCT_DIRECTORY);
    let exact_stage = candidate.parent() == Some(program_files)
        && candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".Obsession.installing-"));
    if (exact_target || exact_stage) && reject_reparse_tree(candidate).is_ok() {
        let _ = fs::remove_dir_all(candidate);
    }
}

fn reject_reparse_tree(root: &Path) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    reject_reparse_directory(root)?;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("could not inspect {}: {error}", directory.display()))?
        {
            let entry =
                entry.map_err(|error| format!("could not inspect directory entry: {error}"))?;
            let path = entry.path();
            reject_reparse_point(&path)?;
            if entry
                .file_type()
                .map_err(|error| format!("could not inspect {}: {error}", path.display()))?
                .is_dir()
            {
                pending.push(path);
            }
        }
    }
    Ok(())
}

fn verify_install_layout(paths: &MachinePaths) -> Result<(), String> {
    let layout = ProtectedLayout::from_program_files(&paths.program_files)
        .map_err(|error| format!("invalid protected installation layout: {error}"))?;
    let expected_root = fs::canonicalize(&paths.install_root)
        .map_err(|error| format!("could not canonicalize protected installation root: {error}"))?;
    if path_key(layout.root()) != path_key(&expected_root) {
        return Err("protected installation resolved to an unexpected directory".into());
    }
    layout
        .load_verified_catalog()
        .map_err(|error| format!("runtime manifest preflight failed: {error}"))?;
    verify_service_manifest_entry(&paths.manifest, &paths.service_binary)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestIndex {
    schema_version: u32,
    engines: Vec<EngineIndex>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineIndex {
    engine: serde_json::Value,
    executable: String,
    files: Vec<ResourceIndex>,
    strategies: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ResourceIndex {
    path: String,
    size: u64,
    sha256: String,
}

fn verify_service_manifest_entry(
    manifest_path: &Path,
    service_binary: &Path,
) -> Result<(), String> {
    reject_reparse_point(manifest_path)?;
    reject_reparse_point(service_binary)?;
    let manifest_metadata = fs::metadata(manifest_path)
        .map_err(|error| format!("could not inspect {}: {error}", manifest_path.display()))?;
    if !manifest_metadata.is_file()
        || manifest_metadata.len() == 0
        || manifest_metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err("runtime manifest is missing, empty, or oversized".into());
    }
    let mut manifest_bytes = Vec::with_capacity(manifest_metadata.len() as usize);
    File::open(manifest_path)
        .and_then(|file| {
            file.take(MAX_MANIFEST_BYTES + 1)
                .read_to_end(&mut manifest_bytes)
        })
        .map_err(|error| format!("could not read {}: {error}", manifest_path.display()))?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("runtime manifest grew beyond its size limit".into());
    }
    let manifest: ManifestIndex = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("could not parse runtime manifest index: {error}"))?;
    if manifest.schema_version != 1 {
        return Err("unsupported runtime manifest schema".into());
    }

    let mut service_records = manifest
        .engines
        .iter()
        .flat_map(|engine| {
            let _ = (&engine.engine, &engine.executable, &engine.strategies);
            engine.files.iter()
        })
        .filter(|entry| entry.path.eq_ignore_ascii_case(SERVICE_MANIFEST_PATH));
    let record = service_records.next().ok_or_else(|| {
        "runtime manifest does not authenticate the service executable".to_string()
    })?;
    if service_records.next().is_some() {
        return Err("runtime manifest authenticates the service executable more than once".into());
    }
    if record.size == 0
        || record.size > MAX_SERVICE_BYTES
        || record.sha256.len() != 64
        || !record.sha256.as_bytes().iter().all(u8::is_ascii_hexdigit)
    {
        return Err("runtime manifest contains an invalid service record".into());
    }

    let service_metadata = fs::metadata(service_binary)
        .map_err(|error| format!("could not inspect {}: {error}", service_binary.display()))?;
    if !service_metadata.is_file() || service_metadata.len() != record.size {
        return Err("service executable size does not match the runtime manifest".into());
    }
    let actual = sha256_file(service_binary, MAX_SERVICE_BYTES)?;
    if !actual.eq_ignore_ascii_case(&record.sha256) {
        return Err("service executable hash does not match the runtime manifest".into());
    }
    Ok(())
}

fn sha256_file(path: &Path, limit: u64) -> Result<String, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let mut reader = BufReader::new(file.take(limit + 1));
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("could not hash {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > limit {
            return Err(format!("{} exceeds the hashing limit", path.display()));
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

struct LocalSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl LocalSecurityDescriptor {
    fn state_directory() -> Result<Self, String> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: STATE_DIRECTORY_SDDL is a fixed, NUL-terminated SDDL string
        // and `descriptor` receives LocalAlloc-owned storage.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                STATE_DIRECTORY_SDDL,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|error| format!("could not build the protected state ACL: {error}"))?;
        if descriptor.is_invalid() {
            return Err("Windows returned an invalid state security descriptor".into());
        }
        Ok(Self(descriptor))
    }

    fn owner_and_dacl(&self) -> Result<(PSID, *mut windows::Win32::Security::ACL), String> {
        let mut owner = PSID::default();
        let mut owner_defaulted = false.into();
        let mut dacl_present = false.into();
        let mut dacl_defaulted = false.into();
        let mut dacl = std::ptr::null_mut();
        // SAFETY: the self-relative descriptor remains live and the output
        // pointers refer into it.
        unsafe { GetSecurityDescriptorOwner(self.0, &mut owner, &mut owner_defaulted) }
            .map_err(|error| format!("protected state ACL has no valid owner: {error}"))?;
        // SAFETY: same descriptor lifetime and valid output pointers as above.
        unsafe {
            GetSecurityDescriptorDacl(self.0, &mut dacl_present, &mut dacl, &mut dacl_defaulted)
        }
        .map_err(|error| format!("protected state ACL has no valid DACL: {error}"))?;
        if owner.is_invalid() || !dacl_present.as_bool() || dacl.is_null() {
            return Err("protected state security descriptor is incomplete".into());
        }
        Ok((owner, dacl))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0 .0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: ConvertStringSecurityDescriptor... allocated this buffer
            // with LocalAlloc and this guard owns it exactly once.
            unsafe {
                let _ = windows::Win32::Foundation::LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }
}

struct ProtectedDirectoryHandles {
    _product: KernelHandle,
    _runtime: KernelHandle,
}

fn provision_state_layout(paths: &MachinePaths) -> Result<ProtectedDirectoryHandles, String> {
    let descriptor = LocalSecurityDescriptor::state_directory()?;
    let product = ensure_protected_directory(&paths.product_data, &descriptor)?;
    let runtime = ensure_protected_directory(&paths.runtime_data, &descriptor)?;
    ProtectedDataLayout::from_program_data(&paths.program_data)
        .map_err(|error| format!("protected ProgramData preflight failed: {error}"))?;
    Ok(ProtectedDirectoryHandles {
        _product: product,
        _runtime: runtime,
    })
}

fn ensure_protected_directory(
    path: &Path,
    descriptor: &LocalSecurityDescriptor,
) -> Result<KernelHandle, String> {
    let wide = wide_path(path)?;
    let attributes = descriptor.attributes();
    // SAFETY: `wide` and `attributes` remain live; the descriptor referenced by
    // the attributes outlives the call.
    if let Err(error) = unsafe { CreateDirectoryW(PCWSTR(wide.as_ptr()), Some(&attributes)) } {
        let code = unsafe { GetLastError() };
        if code != ERROR_ALREADY_EXISTS {
            return Err(format!("could not create {}: {error}", path.display()));
        }
    }
    let directory = open_directory_for_security(path)?;
    reject_reparse_directory_handle(path, &directory)?;

    let (owner, dacl) = descriptor.owner_and_dacl()?;
    // SAFETY: owner/DACL point into the live descriptor. The handle names the
    // already-opened non-reparse directory and is held without delete sharing,
    // so a path swap cannot redirect this security operation.
    let status = unsafe {
        SetSecurityInfo(
            directory.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            Some(owner),
            None,
            Some(dacl),
            None,
        )
    };
    if status.0 != 0 {
        return Err(format!(
            "could not secure {}: Win32 error {}",
            path.display(),
            status.0
        ));
    }
    reject_reparse_directory(path)?;
    reject_reparse_directory_handle(path, &directory)?;
    Ok(directory)
}

fn open_directory_for_security(path: &Path) -> Result<KernelHandle, String> {
    let wide = wide_path(path)?;
    // No FILE_SHARE_DELETE: once opened, the directory cannot be renamed or
    // replaced until service startup and post-ACL preflight have completed.
    // OPEN_REPARSE_POINT makes a junction itself visible to the handle check.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            (READ_CONTROL | WRITE_DAC | WRITE_OWNER).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| {
        format!(
            "could not open protected directory {}: {error}",
            path.display()
        )
    })?;
    Ok(KernelHandle(handle))
}

fn reject_reparse_directory_handle(path: &Path, handle: &KernelHandle) -> Result<(), String> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle remains live and the output structure is writable.
    unsafe { GetFileInformationByHandle(handle.0, &mut information) }.map_err(|error| {
        format!(
            "could not inspect directory handle {}: {error}",
            path.display()
        )
    })?;
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err(format!(
            "protected state handle is not a plain directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn reject_reparse_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(format!(
            "protected state path is not a plain directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn reject_reparse_point(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(format!(
            "protected path contains a reparse point: {}",
            path.display()
        ));
    }
    Ok(())
}

fn wide_path(path: &Path) -> Result<Vec<u16>, String> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.is_empty() || wide.contains(&0) {
        return Err(format!("invalid Windows path: {}", path.display()));
    }
    wide.push(0);
    Ok(wide)
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

struct ServiceHandle(SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: this guard is the sole owner of the SCM handle.
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }
}

fn machine_uninstall_command(paths: &MachinePaths) -> Result<String, String> {
    let path = paths.install_root.join(MACHINE_UNINSTALL_BINARY);
    let value = path
        .to_str()
        .ok_or_else(|| "uninstaller path is not valid Unicode".to_string())?;
    if value.is_empty() || value.contains(['\0', '"', '\r', '\n']) {
        return Err("uninstaller path contains an unsafe command-line character".into());
    }
    Ok(format!("\"{value}\" --uninstall"))
}

fn register_machine_uninstall(paths: &MachinePaths, version: &str) -> Result<(), String> {
    let uninstaller = paths.install_root.join(MACHINE_UNINSTALL_BINARY);
    reject_reparse_point(&uninstaller)?;
    let command = machine_uninstall_command(paths)?;
    let display_icon = format!(
        "\"{}\",0",
        paths.install_root.join(MACHINE_MAIN_BINARY).display()
    );
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (key, _) = machine
        .create_subkey_with_flags(MACHINE_UNINSTALL_KEY, KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|error| format!("could not create HKLM uninstall registration: {error}"))?;
    key.set_value("DisplayName", &"Obsession")
        .and_then(|()| key.set_value("DisplayVersion", &version))
        .and_then(|()| key.set_value("Publisher", &"VlarpSu"))
        .and_then(|()| {
            key.set_value(
                "InstallLocation",
                &paths.install_root.to_string_lossy().as_ref(),
            )
        })
        .and_then(|()| key.set_value("DisplayIcon", &display_icon))
        .and_then(|()| key.set_value("UninstallString", &command))
        .and_then(|()| key.set_value("NoModify", &1u32))
        .and_then(|()| key.set_value("NoRepair", &1u32))
        .map_err(|error| format!("could not persist HKLM uninstall registration: {error}"))
}

fn reject_registered_machine_downgrade(current: &str) -> Result<(), String> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key =
        match machine.open_subkey_with_flags(MACHINE_UNINSTALL_KEY, KEY_READ | KEY_WOW64_64KEY) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("could not inspect HKLM installed version: {error}")),
        };
    let installed: String = key
        .get_value("DisplayVersion")
        .map_err(|error| format!("HKLM uninstall registration has no valid version: {error}"))?;
    let installed = Version::parse(&installed)
        .map_err(|error| format!("HKLM installed version is invalid: {error}"))?;
    let current = Version::parse(current)
        .map_err(|error| format!("setup machine payload version is invalid: {error}"))?;
    if installed > current {
        return Err(format!(
            "a newer protected Obsession version ({installed}) is already installed; downgrade to {current} is blocked"
        ));
    }
    Ok(())
}

fn unregister_machine_uninstall() -> Result<(), String> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    match machine.delete_subkey_all(MACHINE_UNINSTALL_KEY) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "could not remove HKLM uninstall registration: {error}"
        )),
    }
}

fn verify_runtime_service_before_update(service_binary: &Path) -> Result<bool, String> {
    let expected_binary = service_binary_command(service_binary)?;
    // SAFETY: null machine/database names select the local active SCM database.
    let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(ServiceHandle)
        .map_err(|error| format!("could not open the Service Control Manager: {error}"))?;
    // SAFETY: the fixed service name remains valid for this call.
    match unsafe {
        OpenServiceW(
            manager.0,
            SERVICE_NAME,
            SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_START | SERVICE_STOP,
        )
    } {
        Ok(handle) => {
            let service = ServiceHandle(handle);
            verify_service_configuration(&service, &expected_binary)?;
            Ok(true)
        }
        Err(_) if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST => Ok(false),
        Err(error) => Err(format!(
            "could not preflight ObsessionRuntime before update: {error}"
        )),
    }
}

fn stop_runtime_service_if_present() -> Result<bool, String> {
    // SAFETY: null machine/database names select the local active SCM database.
    let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(ServiceHandle)
        .map_err(|error| format!("could not open the Service Control Manager: {error}"))?;
    // SAFETY: the fixed service name remains valid for this call.
    match unsafe { OpenServiceW(manager.0, SERVICE_NAME, SERVICE_QUERY_STATUS | SERVICE_STOP) } {
        Ok(handle) => {
            let service = ServiceHandle(handle);
            stop_service(&service)?;
            Ok(true)
        }
        Err(_) if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST => Ok(false),
        Err(error) => Err(format!("could not open ObsessionRuntime for stop: {error}")),
    }
}

fn machine_windivert_driver_path(paths: &MachinePaths) -> PathBuf {
    paths.install_root.join(WINDIVERT_DRIVER_RELATIVE_PATH)
}

fn stop_owned_windivert_driver(expected_driver: &Path) -> Result<(), String> {
    if !expected_driver.exists() {
        return Ok(());
    }
    reject_reparse_point(expected_driver)?;

    // SAFETY: null machine/database names select the local active SCM database.
    let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(ServiceHandle)
        .map_err(|error| format!("could not open SCM for WinDivert cleanup: {error}"))?;
    // SAFETY: the fixed service name remains valid for this call.
    let service = match unsafe {
        OpenServiceW(
            manager.0,
            WINDIVERT_SERVICE_NAME,
            SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS | SERVICE_STOP,
        )
    } {
        Ok(handle) => ServiceHandle(handle),
        Err(_) if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST => return Ok(()),
        Err(error) => {
            return Err(format!(
                "could not inspect WinDivert before cleanup: {error}"
            ))
        }
    };

    let (service_type, binary) = query_service_type_and_binary(&service)?;
    if !registered_windivert_driver_matches(service_type, &binary, expected_driver) {
        // The generic WinDivert service name may belong to unrelated software.
        // A path mismatch is therefore a strict no-op, never authority to stop it.
        return Ok(());
    }
    stop_service(&service)
        .map_err(|error| format!("could not stop the owned WinDivert driver: {error}"))
}

fn query_service_type_and_binary(service: &ServiceHandle) -> Result<(u32, String), String> {
    let mut needed = 0u32;
    // SAFETY: the zero-length probe requests the required buffer size.
    let probe = unsafe { QueryServiceConfigW(service.0, None, 0, &mut needed) };
    let probe_code = unsafe { GetLastError() };
    if probe.is_ok() || probe_code != ERROR_INSUFFICIENT_BUFFER || needed == 0 {
        return Err("SCM did not return a valid WinDivert configuration size".into());
    }
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    // SAFETY: usize-backed storage is aligned and large enough for the SCM result.
    unsafe {
        QueryServiceConfigW(
            service.0,
            Some(storage.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()),
            needed,
            &mut needed,
        )
    }
    .map_err(|error| format!("could not query WinDivert configuration: {error}"))?;
    // SAFETY: the successful SCM call initialized the structure in live storage.
    let config = unsafe { &*storage.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
    if config.lpBinaryPathName.is_null() {
        return Err("SCM returned WinDivert without a binary path".into());
    }
    let binary = unsafe { config.lpBinaryPathName.to_string() }
        .map_err(|error| format!("SCM returned an invalid WinDivert binary path: {error}"))?;
    Ok((config.dwServiceType.0, binary))
}

fn registered_windivert_driver_matches(
    service_type: u32,
    binary: &str,
    expected_driver: &Path,
) -> bool {
    if service_type != SERVICE_KERNEL_DRIVER.0 || binary.contains(['\0', '"', '\r', '\n']) {
        return false;
    }
    let Some(binary) = binary
        .strip_prefix(r"\??\")
        .or_else(|| binary.strip_prefix(r"\\?\"))
    else {
        return false;
    };
    path_key(Path::new(binary)) == path_key(expected_driver)
}

fn delete_runtime_service_if_present() -> Result<(), String> {
    // SAFETY: null machine/database names select the local active SCM database.
    let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(ServiceHandle)
        .map_err(|error| format!("could not open the Service Control Manager: {error}"))?;
    // SAFETY: the fixed service name remains valid for this call.
    match unsafe {
        OpenServiceW(
            manager.0,
            SERVICE_NAME,
            SERVICE_QUERY_STATUS | SERVICE_STOP | DELETE.0,
        )
    } {
        Ok(handle) => {
            let service = ServiceHandle(handle);
            stop_service(&service)?;
            // SAFETY: the service handle was opened with DELETE access.
            unsafe { DeleteService(service.0) }
                .map_err(|error| format!("could not delete rolled-back ObsessionRuntime: {error}"))
        }
        Err(_) if unsafe { GetLastError() } == ERROR_SERVICE_DOES_NOT_EXIST => Ok(()),
        Err(error) => Err(format!(
            "could not open ObsessionRuntime for rollback deletion: {error}"
        )),
    }
}

fn configure_runtime_service(service_binary: &Path) -> Result<(), String> {
    let binary_command = service_binary_command(service_binary)?;
    let binary_wide = wide_string(&binary_command)?;
    // ChangeServiceConfig interprets a null dependencies pointer as "leave the
    // old value unchanged". A double-NUL MULTI_SZ explicitly clears it, so an
    // existing service cannot retain an unrelated privileged dependency.
    let empty_dependencies = [0u16, 0u16];
    let empty_dependencies = PCWSTR(empty_dependencies.as_ptr());
    let desired_access = SERVICE_QUERY_CONFIG
        | SERVICE_CHANGE_CONFIG
        | SERVICE_QUERY_STATUS
        | SERVICE_START
        | SERVICE_STOP;
    // SAFETY: null machine/database names select the local active SCM database.
    let manager = unsafe {
        OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE,
        )
    }
    .map(ServiceHandle)
    .map_err(|error| format!("could not open the Service Control Manager: {error}"))?;

    // SAFETY: fixed NUL-terminated service name and live SCM handle.
    let existing = unsafe { OpenServiceW(manager.0, SERVICE_NAME, desired_access) };
    let service = match existing {
        Ok(handle) => {
            let service = ServiceHandle(handle);
            stop_service(&service)?;
            // SAFETY: every pointer is either a fixed string, a live fixed
            // binary command, or null for an unchanged/empty field.
            unsafe {
                ChangeServiceConfigW(
                    service.0,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    PCWSTR(binary_wide.as_ptr()),
                    w!(""),
                    None,
                    empty_dependencies,
                    LOCAL_SYSTEM_ACCOUNT,
                    PCWSTR::null(),
                    SERVICE_DISPLAY_NAME,
                )
            }
            .map_err(|error| format!("could not update ObsessionRuntime: {error}"))?;
            service
        }
        Err(error) => {
            let code = unsafe { GetLastError() };
            if code != ERROR_SERVICE_DOES_NOT_EXIST {
                return Err(format!("could not open ObsessionRuntime: {error}"));
            }
            // SAFETY: fixed service identity/configuration and live buffers.
            let handle = unsafe {
                CreateServiceW(
                    manager.0,
                    SERVICE_NAME,
                    SERVICE_DISPLAY_NAME,
                    desired_access,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    PCWSTR(binary_wide.as_ptr()),
                    w!(""),
                    None,
                    empty_dependencies,
                    PCWSTR::null(),
                    PCWSTR::null(),
                )
            }
            .map_err(|create_error| {
                format!("could not create ObsessionRuntime after {error}: {create_error}")
            })?;
            ServiceHandle(handle)
        }
    };

    verify_service_configuration(&service, &binary_command)?;
    // SAFETY: the service takes no caller-controlled argv.
    if let Err(error) = unsafe { StartServiceW(service.0, None) } {
        let code = unsafe { GetLastError() };
        if code != ERROR_SERVICE_ALREADY_RUNNING {
            return Err(format!("could not start ObsessionRuntime: {error}"));
        }
    }
    wait_for_state(&service, SERVICE_RUNNING, SERVICE_TRANSITION_TIMEOUT)
}

fn service_binary_command(path: &Path) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or_else(|| "service path is not valid Unicode".to_string())?;
    if value.is_empty() || value.contains(['\0', '"', '\r', '\n']) {
        return Err("service path contains an unsafe command-line character".into());
    }
    Ok(format!("\"{value}\""))
}

fn wide_string(value: &str) -> Result<Vec<u16>, String> {
    let mut wide: Vec<u16> = value.encode_utf16().collect();
    if wide.contains(&0) {
        return Err("Windows string contains an embedded NUL".into());
    }
    wide.push(0);
    Ok(wide)
}

fn service_stop_control_code() -> u32 {
    SERVICE_CONTROL_STOP
}

fn stop_service(service: &ServiceHandle) -> Result<(), String> {
    let mut status = query_service_status(service)?;
    if status.dwCurrentState == SERVICE_STOPPED {
        return Ok(());
    }
    if status.dwCurrentState == SERVICE_START_PENDING {
        status =
            wait_until_not_pending(service, SERVICE_START_PENDING, SERVICE_TRANSITION_TIMEOUT)?;
        if status.dwCurrentState == SERVICE_STOPPED {
            return Ok(());
        }
    }
    if status.dwCurrentState == SERVICE_STOP_PENDING {
        return wait_for_state(service, SERVICE_STOPPED, SERVICE_TRANSITION_TIMEOUT);
    }

    let mut legacy_status = SERVICE_STATUS::default();
    // SAFETY: service was opened with SERVICE_STOP and the output is writable.
    if let Err(error) =
        unsafe { ControlService(service.0, service_stop_control_code(), &mut legacy_status) }
    {
        let code = unsafe { GetLastError() };
        if code != ERROR_SERVICE_NOT_ACTIVE {
            return Err(format!(
                "could not stop the existing ObsessionRuntime: {error}"
            ));
        }
    }
    wait_for_state(service, SERVICE_STOPPED, SERVICE_TRANSITION_TIMEOUT)
}

fn wait_until_not_pending(
    service: &ServiceHandle,
    pending: SERVICE_STATUS_CURRENT_STATE,
    timeout: Duration,
) -> Result<SERVICE_STATUS_PROCESS, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = query_service_status(service)?;
        if status.dwCurrentState != pending {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err("ObsessionRuntime did not leave its pending state in time".into());
        }
        std::thread::sleep(status_poll_interval(&status));
    }
}

fn wait_for_state(
    service: &ServiceHandle,
    expected: SERVICE_STATUS_CURRENT_STATE,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let status = query_service_status(service)?;
        if status.dwCurrentState == expected {
            return Ok(());
        }
        if expected == SERVICE_RUNNING && status.dwCurrentState == SERVICE_STOPPED {
            return Err(format!(
                "ObsessionRuntime stopped during startup (Win32 {}, service {})",
                status.dwWin32ExitCode, status.dwServiceSpecificExitCode
            ));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "ObsessionRuntime did not reach state {} in time (current {})",
                expected.0, status.dwCurrentState.0
            ));
        }
        std::thread::sleep(status_poll_interval(&status));
    }
}

fn status_poll_interval(status: &SERVICE_STATUS_PROCESS) -> Duration {
    Duration::from_millis(u64::from((status.dwWaitHint / 10).clamp(100, 500)))
}

fn query_service_status(service: &ServiceHandle) -> Result<SERVICE_STATUS_PROCESS, String> {
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0u32;
    // SAFETY: the byte slice exactly covers the writable status structure.
    let buffer = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast::<u8>(),
            size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };
    unsafe { QueryServiceStatusEx(service.0, SC_STATUS_PROCESS_INFO, Some(buffer), &mut needed) }
        .map_err(|error| format!("could not query ObsessionRuntime status: {error}"))?;
    if needed as usize > size_of::<SERVICE_STATUS_PROCESS>() {
        return Err("SCM returned an oversized service status".into());
    }
    Ok(status)
}

fn verify_service_configuration(
    service: &ServiceHandle,
    expected_binary: &str,
) -> Result<(), String> {
    let mut needed = 0u32;
    // SAFETY: the zero-length probe requests the required buffer size.
    let probe = unsafe { QueryServiceConfigW(service.0, None, 0, &mut needed) };
    let probe_code = unsafe { GetLastError() };
    if probe.is_ok() || probe_code != ERROR_INSUFFICIENT_BUFFER || needed == 0 {
        return Err("SCM did not return a valid service configuration size".into());
    }
    let words = (needed as usize).div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    // SAFETY: usize-backed storage is suitably aligned and at least `needed`
    // bytes long. SCM writes a QUERY_SERVICE_CONFIGW and internal strings.
    unsafe {
        QueryServiceConfigW(
            service.0,
            Some(storage.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()),
            needed,
            &mut needed,
        )
    }
    .map_err(|error| format!("could not verify ObsessionRuntime configuration: {error}"))?;
    // SAFETY: the successful SCM call initialized the structure at the start
    // of the aligned storage, which remains live during string decoding.
    let config = unsafe { &*storage.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
    let binary = unsafe { config.lpBinaryPathName.to_string() }
        .map_err(|error| format!("SCM returned an invalid service binary path: {error}"))?;
    let account = unsafe { config.lpServiceStartName.to_string() }
        .map_err(|error| format!("SCM returned an invalid service account: {error}"))?;
    let load_order_group = if config.lpLoadOrderGroup.is_null() {
        String::new()
    } else {
        unsafe { config.lpLoadOrderGroup.to_string() }
            .map_err(|error| format!("SCM returned an invalid load-order group: {error}"))?
    };
    let has_dependencies = !config.lpDependencies.is_null()
        && unsafe {
            // SAFETY: SCM initialized this MULTI_SZ pointer inside the live
            // query buffer. An empty dependency set begins with NUL.
            *config.lpDependencies.0 != 0
        };
    if config.dwServiceType != SERVICE_WIN32_OWN_PROCESS
        || config.dwStartType != SERVICE_AUTO_START
        || config.dwErrorControl != SERVICE_ERROR_NORMAL
        || !binary.eq_ignore_ascii_case(expected_binary)
        || !account.eq_ignore_ascii_case("LocalSystem")
        || !load_order_group.is_empty()
        || has_dependencies
    {
        return Err("SCM retained an unexpected ObsessionRuntime configuration".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::current_dir()
                .unwrap()
                .join("target")
                .join("machine-worker-tests")
                .join(format!("{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Default)]
    struct TestMachineUpdateService;

    impl MachineUpdateService for TestMachineUpdateService {
        fn stop_before_swap(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn activate_new(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn stop_before_rollback(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn restore_after_rollback(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn windivert_cleanup_requires_the_exact_protected_kernel_driver() {
        let expected = Path::new(r"C:\Program Files\Obsession\bin\WinDivert64.sys");
        let registered = r"\??\C:\Program Files\Obsession\bin\WinDivert64.sys";
        assert!(registered_windivert_driver_matches(
            SERVICE_KERNEL_DRIVER.0,
            registered,
            expected
        ));
        assert!(!registered_windivert_driver_matches(
            SERVICE_WIN32_OWN_PROCESS.0,
            registered,
            expected
        ));
        for untrusted in [
            r"C:\Program Files\Obsession\bin\WinDivert64.sys",
            r"\??\C:\Program Files\Other VPN\WinDivert64.sys",
            r"\??\C:\Program Files\Obsession-Evil\bin\WinDivert64.sys",
            r#""\??\C:\Program Files\Obsession\bin\WinDivert64.sys""#,
        ] {
            assert!(!registered_windivert_driver_matches(
                SERVICE_KERNEL_DRIVER.0,
                untrusted,
                expected
            ));
        }
    }

    #[test]
    fn machine_update_cleanup_waits_for_a_transient_windows_file_lock() {
        let test = TestDirectory::new();
        let program_files = test.0.join("Program Files");
        let program_data = test.0.join("ProgramData");
        fs::create_dir_all(&program_files).unwrap();
        fs::create_dir_all(&program_data).unwrap();
        let paths = MachinePaths::from_roots(program_files, program_data);
        let transaction_id = "c".repeat(32);
        let backup = machine_update_paths(&paths, &transaction_id).1;
        fs::create_dir(&backup).unwrap();
        let locked_path = backup.join("WinDivert64.sys");
        fs::write(&locked_path, b"driver").unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked_path)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            drop(locked);
        });

        cleanup_machine_update_directory(&paths, &backup, &transaction_id, false).unwrap();
        release.join().unwrap();
        assert!(!backup.exists());
        drop(test);
    }

    #[test]
    fn only_a_locked_committed_backup_can_be_deferred_until_reboot() {
        let access_denied = std::io::Error::from_raw_os_error(5);
        let invalid_path = std::io::Error::from_raw_os_error(123);
        assert!(should_defer_machine_backup_cleanup(false, &access_denied));
        assert!(!should_defer_machine_backup_cleanup(true, &access_denied));
        assert!(!should_defer_machine_backup_cleanup(false, &invalid_path));
    }

    fn authenticated_test_payload(app: &[u8], service: &[u8]) -> Vec<u8> {
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "engines": [{
                "engine": "legacy",
                "executable": SERVICE_MANIFEST_PATH,
                "files": [{
                    "path": SERVICE_MANIFEST_PATH,
                    "size": service.len(),
                    "sha256": sha256_bytes(service),
                }],
                "strategies": [{
                    "id": "discord_1.conf",
                    "category": "discord",
                    "artifact": SERVICE_MANIFEST_PATH,
                    "dependencies": [],
                }],
            }],
        }))
        .unwrap();
        test_payload(&[
            (MACHINE_MAIN_BINARY, app),
            (SERVICE_MANIFEST_PATH, service),
            (MACHINE_RUNTIME_MANIFEST, &manifest),
        ])
    }

    fn update_test_fixture() -> (TestDirectory, MachinePaths, Vec<u8>, Vec<u8>) {
        let test = TestDirectory::new();
        let program_files = test.0.join("Program Files");
        let program_data = test.0.join("ProgramData");
        fs::create_dir_all(&program_files).unwrap();
        fs::create_dir_all(&program_data).unwrap();
        let paths = MachinePaths::from_roots(program_files, program_data);
        fs::create_dir_all(&paths.runtime_data).unwrap();
        fs::create_dir(&paths.install_root).unwrap();
        let old = authenticated_test_payload(b"old app", b"old service");
        let new = authenticated_test_payload(b"new app", b"new service");
        let old_payload = parse_machine_payload(&old).unwrap();
        materialize_machine_payload(&old_payload, &paths.install_root).unwrap();
        verify_install_layout(&paths).unwrap();
        (test, paths, old, new)
    }

    #[test]
    fn machine_update_rolls_back_at_every_precommit_fault_checkpoint() {
        let checkpoints = [
            MachineUpdateCheckpoint::Prepared,
            MachineUpdateCheckpoint::ServiceStopped,
            MachineUpdateCheckpoint::TargetBackedUp,
            MachineUpdateCheckpoint::TargetSwapped,
            MachineUpdateCheckpoint::SwappedJournaled,
            MachineUpdateCheckpoint::PayloadVerified,
            MachineUpdateCheckpoint::ServiceActivated,
        ];
        for checkpoint_to_fail in checkpoints {
            let (test, paths, old, new) = update_test_fixture();
            let new_payload = parse_machine_payload(&new).unwrap();
            let mut service = TestMachineUpdateService;
            let result = transactional_update_machine_payload(
                &paths,
                &new_payload,
                &format!("{:032x}", checkpoint_to_fail as u8 + 1),
                true,
                None,
                &mut service,
                |checkpoint| {
                    if checkpoint == checkpoint_to_fail {
                        Err(format!("fault at {checkpoint:?}"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err());
            assert_eq!(
                fs::read(paths.install_root.join(MACHINE_MAIN_BINARY)).unwrap(),
                b"old app"
            );
            assert!(!machine_update_paths(
                &paths,
                &format!("{:032x}", checkpoint_to_fail as u8 + 1)
            )
            .0
            .exists());
            assert!(!machine_update_paths(
                &paths,
                &format!("{:032x}", checkpoint_to_fail as u8 + 1)
            )
            .1
            .exists());
            assert!(load_machine_update_journal(&paths).unwrap().is_none());
            drop(test);
            let _ = old;
        }
    }

    #[test]
    fn machine_update_commit_recovery_keeps_new_payload_and_cleans_backup() {
        let (test, paths, _, new) = update_test_fixture();
        let new_payload = parse_machine_payload(&new).unwrap();
        let transaction_id = "e".repeat(32);
        let mut service = TestMachineUpdateService;
        let result = transactional_update_machine_payload(
            &paths,
            &new_payload,
            &transaction_id,
            true,
            None,
            &mut service,
            |checkpoint| {
                if checkpoint == MachineUpdateCheckpoint::Committed {
                    Err("simulated crash after durable commit".into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.unwrap_err().contains("committed"));
        assert_eq!(
            load_machine_update_journal(&paths).unwrap().unwrap().phase,
            MachineUpdatePhase::Committed
        );
        let journal = load_machine_update_journal(&paths).unwrap().unwrap();
        recover_machine_update_with_service(&paths, &journal, &mut service).unwrap();
        assert_eq!(
            fs::read(paths.install_root.join(MACHINE_MAIN_BINARY)).unwrap(),
            b"new app"
        );
        assert!(load_machine_update_journal(&paths).unwrap().is_none());
        assert!(!machine_update_paths(&paths, &transaction_id).1.exists());
        drop(test);
    }

    #[test]
    fn machine_update_recovery_finishes_a_partially_completed_rollback() {
        let (test, paths, _, new) = update_test_fixture();
        let new_payload = parse_machine_payload(&new).unwrap();
        let transaction_id = "d".repeat(32);
        let (stage, backup) = machine_update_paths(&paths, &transaction_id);
        fs::create_dir(&stage).unwrap();
        materialize_machine_payload(&new_payload, &stage).unwrap();
        let mut journal = MachineUpdateJournal::new(&transaction_id, true);
        journal
            .write_phase(&paths, MachineUpdatePhase::Prepared)
            .unwrap();
        journal
            .write_phase(&paths, MachineUpdatePhase::Committing)
            .unwrap();
        durable_rename(&paths.install_root, &backup).unwrap();
        durable_rename(&stage, &paths.install_root).unwrap();
        journal
            .write_phase(&paths, MachineUpdatePhase::Swapped)
            .unwrap();

        durable_rename(&paths.install_root, &stage).unwrap();
        durable_rename(&backup, &paths.install_root).unwrap();
        let mut service = TestMachineUpdateService;
        recover_machine_update_with_service(&paths, &journal, &mut service).unwrap();

        assert_eq!(
            fs::read(paths.install_root.join(MACHINE_MAIN_BINARY)).unwrap(),
            b"old app"
        );
        assert!(!stage.exists());
        assert!(load_machine_update_journal(&paths).unwrap().is_none());
        drop(test);
    }

    #[test]
    fn machine_update_journal_falls_back_to_previous_slot_after_partial_write() {
        let test = TestDirectory::new();
        let paths =
            MachinePaths::from_roots(test.0.join("Program Files"), test.0.join("ProgramData"));
        fs::create_dir_all(&paths.runtime_data).unwrap();
        let transaction_id = "f".repeat(32);
        let mut journal = MachineUpdateJournal::new(&transaction_id, true);
        journal
            .write_phase(&paths, MachineUpdatePhase::Prepared)
            .unwrap();
        journal
            .write_phase(&paths, MachineUpdatePhase::Committing)
            .unwrap();
        let slots = machine_update_journal_paths(&paths);
        fs::write(&slots[journal.sequence as usize % slots.len()], b"{").unwrap();
        assert_eq!(
            load_machine_update_journal(&paths).unwrap().unwrap().phase,
            MachineUpdatePhase::Prepared
        );
        drop(test);
    }

    #[test]
    fn stale_machine_results_are_bounded_and_keep_the_current_request() {
        let test = TestDirectory::new();
        let runtime_data = test.0.join("Runtime");
        fs::create_dir_all(&runtime_data).unwrap();
        let keep = "a".repeat(32);
        fs::write(
            runtime_data.join(format!("{MACHINE_RESULT_PREFIX}{keep}.json")),
            b"current",
        )
        .unwrap();
        for index in 0..40u8 {
            let id = format!("{index:02x}").repeat(16);
            fs::write(
                runtime_data.join(format!("{MACHINE_RESULT_PREFIX}{id}.json")),
                b"old",
            )
            .unwrap();
        }
        cleanup_stale_machine_results(&runtime_data, &keep).unwrap();
        let remaining = fs::read_dir(&runtime_data).unwrap().count();
        assert_eq!(remaining, MAX_RETAINED_MACHINE_RESULTS + 1);
        assert!(runtime_data
            .join(format!("{MACHINE_RESULT_PREFIX}{keep}.json"))
            .exists());
        drop(test);
    }

    fn test_payload(files: &[(&str, &[u8])]) -> Vec<u8> {
        let records: Vec<_> = files
            .iter()
            .map(|(path, bytes)| {
                serde_json::json!({
                    "path": path,
                    "size": bytes.len(),
                    "sha256": sha256_bytes(bytes),
                })
            })
            .collect();
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "product_id": MACHINE_PAYLOAD_PRODUCT_ID,
            "version": expected_machine_payload_version(),
            "files": records,
        }))
        .unwrap();
        let mut payload = Vec::with_capacity(
            MACHINE_PAYLOAD_HEADER_BYTES
                + manifest.len()
                + files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(),
        );
        payload.extend_from_slice(MACHINE_PAYLOAD_MAGIC);
        payload.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
        payload.extend_from_slice(&(files.len() as u32).to_le_bytes());
        payload.extend_from_slice(&manifest);
        for (_, bytes) in files {
            payload.extend_from_slice(bytes);
        }
        payload
    }

    fn required_test_files() -> [(&'static str, &'static [u8]); 3] {
        [
            (MACHINE_MAIN_BINARY, b"app fixture"),
            (SERVICE_MANIFEST_PATH, b"service fixture"),
            (MACHINE_RUNTIME_MANIFEST, b"manifest fixture"),
        ]
    }

    #[test]
    fn normal_launch_does_not_enter_internal_mode() {
        assert!(dispatch_arguments(&args(&["setup.exe"])).is_none());
        assert!(dispatch_arguments(&args(&["setup.exe", "--help"])).is_none());
    }

    #[test]
    fn malformed_internal_mode_fails_closed_without_running_worker() {
        let missing = dispatch_arguments(&args(&["setup.exe", INTERNAL_SWITCH])).unwrap();
        assert!(missing
            .unwrap_err()
            .contains("invalid internal worker arguments"));
        let extra = dispatch_arguments(&args(&[
            "setup.exe",
            INTERNAL_SWITCH,
            PROVISION_ACTION,
            r"C:\attacker",
        ]))
        .unwrap();
        assert!(extra
            .unwrap_err()
            .contains("invalid machine worker request id"));
        assert!(valid_machine_request_id(&"a".repeat(32)));
        assert!(!valid_machine_request_id(&"A".repeat(32)));
        assert!(!valid_machine_request_id("../result.json"));
    }

    #[test]
    fn machine_shortcut_flags_accept_only_canonical_boolean_tokens() {
        assert_eq!(
            MachineShortcutOptions::parse(OsStr::new("1"), OsStr::new("0")).unwrap(),
            MachineShortcutOptions {
                desktop: true,
                start_menu: false,
            }
        );
        for invalid in ["", "true", "01", "2", "-1"] {
            assert!(MachineShortcutOptions::parse(OsStr::new(invalid), OsStr::new("1")).is_err());
            assert!(MachineShortcutOptions::parse(OsStr::new("1"), OsStr::new(invalid)).is_err());
        }
    }

    #[test]
    fn native_machine_shortcuts_are_verified_and_removed_only_when_owned() {
        let _com = ComApartment::initialize().unwrap();
        let test = TestDirectory::new();
        let install = test.0.join("install");
        fs::create_dir(&install).unwrap();
        let target = install.join(MACHINE_MAIN_BINARY);
        let foreign_target = install.join("foreign.exe");
        fs::write(&target, b"app").unwrap();
        fs::write(&foreign_target, b"foreign").unwrap();
        let shortcut = test.0.join(MACHINE_SHORTCUT_NAME);
        let request_id = "a".repeat(32);

        create_owned_machine_shortcut(&shortcut, &target, &install, &request_id).unwrap();
        assert_eq!(
            path_key(&shortcut_target(&shortcut).unwrap()),
            path_key(&target)
        );
        remove_owned_machine_shortcut(&shortcut, &foreign_target).unwrap();
        assert!(shortcut.is_file());
        remove_owned_machine_shortcut(&shortcut, &target).unwrap();
        assert!(!shortcut.exists());

        create_owned_machine_shortcut(&shortcut, &foreign_target, &install, &request_id).unwrap();
        assert!(
            create_owned_machine_shortcut(&shortcut, &target, &install, &request_id)
                .unwrap_err()
                .contains("unowned")
        );
        remove_owned_machine_shortcut(&shortcut, &target).unwrap();
        assert!(shortcut.is_file());
    }

    #[test]
    fn machine_result_writer_uses_create_new_and_monotonic_reports() {
        let test = TestDirectory::new();
        let request_id = "d".repeat(32);
        let path = test.0.join("result.json");
        let mut writer = MachineResultWriter::create(&path, &request_id).unwrap();
        writer.running(25, "install").unwrap();
        let first: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(first["sequence"], 1);
        assert_eq!(first["state"], "running");

        writer.failed("fixture failure").unwrap();
        let second: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(second["sequence"], 2);
        assert_eq!(second["state"], "failed");
        assert_eq!(second["error"], "fixture failure");
        assert!(MachineResultWriter::create(&path, &request_id).is_err());
    }

    #[test]
    fn valid_machine_payload_is_parsed_and_authenticated() {
        let bytes = test_payload(&required_test_files());
        let payload = parse_machine_payload(&bytes).unwrap();
        assert_eq!(payload.files.len(), 3);
        assert_eq!(
            payload.files[0].relative,
            PathBuf::from(MACHINE_MAIN_BINARY)
        );
        assert_eq!(payload.files[0].bytes, b"app fixture");
    }

    #[test]
    fn machine_payload_rejects_traversal_and_case_insensitive_duplicates() {
        let traversal = test_payload(&[
            ("../obsession.exe", b"app"),
            (SERVICE_MANIFEST_PATH, b"service"),
            (MACHINE_RUNTIME_MANIFEST, b"manifest"),
        ]);
        assert!(parse_machine_payload(&traversal)
            .err()
            .unwrap()
            .contains("unsafe machine payload path"));

        let duplicate = test_payload(&[
            (MACHINE_MAIN_BINARY, b"app"),
            (SERVICE_MANIFEST_PATH, b"service"),
            (MACHINE_RUNTIME_MANIFEST, b"manifest"),
            ("assets/File.bin", b"first"),
            ("ASSETS/file.bin", b"second"),
        ]);
        assert!(parse_machine_payload(&duplicate)
            .err()
            .unwrap()
            .contains("duplicate machine payload path"));
    }

    #[test]
    fn machine_payload_rejects_corruption_trailing_bytes_and_missing_required_files() {
        let mut corrupt = test_payload(&required_test_files());
        *corrupt.last_mut().unwrap() ^= 0xff;
        assert!(parse_machine_payload(&corrupt)
            .err()
            .unwrap()
            .contains("machine payload hash mismatch"));

        let mut trailing = test_payload(&required_test_files());
        trailing.push(0);
        assert!(parse_machine_payload(&trailing)
            .err()
            .unwrap()
            .contains("unauthenticated trailing bytes"));

        let missing = test_payload(&[
            (MACHINE_MAIN_BINARY, b"app"),
            (SERVICE_MANIFEST_PATH, b"service"),
        ]);
        assert!(parse_machine_payload(&missing)
            .err()
            .unwrap()
            .contains("missing required file"));
    }

    #[test]
    fn machine_payload_materialization_requires_an_empty_root() {
        let test = TestDirectory::new();
        let bytes = test_payload(&required_test_files());
        let payload = parse_machine_payload(&bytes).unwrap();
        materialize_machine_payload(&payload, &test.0).unwrap();
        verify_materialized_machine_payload(&payload, &test.0).unwrap();
        assert_eq!(
            fs::read(test.0.join(MACHINE_MAIN_BINARY)).unwrap(),
            b"app fixture"
        );
        assert_eq!(
            fs::read(test.0.join(SERVICE_MANIFEST_PATH)).unwrap(),
            b"service fixture"
        );

        assert!(materialize_machine_payload(&payload, &test.0)
            .unwrap_err()
            .contains("not empty"));
    }

    #[test]
    fn existing_machine_install_must_match_the_embedded_payload_exactly() {
        let test = TestDirectory::new();
        let paths = MachinePaths::from_roots(test.0.clone(), test.0.join("ProgramData"));
        fs::create_dir(&paths.install_root).unwrap();
        let bytes = test_payload(&required_test_files());
        let payload = parse_machine_payload(&bytes).unwrap();
        materialize_machine_payload(&payload, &paths.install_root).unwrap();
        assert!(install_machine_payload_if_missing(&paths, &bytes, &"a".repeat(32), None).is_ok());

        fs::write(paths.install_root.join(MACHINE_MAIN_BINARY), b"corrupt").unwrap();
        assert!(verify_exact_machine_payload(&payload, &paths.install_root).is_err());
        fs::write(paths.install_root.join(MACHINE_MAIN_BINARY), b"app fixture").unwrap();
        fs::write(paths.install_root.join("unexpected.bin"), b"extra").unwrap();
        assert!(verify_exact_machine_payload(&payload, &paths.install_root)
            .unwrap_err()
            .contains("unexpected entry"));
    }

    #[test]
    fn protected_uninstaller_is_an_exact_copy_of_the_setup_image() {
        let test = TestDirectory::new();
        let source = test.0.join("setup.exe");
        let install = test.0.join("install");
        fs::write(&source, b"signed setup fixture").unwrap();
        fs::create_dir(&install).unwrap();
        let identity = setup_image_identity(&source).unwrap();
        materialize_machine_uninstaller(&install, &identity).unwrap();
        verify_machine_uninstaller(&install, &identity).unwrap();

        fs::write(install.join(MACHINE_UNINSTALL_BINARY), b"tampered").unwrap();
        assert!(verify_machine_uninstaller(&install, &identity).is_err());
    }

    #[test]
    fn uninstall_planner_only_defers_the_two_executables_and_active_result() {
        let test = TestDirectory::new();
        let root = test.0.join("owned-tree");
        let runtime = root.join(RUNTIME_DIRECTORY);
        fs::create_dir_all(&runtime).unwrap();
        fs::write(root.join(MACHINE_MAIN_BINARY), b"app").unwrap();
        fs::write(root.join("UNINSTALL.EXE"), b"uninstaller").unwrap();
        fs::write(root.join("ordinary.dll"), b"payload").unwrap();
        fs::write(runtime.join("engine.exe"), b"service").unwrap();
        let active_result = format!("{MACHINE_RESULT_PREFIX}{}.json", "a".repeat(32));
        let stale_result = format!("{MACHINE_RESULT_PREFIX}{}.json", "b".repeat(32));
        fs::write(runtime.join(&active_result), b"active").unwrap();
        fs::write(runtime.join(&stale_result), b"stale").unwrap();

        let preserve = [
            MACHINE_MAIN_BINARY.to_owned(),
            MACHINE_UNINSTALL_BINARY.to_owned(),
            payload_path_key(&Path::new(RUNTIME_DIRECTORY).join(&active_result)),
        ];
        let plan = plan_owned_machine_tree_removal(&root, &root, &preserve).unwrap();
        let relative_keys = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|path| payload_path_key(path.strip_prefix(&root).unwrap()))
                .collect::<BTreeSet<_>>()
        };

        assert_eq!(
            relative_keys(&plan.delete_after_reboot),
            BTreeSet::from([
                MACHINE_MAIN_BINARY.to_owned(),
                MACHINE_UNINSTALL_BINARY.to_owned(),
                format!(
                    "{}/{}",
                    RUNTIME_DIRECTORY.to_ascii_lowercase(),
                    active_result
                ),
            ])
        );
        assert_eq!(
            relative_keys(&plan.delete_now),
            BTreeSet::from([
                "ordinary.dll".to_owned(),
                format!("{}/engine.exe", RUNTIME_DIRECTORY.to_ascii_lowercase()),
                format!(
                    "{}/{}",
                    RUNTIME_DIRECTORY.to_ascii_lowercase(),
                    stale_result
                ),
            ])
        );
        assert_eq!(
            plan.directories_after_reboot,
            vec![runtime.clone(), root.clone()]
        );
        assert!(root.join(MACHINE_MAIN_BINARY).is_file());
        assert!(root.join("UNINSTALL.EXE").is_file());
        assert!(runtime.join(active_result).is_file());
        assert!(plan_owned_machine_tree_removal(&root, &test.0, &preserve).is_err());
    }

    #[test]
    fn hklm_uninstall_command_uses_only_the_fixed_protected_binary() {
        let paths = MachinePaths::from_roots(
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\ProgramData"),
        );
        assert_eq!(
            machine_uninstall_command(&paths).unwrap(),
            r#""C:\Program Files\Obsession\uninstall.exe" --uninstall"#
        );
    }

    #[test]
    fn machine_paths_are_fixed_below_known_roots() {
        let paths = MachinePaths::from_roots(
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\ProgramData"),
        );
        assert_eq!(
            paths.service_binary,
            PathBuf::from(r"C:\Program Files\Obsession\runtime\Obsession.Runtime.exe")
        );
        assert_eq!(
            paths.runtime_data,
            PathBuf::from(r"C:\ProgramData\Obsession\Runtime")
        );
    }

    #[test]
    fn service_command_is_quoted_and_rejects_command_line_injection() {
        assert_eq!(
            service_binary_command(Path::new(
                r"C:\Program Files\Obsession\runtime\Obsession.Runtime.exe"
            ))
            .unwrap(),
            r#""C:\Program Files\Obsession\runtime\Obsession.Runtime.exe""#
        );
        assert!(service_binary_command(Path::new("C:\\bad\" -arg")).is_err());
    }

    #[test]
    fn service_stop_uses_control_code_not_handle_access_right() {
        assert_eq!(service_stop_control_code(), SERVICE_CONTROL_STOP);
        assert_eq!(service_stop_control_code(), 1);
        assert_ne!(service_stop_control_code(), SERVICE_STOP);
    }

    #[test]
    fn service_manifest_record_is_hashed_unique_and_mandatory() {
        let test = TestDirectory::new();
        let service = test.0.join(SERVICE_FILE_NAME);
        let manifest = test.0.join(MANIFEST_FILE_NAME);
        let service_bytes = b"service fixture";
        fs::write(&service, service_bytes).unwrap();
        let hash: String = Sha256::digest(service_bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let record = serde_json::json!({
            "path": SERVICE_MANIFEST_PATH,
            "size": service_bytes.len(),
            "sha256": hash,
        });
        let write_manifest = |records: Vec<serde_json::Value>| {
            fs::write(
                &manifest,
                serde_json::to_vec(&serde_json::json!({
                    "schema_version": 1,
                    "engines": [{
                        "engine": "legacy",
                        "executable": "bin/winws.exe",
                        "files": records,
                        "strategies": [],
                    }],
                }))
                .unwrap(),
            )
            .unwrap();
        };

        write_manifest(vec![record.clone()]);
        assert!(verify_service_manifest_entry(&manifest, &service).is_ok());

        write_manifest(vec![record.clone(), record.clone()]);
        assert!(verify_service_manifest_entry(&manifest, &service)
            .unwrap_err()
            .contains("more than once"));

        write_manifest(vec![]);
        assert!(verify_service_manifest_entry(&manifest, &service)
            .unwrap_err()
            .contains("does not authenticate"));

        let mut corrupt = record;
        corrupt["sha256"] = serde_json::Value::String("0".repeat(64));
        write_manifest(vec![corrupt]);
        assert!(verify_service_manifest_entry(&manifest, &service)
            .unwrap_err()
            .contains("hash does not match"));
    }

    #[test]
    fn state_descriptor_is_native_and_non_null() {
        let descriptor = LocalSecurityDescriptor::state_directory().unwrap();
        let (owner, dacl) = descriptor.owner_and_dacl().unwrap();
        assert!(!owner.is_invalid());
        assert!(!dacl.is_null());
    }

    #[test]
    fn polling_interval_is_bounded() {
        let immediate = SERVICE_STATUS_PROCESS {
            dwWaitHint: 0,
            ..Default::default()
        };
        assert_eq!(status_poll_interval(&immediate), Duration::from_millis(100));
        let slow = SERVICE_STATUS_PROCESS {
            dwWaitHint: 60_000,
            ..Default::default()
        };
        assert_eq!(status_poll_interval(&slow), Duration::from_millis(500));
    }
}
