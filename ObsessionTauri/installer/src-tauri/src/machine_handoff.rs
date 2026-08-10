//! Medium-integrity launcher for the fixed native machine worker.
//!
//! The graphical setup never elevates itself in-place. It keeps its own image
//! open without write/delete sharing, then asks ShellExecuteEx to start the
//! exact same image with the worker's fixed argv and waits for a bounded result.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_SHARE_READ};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
    SHELLEXECUTEINFOW,
};

use crate::machine_worker::{
    machine_result_path, INTERNAL_SWITCH, PROVISION_ACTION, UNINSTALL_ACTION,
};

const MACHINE_WORKER_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MACHINE_REPORT_POLL: Duration = Duration::from_millis(100);
const MAX_MACHINE_REPORT_BYTES: u64 = 16 * 1024;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: this guard owns the process handle returned by
            // ShellExecuteExW and closes it exactly once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn wide_path(path: &Path) -> Result<Vec<u16>, String> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.is_empty() || value.contains(&0) {
        return Err("setup executable path is empty or contains NUL".into());
    }
    value.push(0);
    Ok(value)
}

fn machine_request_id() -> String {
    let sequence = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = Sha256::new();
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(sequence.to_le_bytes());
    hasher.update(timestamp.to_le_bytes());
    let digest = hasher.finalize();
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn worker_parameters(
    action: &str,
    request_id: &str,
    shortcut_options: Option<(bool, bool)>,
) -> String {
    match shortcut_options {
        Some((desktop, start_menu)) => format!(
            "{INTERNAL_SWITCH} {action} {request_id} {} {}",
            u8::from(desktop),
            u8::from(start_menu)
        ),
        None => format!("{INTERNAL_SWITCH} {action} {request_id}"),
    }
}

fn lock_setup_image(path: &Path) -> Result<File, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect setup executable: {error}"))?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("setup executable is missing, not a file, or a reparse point".into());
    }
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(path)
        .map_err(|error| format!("could not lock setup executable against replacement: {error}"))
}

pub(crate) fn lock_current_setup_image() -> Result<File, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not resolve setup image: {error}"))?;
    lock_setup_image(&executable)
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MachineReportState {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineWorkerReport {
    schema_version: u32,
    request_id: String,
    sequence: u32,
    pct: u32,
    stage: String,
    state: MachineReportState,
    error: Option<String>,
}

fn canonical_stage(stage: &str) -> Option<&'static str> {
    match stage {
        "prepare" => Some("prepare"),
        "install" => Some("install"),
        "verify" => Some("verify"),
        "finish" => Some("finish"),
        _ => None,
    }
}

fn read_machine_report(
    path: &Path,
    request_id: &str,
    final_read: bool,
) -> Result<Option<MachineWorkerReport>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Не удалось прочитать статус install worker: {error}"
            ))
        }
    };
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MACHINE_REPORT_BYTES {
        if final_read {
            return Err("Install worker оставил некорректный result-файл.".into());
        }
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| format!("Не удалось прочитать результат install worker: {error}"))?;
    let report: MachineWorkerReport = match serde_json::from_slice(&bytes) {
        Ok(report) => report,
        Err(_) if !final_read => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Install worker оставил повреждённый result-файл: {error}"
            ))
        }
    };
    if report.schema_version != 1
        || report.request_id != request_id
        || report.sequence == 0
        || report.pct > 100
        || canonical_stage(&report.stage).is_none()
        || report
            .error
            .as_ref()
            .is_some_and(|error| error.len() > 4096)
    {
        return Err("Install worker вернул результат с неверной идентичностью или схемой.".into());
    }
    Ok(Some(report))
}

pub(crate) fn provision_machine_runtime(
    desktop: bool,
    start_menu: bool,
    progress: impl FnMut(u32, &'static str),
) -> Result<(), String> {
    run_machine_worker(PROVISION_ACTION, Some((desktop, start_menu)), progress)
}

pub(crate) fn uninstall_machine_runtime(
    progress: impl FnMut(u32, &'static str),
) -> Result<(), String> {
    run_machine_worker(UNINSTALL_ACTION, None, progress)
}

fn run_machine_worker(
    action: &str,
    shortcut_options: Option<(bool, bool)>,
    mut progress: impl FnMut(u32, &'static str),
) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Err("machine operations are available only in a release setup build".into());
    }
    if action != PROVISION_ACTION && action != UNINSTALL_ACTION {
        return Err("unsupported fixed machine worker action".into());
    }
    if (action == PROVISION_ACTION) != shortcut_options.is_some() {
        return Err("invalid fixed machine worker shortcut options".into());
    }

    let request_id = machine_request_id();
    let result_path = machine_result_path(&request_id)?;
    if result_path.exists() {
        return Err("machine worker request id unexpectedly collided".into());
    }
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not resolve setup image: {error}"))?;
    let _image_lock = lock_setup_image(&executable)?;
    let executable_wide = wide_path(&executable)?;
    let mut parameters_wide: Vec<u16> = worker_parameters(action, &request_id, shortcut_options)
        .encode_utf16()
        .collect();
    parameters_wide.push(0);
    let mut execution = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(executable_wide.as_ptr()),
        lpParameters: PCWSTR(parameters_wide.as_ptr()),
        nShow: 0,
        ..Default::default()
    };

    // SAFETY: all UTF-16 buffers remain live for the call and the structure is
    // initialized with the documented size and flags.
    if let Err(error) = unsafe { ShellExecuteExW(&mut execution) } {
        if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
            return Err("Запрос прав администратора был отменён пользователем.".into());
        }
        return Err(format!(
            "Не удалось запустить защищённый install worker: {error}"
        ));
    }
    if execution.hProcess.is_invalid() {
        return Err("Windows did not return a machine worker process handle".into());
    }
    let process = ProcessHandle(execution.hProcess);
    let started = Instant::now();
    let mut latest_sequence = 0u32;
    loop {
        let poll = u32::try_from(MACHINE_REPORT_POLL.as_millis()).unwrap_or(100);
        // SAFETY: the ShellExecuteEx process handle remains owned by `process`.
        let wait = unsafe { WaitForSingleObject(process.0, poll) };
        if let Some(report) = read_machine_report(&result_path, &request_id, false)? {
            if report.sequence > latest_sequence {
                latest_sequence = report.sequence;
                if let Some(stage) = canonical_stage(&report.stage) {
                    progress(report.pct, stage);
                }
            }
        }
        if wait == WAIT_OBJECT_0 {
            break;
        }
        if wait != WAIT_TIMEOUT {
            return Err(format!(
                "Не удалось дождаться install worker: {}",
                std::io::Error::last_os_error()
            ));
        }
        if started.elapsed() >= MACHINE_WORKER_TIMEOUT {
            // SAFETY: a timed-out worker must not continue mutating machine
            // state after the medium setup reports failure.
            let _ = unsafe { TerminateProcess(process.0, 1) };
            let _ = unsafe { WaitForSingleObject(process.0, 30_000) };
            return Err("Защищённый install worker не завершился за 10 минут.".into());
        }
    }

    let mut exit_code = 0u32;
    // SAFETY: the process is signaled and the output pointer is valid.
    unsafe { GetExitCodeProcess(process.0, &mut exit_code) }
        .map_err(|error| format!("Не удалось прочитать результат install worker: {error}"))?;
    let final_report = read_machine_report(&result_path, &request_id, true)?;
    if exit_code != 0 {
        if let Some(report) = final_report {
            if report.state == MachineReportState::Failed {
                if let Some(error) = report.error {
                    return Err(error);
                }
            }
        }
        return Err(format!(
            "Защищённый install worker завершился с кодом {exit_code}."
        ));
    }
    let Some(report) = final_report else {
        return Err("Install worker завершился без защищённого result-файла.".into());
    };
    if report.state != MachineReportState::Succeeded {
        return Err("Install worker не подтвердил успешное завершение операции.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_worker_command_line_is_fixed() {
        let request_id = "a".repeat(32);
        assert_eq!(
            worker_parameters(PROVISION_ACTION, &request_id, Some((true, false))),
            format!("--obsession-machine-worker-v1 provision {request_id} 1 0")
        );
        assert_eq!(
            worker_parameters(UNINSTALL_ACTION, &request_id, None),
            format!("--obsession-machine-worker-v1 uninstall {request_id}")
        );
    }

    #[test]
    fn machine_request_ids_are_bounded_lowercase_hex_and_unique() {
        let first = machine_request_id();
        let second = machine_request_id();
        assert_eq!(first.len(), 32);
        assert!(first
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')));
        assert_ne!(first, second);
    }

    #[test]
    fn machine_report_parser_rejects_partial_and_wrong_identity_reports() {
        let request_id = "b".repeat(32);
        let path = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("machine-report-{}.json", machine_request_id()));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{").unwrap();
        assert!(read_machine_report(&path, &request_id, false)
            .unwrap()
            .is_none());
        assert!(read_machine_report(&path, &request_id, true).is_err());

        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "request_id": "c".repeat(32),
                "sequence": 1,
                "pct": 25,
                "stage": "install",
                "state": "running",
                "error": null,
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(read_machine_report(&path, &request_id, true).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn current_test_image_can_be_locked_read_only() {
        let lock = lock_current_setup_image().unwrap();
        assert!(lock.metadata().unwrap().is_file());
    }
}
