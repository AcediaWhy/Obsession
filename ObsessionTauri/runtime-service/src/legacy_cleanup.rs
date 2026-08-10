//! Narrow LocalSystem cleanup for obsolete per-user Obsession installations.
//!
//! The wire request has no payload. Both roots are derived from the authenticated
//! named-pipe client's SID through HKLM ProfileList and fixed relative paths.

#![cfg(windows)]

mod firewall;
mod hosts;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::{Error as WindowsError, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_FILES, ERROR_SUCCESS, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandle, MoveFileExW, BY_HANDLE_FILE_INFORMATION,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    MOVEFILE_DELAY_UNTIL_REBOOT, OPEN_EXISTING,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY_LOCAL_MACHINE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RRF_SUBKEY_WOW6464KEY,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};

use crate::BackendError;

const PROFILE_LIST_PREFIX: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
const PROFILE_IMAGE_VALUE: &str = "ProfileImagePath";
const MAX_PROFILE_PATH_UTF16: usize = 32_768;
const MAX_PROCESS_PATH_UTF16: usize = 32_768;
const MAX_PROCESSES_SCANNED: usize = 16_384;
const MAX_MATCHING_PROCESSES: usize = 64;
const MAX_LEGACY_TREE_ENTRIES: usize = 8_192;
const MAX_LEGACY_TREE_DEPTH: usize = 32;
const PROCESS_EXIT_CODE: u32 = 0xE000_0B51;
const PROCESS_SYNCHRONIZE: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);
const PROCESS_TERMINATION_TIMEOUT: Duration = Duration::from_secs(2);
const QUARANTINE_ATTEMPTS: usize = 32;

static NEXT_QUARANTINE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub(crate) enum LegacyCleanupError {
    InvalidIdentity,
    ProfileLookup(u32),
    UnsafeProfilePath,
    ReparsePoint(PathBuf),
    EntryBoundExceeded,
    DepthBoundExceeded,
    ProcessBoundExceeded,
    HostsStateInvalid(&'static str),
    HostsRecoveryRequired(&'static str),
    FirewallRuleInvalid(String),
    FirewallRuleBoundExceeded,
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Windows {
        operation: &'static str,
        source: WindowsError,
    },
}

impl LegacyCleanupError {
    pub(crate) fn backend_error(&self) -> BackendError {
        match self {
            Self::InvalidIdentity => BackendError::InvalidRequest,
            Self::UnsafeProfilePath
            | Self::ReparsePoint(_)
            | Self::EntryBoundExceeded
            | Self::DepthBoundExceeded
            | Self::ProcessBoundExceeded
            | Self::HostsStateInvalid(_)
            | Self::HostsRecoveryRequired(_)
            | Self::FirewallRuleInvalid(_)
            | Self::FirewallRuleBoundExceeded => BackendError::ProtectedResourceInvalid,
            Self::ProfileLookup(_) | Self::Io { .. } | Self::Windows { .. } => {
                BackendError::RuntimeFailed
            }
        }
    }
}

impl std::fmt::Display for LegacyCleanupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentity => formatter.write_str("authenticated user SID is invalid"),
            Self::ProfileLookup(code) => {
                write!(
                    formatter,
                    "ProfileList lookup failed with Win32 error {code}"
                )
            }
            Self::UnsafeProfilePath => {
                formatter.write_str("ProfileList returned an unsafe profile path")
            }
            Self::ReparsePoint(path) => {
                write!(
                    formatter,
                    "legacy cleanup rejected reparse point {}",
                    path.display()
                )
            }
            Self::EntryBoundExceeded => {
                formatter.write_str("legacy tree exceeds its bounded entry limit")
            }
            Self::DepthBoundExceeded => {
                formatter.write_str("legacy tree exceeds its bounded depth limit")
            }
            Self::ProcessBoundExceeded => {
                formatter.write_str("legacy process scan exceeds its bounded limit")
            }
            Self::HostsStateInvalid(reason) => {
                write!(formatter, "legacy hosts state is invalid: {reason}")
            }
            Self::HostsRecoveryRequired(reason) => {
                write!(formatter, "legacy hosts requires manual recovery: {reason}")
            }
            Self::FirewallRuleInvalid(name) => {
                write!(
                    formatter,
                    "legacy firewall rule has unexpected properties: {name}"
                )
            }
            Self::FirewallRuleBoundExceeded => {
                formatter.write_str("firewall rule enumeration exceeded its bounded limit")
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "could not {operation} {}: {source}",
                path.display()
            ),
            Self::Windows { operation, source } => {
                write!(formatter, "Windows {operation} failed: {source}")
            }
        }
    }
}

impl std::error::Error for LegacyCleanupError {}

#[derive(Debug, Default, PartialEq, Eq)]
struct LegacyTreePlan {
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

pub(crate) fn cleanup_authenticated_user(user_sid: &str) -> Result<(), LegacyCleanupError> {
    if !super::valid_user_sid_text(user_sid) {
        return Err(LegacyCleanupError::InvalidIdentity);
    }
    let profile = profile_path_for_sid(user_sid)?;
    validate_profile_path(&profile)?;

    let [local_root, roaming_root] = legacy_roots(&profile);
    let roots = [&local_root, &roaming_root];
    let mut process_roots = Vec::new();
    for root in roots {
        preflight_legacy_root(&profile, root)?;
        if root.exists() {
            process_roots.push(root.clone());
        }
    }
    if !process_roots.is_empty() {
        terminate_legacy_processes(&process_roots)?;
    }

    // The executable-bearing Local tree can be removed immediately. Roaming
    // contains the only byte-exact legacy hosts recovery point and therefore
    // remains untouched until recovery has succeeded or proved unnecessary.
    if local_root.exists() {
        remove_legacy_root(&local_root)?;
    }

    hosts::recover_legacy_hosts(&roaming_root)?;
    firewall::remove_owned_legacy_rules()?;

    if roaming_root.exists() {
        remove_legacy_root(&roaming_root)?;
    }
    Ok(())
}

fn remove_legacy_root(root: &Path) -> Result<(), LegacyCleanupError> {
    // Re-check the leaf immediately before the path-changing operation.
    reject_plain_directory(root)?;
    let quarantine = quarantine_legacy_root(root)?;
    let plan =
        plan_legacy_tree_with_limits(&quarantine, MAX_LEGACY_TREE_ENTRIES, MAX_LEGACY_TREE_DEPTH)?;
    remove_planned_tree(&quarantine, plan)
}

fn profile_path_for_sid(user_sid: &str) -> Result<PathBuf, LegacyCleanupError> {
    let subkey = wide_string(&format!(r"{PROFILE_LIST_PREFIX}\{user_sid}"))?;
    let value_name = wide_string(PROFILE_IMAGE_VALUE)?;
    // Do not expand REG_EXPAND_SZ in the LocalSystem service environment: a
    // profile value containing user-scoped variables could otherwise resolve
    // to the service account. Normal ProfileList values are absolute paths;
    // anything else fails closed below.
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_SUBKEY_WOW6464KEY | RRF_NOEXPAND;
    let mut byte_count = 0u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            flags,
            None,
            None,
            Some(&mut byte_count),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(LegacyCleanupError::ProfileLookup(status.0));
    }
    if byte_count < 2
        || byte_count as usize > MAX_PROFILE_PATH_UTF16 * std::mem::size_of::<u16>()
        || !byte_count.is_multiple_of(2)
    {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }

    let mut buffer = vec![0u16; byte_count as usize / 2];
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut byte_count),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(LegacyCleanupError::ProfileLookup(status.0));
    }
    if byte_count < 2
        || byte_count as usize > buffer.len() * std::mem::size_of::<u16>()
        || !byte_count.is_multiple_of(2)
    {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    let units = byte_count as usize / 2;
    let nul = buffer[..units]
        .iter()
        .position(|unit| *unit == 0)
        .ok_or(LegacyCleanupError::UnsafeProfilePath)?;
    if nul == 0 || buffer[nul + 1..units].iter().any(|unit| *unit != 0) {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..nul])))
}

fn validate_profile_path(profile: &Path) -> Result<(), LegacyCleanupError> {
    if profile.to_string_lossy().contains('%') {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    let mut components = profile.components();
    let valid_prefix = matches!(
        components.next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
    );
    if !valid_prefix || !matches!(components.next(), Some(Component::RootDir)) {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    let remainder: Vec<_> = components.collect();
    if remainder.is_empty()
        || remainder
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    Ok(())
}

fn legacy_roots(profile: &Path) -> [PathBuf; 2] {
    [
        profile.join("AppData").join("Local").join("Obsession"),
        profile.join("AppData").join("Roaming").join("Obsession"),
    ]
}

fn preflight_legacy_root(profile: &Path, root: &Path) -> Result<(), LegacyCleanupError> {
    if !path_belongs_to_root(root, profile) {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    let parent = root.parent().ok_or(LegacyCleanupError::UnsafeProfilePath)?;
    reject_plain_directory_chain(parent)?;
    match fs::symlink_metadata(root) {
        Ok(metadata) => reject_plain_directory_metadata(root, &metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error("inspect", root, source)),
    }
}

pub(super) fn reject_plain_directory_chain(target: &Path) -> Result<(), LegacyCleanupError> {
    let mut current = PathBuf::new();
    let mut rooted = false;
    for component in target.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::RootDir) {
            rooted = true;
        }
        if rooted {
            reject_plain_directory(&current)?;
        }
    }
    Ok(())
}

pub(super) fn reject_plain_directory(path: &Path) -> Result<(), LegacyCleanupError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io_error("inspect", path, source))?;
    reject_plain_directory_metadata(path, &metadata)
}

pub(super) fn reject_plain_directory_metadata(
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(), LegacyCleanupError> {
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(LegacyCleanupError::ReparsePoint(path.to_path_buf()));
    }
    if !metadata.is_dir() {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    Ok(())
}

fn terminate_legacy_processes(roots: &[PathBuf]) -> Result<(), LegacyCleanupError> {
    let snapshot = OwnedHandle(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|source| {
            LegacyCleanupError::Windows {
                operation: "process snapshot",
                source,
            }
        })?,
    );
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if let Err(error) = unsafe { Process32FirstW(snapshot.0, &mut entry) } {
        if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
            return Ok(());
        }
        return Err(LegacyCleanupError::Windows {
            operation: "first process enumeration",
            source: error,
        });
    }

    let current_process = unsafe { GetCurrentProcessId() };
    let mut scanned = 0usize;
    let mut terminated = Vec::new();
    loop {
        scanned += 1;
        if scanned > MAX_PROCESSES_SCANNED {
            return Err(LegacyCleanupError::ProcessBoundExceeded);
        }
        let process_id = entry.th32ProcessID;
        if process_id != 0 && process_id != current_process {
            let access =
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
            if let Ok(process) = unsafe { OpenProcess(access, false, process_id) } {
                let process = OwnedHandle(process);
                if process_image_path(process.0).ok().is_some_and(|image| {
                    roots.iter().any(|root| path_belongs_to_root(&image, root))
                }) {
                    if terminated.len() >= MAX_MATCHING_PROCESSES {
                        return Err(LegacyCleanupError::ProcessBoundExceeded);
                    }
                    unsafe { TerminateProcess(process.0, PROCESS_EXIT_CODE) }.map_err(
                        |source| LegacyCleanupError::Windows {
                            operation: "legacy process termination",
                            source,
                        },
                    )?;
                    terminated.push(process);
                }
            }
        }

        match unsafe { Process32NextW(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => break,
            Err(source) => {
                return Err(LegacyCleanupError::Windows {
                    operation: "process enumeration",
                    source,
                })
            }
        }
    }

    let deadline = Instant::now() + PROCESS_TERMINATION_TIMEOUT;
    for process in terminated {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(LegacyCleanupError::Windows {
                operation: "legacy process termination timeout",
                source: WindowsError::from_win32(),
            });
        }
        let wait_ms = remaining.as_millis().min(u32::MAX as u128).max(1) as u32;
        match unsafe { WaitForSingleObject(process.0, wait_ms) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT | WAIT_FAILED => {
                return Err(LegacyCleanupError::Windows {
                    operation: "legacy process termination wait",
                    source: WindowsError::from_win32(),
                })
            }
            _ => {
                return Err(LegacyCleanupError::Windows {
                    operation: "unexpected legacy process wait result",
                    source: WindowsError::from_win32(),
                })
            }
        }
    }
    Ok(())
}

fn process_image_path(process: HANDLE) -> Result<PathBuf, LegacyCleanupError> {
    let mut buffer = vec![0u16; MAX_PROCESS_PATH_UTF16];
    let mut length = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|source| LegacyCleanupError::Windows {
        operation: "process image query",
        source,
    })?;
    if length == 0 || length as usize >= buffer.len() {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}

fn quarantine_legacy_root(root: &Path) -> Result<PathBuf, LegacyCleanupError> {
    let parent = root.parent().ok_or(LegacyCleanupError::UnsafeProfilePath)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for _ in 0..QUARANTINE_ATTEMPTS {
        let id = NEXT_QUARANTINE_ID.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".obsession-legacy-cleanup-{}-{nonce:x}-{id:x}",
            unsafe { GetCurrentProcessId() }
        ));
        match fs::rename(root, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(io_error("quarantine", root, source)),
        }
    }
    Err(LegacyCleanupError::EntryBoundExceeded)
}

fn plan_legacy_tree_with_limits(
    root: &Path,
    max_entries: usize,
    max_depth: usize,
) -> Result<LegacyTreePlan, LegacyCleanupError> {
    let mut plan = LegacyTreePlan::default();
    let mut entries = 0usize;
    plan_directory(
        root,
        root,
        0,
        max_entries,
        max_depth,
        &mut entries,
        &mut plan,
    )?;
    Ok(plan)
}

#[allow(clippy::too_many_arguments)]
fn plan_directory(
    root: &Path,
    directory: &Path,
    depth: usize,
    max_entries: usize,
    max_depth: usize,
    entries: &mut usize,
    plan: &mut LegacyTreePlan,
) -> Result<(), LegacyCleanupError> {
    if depth > max_depth {
        return Err(LegacyCleanupError::DepthBoundExceeded);
    }
    *entries = entries
        .checked_add(1)
        .ok_or(LegacyCleanupError::EntryBoundExceeded)?;
    if *entries > max_entries {
        return Err(LegacyCleanupError::EntryBoundExceeded);
    }

    let _directory_lock = open_locked_plain_directory(directory)?;
    let iterator =
        fs::read_dir(directory).map_err(|source| io_error("enumerate", directory, source))?;
    for entry in iterator {
        let entry = entry.map_err(|source| io_error("enumerate", directory, source))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| LegacyCleanupError::UnsafeProfilePath)?
            .to_path_buf();
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(LegacyCleanupError::UnsafeProfilePath);
        }
        let metadata =
            fs::symlink_metadata(&path).map_err(|source| io_error("inspect", &path, source))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(LegacyCleanupError::ReparsePoint(path));
        }
        if metadata.is_dir() {
            plan_directory(
                root,
                &path,
                depth + 1,
                max_entries,
                max_depth,
                entries,
                plan,
            )?;
        } else if metadata.is_file() {
            *entries = entries
                .checked_add(1)
                .ok_or(LegacyCleanupError::EntryBoundExceeded)?;
            if *entries > max_entries {
                return Err(LegacyCleanupError::EntryBoundExceeded);
            }
            plan.files.push(relative);
        } else {
            return Err(LegacyCleanupError::UnsafeProfilePath);
        }
    }
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| LegacyCleanupError::UnsafeProfilePath)?
        .to_path_buf();
    plan.directories.push(relative);
    Ok(())
}

fn open_locked_plain_directory(path: &Path) -> Result<OwnedHandle, LegacyCleanupError> {
    let wide = wide_path(path)?;
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|source| LegacyCleanupError::Windows {
        operation: "legacy directory open",
        source,
    })?;
    let handle = OwnedHandle(handle);
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle.0, &mut information) }.map_err(|source| {
        LegacyCleanupError::Windows {
            operation: "legacy directory inspection",
            source,
        }
    })?;
    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(LegacyCleanupError::ReparsePoint(path.to_path_buf()));
    }
    if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    Ok(handle)
}

fn remove_planned_tree(
    quarantine_root: &Path,
    plan: LegacyTreePlan,
) -> Result<(), LegacyCleanupError> {
    for relative in plan.files {
        let path = checked_plan_path(quarantine_root, &relative)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                    return Err(LegacyCleanupError::ReparsePoint(path));
                }
                if !metadata.is_file() {
                    return Err(LegacyCleanupError::UnsafeProfilePath);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(io_error("inspect", &path, source)),
        }
        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != io::ErrorKind::NotFound {
                schedule_delete_after_reboot(quarantine_root, &path)?;
            }
        }
    }

    for relative in plan.directories {
        let path = checked_plan_path(quarantine_root, &relative)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => reject_plain_directory_metadata(&path, &metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(source) => return Err(io_error("inspect", &path, source)),
        }
        match fs::remove_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => schedule_delete_after_reboot(quarantine_root, &path)?,
        }
    }
    Ok(())
}

fn checked_plan_path(root: &Path, relative: &Path) -> Result<PathBuf, LegacyCleanupError> {
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
        && !relative.as_os_str().is_empty()
    {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    let path = root.join(relative);
    if path_key(&path) != path_key(root) && !path_belongs_to_root(&path, root) {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    Ok(path)
}

fn schedule_delete_after_reboot(root: &Path, path: &Path) -> Result<(), LegacyCleanupError> {
    if path_key(path) != path_key(root) && !path_belongs_to_root(path, root) {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    let wide = wide_path(path)?;
    unsafe {
        MoveFileExW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    }
    .map_err(|source| LegacyCleanupError::Windows {
        operation: "legacy reboot-delete scheduling",
        source,
    })
}

pub(super) fn path_belongs_to_root(path: &Path, root: &Path) -> bool {
    let path = path_key(path);
    let root = path_key(root);
    path.len() > root.len()
        && path.starts_with(&root)
        && path.as_bytes().get(root.len()) == Some(&b'\\')
}

fn path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_owned();
    }
    value.trim_end_matches('\\').to_lowercase()
}

fn wide_string(value: &str) -> Result<Vec<u16>, LegacyCleanupError> {
    let mut wide: Vec<u16> = value.encode_utf16().collect();
    if wide.is_empty() || wide.contains(&0) || wide.len() >= MAX_PROFILE_PATH_UTF16 {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    wide.push(0);
    Ok(wide)
}

pub(super) fn wide_path(path: &Path) -> Result<Vec<u16>, LegacyCleanupError> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.is_empty() || wide.contains(&0) || wide.len() >= MAX_PROFILE_PATH_UTF16 {
        return Err(LegacyCleanupError::UnsafeProfilePath);
    }
    wide.push(0);
    Ok(wide)
}

pub(super) fn io_error(
    operation: &'static str,
    path: &Path,
    source: io::Error,
) -> LegacyCleanupError {
    LegacyCleanupError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "obsession-legacy-cleanup-test-{}-{nonce}",
                unsafe { GetCurrentProcessId() }
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn profile_path_must_be_a_normal_drive_absolute_path() {
        assert!(validate_profile_path(Path::new(r"C:\Users\Alice")).is_ok());
        for path in [
            Path::new(r"..\Alice"),
            Path::new(r"\\server\profiles\Alice"),
            Path::new(r"C:\Users\..\Admin"),
            Path::new(r"%SystemDrive%\Users\Alice"),
            Path::new(r"C:\"),
        ] {
            assert!(matches!(
                validate_profile_path(path),
                Err(LegacyCleanupError::UnsafeProfilePath)
            ));
        }
    }

    #[test]
    fn process_path_membership_requires_a_component_boundary() {
        let root = Path::new(r"C:\Users\Alice\AppData\Local\Obsession");
        assert!(path_belongs_to_root(
            Path::new(r"C:\Users\Alice\AppData\Local\Obsession\Obsession.exe"),
            root
        ));
        assert!(path_belongs_to_root(
            Path::new(r"\\?\C:\Users\Alice\AppData\Local\Obsession\bin\winws.exe"),
            root
        ));
        assert!(!path_belongs_to_root(
            Path::new(r"C:\Users\Alice\AppData\Local\Obsession-Evil\Obsession.exe"),
            root
        ));
        assert!(!path_belongs_to_root(
            Path::new(r"C:\Program Files\Obsession\Obsession.exe"),
            root
        ));
    }

    #[test]
    fn tree_planning_is_read_only_postorder_and_bounded() {
        let test = TestRoot::new();
        let child = test.0.join("bin");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("winws.exe"), b"legacy").unwrap();
        fs::write(test.0.join("Obsession.exe"), b"legacy").unwrap();

        let plan = plan_legacy_tree_with_limits(&test.0, 8, 4).unwrap();
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.directories, [PathBuf::from("bin"), PathBuf::new()]);
        assert!(test.0.join("Obsession.exe").is_file());
        assert!(child.join("winws.exe").is_file());

        assert!(matches!(
            plan_legacy_tree_with_limits(&test.0, 2, 4),
            Err(LegacyCleanupError::EntryBoundExceeded)
        ));
        assert!(test.0.join("Obsession.exe").is_file());
    }

    #[test]
    fn planner_rejects_reparse_entries_without_following_them() {
        use std::os::windows::fs::symlink_file;

        let test = TestRoot::new();
        let outside = test.0.with_extension("outside");
        fs::write(&outside, b"outside").unwrap();
        let link = test.0.join("payload.exe");
        if symlink_file(&outside, &link).is_err() {
            let _ = fs::remove_file(&outside);
            return;
        }
        assert!(matches!(
            plan_legacy_tree_with_limits(&test.0, 8, 4),
            Err(LegacyCleanupError::ReparsePoint(path)) if path == link
        ));
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn quarantine_and_removal_do_not_touch_a_neighbor_tree() {
        let test = TestRoot::new();
        let legacy = test.0.join("Obsession");
        let neighbor = test.0.join("Obsession-UserFiles");
        fs::create_dir(&legacy).unwrap();
        fs::create_dir(&neighbor).unwrap();
        fs::write(legacy.join("Obsession.exe"), b"legacy").unwrap();
        fs::write(neighbor.join("keep.txt"), b"keep").unwrap();

        let quarantine = quarantine_legacy_root(&legacy).unwrap();
        assert!(!legacy.exists());
        let plan = plan_legacy_tree_with_limits(&quarantine, 8, 4).unwrap();
        remove_planned_tree(&quarantine, plan).unwrap();

        assert!(!quarantine.exists());
        assert_eq!(fs::read(neighbor.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn planned_paths_cannot_escape_the_quarantine_root() {
        let root = Path::new(r"C:\Users\Alice\AppData\Local\.cleanup");
        assert!(matches!(
            checked_plan_path(root, Path::new(r"..\Obsession.exe")),
            Err(LegacyCleanupError::UnsafeProfilePath)
        ));
    }
}
