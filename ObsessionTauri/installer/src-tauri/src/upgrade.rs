use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use obsession_runtime_client::RuntimeClient;
use obsession_runtime_protocol::{Request as RuntimeRequest, Response as RuntimeResponse};
use semver::Version;
use tauri::AppHandle;
use winreg::enums::{
    RegType, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_BINARY, REG_DWORD, REG_DWORD_BIG_ENDIAN,
    REG_EXPAND_SZ, REG_FULL_RESOURCE_DESCRIPTOR, REG_LINK, REG_MULTI_SZ, REG_NONE, REG_QWORD,
    REG_RESOURCE_LIST, REG_RESOURCE_REQUIREMENTS_LIST, REG_SZ,
};
use winreg::{RegKey, RegValue};

use super::{
    emit_progress, path_key, reject_reparse_points, validate_directory_ownership,
    validate_install_dir, CREATE_NO_WINDOW,
};

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Obsession";
const PRODUCT_KEY: &str = r"Software\vlarpsu\Obsession";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Obsession";
const CHILD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(250);
const LOG_ROTATE_BYTES: u64 = 2 * 1024 * 1024;
const TRANSACTION_JOURNAL_SCHEMA: u32 = 1;
const TRANSACTION_JOURNAL_FILES: [&str; 2] = [
    "installer-transaction-a.json",
    "installer-transaction-b.json",
];
const LEGACY_USER_DATA_DIRECTORY: &str = "Obsession";
const USER_DATA_VENDOR_DIRECTORY: &str = "vlarpsu";
const USER_DATA_DIRECTORY: &str = "Obsession";
const MAX_MIGRATED_SETTINGS_BYTES: u64 = 1024 * 1024;
const MAX_MIGRATED_PROFILES_BYTES: u64 = 4 * 1024 * 1024;

static SAFETY_HELPER: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src-tauri/nsis/installer-safety.ps1"
));

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InstallMode {
    Install,
    Update,
    Repair,
    Blocked,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallerSnapshot {
    pub dir: String,
    pub mode: InstallMode,
    pub installed_version: Option<String>,
    pub log_path: String,
}

#[derive(Clone, Debug)]
struct ExistingInstallation {
    dir: PathBuf,
    version: Option<String>,
}

#[derive(Debug)]
pub(crate) struct InstallPlan {
    pub mode: InstallMode,
    pub target: PathBuf,
    installed_version: Option<String>,
}

#[derive(Debug)]
struct RawRegistryValue {
    name: String,
    value: RegValue,
}

#[derive(Debug)]
struct RegistryKeySnapshot {
    path: &'static str,
    existed: bool,
    values: Vec<RawRegistryValue>,
}

#[derive(Debug)]
struct RegistrySnapshot {
    keys: Vec<RegistryKeySnapshot>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurableRawRegistryValue {
    name: String,
    value_type: String,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurableRegistryKeySnapshot {
    path: String,
    existed: bool,
    values: Vec<DurableRawRegistryValue>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurableRegistrySnapshot {
    keys: Vec<DurableRegistryKeySnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    Prepared,
    Committing,
    Swapped,
    Committed,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionJournal {
    schema_version: u32,
    sequence: u64,
    transaction_id: String,
    phase: TransactionPhase,
    target: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
    target_existed: bool,
    old_registry: DurableRegistrySnapshot,
}

struct InstallerLog {
    path: PathBuf,
    file: File,
}

struct TempArtifacts {
    paths: Vec<PathBuf>,
}

impl TempArtifacts {
    fn new() -> Self {
        Self { paths: Vec::new() }
    }

    fn write(&mut self, prefix: &str, extension: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for attempt in 0..32u32 {
            let path = std::env::temp_dir().join(format!(
                "{prefix}-{}-{nonce:x}-{attempt}.{extension}",
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    file.write_all(bytes)
                        .and_then(|_| file.sync_all())
                        .map_err(|e| format!("Не удалось подготовить {}: {e}", path.display()))?;
                    self.paths.push(path.clone());
                    return Ok(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!(
                        "Не удалось создать временный файл {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        Err("Не удалось подобрать уникальное имя временного файла.".into())
    }
}

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

impl InstallerLog {
    fn open() -> Result<Self, String> {
        let path = persistent_log_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Не удалось создать каталог журнала: {e}"))?;
        }
        if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > LOG_ROTATE_BYTES) {
            let old = path.with_extension("old.log");
            let _ = fs::remove_file(&old);
            let _ = fs::rename(&path, old);
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("Не удалось открыть журнал инсталлера: {e}"))?;
        Ok(Self { path, file })
    }

    fn write(&mut self, level: &str, message: impl AsRef<str>) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(
            self.file,
            "[{timestamp}] {level}: {}",
            message.as_ref().replace(['\r', '\n'], " ")
        );
        let _ = self.file.flush();
    }
}

impl RegistrySnapshot {
    fn capture() -> Result<Self, String> {
        Ok(Self {
            keys: vec![
                capture_registry_key(UNINSTALL_KEY)?,
                capture_registry_key(PRODUCT_KEY)?,
            ],
        })
    }

    fn restore(&self) -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for snapshot in &self.keys {
            match hkcu.delete_subkey_all(snapshot.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "Не удалось очистить ключ реестра {}: {error}",
                        snapshot.path
                    ));
                }
            }
            if !snapshot.existed {
                continue;
            }
            let (key, _) = hkcu
                .create_subkey(snapshot.path)
                .map_err(|e| format!("Не удалось восстановить {}: {e}", snapshot.path))?;
            for entry in &snapshot.values {
                key.set_raw_value(&entry.name, &entry.value).map_err(|e| {
                    format!(
                        "Не удалось восстановить значение {}\\{}: {e}",
                        snapshot.path, entry.name
                    )
                })?;
            }
        }
        Ok(())
    }
}

fn registry_type_name(value_type: &RegType) -> &'static str {
    match value_type {
        REG_NONE => "none",
        REG_SZ => "sz",
        REG_EXPAND_SZ => "expand_sz",
        REG_BINARY => "binary",
        REG_DWORD => "dword",
        REG_DWORD_BIG_ENDIAN => "dword_big_endian",
        REG_LINK => "link",
        REG_MULTI_SZ => "multi_sz",
        REG_RESOURCE_LIST => "resource_list",
        REG_FULL_RESOURCE_DESCRIPTOR => "full_resource_descriptor",
        REG_RESOURCE_REQUIREMENTS_LIST => "resource_requirements_list",
        REG_QWORD => "qword",
    }
}

fn registry_type_from_name(value_type: &str) -> Result<RegType, String> {
    match value_type {
        "none" => Ok(REG_NONE),
        "sz" => Ok(REG_SZ),
        "expand_sz" => Ok(REG_EXPAND_SZ),
        "binary" => Ok(REG_BINARY),
        "dword" => Ok(REG_DWORD),
        "dword_big_endian" => Ok(REG_DWORD_BIG_ENDIAN),
        "link" => Ok(REG_LINK),
        "multi_sz" => Ok(REG_MULTI_SZ),
        "resource_list" => Ok(REG_RESOURCE_LIST),
        "full_resource_descriptor" => Ok(REG_FULL_RESOURCE_DESCRIPTOR),
        "resource_requirements_list" => Ok(REG_RESOURCE_REQUIREMENTS_LIST),
        "qword" => Ok(REG_QWORD),
        other => Err(format!(
            "Журнал установки содержит неизвестный тип значения реестра: {other}"
        )),
    }
}

impl DurableRegistrySnapshot {
    fn from_runtime(snapshot: &RegistrySnapshot) -> Self {
        Self {
            keys: snapshot
                .keys
                .iter()
                .map(|key| DurableRegistryKeySnapshot {
                    path: key.path.to_string(),
                    existed: key.existed,
                    values: key
                        .values
                        .iter()
                        .map(|entry| DurableRawRegistryValue {
                            name: entry.name.clone(),
                            value_type: registry_type_name(&entry.value.vtype).to_string(),
                            bytes: entry.value.bytes.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn into_runtime(self) -> Result<RegistrySnapshot, String> {
        let mut paths = HashSet::new();
        let mut keys = Vec::with_capacity(self.keys.len());
        for key in self.keys {
            let path = match key.path.as_str() {
                UNINSTALL_KEY => UNINSTALL_KEY,
                PRODUCT_KEY => PRODUCT_KEY,
                other => {
                    return Err(format!(
                        "Журнал установки содержит неожиданный ключ реестра: {other}"
                    ));
                }
            };
            if !paths.insert(path) {
                return Err(format!(
                    "Журнал установки содержит повторный ключ реестра: {path}"
                ));
            }

            let mut names = HashSet::new();
            let mut values = Vec::with_capacity(key.values.len());
            for entry in key.values {
                if !names.insert(entry.name.to_lowercase()) {
                    return Err(format!(
                        "Журнал установки содержит повторное значение {path}\\{}",
                        entry.name
                    ));
                }
                values.push(RawRegistryValue {
                    name: entry.name,
                    value: RegValue {
                        bytes: entry.bytes,
                        vtype: registry_type_from_name(&entry.value_type)?,
                    },
                });
            }
            keys.push(RegistryKeySnapshot {
                path,
                existed: key.existed,
                values,
            });
        }
        if paths.len() != 2 || !paths.contains(UNINSTALL_KEY) || !paths.contains(PRODUCT_KEY) {
            return Err("Журнал установки не содержит полный снимок реестра Obsession.".into());
        }
        Ok(RegistrySnapshot { keys })
    }
}

fn capture_registry_key(path: &'static str) -> Result<RegistryKeySnapshot, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(path, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RegistryKeySnapshot {
                path,
                existed: false,
                values: Vec::new(),
            });
        }
        Err(error) => return Err(format!("Не удалось прочитать {path}: {error}")),
    };

    let values = key
        .enum_values()
        .map(|entry| {
            entry
                .map(|(name, value)| RawRegistryValue { name, value })
                .map_err(|e| format!("Не удалось прочитать значение {path}: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RegistryKeySnapshot {
        path,
        existed: true,
        values,
    })
}

fn persistent_state_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("vlarpsu")
        .join("Obsession")
}

fn transaction_journal_paths(state_dir: &Path) -> [PathBuf; 2] {
    TRANSACTION_JOURNAL_FILES.map(|name| state_dir.join(name))
}

fn load_transaction_journal_from(state_dir: &Path) -> Result<Option<TransactionJournal>, String> {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for (index, path) in transaction_journal_paths(state_dir).into_iter().enumerate() {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                invalid.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        match serde_json::from_slice::<TransactionJournal>(&bytes) {
            Ok(journal)
                if journal.schema_version == TRANSACTION_JOURNAL_SCHEMA && journal.sequence > 0 =>
            {
                valid.push((index, journal));
            }
            Ok(journal) => invalid.push(format!(
                "{}: неподдерживаемая схема {} или sequence=0",
                path.display(),
                journal.schema_version
            )),
            Err(error) => invalid.push(format!("{}: {error}", path.display())),
        }
    }

    if valid.is_empty() {
        return if invalid.is_empty() {
            Ok(None)
        } else {
            Err(format!(
                "Журнал предыдущей установки повреждён: {}",
                invalid.join("; ")
            ))
        };
    }
    if valid.len() == 2 {
        let (_, first) = &valid[0];
        let (_, second) = &valid[1];
        if first.transaction_id != second.transaction_id || first.sequence == second.sequence {
            return Err("Обнаружены конфликтующие журналы предыдущей установки.".into());
        }
    }
    valid.sort_by_key(|(_, journal)| journal.sequence);
    Ok(valid.pop().map(|(_, journal)| journal))
}

impl TransactionJournal {
    fn new(
        transaction_id: String,
        target: PathBuf,
        stage: PathBuf,
        backup: PathBuf,
        target_existed: bool,
        old_registry: &RegistrySnapshot,
    ) -> Self {
        Self {
            schema_version: TRANSACTION_JOURNAL_SCHEMA,
            sequence: 0,
            transaction_id,
            phase: TransactionPhase::Prepared,
            target,
            stage,
            backup,
            target_existed,
            old_registry: DurableRegistrySnapshot::from_runtime(old_registry),
        }
    }

    fn write_phase_to(&mut self, state_dir: &Path, phase: TransactionPhase) -> Result<(), String> {
        let mut next = self.clone();
        next.sequence = next
            .sequence
            .checked_add(1)
            .ok_or_else(|| "Переполнен счётчик журнала установки.".to_string())?;
        next.phase = phase;
        fs::create_dir_all(state_dir)
            .map_err(|e| format!("Не удалось создать каталог журнала установки: {e}"))?;
        reject_reparse_points(state_dir)?;

        let paths = transaction_journal_paths(state_dir);
        let slot = next.sequence as usize % paths.len();
        let bytes = serde_json::to_vec(&next)
            .map_err(|e| format!("Не удалось сериализовать журнал установки: {e}"))?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&paths[slot])
            .map_err(|e| format!("Не удалось открыть журнал установки: {e}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("Не удалось зафиксировать журнал установки: {e}"))?;
        *self = next;
        Ok(())
    }

    fn write_phase(&mut self, phase: TransactionPhase) -> Result<(), String> {
        self.write_phase_to(&persistent_state_dir(), phase)
    }

    fn clear_from(&self, state_dir: &Path) -> Result<(), String> {
        let paths = transaction_journal_paths(state_dir);
        let newest = self.sequence as usize % paths.len();
        for index in [1 - newest, newest] {
            match fs::remove_file(&paths[index]) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "Не удалось очистить журнал установки {}: {error}",
                        paths[index].display()
                    ));
                }
            }
        }
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        self.clear_from(&persistent_state_dir())
    }
}

fn validate_transaction_journal(journal: &TransactionJournal) -> Result<(), String> {
    if journal.schema_version != TRANSACTION_JOURNAL_SCHEMA
        || journal.transaction_id.len() < 3
        || journal.transaction_id.len() > 80
        || !journal
            .transaction_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
    {
        return Err("Журнал установки содержит некорректный идентификатор транзакции.".into());
    }
    validate_install_dir(journal.target.to_string_lossy().as_ref())?;
    let parent = journal
        .target
        .parent()
        .ok_or_else(|| "Целевой каталог из журнала не имеет родителя.".to_string())?;
    let expected_stage = parent.join(format!(".obsession-stage-{}", journal.transaction_id));
    let expected_backup = parent.join(format!(".obsession-backup-{}", journal.transaction_id));
    if path_key(&journal.stage) != path_key(&expected_stage)
        || path_key(&journal.backup) != path_key(&expected_backup)
    {
        return Err("Журнал установки содержит неподтверждённые staging/backup пути.".into());
    }
    Ok(())
}

fn validate_owned_recovery_directory(path: &Path, label: &str) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    reject_reparse_points(path)?;
    validate_directory_ownership(path)
        .map_err(|error| format!("Небезопасный {label}-каталог {}: {error}", path.display()))
}

fn recover_transaction_files(
    journal: &TransactionJournal,
    log: &mut InstallerLog,
) -> Result<(), String> {
    validate_owned_recovery_directory(&journal.target, "target")?;
    validate_owned_recovery_directory(&journal.backup, "backup")?;

    if journal.phase == TransactionPhase::Committed {
        if !journal.target.exists() {
            return Err(format!(
                "Зафиксированная установка отсутствует: {}",
                journal.target.display()
            ));
        }
        cleanup_controlled_directory(&journal.stage, log)?;
        cleanup_controlled_directory(&journal.backup, log)?;
        return Ok(());
    }

    let target_exists = journal.target.exists();
    let stage_exists = journal.stage.exists();
    let backup_exists = journal.backup.exists();
    if journal.target_existed {
        match (target_exists, stage_exists, backup_exists) {
            (true, true, true) => {
                Err("Recovery остановлен: одновременно существуют target, staging и backup.".into())
            }
            (_, _, true) => {
                rollback_directory_swap(&journal.target, &journal.stage, &journal.backup, true)
            }
            (true, true, false) => cleanup_controlled_directory(&journal.stage, log),
            (true, false, false) => Ok(()),
            (false, _, false) => Err(format!(
                "Recovery не нашёл предыдущую установку или backup для {}",
                journal.target.display()
            )),
        }
    } else {
        if backup_exists {
            return Err("Recovery обнаружил неожиданный backup свежей установки.".into());
        }
        cleanup_controlled_directory(&journal.target, log)?;
        cleanup_controlled_directory(&journal.stage, log)
    }
}

pub(crate) fn recover_pending_transaction(expected_target: Option<&Path>) -> Result<(), String> {
    let Some(journal) = load_transaction_journal_from(&persistent_state_dir())? else {
        return Ok(());
    };
    validate_transaction_journal(&journal)?;
    if expected_target.is_some_and(|target| path_key(target) != path_key(&journal.target)) {
        return Err(format!(
            "Сначала требуется восстановить предыдущую установку в {}.",
            journal.target.display()
        ));
    }
    let old_registry = journal.old_registry.clone().into_runtime()?;
    let mut log = InstallerLog::open()?;
    log.write(
        "WARN",
        format!(
            "recovering transaction id={} phase={:?}",
            journal.transaction_id, journal.phase
        ),
    );
    recover_transaction_files(&journal, &mut log)?;
    if journal.phase != TransactionPhase::Committed {
        old_registry.restore()?;
    }
    journal.clear()?;
    log.write("INFO", "pending installer transaction recovered");
    Ok(())
}

pub(crate) fn persistent_log_path() -> PathBuf {
    persistent_state_dir().join("installer.log")
}

fn default_install_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\"))
        .join("Obsession")
}

fn trim_registry_path(value: String) -> Option<PathBuf> {
    let value = value.trim().trim_matches('"').trim();
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn discover_existing_installation() -> Result<Option<ExistingInstallation>, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall = match hkcu.open_subkey_with_flags(UNINSTALL_KEY, KEY_READ) {
        Ok(key) => Some(key),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Не удалось прочитать сведения об установке: {error}"
            ))
        }
    };
    let product = match hkcu.open_subkey_with_flags(PRODUCT_KEY, KEY_READ) {
        Ok(key) => Some(key),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Не удалось прочитать путь установки: {error}")),
    };

    let version = uninstall
        .as_ref()
        .and_then(|key| key.get_value::<String, _>("DisplayVersion").ok())
        .filter(|value| !value.trim().is_empty());
    let dir = uninstall
        .as_ref()
        .and_then(|key| key.get_value::<String, _>("InstallLocation").ok())
        .and_then(trim_registry_path)
        .or_else(|| {
            product
                .as_ref()
                .and_then(|key| key.get_value::<String, _>("").ok())
                .and_then(trim_registry_path)
        })
        .or_else(|| {
            uninstall
                .as_ref()
                .and_then(|key| key.get_value::<String, _>("UninstallString").ok())
                .and_then(trim_registry_path)
                .and_then(|path| path.parent().map(Path::to_path_buf))
        });

    match dir {
        Some(dir) => Ok(Some(ExistingInstallation { dir, version })),
        None if uninstall.is_some() || product.is_some() => {
            Err("В реестре найдена повреждённая запись Obsession без каталога установки.".into())
        }
        None => Ok(None),
    }
}

fn planned_mode(installed: Option<&str>, current: &str) -> Result<InstallMode, String> {
    let Some(installed) = installed else {
        return Ok(InstallMode::Repair);
    };
    let current = Version::parse(current)
        .map_err(|e| format!("Некорректная версия текущего инсталлера: {e}"))?;
    let Ok(installed) = Version::parse(installed) else {
        return Ok(InstallMode::Repair);
    };
    Ok(match installed.cmp(&current) {
        std::cmp::Ordering::Less => InstallMode::Update,
        std::cmp::Ordering::Equal => InstallMode::Repair,
        std::cmp::Ordering::Greater => InstallMode::Blocked,
    })
}

pub(crate) fn installer_snapshot(current_version: &str) -> Result<InstallerSnapshot, String> {
    recover_pending_transaction(None)?;
    let existing = discover_existing_installation()?;
    let (dir, mode, installed_version) = match existing {
        Some(existing) => {
            let mode = planned_mode(existing.version.as_deref(), current_version)?;
            (existing.dir, mode, existing.version)
        }
        None => (default_install_dir(), InstallMode::Install, None),
    };
    Ok(InstallerSnapshot {
        dir: dir.to_string_lossy().into_owned(),
        mode,
        installed_version,
        log_path: persistent_log_path().to_string_lossy().into_owned(),
    })
}

pub(crate) fn record_preflight_error(error: String) -> String {
    match InstallerLog::open() {
        Ok(mut log) => {
            log.write("ERROR", format!("preflight: {error}"));
            format!("{error}\nЖурнал: {}", log.path.display())
        }
        Err(log_error) => format!("{error}\nДополнительно: {log_error}"),
    }
}

pub(crate) fn prepare_plan(raw_dir: &str, current_version: &str) -> Result<InstallPlan, String> {
    let target = validate_install_dir(raw_dir)?;
    recover_pending_transaction(Some(&target))?;
    let existing = discover_existing_installation()?;
    let (mode, installed_version) = match existing {
        Some(existing) => {
            if path_key(&existing.dir) != path_key(&target) {
                return Err(format!(
                    "Obsession уже установлен в {}. Обновление и восстановление выполняются только в существующем каталоге.",
                    existing.dir.display()
                ));
            }
            (
                planned_mode(existing.version.as_deref(), current_version)?,
                existing.version,
            )
        }
        None => {
            let non_empty_owned = target.is_dir()
                && fs::read_dir(&target)
                    .map(|mut entries| entries.next().is_some())
                    .unwrap_or(false);
            if non_empty_owned {
                (InstallMode::Repair, None)
            } else {
                (InstallMode::Install, None)
            }
        }
    };
    if mode == InstallMode::Blocked {
        return Err(format!(
            "Установлена более новая версия Obsession ({}). Понижение до {current_version} заблокировано.",
            installed_version.as_deref().unwrap_or("неизвестно")
        ));
    }
    Ok(InstallPlan {
        mode,
        target,
        installed_version,
    })
}

fn transaction_paths(target: &Path) -> Result<(String, PathBuf, PathBuf), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "У каталога установки нет родительской директории.".to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let suffix = format!("{}-{nonce:x}", std::process::id());
    let stage = parent.join(format!(".obsession-stage-{suffix}"));
    let backup = parent.join(format!(".obsession-backup-{suffix}"));
    if stage.exists() || backup.exists() {
        return Err("Не удалось создать уникальный staging-каталог.".into());
    }
    Ok((suffix, stage, backup))
}

fn repair_registry_paths(target: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (product, _) = hkcu
        .create_subkey(PRODUCT_KEY)
        .map_err(|e| format!("Не удалось обновить путь продукта: {e}"))?;
    product
        .set_value("", &target.to_string_lossy().as_ref())
        .map_err(|e| format!("Не удалось записать путь продукта: {e}"))?;

    let (uninstall, _) = hkcu
        .create_subkey(UNINSTALL_KEY)
        .map_err(|e| format!("Не удалось обновить uninstall-запись: {e}"))?;
    let exe = target.join("obsession.exe");
    let uninstaller = target.join("uninstall.exe");
    uninstall
        .set_value("DisplayIcon", &format!("\"{}\"", exe.display()))
        .and_then(|_| uninstall.set_value("InstallLocation", &format!("\"{}\"", target.display())))
        .and_then(|_| {
            uninstall.set_value("UninstallString", &format!("\"{}\"", uninstaller.display()))
        })
        .map_err(|e| format!("Не удалось зафиксировать новый путь установки: {e}"))?;
    Ok(())
}

fn capture_autostart_value() -> Result<Option<RegValue>, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ) {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Не удалось прочитать автозапуск Obsession: {error}"
            ))
        }
    };
    match key.get_raw_value(RUN_VALUE) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Не удалось прочитать автозапуск Obsession: {error}"
        )),
    }
}

fn restore_autostart_value(value: Option<&RegValue>) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match value {
        Some(value) => {
            let (key, _) = hkcu
                .create_subkey(RUN_KEY)
                .map_err(|error| format!("Не удалось восстановить ключ автозапуска: {error}"))?;
            key.set_raw_value(RUN_VALUE, value)
                .map_err(|error| format!("Не удалось восстановить автозапуск: {error}"))
        }
        // If the value was absent, machine finalization never creates it.
        None => Ok(()),
    }
}

fn apply_machine_user_registry(
    target: &Path,
    current_version: &str,
    autostart_was_enabled: bool,
) -> Result<(), String> {
    let executable = target.join("obsession.exe");
    if !executable.is_file() {
        return Err(format!(
            "Не найден защищённый executable: {}",
            executable.display()
        ));
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (product, _) = hkcu
        .create_subkey(PRODUCT_KEY)
        .map_err(|error| format!("Не удалось обновить путь Obsession: {error}"))?;
    product
        .set_value("", &target.to_string_lossy().as_ref())
        .and_then(|()| product.set_value("Version", &current_version))
        .map_err(|error| format!("Не удалось сохранить путь Obsession: {error}"))?;

    match hkcu.delete_subkey_all(UNINSTALL_KEY) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "Не удалось удалить устаревшую пользовательскую uninstall-запись: {error}"
            ))
        }
    }

    if autostart_was_enabled {
        let (run, _) = hkcu
            .create_subkey(RUN_KEY)
            .map_err(|error| format!("Не удалось открыть автозапуск: {error}"))?;
        run.set_value(RUN_VALUE, &format!("\"{}\"", executable.display()))
            .map_err(|error| format!("Не удалось перенести автозапуск: {error}"))?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum MigratedJsonShape {
    Object,
    Array,
}

fn read_bounded_migration_json(
    path: &Path,
    max_bytes: u64,
    shape: MigratedJsonShape,
) -> Result<Vec<u8>, String> {
    reject_reparse_points(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Could not inspect legacy user data {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "Legacy user data is not a regular file: {}",
            path.display()
        ));
    }
    if metadata.len() > max_bytes {
        return Err(format!(
            "Legacy user data exceeds its {} byte migration limit: {}",
            max_bytes,
            path.display()
        ));
    }

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .map_err(|error| {
            format!(
                "Could not open legacy user data {}: {error}",
                path.display()
            )
        })?
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            format!(
                "Could not read legacy user data {}: {error}",
                path.display()
            )
        })?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!(
            "Legacy user data grew beyond its migration limit: {}",
            path.display()
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "Legacy user data is not valid JSON ({}): {error}",
            path.display()
        )
    })?;
    let valid_shape = match shape {
        MigratedJsonShape::Object => value.is_object(),
        MigratedJsonShape::Array => value.is_array(),
    };
    if !valid_shape {
        return Err(format!(
            "Legacy user data has an unexpected JSON shape: {}",
            path.display()
        ));
    }
    Ok(bytes)
}

fn write_migrated_user_data_create_new(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    if path.exists() {
        return Ok(false);
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Migration destination has no valid file name".to_string())?;
    for attempt in 0..32u32 {
        let temporary = path.with_file_name(format!(
            ".{file_name}.migration.{}.{nonce:x}.{attempt}.tmp",
            std::process::id()
        ));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Could not create migration file {}: {error}",
                    temporary.display()
                ))
            }
        };
        if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "Could not persist migrated user data {}: {error}",
                temporary.display()
            ));
        }
        drop(file);
        match fs::rename(&temporary, path) {
            Ok(()) => return Ok(true),
            Err(_) if path.exists() => {
                reject_reparse_points(path)?;
                let metadata = fs::symlink_metadata(path).map_err(|error| {
                    format!(
                        "Could not inspect concurrent migration destination {}: {error}",
                        path.display()
                    )
                })?;
                let _ = fs::remove_file(&temporary);
                if metadata.is_file() {
                    return Ok(false);
                }
                return Err(format!(
                    "Concurrent migration destination is not a regular file: {}",
                    path.display()
                ));
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(format!(
                    "Could not commit migrated user data {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Err("Could not allocate a unique migration file name".into())
}

fn migrate_allowlisted_user_data(source: &Path, target: &Path) -> Result<usize, String> {
    if path_key(source) == path_key(target) {
        return Err("Legacy and destination user data directories unexpectedly match".into());
    }
    if !source.exists() {
        return Ok(0);
    }
    reject_reparse_points(source)?;
    reject_reparse_points(target)?;
    fs::create_dir_all(target).map_err(|error| {
        format!(
            "Could not create user data directory {}: {error}",
            target.display()
        )
    })?;
    reject_reparse_points(target)?;

    let allowlist = [
        (
            "settings.json",
            MAX_MIGRATED_SETTINGS_BYTES,
            MigratedJsonShape::Object,
        ),
        (
            "profiles.json",
            MAX_MIGRATED_PROFILES_BYTES,
            MigratedJsonShape::Array,
        ),
    ];
    let mut migrated = 0usize;
    for (name, max_bytes, shape) in allowlist {
        let source_file = source.join(name);
        if !source_file.exists() {
            continue;
        }
        let target_file = target.join(name);
        if target_file.exists() {
            read_bounded_migration_json(&target_file, max_bytes, shape)?;
            continue;
        }
        let bytes = read_bounded_migration_json(&source_file, max_bytes, shape)?;
        if write_migrated_user_data_create_new(&target_file, &bytes)? {
            migrated += 1;
        }
    }
    Ok(migrated)
}

fn migrate_current_user_data(log: &mut InstallerLog) -> Result<(), String> {
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "Windows did not provide APPDATA for user-data migration".to_string())?;
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "Windows did not provide LOCALAPPDATA for user-data migration".to_string()
        })?;
    if !roaming.is_absolute() || !local.is_absolute() {
        return Err("Windows returned a non-absolute AppData path".into());
    }
    let source = roaming.join(LEGACY_USER_DATA_DIRECTORY);
    let target = local
        .join(USER_DATA_VENDOR_DIRECTORY)
        .join(USER_DATA_DIRECTORY);
    let migrated = migrate_allowlisted_user_data(&source, &target)?;
    log.write(
        "INFO",
        format!(
            "allowlisted user data migration source={} target={} copied={migrated}",
            source.display(),
            target.display()
        ),
    );
    Ok(())
}

fn cleanup_legacy_user_install(log: &mut InstallerLog) -> Result<(), String> {
    let client = RuntimeClient::new(Duration::from_secs(15))
        .map_err(|error| format!("could not initialize protected legacy cleanup: {error}"))?;
    let response = client
        .call("setup-legacy-cleanup", RuntimeRequest::LegacyCleanup)
        .map_err(|error| format!("protected legacy cleanup request failed: {error}"))?;
    match response.response {
        RuntimeResponse::LegacyCleanupCompleted => {
            log.write("INFO", "protected legacy per-user cleanup completed");
            Ok(())
        }
        RuntimeResponse::Error(error) => Err(format!(
            "protected legacy cleanup was rejected by the runtime service: {:?}",
            error.code
        )),
        other => Err(format!(
            "runtime service returned an unexpected legacy cleanup response: {other:?}"
        )),
    }
}

pub(crate) fn finalize_machine_user_state(
    target: &Path,
    current_version: &str,
) -> Result<(), String> {
    let expected = super::machine_worker::machine_install_root()?;
    if path_key(target) != path_key(&expected) {
        return Err("Medium setup получил неожиданный machine install path.".into());
    }
    reject_reparse_points(target)?;

    let mut log = InstallerLog::open()?;
    let old_registry = RegistrySnapshot::capture()?;
    let old_autostart = capture_autostart_value()?;
    let result = (|| {
        migrate_current_user_data(&mut log)?;
        let mut artifacts = TempArtifacts::new();
        let helper = artifacts.write("obsession-installer-safety", "ps1", SAFETY_HELPER)?;
        run_safety_helper(
            &helper,
            "CleanupMachineUserShortcuts",
            target,
            None,
            None,
            &mut log,
        )?;
        apply_machine_user_registry(target, current_version, old_autostart.is_some())
    })();

    if let Err(error) = result {
        let registry_restore = old_registry.restore().err();
        let autostart_restore = restore_autostart_value(old_autostart.as_ref()).err();
        log.write(
            "ERROR",
            format!("machine user finalization failed: {error}"),
        );
        if registry_restore.is_some() || autostart_restore.is_some() {
            return Err(format!(
                "{error}; rollback HKCU также завершился ошибкой: registry={registry_restore:?}, autostart={autostart_restore:?}"
            ));
        }
        return Err(error);
    }
    // Do not roll HKCU/shortcuts back to the obsolete AppData executable after
    // cleanup begins. At this point every user-facing pointer already targets
    // the protected Program Files installation; a cleanup failure is retryable
    // without restoring the vulnerable legacy launch path.
    if let Err(error) = cleanup_legacy_user_install(&mut log) {
        log.write("ERROR", format!("protected legacy cleanup failed: {error}"));
        return Err(error);
    }
    log.write(
        "INFO",
        format!("machine user state finalized target={}", target.display()),
    );
    Ok(())
}

pub(crate) fn cleanup_machine_user_state(target: &Path) -> Result<(), String> {
    let expected = super::machine_worker::machine_install_root()?;
    if path_key(target) != path_key(&expected) {
        return Err("Medium uninstaller received an unexpected machine install path.".into());
    }
    reject_reparse_points(target)?;

    let mut log = InstallerLog::open()?;
    let mut artifacts = TempArtifacts::new();
    let helper = artifacts.write("obsession-installer-safety", "ps1", SAFETY_HELPER)?;
    run_safety_helper(
        &helper,
        "CleanupMachineUserShortcuts",
        target,
        None,
        None,
        &mut log,
    )?;

    let executable = target.join("obsession.exe");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE) {
        let owned = run
            .get_value::<String, _>(RUN_VALUE)
            .ok()
            .and_then(trim_registry_path)
            .is_some_and(|path| path_key(&path) == path_key(&executable));
        if owned {
            run.delete_value(RUN_VALUE)
                .map_err(|error| format!("could not remove current-user autostart: {error}"))?;
        }
    }

    delete_hkcu_install_key_if_owned(&hkcu, PRODUCT_KEY, "", target)?;
    delete_hkcu_install_key_if_owned(&hkcu, UNINSTALL_KEY, "InstallLocation", target)?;
    log.write(
        "INFO",
        format!(
            "machine current-user state removed target={}",
            target.display()
        ),
    );
    Ok(())
}

fn delete_hkcu_install_key_if_owned(
    hkcu: &RegKey,
    key_path: &str,
    value_name: &str,
    expected: &Path,
) -> Result<(), String> {
    let owned = match hkcu.open_subkey_with_flags(key_path, KEY_READ) {
        Ok(key) => key
            .get_value::<String, _>(value_name)
            .ok()
            .and_then(trim_registry_path)
            .is_some_and(|path| path_key(&path) == path_key(expected)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("could not inspect HKCU {key_path}: {error}")),
    };
    if !owned {
        return Ok(());
    }
    match hkcu.delete_subkey_all(key_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not remove HKCU {key_path}: {error}")),
    }
}

fn kill_child_tree(child: &mut Child, log: &mut InstallerLog) {
    let pid = child.id().to_string();
    log.write("WARN", format!("terminating hung NSIS tree pid={pid}"));
    let _ = Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.wait();
}

fn wait_for_child_with_timeout(
    child: &mut Child,
    log: &mut InstallerLog,
    timeout: Duration,
) -> Result<ExitStatus, String> {
    let started = Instant::now();
    let mut next_heartbeat = Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() >= timeout => {
                kill_child_tree(child, log);
                let duration = if timeout.as_secs() >= 60 {
                    format!("{} минут", timeout.as_secs() / 60)
                } else {
                    format!("{} секунд", timeout.as_secs())
                };
                return Err(format!(
                    "Внутренний установщик не завершился за {duration} и был остановлен."
                ));
            }
            Ok(None) => {
                if started.elapsed() >= next_heartbeat {
                    log.write(
                        "INFO",
                        format!("NSIS still running after {}s", started.elapsed().as_secs()),
                    );
                    next_heartbeat += Duration::from_secs(30);
                }
                std::thread::sleep(CHILD_POLL_INTERVAL);
            }
            Err(error) => {
                kill_child_tree(child, log);
                return Err(format!("Ошибка ожидания внутреннего установщика: {error}"));
            }
        }
    }
}

fn wait_for_child(child: &mut Child, log: &mut InstallerLog) -> Result<ExitStatus, String> {
    wait_for_child_with_timeout(child, log, CHILD_TIMEOUT)
}

fn run_payload(
    payload: &Path,
    target: &Path,
    staged_update: bool,
    log: &mut InstallerLog,
) -> Result<(), String> {
    let mut command = Command::new(payload);
    command.raw_arg("/S");
    if staged_update {
        command.raw_arg("/UPDATE").raw_arg("/NS");
    }
    command
        .raw_arg(format!("/D={}", target.display()))
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Не удалось запустить внутренний установщик: {e}"))?;
    log.write(
        "INFO",
        format!(
            "spawned NSIS pid={} target={} staged={staged_update}",
            child.id(),
            target.display()
        ),
    );
    let status = wait_for_child(&mut child, log)?;
    log.write("INFO", format!("NSIS exit status={status}"));
    if !status.success() {
        return Err(format!(
            "Внутренний установщик завершился с кодом {}.",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn run_safety_helper(
    helper: &Path,
    action: &str,
    target: &Path,
    desktop: Option<bool>,
    start_menu: Option<bool>,
    log: &mut InstallerLog,
) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(helper)
        .env("OBSESSION_INSTALL_DIR", target)
        .env("OBSESSION_SETUP_ACTION", action)
        .creation_flags(CREATE_NO_WINDOW);
    if let Some(desktop) = desktop {
        command.env(
            "OBSESSION_SHORTCUT_DESKTOP",
            if desktop { "1" } else { "0" },
        );
    }
    if let Some(start_menu) = start_menu {
        command.env(
            "OBSESSION_SHORTCUT_START_MENU",
            if start_menu { "1" } else { "0" },
        );
    }
    let output = command
        .output()
        .map_err(|e| format!("Не удалось запустить safety helper: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    log.write("ERROR", format!("safety action {action} failed: {detail}"));
    Err(if detail.is_empty() {
        format!("Safety helper завершился с кодом {}.", output.status)
    } else {
        detail
    })
}

fn cleanup_controlled_directory(path: &Path, log: &mut InstallerLog) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if let Err(error) = reject_reparse_points(path) {
        log.write("ERROR", &error);
        return Err(error);
    }
    fs::remove_dir_all(path).map_err(|e| {
        let message = format!("Не удалось удалить {}: {e}", path.display());
        log.write("ERROR", &message);
        message
    })
}

fn commit_directory_swap(target: &Path, stage: &Path, backup: &Path) -> Result<bool, String> {
    let old_moved = if target.exists() {
        fs::rename(target, backup)
            .map_err(|e| format!("Не удалось создать резервную копию установленной версии: {e}"))?;
        true
    } else {
        false
    };

    if let Err(error) = fs::rename(stage, target) {
        let restore = if old_moved {
            fs::rename(backup, target).err()
        } else {
            None
        };
        return Err(match restore {
            Some(restore) => format!(
                "Не удалось зафиксировать новую версию: {error}. Возврат backup также не удался: {restore}"
            ),
            None => format!("Не удалось зафиксировать новую версию: {error}"),
        });
    }
    Ok(old_moved)
}

fn rollback_directory_swap(
    target: &Path,
    stage: &Path,
    backup: &Path,
    old_moved: bool,
) -> Result<(), String> {
    let mut errors = Vec::new();
    if target.exists() {
        if stage.exists() {
            if let Err(error) = reject_reparse_points(stage).and_then(|_| {
                fs::remove_dir_all(stage)
                    .map_err(|e| format!("Не удалось удалить {}: {e}", stage.display()))
            }) {
                errors.push(format!("не удалось очистить staging: {error}"));
            }
        }
        if let Err(error) = fs::rename(target, stage) {
            errors.push(format!("не удалось убрать новую версию: {error}"));
        }
    }
    if old_moved {
        if backup.exists() {
            if let Err(error) = fs::rename(backup, target) {
                errors.push(format!("не удалось вернуть резервную копию: {error}"));
            }
        } else {
            errors.push(format!("резервная копия отсутствует: {}", backup.display()));
        }
    }
    if stage.exists() {
        if let Err(error) = reject_reparse_points(stage).and_then(|_| {
            fs::remove_dir_all(stage)
                .map_err(|e| format!("Не удалось удалить {}: {e}", stage.display()))
        }) {
            errors.push(format!(
                "не удалось удалить неудачную новую версию: {error}"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn rollback_committed_swap(
    target: &Path,
    stage: &Path,
    backup: &Path,
    old_moved: bool,
    old_registry: &RegistrySnapshot,
    log: &mut InstallerLog,
) -> Result<(), String> {
    log.write("WARN", "rolling back committed directory swap");
    let mut errors = Vec::new();
    if let Err(error) = rollback_directory_swap(target, stage, backup, old_moved) {
        errors.push(error);
    }
    if let Err(error) = old_registry.restore() {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn rollback_precommit_failure(
    error: String,
    journal: &TransactionJournal,
    old_registry: &RegistrySnapshot,
    stage: &Path,
    log: &mut InstallerLog,
) -> String {
    let mut recovery_errors = Vec::new();
    if let Err(restore) = old_registry.restore() {
        recovery_errors.push(format!("не удалось восстановить реестр: {restore}"));
    }
    if let Err(cleanup) = cleanup_controlled_directory(stage, log) {
        recovery_errors.push(format!("не удалось очистить staging: {cleanup}"));
    }
    if recovery_errors.is_empty() {
        if let Err(clear) = journal.clear() {
            log.write(
                "WARN",
                format!("precommit rollback completed but journal remains: {clear}"),
            );
        }
        error
    } else {
        format!(
            "{error} Автоматическое восстановление не завершено: {}. Повторный запуск setup продолжит recovery.",
            recovery_errors.join("; ")
        )
    }
}

fn rollback_swapped_failure(
    error: String,
    journal: &TransactionJournal,
    old_moved: bool,
    old_registry: &RegistrySnapshot,
    log: &mut InstallerLog,
) -> String {
    match rollback_committed_swap(
        &journal.target,
        &journal.stage,
        &journal.backup,
        old_moved,
        old_registry,
        log,
    ) {
        Ok(()) => {
            if let Err(clear) = journal.clear() {
                log.write(
                    "WARN",
                    format!("rollback completed but transaction journal remains: {clear}"),
                );
            }
            format!("{error} Предыдущая версия восстановлена.")
        }
        Err(rollback) => format!(
            "{error} Автоматический rollback также завершился ошибкой: {rollback}. Резервная копия: {}. Повторный запуск setup продолжит recovery.",
            journal.backup.display()
        ),
    }
}

fn run_fresh_install(
    app: &AppHandle,
    plan: &InstallPlan,
    payload: &Path,
    helper: &Path,
    desktop: bool,
    start_menu: bool,
    log: &mut InstallerLog,
) -> Result<(), String> {
    let target_preexisted = plan.target.exists();
    let old_registry = RegistrySnapshot::capture()?;
    emit_progress(app, 20, "install");
    if let Err(error) = run_payload(payload, &plan.target, false, log) {
        let _ = old_registry.restore();
        if validate_directory_ownership(&plan.target).is_ok() {
            let _ = cleanup_controlled_directory(&plan.target, log);
            if target_preexisted {
                let _ = fs::create_dir_all(&plan.target);
            }
        }
        return Err(error);
    }

    emit_progress(app, 90, "shortcuts");
    if let Err(error) = run_safety_helper(
        helper,
        "SyncShortcuts",
        &plan.target,
        Some(desktop),
        Some(start_menu),
        log,
    ) {
        if let Err(cleanup_error) = run_safety_helper(
            helper,
            "SyncShortcuts",
            &plan.target,
            Some(false),
            Some(false),
            log,
        ) {
            log.write(
                "WARN",
                format!("failed to remove shortcuts during rollback: {cleanup_error}"),
            );
        }
        let _ = old_registry.restore();
        let _ = cleanup_controlled_directory(&plan.target, log);
        if target_preexisted {
            let _ = fs::create_dir_all(&plan.target);
        }
        return Err(format!("Не удалось применить параметры ярлыков: {error}"));
    }
    Ok(())
}

fn run_transactional_update(
    app: &AppHandle,
    plan: &InstallPlan,
    payload: &Path,
    helper: &Path,
    desktop: bool,
    start_menu: bool,
    log: &mut InstallerLog,
) -> Result<(), String> {
    emit_progress(app, 8, "stop");
    run_safety_helper(
        helper,
        "StopOwnedApplication",
        &plan.target,
        None,
        None,
        log,
    )
    .map_err(|e| format!("Не удалось остановить установленный Obsession: {e}"))?;

    // winws/tg_ws_proxy живут в %APPDATA%\Obsession\bin и переживают падение
    // приложения, поэтому StopOwnedApplication их не видит. Нефатально: они
    // лежат вне каталога установки и подмену каталога не блокируют, но
    // осиротевший winws продолжит фильтровать трафик — фиксируем в журнале.
    if let Err(error) = run_safety_helper(helper, "StopOwnedRuntime", &plan.target, None, None, log)
    {
        log.write("WARN", format!("stop owned runtime processes: {error}"));
    }

    run_safety_helper(helper, "CleanupFirewall", &plan.target, None, None, log)
        .map_err(|e| format!("Не удалось очистить правила брандмауэра Obsession: {e}"))?;

    let old_registry = RegistrySnapshot::capture()?;
    let target_existed = plan.target.exists();
    let (transaction_id, stage, backup) = transaction_paths(&plan.target)?;
    let mut journal = TransactionJournal::new(
        transaction_id,
        plan.target.clone(),
        stage.clone(),
        backup.clone(),
        target_existed,
        &old_registry,
    );
    journal.write_phase(TransactionPhase::Prepared)?;
    if let Err(error) = fs::create_dir(&stage) {
        if let Err(clear) = journal.clear() {
            log.write(
                "WARN",
                format!("staging creation failed and journal remains: {clear}"),
            );
        }
        return Err(format!(
            "Не удалось создать staging-каталог {}: {error}",
            stage.display()
        ));
    }
    log.write(
        "INFO",
        format!(
            "transaction id={} stage={} backup={}",
            journal.transaction_id,
            stage.display(),
            backup.display()
        ),
    );

    emit_progress(app, 18, "stage");
    emit_progress(app, 32, "install");
    if let Err(error) = run_payload(payload, &stage, true, log) {
        return Err(rollback_precommit_failure(
            error,
            &journal,
            &old_registry,
            &stage,
            log,
        ));
    }

    emit_progress(app, 65, "verify");
    let validation = validate_directory_ownership(&stage).and_then(|_| {
        if stage.join("obsession.exe").is_file() && stage.join("uninstall.exe").is_file() {
            Ok(())
        } else {
            Err("Staging-каталог не содержит полный комплект Obsession.".into())
        }
    });
    if let Err(error) = validation {
        return Err(rollback_precommit_failure(
            error,
            &journal,
            &old_registry,
            &stage,
            log,
        ));
    }

    let new_registry = match RegistrySnapshot::capture() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Err(rollback_precommit_failure(
                format!("Не удалось зафиксировать новый снимок реестра: {error}"),
                &journal,
                &old_registry,
                &stage,
                log,
            ));
        }
    };
    if let Err(error) = old_registry.restore() {
        return Err(rollback_precommit_failure(
            format!("Не удалось вернуть старый registry перед атомарной заменой: {error}"),
            &journal,
            &old_registry,
            &stage,
            log,
        ));
    }

    emit_progress(app, 76, "commit");
    if let Err(error) = journal.write_phase(TransactionPhase::Committing) {
        return Err(rollback_precommit_failure(
            error,
            &journal,
            &old_registry,
            &stage,
            log,
        ));
    }
    let old_moved = match commit_directory_swap(&plan.target, &stage, &backup) {
        Ok(old_moved) => old_moved,
        Err(error) => {
            return Err(rollback_precommit_failure(
                error,
                &journal,
                &old_registry,
                &stage,
                log,
            ));
        }
    };
    if let Err(error) = journal.write_phase(TransactionPhase::Swapped) {
        return Err(rollback_swapped_failure(
            error,
            &journal,
            old_moved,
            &old_registry,
            log,
        ));
    }

    emit_progress(app, 84, "registry");
    let commit_result = new_registry
        .restore()
        .and_then(|_| repair_registry_paths(&plan.target))
        .and_then(|_| {
            run_safety_helper(
                helper,
                "SyncShortcuts",
                &plan.target,
                Some(desktop),
                Some(start_menu),
                log,
            )
            .map_err(|e| format!("Не удалось применить параметры ярлыков: {e}"))
        });

    if let Err(error) = commit_result {
        emit_progress(app, 86, "rollback");
        return Err(rollback_swapped_failure(
            error,
            &journal,
            old_moved,
            &old_registry,
            log,
        ));
    }

    if let Err(error) = journal.write_phase(TransactionPhase::Committed) {
        emit_progress(app, 86, "rollback");
        return Err(rollback_swapped_failure(
            error,
            &journal,
            old_moved,
            &old_registry,
            log,
        ));
    }

    emit_progress(app, 96, "cleanup");
    let mut cleanup_complete = true;
    if old_moved {
        if let Err(error) = cleanup_controlled_directory(&backup, log) {
            cleanup_complete = false;
            log.write(
                "WARN",
                format!("update committed but backup cleanup failed: {error}"),
            );
        }
    }
    if cleanup_complete {
        if let Err(error) = journal.clear() {
            log.write(
                "WARN",
                format!("update committed but journal cleanup failed: {error}"),
            );
        }
    }
    Ok(())
}

pub(crate) fn run_install(
    app: &AppHandle,
    plan: InstallPlan,
    desktop: bool,
    start_menu: bool,
    payload_bytes: &[u8],
) -> Result<(), String> {
    let mut log = InstallerLog::open()?;
    log.write(
        "INFO",
        format!(
            "start mode={:?} target={} installed_version={:?}",
            plan.mode,
            plan.target.display(),
            plan.installed_version
        ),
    );
    emit_progress(app, 2, "prepare");

    if cfg!(debug_assertions) {
        for (pct, stage) in [
            (8, "stop"),
            (18, "stage"),
            (45, "install"),
            (65, "verify"),
            (76, "commit"),
            (84, "registry"),
            (92, "shortcuts"),
            (96, "cleanup"),
        ] {
            if plan.mode == InstallMode::Install
                && matches!(stage, "stop" | "stage" | "verify" | "commit" | "registry")
            {
                continue;
            }
            std::thread::sleep(Duration::from_millis(120));
            emit_progress(app, pct, stage);
        }
        emit_progress(app, 100, "finish");
        log.write("INFO", "debug install simulation completed");
        return Ok(());
    }

    let result = (|| {
        let mut artifacts = TempArtifacts::new();
        let payload = artifacts.write("obsession-setup-payload", "exe", payload_bytes)?;
        let helper = artifacts.write("obsession-installer-safety", "ps1", SAFETY_HELPER)?;

        match plan.mode {
            InstallMode::Install => {
                run_fresh_install(app, &plan, &payload, &helper, desktop, start_menu, &mut log)
            }
            InstallMode::Update | InstallMode::Repair => run_transactional_update(
                app, &plan, &payload, &helper, desktop, start_menu, &mut log,
            ),
            InstallMode::Blocked => Err("Понижение версии заблокировано.".into()),
        }
    })();

    match result {
        Ok(()) => {
            emit_progress(app, 100, "finish");
            log.write("INFO", "installation completed successfully");
            Ok(())
        }
        Err(error) => {
            log.write("ERROR", &error);
            Err(format!("{error}\nЖурнал: {}", log.path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::current_dir()
                .unwrap()
                .join("target")
                .join("installer-upgrade-tests")
                .join(format!("{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_log(root: &TestRoot) -> InstallerLog {
        let path = root.0.join("test.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        InstallerLog { path, file }
    }

    fn mark_owned(path: &Path, version: &str) {
        fs::create_dir_all(path).unwrap();
        fs::write(
            path.join(super::super::OWNER_MARKER),
            super::super::OWNER_ID,
        )
        .unwrap();
        fs::write(path.join("version.txt"), version).unwrap();
    }

    fn empty_registry_snapshot() -> RegistrySnapshot {
        RegistrySnapshot {
            keys: vec![
                RegistryKeySnapshot {
                    path: UNINSTALL_KEY,
                    existed: false,
                    values: Vec::new(),
                },
                RegistryKeySnapshot {
                    path: PRODUCT_KEY,
                    existed: false,
                    values: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn user_data_migration_copies_only_settings_and_profiles_without_overwrite() {
        let root = TestRoot::new();
        let source = root.0.join("legacy-roaming");
        let target = root.0.join("new-local");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::create_dir_all(source.join("configs")).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(source.join("settings.json"), br#"{"reduce_motion":true}"#).unwrap();
        fs::write(source.join("profiles.json"), br#"[{"id":"legacy"}]"#).unwrap();
        fs::write(source.join("bin/winws.exe"), b"must not migrate").unwrap();
        fs::write(source.join("configs/legacy.conf"), b"--unsafe").unwrap();
        fs::write(source.join("manifest.json"), br#"{"files":[]}"#).unwrap();
        fs::write(source.join("cache.json"), br#"{"stale":true}"#).unwrap();
        fs::write(target.join("profiles.json"), br#"[{"id":"new"}]"#).unwrap();

        assert_eq!(migrate_allowlisted_user_data(&source, &target).unwrap(), 1);
        assert_eq!(
            fs::read(target.join("settings.json")).unwrap(),
            br#"{"reduce_motion":true}"#
        );
        assert_eq!(
            fs::read(target.join("profiles.json")).unwrap(),
            br#"[{"id":"new"}]"#
        );
        assert!(!target.join("bin").exists());
        assert!(!target.join("configs").exists());
        assert!(!target.join("manifest.json").exists());
        assert!(!target.join("cache.json").exists());
        assert!(source.join("bin/winws.exe").is_file());
        assert_eq!(migrate_allowlisted_user_data(&source, &target).unwrap(), 0);
    }

    #[test]
    fn user_data_migration_rejects_invalid_shapes_and_oversized_files() {
        let root = TestRoot::new();
        let source = root.0.join("legacy-roaming");
        let target = root.0.join("new-local");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("settings.json"), b"[]").unwrap();
        assert!(migrate_allowlisted_user_data(&source, &target)
            .unwrap_err()
            .contains("unexpected JSON shape"));

        fs::write(source.join("settings.json"), b"{}").unwrap();
        let profiles = File::create(source.join("profiles.json")).unwrap();
        profiles.set_len(MAX_MIGRATED_PROFILES_BYTES + 1).unwrap();
        assert!(migrate_allowlisted_user_data(&source, &target)
            .unwrap_err()
            .contains("migration limit"));
    }

    fn test_journal(
        root: &TestRoot,
        phase: TransactionPhase,
        target_existed: bool,
    ) -> TransactionJournal {
        let transaction_id = "123-abc".to_string();
        TransactionJournal {
            schema_version: TRANSACTION_JOURNAL_SCHEMA,
            sequence: 3,
            transaction_id: transaction_id.clone(),
            phase,
            target: root.0.join("Obsession"),
            stage: root.0.join(format!(".obsession-stage-{transaction_id}")),
            backup: root.0.join(format!(".obsession-backup-{transaction_id}")),
            target_existed,
            old_registry: DurableRegistrySnapshot::from_runtime(&empty_registry_snapshot()),
        }
    }

    #[test]
    fn mode_planner_distinguishes_update_repair_and_blocked_downgrade() {
        assert_eq!(
            planned_mode(Some("1.0.0"), "1.1.0").unwrap(),
            InstallMode::Update
        );
        assert_eq!(
            planned_mode(Some("1.1.0"), "1.1.0").unwrap(),
            InstallMode::Repair
        );
        assert_eq!(
            planned_mode(Some("1.2.0"), "1.1.0").unwrap(),
            InstallMode::Blocked
        );
        assert_eq!(
            planned_mode(Some("legacy"), "1.1.0").unwrap(),
            InstallMode::Repair
        );
    }

    #[test]
    fn transaction_paths_are_siblings_and_unique() {
        let target = Path::new(r"C:\Users\tester\AppData\Local\Obsession");
        let (transaction_id, stage, backup) = transaction_paths(target).unwrap();
        assert_eq!(stage.parent(), target.parent());
        assert_eq!(backup.parent(), target.parent());
        assert_ne!(stage, backup);
        assert!(transaction_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert!(stage
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".obsession-stage-"));
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".obsession-backup-"));
    }

    #[test]
    fn two_slot_journal_falls_back_to_the_previous_valid_phase() {
        let root = TestRoot::new();
        let registry = empty_registry_snapshot();
        let target = root.0.join("Obsession");
        let mut journal = TransactionJournal::new(
            "123-abc".into(),
            target.clone(),
            root.0.join(".obsession-stage-123-abc"),
            root.0.join(".obsession-backup-123-abc"),
            true,
            &registry,
        );
        journal
            .write_phase_to(&root.0, TransactionPhase::Prepared)
            .unwrap();
        journal
            .write_phase_to(&root.0, TransactionPhase::Committing)
            .unwrap();

        let latest = transaction_journal_paths(&root.0)[journal.sequence as usize % 2].clone();
        fs::write(latest, b"{broken").unwrap();
        let recovered = load_transaction_journal_from(&root.0).unwrap().unwrap();
        assert_eq!(recovered.phase, TransactionPhase::Prepared);
        assert_eq!(recovered.sequence, 1);
    }

    #[test]
    fn failed_journal_write_does_not_advance_the_in_memory_phase() {
        let root = TestRoot::new();
        let blocked_state_dir = root.0.join("not-a-directory");
        fs::write(&blocked_state_dir, "blocked").unwrap();
        let registry = empty_registry_snapshot();
        let mut journal = TransactionJournal::new(
            "123-abc".into(),
            root.0.join("Obsession"),
            root.0.join(".obsession-stage-123-abc"),
            root.0.join(".obsession-backup-123-abc"),
            true,
            &registry,
        );

        assert!(journal
            .write_phase_to(&blocked_state_dir, TransactionPhase::Committing)
            .is_err());
        assert_eq!(journal.sequence, 0);
        assert_eq!(journal.phase, TransactionPhase::Prepared);
    }

    #[test]
    fn durable_registry_snapshot_roundtrips_raw_values() {
        let snapshot = RegistrySnapshot {
            keys: vec![
                RegistryKeySnapshot {
                    path: UNINSTALL_KEY,
                    existed: true,
                    values: vec![RawRegistryValue {
                        name: "DisplayName".into(),
                        value: RegValue {
                            bytes: vec![79, 0, 98, 0, 115, 0, 0, 0],
                            vtype: REG_SZ,
                        },
                    }],
                },
                RegistryKeySnapshot {
                    path: PRODUCT_KEY,
                    existed: true,
                    values: vec![RawRegistryValue {
                        name: String::new(),
                        value: RegValue {
                            bytes: vec![1, 2, 3, 4],
                            vtype: REG_BINARY,
                        },
                    }],
                },
            ],
        };
        let restored = DurableRegistrySnapshot::from_runtime(&snapshot)
            .into_runtime()
            .unwrap();
        assert_eq!(restored.keys.len(), 2);
        assert_eq!(
            restored.keys[0].values[0].value,
            snapshot.keys[0].values[0].value
        );
        assert_eq!(
            restored.keys[1].values[0].value,
            snapshot.keys[1].values[0].value
        );
    }

    #[test]
    fn recovery_cleans_prepared_stage_and_keeps_old_target() {
        let root = TestRoot::new();
        let journal = test_journal(&root, TransactionPhase::Prepared, true);
        mark_owned(&journal.target, "old");
        fs::create_dir_all(&journal.stage).unwrap();
        fs::write(journal.stage.join("partial.tmp"), "partial").unwrap();

        recover_transaction_files(&journal, &mut test_log(&root)).unwrap();
        assert_eq!(
            fs::read_to_string(journal.target.join("version.txt")).unwrap(),
            "old"
        );
        assert!(!journal.stage.exists());
    }

    #[test]
    fn recovery_restores_backup_from_the_middle_of_swap() {
        let root = TestRoot::new();
        let journal = test_journal(&root, TransactionPhase::Committing, true);
        mark_owned(&journal.backup, "old");
        mark_owned(&journal.stage, "new");

        recover_transaction_files(&journal, &mut test_log(&root)).unwrap();
        assert_eq!(
            fs::read_to_string(journal.target.join("version.txt")).unwrap(),
            "old"
        );
        assert!(!journal.stage.exists());
        assert!(!journal.backup.exists());
    }

    #[test]
    fn recovery_rolls_back_a_swapped_directory() {
        let root = TestRoot::new();
        let journal = test_journal(&root, TransactionPhase::Swapped, true);
        mark_owned(&journal.target, "new");
        mark_owned(&journal.backup, "old");

        recover_transaction_files(&journal, &mut test_log(&root)).unwrap();
        assert_eq!(
            fs::read_to_string(journal.target.join("version.txt")).unwrap(),
            "old"
        );
        assert!(!journal.stage.exists());
        assert!(!journal.backup.exists());
    }

    #[test]
    fn recovery_finalizes_a_committed_directory() {
        let root = TestRoot::new();
        let journal = test_journal(&root, TransactionPhase::Committed, true);
        mark_owned(&journal.target, "new");
        mark_owned(&journal.backup, "old");

        recover_transaction_files(&journal, &mut test_log(&root)).unwrap();
        assert_eq!(
            fs::read_to_string(journal.target.join("version.txt")).unwrap(),
            "new"
        );
        assert!(!journal.backup.exists());
    }

    #[test]
    fn registry_path_trimming_handles_tauri_quotes() {
        assert_eq!(
            trim_registry_path(r#""C:\Apps\Obsession""#.to_string()).unwrap(),
            PathBuf::from(r"C:\Apps\Obsession")
        );
        assert!(trim_registry_path("   ".to_string()).is_none());
    }

    #[test]
    fn owner_marker_name_matches_nsis_contract() {
        assert_eq!(super::super::OWNER_MARKER, ".obsession-install-owner");
    }

    /// Каждое действие, которое мы просим у хелпера, должно быть в его switch.
    /// Строки действий компилятор не проверяет, а хелпер встроен через
    /// include_bytes! — без этого теста опечатка или переименование в .ps1
    /// проявились бы только на живой установке, в рантайме.
    #[test]
    fn every_requested_safety_action_exists_in_the_embedded_helper() {
        let helper = std::str::from_utf8(SAFETY_HELPER).expect("helper must be valid UTF-8");
        for action in [
            "ValidateInstallDir",
            "StopOwnedApplication",
            "StopOwnedRuntime",
            "CleanupFirewall",
            "SyncShortcuts",
            "CleanupMachineUserShortcuts",
        ] {
            assert!(
                helper.contains(&format!("'{action}'")),
                "action {action} is not dispatched by installer-safety.ps1"
            );
        }
    }

    #[test]
    fn embedded_helper_never_self_elevates() {
        let helper = std::str::from_utf8(SAFETY_HELPER).expect("helper must be valid UTF-8");
        let helper = helper.to_ascii_lowercase();

        assert!(
            !helper.contains("-verb runas"),
            "installer-safety.ps1 must not elevate a temporary helper through UAC"
        );
        assert!(
            !helper.contains("start-process -filepath 'powershell.exe'"),
            "installer-safety.ps1 must not start an elevated PowerShell process"
        );
    }

    /// Хелпер читается Windows PowerShell 5.1 как ANSI (BOM у файла нет), поэтому
    /// любой не-ASCII литерал в нём ломает парсер ещё до выполнения.
    #[test]
    fn embedded_helper_is_ascii_only() {
        let offender = SAFETY_HELPER.iter().position(|byte| !byte.is_ascii());
        assert!(
            offender.is_none(),
            "installer-safety.ps1 must stay ASCII-only; first non-ASCII byte at offset {:?}",
            offender
        );
    }

    #[test]
    fn directory_swap_can_commit_and_restore_the_previous_version() {
        let root = TestRoot::new();
        let target = root.0.join("Obsession");
        let stage = root.0.join("stage");
        let backup = root.0.join("backup");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&stage).unwrap();
        fs::write(target.join("version.txt"), "old").unwrap();
        fs::write(stage.join("version.txt"), "new").unwrap();

        let old_moved = commit_directory_swap(&target, &stage, &backup).unwrap();
        assert!(old_moved);
        assert_eq!(
            fs::read_to_string(target.join("version.txt")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(backup.join("version.txt")).unwrap(),
            "old"
        );

        rollback_directory_swap(&target, &stage, &backup, old_moved).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("version.txt")).unwrap(),
            "old"
        );
        assert!(!stage.exists());
        assert!(!backup.exists());
    }

    #[test]
    fn fresh_directory_swap_rolls_back_to_no_target() {
        let root = TestRoot::new();
        let target = root.0.join("Obsession");
        let stage = root.0.join("stage");
        let backup = root.0.join("backup");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("version.txt"), "new").unwrap();

        let old_moved = commit_directory_swap(&target, &stage, &backup).unwrap();
        assert!(!old_moved);
        assert!(target.exists());
        rollback_directory_swap(&target, &stage, &backup, old_moved).unwrap();
        assert!(!target.exists());
        assert!(!stage.exists());
    }

    #[test]
    fn hung_child_is_terminated_after_bounded_timeout() {
        let root = TestRoot::new();
        let log_path = root.0.join("timeout.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .unwrap();
        let mut log = InstallerLog {
            path: log_path,
            file,
        };
        let mut child = Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 5",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();

        let error = wait_for_child_with_timeout(&mut child, &mut log, Duration::from_millis(50))
            .unwrap_err();
        assert!(error.contains("был остановлен"));
        assert!(child.try_wait().unwrap().is_some());
    }
}
