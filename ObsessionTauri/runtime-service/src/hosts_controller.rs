//! Protected, service-owned controller for the Windows system hosts file.
//!
//! IPC selects only an allowlisted provider. System paths, download URLs,
//! backup names and file contents are reconstructed inside LocalSystem.

#![cfg(windows)]

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::IpAddr;
use std::os::windows::ffi::OsStrExt;
#[cfg(not(test))]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use obsession_runtime_protocol::{
    HostsMutationRequest, HostsProvider, HostsRuntimeSnapshot, OperationAccepted,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
#[cfg(not(test))]
use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::dpi_materializer::{ProtectedDataLayout, RUNTIME_STATE_RELATIVE};
use crate::BackendError;

const STATE_SCHEMA_VERSION: u32 = 1;
const MAX_HOSTS_BYTES: usize = 10 * 1024 * 1024;
const MAX_VERSION_BYTES: usize = 128;
const MARKER_PREFIX: &str = "# obsession:ai-provider=";
const HOSTS_STATE_DIRECTORY: &str = "hosts";
const HOSTS_STATE_FILE: &str = "hosts-state.json";
const HOSTS_BACKUPS_DIRECTORY: &str = "backups";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(not(test))]
const CREATE_NO_WINDOW_FLAG: u32 = CREATE_NO_WINDOW.0;

static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

trait HostsDownloader: Send {
    fn download(&self, provider: HostsProvider) -> Result<Vec<u8>, BackendError>;
}

struct HttpHostsDownloader {
    client: reqwest::blocking::Client,
}

impl HttpHostsDownloader {
    fn new() -> Result<Self, BackendError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .user_agent("Obsession-Runtime/1.1")
            .build()
            .map_err(|_| BackendError::ServiceUnavailable)?;
        Ok(Self { client })
    }
}

impl HostsDownloader for HttpHostsDownloader {
    fn download(&self, provider: HostsProvider) -> Result<Vec<u8>, BackendError> {
        let response = self
            .client
            .get(provider_url(provider))
            .send()
            .map_err(|_| BackendError::RuntimeFailed)?
            .error_for_status()
            .map_err(|_| BackendError::RuntimeFailed)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_HOSTS_BYTES as u64)
        {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_HOSTS_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| BackendError::RuntimeFailed)?;
        if bytes.len() > MAX_HOSTS_BYTES {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotRef {
    file: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ProviderState {
    applied_sha256: Option<String>,
    last_known_good: Option<SnapshotRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HostsManagedState {
    schema_version: u32,
    original: Option<SnapshotRef>,
    providers: BTreeMap<String, ProviderState>,
    active_provider: Option<HostsProvider>,
    /// A pre-operation snapshot persisted before every system-file write.
    /// Startup restores it when the service died between write and commit.
    pending_rollback: Option<SnapshotRef>,
}

impl Default for HostsManagedState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            original: None,
            providers: BTreeMap::new(),
            active_provider: None,
            pending_rollback: None,
        }
    }
}

pub(crate) struct HostsController {
    hosts_path: PathBuf,
    state_path: PathBuf,
    backups_dir: PathBuf,
    state: HostsManagedState,
    downloader: Box<dyn HostsDownloader>,
    next_operation_id: u64,
}

impl HostsController {
    pub(crate) fn discover(layout: &ProtectedDataLayout) -> Result<Self, BackendError> {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .ok_or(BackendError::ServiceUnavailable)?;
        let canonical_program_data =
            fs::canonicalize(program_data).map_err(|_| BackendError::ServiceUnavailable)?;
        let canonical_layout =
            fs::canonicalize(layout.root()).map_err(|_| BackendError::ServiceUnavailable)?;
        if !path_eq(
            &canonical_layout,
            &canonical_program_data.join(RUNTIME_STATE_RELATIVE),
        ) {
            return Err(BackendError::ServiceUnavailable);
        }
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or(BackendError::ServiceUnavailable)?;
        if !system_root.is_absolute() {
            return Err(BackendError::ServiceUnavailable);
        }
        let hosts_path = system_root.join("System32/drivers/etc/hosts");
        if !hosts_path.is_file() {
            return Err(BackendError::ServiceUnavailable);
        }
        let state_root = layout.root().join(HOSTS_STATE_DIRECTORY);
        fs::create_dir_all(&state_root).map_err(|_| BackendError::ServiceUnavailable)?;
        let canonical_parent =
            fs::canonicalize(layout.root()).map_err(|_| BackendError::ServiceUnavailable)?;
        let canonical_state =
            fs::canonicalize(&state_root).map_err(|_| BackendError::ServiceUnavailable)?;
        let expected = canonical_parent.join(HOSTS_STATE_DIRECTORY);
        if !path_eq(&canonical_state, &expected) {
            return Err(BackendError::ServiceUnavailable);
        }
        let backups_dir = state_root.join(HOSTS_BACKUPS_DIRECTORY);
        fs::create_dir_all(&backups_dir).map_err(|_| BackendError::ServiceUnavailable)?;
        Self::from_paths(
            hosts_path,
            state_root.join(HOSTS_STATE_FILE),
            backups_dir,
            Box::new(HttpHostsDownloader::new()?),
        )
    }

    fn from_paths(
        hosts_path: PathBuf,
        state_path: PathBuf,
        backups_dir: PathBuf,
        downloader: Box<dyn HostsDownloader>,
    ) -> Result<Self, BackendError> {
        fs::create_dir_all(&backups_dir).map_err(|_| BackendError::ServiceUnavailable)?;
        let mut controller = Self {
            state: load_state(&state_path),
            hosts_path,
            state_path,
            backups_dir,
            downloader,
            next_operation_id: 0,
        };
        controller.recover_interrupted_transaction()?;
        Ok(controller)
    }

    pub(crate) fn snapshot(&self) -> Option<HostsRuntimeSnapshot> {
        let current = fs::read(&self.hosts_path).ok()?;
        let provider = self
            .state
            .active_provider
            .or_else(|| detect_managed_provider(&current))?;
        let provider_state = self.state.providers.get(provider_key(provider));
        let externally_modified = provider_state
            .and_then(|state| state.applied_sha256.as_deref())
            .is_some_and(|expected| sha256_hex(&current) != expected);
        Some(HostsRuntimeSnapshot {
            provider,
            installed: contains_marker(&current, provider),
            externally_modified,
            rollback_available: provider_state
                .and_then(|state| state.last_known_good.as_ref())
                .is_some(),
            local_version: extract_version(&current),
        })
    }

    pub(crate) fn install(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        let provider = request.provider;
        let current = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        if self.state.original.is_none() {
            if detect_managed_provider(&current).is_some() {
                // An older per-user installation has no protected original.
                // Adopting it would make safe uninstall impossible.
                return Err(BackendError::Conflict);
            }
            let original = self.write_snapshot("original", &current)?;
            self.state.original = Some(original);
            self.save_state()?;
        }
        self.ensure_not_externally_modified(&current)?;

        let downloaded = self.downloader.download(provider)?;
        let (prepared, _version) = prepare_payload(provider, &downloaded)?;

        let before_commit = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        if sha256_hex(&before_commit) != sha256_hex(&current) {
            return Err(BackendError::Conflict);
        }

        let operation_id = self.begin_transaction(&before_commit)?;
        let transaction_snapshot = self.pending_backup().clone();
        if let Err(error) = atomic_write(&self.hosts_path, &prepared) {
            let _ = self.rollback_pending();
            return Err(map_io(error));
        }
        let readback = fs::read(&self.hosts_path).map_err(map_io)?;
        if sha256_hex(&readback) != sha256_hex(&prepared) {
            let _ = self.rollback_pending();
            return Err(BackendError::RuntimeFailed);
        }

        let applied = match self.write_snapshot(&format!("lkg-{operation_id}"), &prepared) {
            Ok(applied) => applied,
            Err(error) => {
                let _ = self.rollback_pending();
                return Err(error);
            }
        };
        let state_before_commit = self.state.clone();
        let previous_lkg = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| state.last_known_good.clone());
        let provider_state = self
            .state
            .providers
            .entry(provider_key(provider).to_owned())
            .or_default();
        provider_state.applied_sha256 = Some(applied.sha256.clone());
        provider_state.last_known_good = Some(applied.clone());
        self.state.active_provider = Some(provider);
        self.state.pending_rollback = None;
        if let Err(error) = self.save_state() {
            let _ = restore_snapshot(&self.hosts_path, &self.backups_dir, &transaction_snapshot);
            self.state = state_before_commit;
            let _ = self.rollback_pending();
            return Err(error);
        }
        if let Some(previous) = previous_lkg.filter(|old| old.file != applied.file) {
            remove_snapshot(&self.backups_dir, &previous);
        }
        remove_snapshot(&self.backups_dir, &transaction_snapshot);
        flush_dns_best_effort();
        Ok(OperationAccepted { operation_id })
    }

    pub(crate) fn uninstall(&mut self) -> Result<OperationAccepted, BackendError> {
        let current = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        self.ensure_not_externally_modified(&current)?;
        let original = self
            .state
            .original
            .clone()
            .or_else(|| find_orphaned_original(&self.backups_dir))
            .ok_or(BackendError::Conflict)?;
        let operation_id = self.begin_transaction(&current)?;
        let transaction_snapshot = self.pending_backup().clone();
        let bytes = match read_snapshot(&self.backups_dir, &original) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = self.rollback_pending();
                return Err(map_io(error));
            }
        };
        if let Err(error) = atomic_write(&self.hosts_path, &bytes) {
            let _ = self.rollback_pending();
            return Err(map_io(error));
        }
        let state_before_commit = self.state.clone();
        for provider in self.state.providers.values_mut() {
            provider.applied_sha256 = None;
        }
        self.state.active_provider = None;
        self.state.pending_rollback = None;
        if let Err(error) = self.save_state() {
            let _ = restore_snapshot(&self.hosts_path, &self.backups_dir, &transaction_snapshot);
            self.state = state_before_commit;
            let _ = self.rollback_pending();
            return Err(error);
        }
        remove_snapshot(&self.backups_dir, &transaction_snapshot);
        flush_dns_best_effort();
        Ok(OperationAccepted { operation_id })
    }

    pub(crate) fn restore(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        let provider = request.provider;
        let current = fs::read(&self.hosts_path).map_err(|_| BackendError::RuntimeFailed)?;
        self.ensure_not_externally_modified(&current)?;
        let lkg = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| state.last_known_good.clone())
            .ok_or(BackendError::Conflict)?;
        let operation_id = self.begin_transaction(&current)?;
        let transaction_snapshot = self.pending_backup().clone();
        let bytes = match read_snapshot(&self.backups_dir, &lkg) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = self.rollback_pending();
                return Err(map_io(error));
            }
        };
        if let Err(error) = atomic_write(&self.hosts_path, &bytes) {
            let _ = self.rollback_pending();
            return Err(map_io(error));
        }
        let state_before_commit = self.state.clone();
        self.state
            .providers
            .entry(provider_key(provider).to_owned())
            .or_default()
            .applied_sha256 = Some(lkg.sha256.clone());
        self.state.active_provider = Some(provider);
        self.state.pending_rollback = None;
        if let Err(error) = self.save_state() {
            let _ = restore_snapshot(&self.hosts_path, &self.backups_dir, &transaction_snapshot);
            self.state = state_before_commit;
            let _ = self.rollback_pending();
            return Err(error);
        }
        remove_snapshot(&self.backups_dir, &transaction_snapshot);
        flush_dns_best_effort();
        Ok(OperationAccepted { operation_id })
    }

    fn ensure_not_externally_modified(&self, current: &[u8]) -> Result<(), BackendError> {
        let Some(provider) = self.state.active_provider else {
            return Ok(());
        };
        let expected = self
            .state
            .providers
            .get(provider_key(provider))
            .and_then(|state| state.applied_sha256.as_deref());
        if expected.is_some_and(|hash| sha256_hex(current) != hash) {
            Err(BackendError::Conflict)
        } else {
            Ok(())
        }
    }

    fn begin_transaction(&mut self, current: &[u8]) -> Result<u64, BackendError> {
        let operation_id = self.next_operation_id();
        let snapshot = self.write_snapshot(&format!("preop-{operation_id}"), current)?;
        self.state.pending_rollback = Some(snapshot);
        self.save_state()?;
        Ok(operation_id)
    }

    fn recover_interrupted_transaction(&mut self) -> Result<(), BackendError> {
        if self.state.pending_rollback.is_none() {
            return Ok(());
        }
        self.rollback_pending()
    }

    fn rollback_pending(&mut self) -> Result<(), BackendError> {
        let snapshot = self
            .state
            .pending_rollback
            .clone()
            .ok_or(BackendError::Internal)?;
        restore_snapshot(&self.hosts_path, &self.backups_dir, &snapshot).map_err(map_io)?;
        self.state.pending_rollback = None;
        self.save_state()?;
        remove_snapshot(&self.backups_dir, &snapshot);
        Ok(())
    }

    fn pending_backup(&self) -> &SnapshotRef {
        self.state
            .pending_rollback
            .as_ref()
            .expect("pending rollback exists until transaction commit")
    }

    fn write_snapshot(&self, label: &str, bytes: &[u8]) -> Result<SnapshotRef, BackendError> {
        fs::create_dir_all(&self.backups_dir).map_err(map_io)?;
        let nonce = operation_nonce();
        let file = format!("hosts_snapshot_{label}-{}-{nonce}.bin", std::process::id());
        let path = self.backups_dir.join(&file);
        atomic_write(&path, bytes).map_err(map_io)?;
        Ok(SnapshotRef {
            file,
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
        })
    }

    fn save_state(&self) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec_pretty(&self.state).map_err(|_| BackendError::Internal)?;
        atomic_write(&self.state_path, &bytes).map_err(map_io)
    }

    fn next_operation_id(&mut self) -> u64 {
        self.next_operation_id = self.next_operation_id.wrapping_add(1).max(1);
        self.next_operation_id
    }
}

fn provider_url(provider: HostsProvider) -> &'static str {
    match provider {
        HostsProvider::Malw => {
            "https://raw.githubusercontent.com/ImMALWARE/dns.malw.link/refs/heads/master/hosts"
        }
        HostsProvider::Geohide => {
            "https://github.com/Internet-Helper/GeoHideDNS/raw/refs/heads/main/hosts/hosts"
        }
    }
}

fn provider_key(provider: HostsProvider) -> &'static str {
    match provider {
        HostsProvider::Malw => "malw",
        HostsProvider::Geohide => "geohide",
    }
}

fn marker(provider: HostsProvider) -> String {
    format!("{MARKER_PREFIX}{}", provider_key(provider))
}

fn contains_marker(bytes: &[u8], provider: HostsProvider) -> bool {
    std::str::from_utf8(bytes)
        .map(|content| content.contains(&marker(provider)))
        .unwrap_or(false)
}

fn detect_managed_provider(bytes: &[u8]) -> Option<HostsProvider> {
    [HostsProvider::Malw, HostsProvider::Geohide]
        .into_iter()
        .find(|provider| contains_marker(bytes, *provider))
}

fn prepare_payload(
    provider: HostsProvider,
    downloaded: &[u8],
) -> Result<(Vec<u8>, Option<String>), BackendError> {
    if downloaded.is_empty() || downloaded.len() > MAX_HOSTS_BYTES || downloaded.contains(&0) {
        return Err(BackendError::ProtectedResourceInvalid);
    }
    let text =
        std::str::from_utf8(downloaded).map_err(|_| BackendError::ProtectedResourceInvalid)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut entries = 0usize;
    let mut addresses = BTreeMap::<(String, bool), IpAddr>::new();
    let mut version = None;
    for line in normalized.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            if version.is_none() {
                version = extract_version(trimmed.as_bytes());
            }
            continue;
        }
        let mut tokens = trimmed.split_whitespace();
        let Some(address) = tokens.next().and_then(|token| token.parse::<IpAddr>().ok()) else {
            continue;
        };
        let mut domains_on_line = 0usize;
        for domain in tokens.take_while(|token| !token.starts_with('#')) {
            if !valid_domain(domain) {
                return Err(BackendError::ProtectedResourceInvalid);
            }
            domains_on_line += 1;
            let key = (domain.to_ascii_lowercase(), address.is_ipv6());
            if addresses
                .insert(key, address)
                .is_some_and(|previous| previous != address)
            {
                return Err(BackendError::ProtectedResourceInvalid);
            }
        }
        if domains_on_line == 0 {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        entries += 1;
    }
    if entries == 0 {
        return Err(BackendError::ProtectedResourceInvalid);
    }
    let mut prepared = Vec::with_capacity(marker(provider).len() + normalized.len() + 2);
    prepared.extend_from_slice(marker(provider).as_bytes());
    prepared.push(b'\n');
    prepared.extend_from_slice(normalized.as_bytes());
    if !prepared.ends_with(b"\n") {
        prepared.push(b'\n');
    }
    if prepared.len() > MAX_HOSTS_BYTES {
        return Err(BackendError::ProtectedResourceInvalid);
    }
    Ok((prepared, version))
}

fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && !domain.starts_with('.')
        && !domain.starts_with('-')
        && !domain.ends_with('-')
        && domain
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn extract_version(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    for line in text.lines().take(128) {
        let comment = line.trim().strip_prefix('#')?.trim();
        for prefix in ["update:", "Последнее обновление:"] {
            if let Some(value) = comment.strip_prefix(prefix).map(str::trim) {
                if !value.is_empty() && value.len() <= MAX_VERSION_BYTES {
                    return Some(value.to_owned());
                }
            }
        }
    }
    None
}

fn load_state(path: &Path) -> HostsManagedState {
    let Ok(bytes) = fs::read(path) else {
        return HostsManagedState::default();
    };
    serde_json::from_slice::<HostsManagedState>(&bytes)
        .ok()
        .filter(|state| state.schema_version == STATE_SCHEMA_VERSION)
        .unwrap_or_default()
}

fn read_snapshot(backups_dir: &Path, snapshot: &SnapshotRef) -> io::Result<Vec<u8>> {
    if !valid_snapshot_name(&snapshot.file) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid hosts snapshot name",
        ));
    }
    let bytes = fs::read(backups_dir.join(&snapshot.file))?;
    if bytes.len() as u64 != snapshot.size || sha256_hex(&bytes) != snapshot.sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "hosts snapshot integrity mismatch",
        ));
    }
    Ok(bytes)
}

fn restore_snapshot(
    hosts_path: &Path,
    backups_dir: &Path,
    snapshot: &SnapshotRef,
) -> io::Result<()> {
    let bytes = read_snapshot(backups_dir, snapshot)?;
    atomic_write(hosts_path, &bytes)?;
    let readback = fs::read(hosts_path)?;
    if sha256_hex(&readback) != snapshot.sha256 {
        return Err(io::Error::other("hosts restore verification failed"));
    }
    Ok(())
}

fn find_orphaned_original(backups_dir: &Path) -> Option<SnapshotRef> {
    let mut files = fs::read_dir(backups_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_str()?.to_owned();
            (file.starts_with("hosts_snapshot_original-") && valid_snapshot_name(&file))
                .then_some(file)
        })
        .collect::<Vec<_>>();
    files.sort();
    let file = files.into_iter().next()?;
    let bytes = fs::read(backups_dir.join(&file)).ok()?;
    Some(SnapshotRef {
        file,
        sha256: sha256_hex(&bytes),
        size: bytes.len() as u64,
    })
}

fn valid_snapshot_name(file: &str) -> bool {
    file.starts_with("hosts_snapshot_")
        && file.ends_with(".bin")
        && file.len() <= 160
        && file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn remove_snapshot(backups_dir: &Path, snapshot: &SnapshotRef) {
    if valid_snapshot_name(&snapshot.file) {
        let _ = fs::remove_file(backups_dir.join(&snapshot.file));
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid filename"))?;
    let temp = parent.join(format!(".{file_name}.obsession.tmp"));
    {
        let mut file = File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    let from = wide_path(&temp);
    let to = wide_path(path);
    let result = unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(io::Error::other(error.to_string()));
    }
    Ok(())
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn operation_nonce() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    millis
        .wrapping_mul(1_000)
        .wrapping_add(OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed) % 1_000)
}

fn path_eq(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn map_io(error: io::Error) -> BackendError {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
            BackendError::ProtectedResourceInvalid
        }
        io::ErrorKind::PermissionDenied => BackendError::ServiceUnavailable,
        _ => BackendError::RuntimeFailed,
    }
}

fn flush_dns_best_effort() {
    #[cfg(test)]
    return;

    #[cfg(not(test))]
    {
        let Some(system_root) = std::env::var_os("SystemRoot") else {
            return;
        };
        let _ = Command::new(PathBuf::from(system_root).join("System32/ipconfig.exe"))
            .arg("/flushdns")
            .creation_flags(CREATE_NO_WINDOW_FLAG)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticDownloader(Vec<u8>);

    impl HostsDownloader for StaticDownloader {
        fn download(&self, _provider: HostsProvider) -> Result<Vec<u8>, BackendError> {
            Ok(self.0.clone())
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let suffix = operation_nonce();
        let root = std::env::temp_dir().join(format!("obsession-service-hosts-{name}-{suffix}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn controller(root: &Path, payload: &[u8]) -> HostsController {
        let hosts_path = root.join("hosts");
        fs::write(&hosts_path, b"127.0.0.1 localhost\r\n").unwrap();
        HostsController::from_paths(
            hosts_path,
            root.join("state/hosts-state.json"),
            root.join("state/backups"),
            Box::new(StaticDownloader(payload.to_vec())),
        )
        .unwrap()
    }

    #[test]
    fn install_is_hash_verified_and_external_changes_fail_closed() {
        let root = temp_root("install");
        let mut controller = controller(&root, b"# update: 2026-08-04\n1.2.3.4 chatgpt.com\n");
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            })
            .unwrap();
        let snapshot = controller.snapshot().unwrap();
        assert!(snapshot.installed);
        assert_eq!(snapshot.local_version.as_deref(), Some("2026-08-04"));
        assert!(snapshot.rollback_available);

        fs::write(&controller.hosts_path, b"127.0.0.1 external-change\n").unwrap();
        assert_eq!(
            controller.install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            }),
            Err(BackendError::Conflict)
        );
        assert!(controller.snapshot().unwrap().externally_modified);
    }

    #[test]
    fn uninstall_restores_the_exact_original_and_lkg_can_be_reapplied() {
        let root = temp_root("rollback");
        let mut controller = controller(&root, b"1.2.3.4 claude.ai\n");
        let original = fs::read(&controller.hosts_path).unwrap();
        controller
            .install(HostsMutationRequest {
                provider: HostsProvider::Geohide,
            })
            .unwrap();
        controller.uninstall().unwrap();
        assert_eq!(fs::read(&controller.hosts_path).unwrap(), original);
        assert!(controller.snapshot().is_none());

        controller
            .restore(HostsMutationRequest {
                provider: HostsProvider::Geohide,
            })
            .unwrap();
        assert!(controller.snapshot().unwrap().installed);
    }

    #[test]
    fn startup_rolls_back_an_interrupted_system_file_write() {
        let root = temp_root("crash-recovery");
        let mut controller = controller(&root, b"1.2.3.4 gemini.google.com\n");
        let original = fs::read(&controller.hosts_path).unwrap();
        controller.begin_transaction(&original).unwrap();
        atomic_write(&controller.hosts_path, b"broken-partial-state\n").unwrap();

        let recovered = HostsController::from_paths(
            controller.hosts_path.clone(),
            controller.state_path.clone(),
            controller.backups_dir.clone(),
            Box::new(StaticDownloader(Vec::new())),
        )
        .unwrap();
        assert_eq!(fs::read(&recovered.hosts_path).unwrap(), original);
        assert!(recovered.state.pending_rollback.is_none());
    }

    #[test]
    fn invalid_or_unbounded_payload_never_touches_system_hosts() {
        let root = temp_root("invalid");
        let mut controller = controller(&root, b"<html>error</html>\n");
        let original = fs::read(&controller.hosts_path).unwrap();
        assert_eq!(
            controller.install(HostsMutationRequest {
                provider: HostsProvider::Malw,
            }),
            Err(BackendError::ProtectedResourceInvalid)
        );
        assert_eq!(fs::read(&controller.hosts_path).unwrap(), original);
    }
}
