//! Fail-closed recovery of the legacy system `hosts` mutation.
//!
//! All metadata below the caller-owned Roaming tree is untrusted. It can only
//! authorize restoring a byte sequence that is itself a harmless localhost-
//! only baseline, and only while the protected system file still exactly
//! matches an Obsession-applied hash and marker.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    MoveFileExW, FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::Win32::System::Threading::GetCurrentProcessId;

use super::{
    io_error, reject_plain_directory, reject_plain_directory_chain, wide_path, LegacyCleanupError,
};

const STATE_FILE: &str = "hosts-state.json";
const BACKUPS_DIRECTORY: &str = "hosts-backups";
const STATE_SCHEMA_VERSION: u32 = 1;
const MAX_STATE_BYTES: u64 = 256 * 1024;
const MAX_HOSTS_BYTES: u64 = 10 * 1024 * 1024;
const MAX_PROVIDERS: usize = 2;
const MAX_CONFIRMED_PER_PROVIDER: usize = 32;
const MAX_FIELD_BYTES: usize = 256;
const MAX_BASELINE_LINES: usize = 65_536;
const MAX_BASELINE_LINE_BYTES: usize = 4_096;
const WINDOWS_PATH_UTF16: usize = 32_768;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyHostsState {
    schema_version: u32,
    original: Option<LegacySnapshotRef>,
    providers: BTreeMap<String, LegacyProviderState>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyProviderState {
    last_known_good: Option<LegacySnapshotRef>,
    confirmed: Vec<LegacySnapshotRef>,
    applied_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySnapshotRef {
    operation_id: String,
    file: String,
    sha256: String,
    size: u64,
    captured_at: String,
    provider: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum LegacyHostsRecoveryPlan {
    NoMutation,
    Restore {
        expected_current_sha256: String,
        original: Vec<u8>,
    },
}

pub(super) fn recover_legacy_hosts(roaming_root: &Path) -> Result<(), LegacyCleanupError> {
    if !roaming_root.exists() {
        return Ok(());
    }
    let hosts_path = system_hosts_path()?;
    recover_legacy_hosts_at(roaming_root, &hosts_path)
}

fn recover_legacy_hosts_at(
    roaming_root: &Path,
    hosts_path: &Path,
) -> Result<(), LegacyCleanupError> {
    let plan = plan_legacy_hosts_recovery(roaming_root, hosts_path)?;
    if let LegacyHostsRecoveryPlan::Restore {
        expected_current_sha256,
        original,
    } = plan
    {
        restore_hosts_atomically(hosts_path, &expected_current_sha256, &original)?;
    }
    Ok(())
}

fn plan_legacy_hosts_recovery(
    roaming_root: &Path,
    hosts_path: &Path,
) -> Result<LegacyHostsRecoveryPlan, LegacyCleanupError> {
    let state_path = roaming_root.join(STATE_FILE);
    let backups_dir = roaming_root.join(BACKUPS_DIRECTORY);
    let state_exists = path_exists_without_following(&state_path)?;
    let backups_exist = path_exists_without_following(&backups_dir)?;
    if !state_exists {
        if backups_exist && directory_has_entries(&backups_dir)? {
            return Err(LegacyCleanupError::HostsStateInvalid(
                "non-empty backups exist but hosts-state.json is missing",
            ));
        }
        let current = read_bounded_regular_file(hosts_path, MAX_HOSTS_BYTES, "read system hosts")?;
        if exact_obsession_marker_provider(&current).is_some() {
            return Err(LegacyCleanupError::HostsRecoveryRequired(
                "system hosts has an Obsession marker but hosts-state.json is missing",
            ));
        }
        return Ok(LegacyHostsRecoveryPlan::NoMutation);
    }

    let state_bytes = read_bounded_regular_file(&state_path, MAX_STATE_BYTES, "read hosts state")?;
    let state: LegacyHostsState = serde_json::from_slice(&state_bytes).map_err(|_| {
        LegacyCleanupError::HostsStateInvalid("hosts-state.json is not strict schema v1 JSON")
    })?;
    validate_state(&state)?;

    if state.original.is_none()
        && state
            .providers
            .values()
            .all(|provider| provider.applied_sha256.is_none())
    {
        return if backups_exist {
            Err(LegacyCleanupError::HostsRecoveryRequired(
                "unreferenced hosts backups must be preserved",
            ))
        } else {
            Ok(LegacyHostsRecoveryPlan::NoMutation)
        };
    }

    let current = read_bounded_regular_file(hosts_path, MAX_HOSTS_BYTES, "read system hosts")?;
    let current_sha256 = sha256_hex(&current);

    let original = match &state.original {
        Some(reference) => {
            if !backups_exist {
                return Err(LegacyCleanupError::HostsStateInvalid(
                    "original snapshot is referenced but hosts-backups is missing",
                ));
            }
            read_verified_original(&backups_dir, reference)?
        }
        None => {
            return Err(LegacyCleanupError::HostsRecoveryRequired(
                "the original hosts snapshot is missing",
            ))
        }
    };

    if !is_safe_localhost_baseline(&original) {
        return Err(LegacyCleanupError::HostsRecoveryRequired(
            "the original snapshot contains custom hosts entries",
        ));
    }

    // No privileged write is needed when the current file already is the
    // byte-exact safe original. This also makes retries idempotent if firewall
    // cleanup failed after hosts restoration.
    if sha256_hex(&original) == current_sha256 {
        return Ok(LegacyHostsRecoveryPlan::NoMutation);
    }

    let Some(marker_provider) = exact_obsession_marker_provider(&current) else {
        return Err(LegacyCleanupError::HostsRecoveryRequired(
            "system hosts no longer has the exact Obsession ownership marker",
        ));
    };
    let applied_matches = state
        .providers
        .get(marker_provider)
        .and_then(|provider| provider.applied_sha256.as_deref())
        == Some(current_sha256.as_str());
    if !applied_matches {
        return Err(LegacyCleanupError::HostsRecoveryRequired(
            "system hosts changed after the last recorded Obsession apply",
        ));
    }
    Ok(LegacyHostsRecoveryPlan::Restore {
        expected_current_sha256: current_sha256,
        original,
    })
}

fn validate_state(state: &LegacyHostsState) -> Result<(), LegacyCleanupError> {
    if state.schema_version != STATE_SCHEMA_VERSION || state.providers.len() > MAX_PROVIDERS {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "unsupported schema version or provider count",
        ));
    }
    if let Some(original) = &state.original {
        validate_snapshot_ref(original, None, true)?;
    }
    for (name, provider) in &state.providers {
        if !matches!(name.as_str(), "malw" | "geohide") {
            return Err(LegacyCleanupError::HostsStateInvalid(
                "unknown hosts provider",
            ));
        }
        if provider.confirmed.len() > MAX_CONFIRMED_PER_PROVIDER {
            return Err(LegacyCleanupError::HostsStateInvalid(
                "too many confirmed snapshots",
            ));
        }
        if let Some(hash) = &provider.applied_sha256 {
            validate_sha256(hash)?;
        }
        if let Some(reference) = &provider.last_known_good {
            validate_snapshot_ref(reference, Some(name), false)?;
        }
        for reference in &provider.confirmed {
            validate_snapshot_ref(reference, Some(name), false)?;
        }
    }
    Ok(())
}

fn validate_snapshot_ref(
    reference: &LegacySnapshotRef,
    expected_provider: Option<&str>,
    original: bool,
) -> Result<(), LegacyCleanupError> {
    if reference.operation_id.is_empty()
        || reference.operation_id.len() > MAX_FIELD_BYTES
        || !reference
            .operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || reference.captured_at.len() > MAX_FIELD_BYTES
        || reference.size > MAX_HOSTS_BYTES
    {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "snapshot metadata is out of bounds",
        ));
    }
    if original != reference.operation_id.ends_with("_original") {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "snapshot original role does not match its operation id",
        ));
    }
    let expected_file = format!("hosts_snapshot_{}.txt", reference.operation_id);
    if reference.file != expected_file
        || Path::new(&reference.file)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || reference.provider.as_deref() != expected_provider
    {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "snapshot path or provider is invalid",
        ));
    }
    validate_sha256(&reference.sha256)
}

fn validate_sha256(value: &str) -> Result<(), LegacyCleanupError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(LegacyCleanupError::HostsStateInvalid(
            "SHA-256 must be 64 lowercase hexadecimal characters",
        ))
    }
}

fn read_verified_original(
    backups_dir: &Path,
    reference: &LegacySnapshotRef,
) -> Result<Vec<u8>, LegacyCleanupError> {
    reject_plain_directory(backups_dir)?;
    let path = backups_dir.join(&reference.file);
    if !super::path_belongs_to_root(&path, backups_dir) {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "snapshot escaped hosts-backups",
        ));
    }
    let bytes = read_bounded_regular_file(&path, MAX_HOSTS_BYTES, "read original hosts snapshot")?;
    if bytes.len() as u64 != reference.size || sha256_hex(&bytes) != reference.sha256 {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "original snapshot size or SHA-256 does not match state",
        ));
    }
    Ok(bytes)
}

fn read_bounded_regular_file(
    path: &Path,
    max_bytes: u64,
    operation: &'static str,
) -> Result<Vec<u8>, LegacyCleanupError> {
    let parent = path
        .parent()
        .ok_or(LegacyCleanupError::HostsStateInvalid("file has no parent"))?;
    reject_plain_directory_chain(parent)?;
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io_error("inspect", path, source))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(LegacyCleanupError::ReparsePoint(path.to_path_buf()));
    }
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "file type or size is invalid",
        ));
    }
    let file = File::open(path).map_err(|source| io_error(operation, path, source))?;
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| LegacyCleanupError::HostsStateInvalid("file is too large"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_error(operation, path, source))?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > max_bytes {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "file changed while it was being read",
        ));
    }
    Ok(bytes)
}

fn path_exists_without_following(path: &Path) -> Result<bool, LegacyCleanupError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                Err(LegacyCleanupError::ReparsePoint(path.to_path_buf()))
            } else {
                Ok(true)
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io_error("inspect", path, source)),
    }
}

fn directory_has_entries(path: &Path) -> Result<bool, LegacyCleanupError> {
    reject_plain_directory(path)?;
    let mut entries = fs::read_dir(path).map_err(|source| io_error("enumerate", path, source))?;
    match entries.next() {
        Some(Ok(_)) => Ok(true),
        Some(Err(source)) => Err(io_error("enumerate", path, source)),
        None => Ok(false),
    }
}

fn exact_obsession_marker_provider(bytes: &[u8]) -> Option<&'static str> {
    let text = std::str::from_utf8(bytes).ok()?;
    let first = text.lines().next()?.trim_end_matches('\r');
    match first {
        "# obsession:ai-provider=malw" => Some("malw"),
        "# obsession:ai-provider=geohide" => Some("geohide"),
        _ => None,
    }
}

fn is_safe_localhost_baseline(bytes: &[u8]) -> bool {
    if bytes.len() as u64 > MAX_HOSTS_BYTES || bytes.contains(&0) {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut count = 0usize;
    for raw_line in text.lines() {
        count += 1;
        if count > MAX_BASELINE_LINES || raw_line.len() > MAX_BASELINE_LINE_BYTES {
            return false;
        }
        let content = raw_line
            .split_once('#')
            .map_or(raw_line, |(before, _)| before);
        let fields: Vec<_> = content.split_ascii_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        if fields.len() < 2
            || !matches!(fields[0], "127.0.0.1" | "::1")
            || !fields[1..]
                .iter()
                .all(|host| host.eq_ignore_ascii_case("localhost"))
        {
            return false;
        }
    }
    true
}

fn restore_hosts_atomically(
    hosts_path: &Path,
    expected_current_sha256: &str,
    original: &[u8],
) -> Result<(), LegacyCleanupError> {
    let current = read_bounded_regular_file(hosts_path, MAX_HOSTS_BYTES, "re-read system hosts")?;
    if sha256_hex(&current) != expected_current_sha256 {
        return Err(LegacyCleanupError::HostsRecoveryRequired(
            "system hosts changed immediately before restore",
        ));
    }
    if !is_safe_localhost_baseline(original) {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "unsafe hosts baseline reached the write phase",
        ));
    }

    let parent = hosts_path
        .parent()
        .ok_or(LegacyCleanupError::HostsStateInvalid(
            "system hosts has no parent",
        ))?;
    reject_plain_directory_chain(parent)?;
    let temp = unique_temp_path(parent);
    let write_result = (|| -> Result<(), LegacyCleanupError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|source| io_error("create hosts restore temp", &temp, source))?;
        file.write_all(original)
            .map_err(|source| io_error("write hosts restore temp", &temp, source))?;
        file.sync_all()
            .map_err(|source| io_error("flush hosts restore temp", &temp, source))?;
        drop(file);

        let source = wide_path(&temp)?;
        let destination = wide_path(hosts_path)?;
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|source| LegacyCleanupError::Windows {
            operation: "atomic legacy hosts restore",
            source,
        })?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result?;

    let readback = read_bounded_regular_file(hosts_path, MAX_HOSTS_BYTES, "verify restored hosts")?;
    if sha256_hex(&readback) != sha256_hex(original) {
        return Err(LegacyCleanupError::HostsRecoveryRequired(
            "restored hosts failed read-back verification",
        ));
    }
    Ok(())
}

fn unique_temp_path(parent: &Path) -> PathBuf {
    let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".obsession-legacy-hosts-restore-{}-{id}.tmp",
        unsafe { GetCurrentProcessId() }
    ))
}

fn system_hosts_path() -> Result<PathBuf, LegacyCleanupError> {
    let mut buffer = vec![0u16; WINDOWS_PATH_UTF16];
    let length = unsafe { GetSystemWindowsDirectoryW(Some(&mut buffer)) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(LegacyCleanupError::Windows {
            operation: "system Windows directory lookup",
            source: windows::core::Error::from_win32(),
        });
    }
    let windows = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]));
    let hosts = windows
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts");
    reject_plain_directory_chain(hosts.parent().ok_or(LegacyCleanupError::HostsStateInvalid(
        "system hosts has no parent",
    ))?)?;
    let metadata =
        fs::symlink_metadata(&hosts).map_err(|source| io_error("inspect", &hosts, source))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || !metadata.is_file() {
        return Err(LegacyCleanupError::HostsStateInvalid(
            "system hosts is not a plain file",
        ));
    }
    Ok(hosts)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("obsession-legacy-hosts-test-{}-{id}", unsafe {
                    GetCurrentProcessId()
                }));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn prepare(&self, original: &[u8], applied: &[u8]) -> (PathBuf, PathBuf) {
            let roaming = self.0.join("roaming");
            let backups = roaming.join(BACKUPS_DIRECTORY);
            fs::create_dir_all(&backups).unwrap();
            let operation_id = "20260730T000000000_original";
            let file = format!("hosts_snapshot_{operation_id}.txt");
            fs::write(backups.join(&file), original).unwrap();
            let state = serde_json::json!({
                "schema_version": 1,
                "original": {
                    "operation_id": operation_id,
                    "file": file,
                    "sha256": sha256_hex(original),
                    "size": original.len(),
                    "captured_at": "2026-07-30T00:00:00+03:00",
                    "provider": null
                },
                "providers": {
                    "malw": {
                        "last_known_good": null,
                        "confirmed": [],
                        "applied_sha256": sha256_hex(applied)
                    }
                }
            });
            fs::write(
                roaming.join(STATE_FILE),
                serde_json::to_vec(&state).unwrap(),
            )
            .unwrap();
            let hosts = self.0.join("system-hosts");
            fs::write(&hosts, applied).unwrap();
            (roaming, hosts)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn localhost_only_baselines_are_safe_but_arbitrary_mappings_are_not() {
        assert!(is_safe_localhost_baseline(
            b"# Windows hosts\r\n127.0.0.1 localhost # local\r\n::1 localhost\r\n"
        ));
        assert!(!is_safe_localhost_baseline(b"1.2.3.4 example.com\n"));
        assert!(!is_safe_localhost_baseline(
            b"127.0.0.1 telemetry.example\n"
        ));
        assert!(!is_safe_localhost_baseline(b"0.0.0.0 localhost\n"));
    }

    #[test]
    fn missing_state_accepts_only_empty_backups_and_unowned_system_hosts() {
        let test = TestRoot::new();
        let roaming = test.0.join("roaming");
        let backups = roaming.join(BACKUPS_DIRECTORY);
        fs::create_dir_all(&backups).unwrap();
        let hosts = test.0.join("system-hosts");
        let external = b"10.0.0.5 intranet\n";
        fs::write(&hosts, external).unwrap();

        assert_eq!(
            plan_legacy_hosts_recovery(&roaming, &hosts).unwrap(),
            LegacyHostsRecoveryPlan::NoMutation
        );
        assert_eq!(fs::read(&hosts).unwrap(), external);

        fs::write(backups.join("unreferenced-snapshot"), b"backup").unwrap();
        assert!(matches!(
            plan_legacy_hosts_recovery(&roaming, &hosts),
            Err(LegacyCleanupError::HostsStateInvalid(_))
        ));
        fs::remove_file(backups.join("unreferenced-snapshot")).unwrap();

        fs::write(
            &hosts,
            b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n",
        )
        .unwrap();
        assert!(matches!(
            plan_legacy_hosts_recovery(&roaming, &hosts),
            Err(LegacyCleanupError::HostsRecoveryRequired(_))
        ));
    }

    #[test]
    fn valid_owned_hosts_produces_a_byte_exact_restore_plan() {
        let test = TestRoot::new();
        let original = b"# baseline\r\n127.0.0.1 localhost\r\n";
        let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
        let (roaming, hosts) = test.prepare(original, applied);
        assert_eq!(
            plan_legacy_hosts_recovery(&roaming, &hosts).unwrap(),
            LegacyHostsRecoveryPlan::Restore {
                expected_current_sha256: sha256_hex(applied),
                original: original.to_vec(),
            }
        );
    }

    #[test]
    fn bad_snapshot_path_size_and_hash_are_rejected() {
        for (field, value) in [
            ("file", serde_json::json!("..\\hosts")),
            ("size", serde_json::json!(999)),
            ("sha256", serde_json::json!("00")),
        ] {
            let test = TestRoot::new();
            let original = b"127.0.0.1 localhost\n";
            let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
            let (roaming, hosts) = test.prepare(original, applied);
            let state_path = roaming.join(STATE_FILE);
            let mut state: serde_json::Value =
                serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
            state["original"][field] = value;
            fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
            assert!(matches!(
                plan_legacy_hosts_recovery(&roaming, &hosts),
                Err(LegacyCleanupError::HostsStateInvalid(_))
            ));
        }
    }

    #[test]
    fn external_hosts_modification_preserves_system_and_roaming_tree() {
        let test = TestRoot::new();
        let original = b"127.0.0.1 localhost\n";
        let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
        let (roaming, hosts) = test.prepare(original, applied);
        let external = b"127.0.0.1 localhost\n10.0.0.5 intranet\n";
        fs::write(&hosts, external).unwrap();

        assert!(matches!(
            recover_legacy_hosts_at(&roaming, &hosts),
            Err(LegacyCleanupError::HostsRecoveryRequired(_))
        ));
        assert_eq!(fs::read(&hosts).unwrap(), external);
        assert!(roaming.join(STATE_FILE).is_file());
        assert!(roaming.join(BACKUPS_DIRECTORY).is_dir());
    }

    #[test]
    fn custom_original_is_never_written_automatically() {
        let test = TestRoot::new();
        let original = b"10.0.0.5 intranet\n";
        let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
        let (roaming, hosts) = test.prepare(original, applied);
        assert!(matches!(
            recover_legacy_hosts_at(&roaming, &hosts),
            Err(LegacyCleanupError::HostsRecoveryRequired(_))
        ));
        assert_eq!(fs::read(&hosts).unwrap(), applied);
        assert!(roaming.exists());
    }

    #[test]
    fn safe_restore_is_atomic_verified_and_idempotent() {
        let test = TestRoot::new();
        let original = b"# baseline\r\n127.0.0.1 localhost\r\n";
        let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
        let (roaming, hosts) = test.prepare(original, applied);

        recover_legacy_hosts_at(&roaming, &hosts).unwrap();
        assert_eq!(fs::read(&hosts).unwrap(), original);
        assert!(
            roaming.exists(),
            "tree deletion belongs to the outer cleanup"
        );

        recover_legacy_hosts_at(&roaming, &hosts).unwrap();
        assert_eq!(fs::read(&hosts).unwrap(), original);
    }

    #[test]
    fn custom_original_remains_manual_even_when_already_restored() {
        let test = TestRoot::new();
        let original = b"10.0.0.5 intranet\n";
        let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
        let (roaming, hosts) = test.prepare(original, applied);
        fs::write(&hosts, original).unwrap();

        assert!(matches!(
            recover_legacy_hosts_at(&roaming, &hosts),
            Err(LegacyCleanupError::HostsRecoveryRequired(_))
        ));
        assert_eq!(fs::read(&hosts).unwrap(), original);
        assert!(roaming.exists());
    }

    #[test]
    fn reparse_original_snapshot_is_rejected_without_following_it() {
        use std::os::windows::fs::symlink_file;

        let test = TestRoot::new();
        let original = b"127.0.0.1 localhost\n";
        let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
        let (roaming, hosts) = test.prepare(original, applied);
        let snapshot = fs::read_dir(roaming.join(BACKUPS_DIRECTORY))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::remove_file(&snapshot).unwrap();
        let outside = test.0.join("outside");
        fs::write(&outside, original).unwrap();
        if symlink_file(&outside, &snapshot).is_err() {
            return;
        }
        assert!(matches!(
            plan_legacy_hosts_recovery(&roaming, &hosts),
            Err(LegacyCleanupError::ReparsePoint(path)) if path == snapshot
        ));
    }

    #[test]
    fn strict_state_rejects_unknown_fields() {
        let test = TestRoot::new();
        let original = b"127.0.0.1 localhost\n";
        let applied = b"# obsession:ai-provider=malw\n1.2.3.4 dns.malw.link\n";
        let (roaming, hosts) = test.prepare(original, applied);
        let state_path = roaming.join(STATE_FILE);
        let mut state: serde_json::Value =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        state["attacker_path"] = serde_json::json!("C:\\Windows\\System32");
        fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(matches!(
            plan_legacy_hosts_recovery(&roaming, &hosts),
            Err(LegacyCleanupError::HostsStateInvalid(_))
        ));
    }
}
