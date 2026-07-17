//! Точные снапшоты системного `hosts` + персистентное состояние — WS1 задача 1.3.
//!
//! Отказ от эвристики «предпоследний файл по имени» ([[hosts.rs]] `restore_latest_backup`):
//! здесь ЯВНЫЙ указатель `last_known_good` на провайдера и БАЙТ-ТОЧНЫЕ снапшоты с
//! SHA-256. Все функции получают пути аргументом (F.3) — тестируются во временной
//! директории, системный путь не зашит.
//!
//! Хранилище (задаёт вызывающий): `%APPDATA%\Obsession\hosts-state.json` +
//! `%APPDATA%\Obsession\hosts-backups\`. Original (до-Obsession) хранится бессрочно;
//! на провайдера — не более `MAX_CONFIRMED` подтверждённых снапшотов + original.

#![allow(dead_code)] // подключается к транзакции apply/rollback в WS1 задача 1.5

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Версия схемы `hosts-state.json`.
pub const STATE_SCHEMA_VERSION: u32 = 1;

/// Максимум подтверждённых снапшотов на провайдера (плюс бессрочный original).
pub const MAX_CONFIRMED: usize = 5;

/// Ссылка на один снапшот: имя файла в backups-каталоге + байтовые метаданные.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotRef {
    pub operation_id: String,
    /// Имя файла в backups-каталоге (НЕ полный путь — переносимо между машинами).
    pub file: String,
    pub sha256: String,
    pub size: u64,
    /// Время захвата — передаётся снаружи (F.3, детерминизм тестов).
    pub captured_at: String,
    /// Провайдер снапшота; `None` — исходный до-Obsession `hosts`.
    pub provider: Option<String>,
}

/// Состояние одного AI-провайдера.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderState {
    /// Явный указатель «вернуть рабочую версию».
    pub last_known_good: Option<SnapshotRef>,
    /// Подтверждённые снапшоты, newest last, ≤ [`MAX_CONFIRMED`].
    pub confirmed: Vec<SnapshotRef>,
    /// SHA-256 последнего применённого Obsession файла — для детекта внешнего
    /// изменения `hosts` (WS1 задача 1.4).
    pub applied_sha256: Option<String>,
}

/// Персистентное состояние управления `hosts`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostsManagedState {
    pub schema_version: u32,
    /// Исходный (до-Obsession) снапшот — хранится БЕССРОЧНО.
    pub original: Option<SnapshotRef>,
    pub providers: BTreeMap<String, ProviderState>,
}

impl Default for HostsManagedState {
    fn default() -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            original: None,
            providers: BTreeMap::new(),
        }
    }
}

/// SHA-256 в hex (нижний регистр).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Загружает состояние. Отсутствующий файл → default. ПОВРЕЖДЁННЫЙ/несовместимый
/// JSON → default, но снапшоты (backups) НЕ трогаются: ошибка чтения state не
/// повод терять резервные копии.
pub fn load_state(state_path: &Path) -> HostsManagedState {
    let Ok(bytes) = std::fs::read(state_path) else {
        return HostsManagedState::default();
    };
    match serde_json::from_slice::<HostsManagedState>(&bytes) {
        Ok(s) if s.schema_version == STATE_SCHEMA_VERSION => s,
        _ => HostsManagedState::default(),
    }
}

/// Атомарно сохраняет состояние (temp + rename), создавая родительский каталог.
pub fn save_state(state_path: &Path, state: &HostsManagedState) -> io::Result<()> {
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(state)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let name = state_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("hosts-state.json");
    let tmp = state_path.with_file_name(format!("{name}.tmp"));
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, state_path)?;
    Ok(())
}

/// Пишет БАЙТ-ТОЧНЫЙ снапшот `hosts_snapshot_{operation_id}.txt` в backups-каталог
/// (атомарно), возвращает [`SnapshotRef`] с посчитанным sha256+size.
pub fn write_snapshot(
    backups_dir: &Path,
    operation_id: &str,
    provider: Option<&str>,
    captured_at: &str,
    bytes: &[u8],
) -> io::Result<SnapshotRef> {
    std::fs::create_dir_all(backups_dir)?;
    let file = format!("hosts_snapshot_{operation_id}.txt");
    let path = backups_dir.join(&file);
    let tmp = backups_dir.join(format!("{file}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(SnapshotRef {
        operation_id: operation_id.to_string(),
        file,
        sha256: sha256_hex(bytes),
        size: bytes.len() as u64,
        captured_at: captured_at.to_string(),
        provider: provider.map(str::to_string),
    })
}

/// Читает снапшот по ссылке и ПРОВЕРЯЕТ его sha256 (гарантия байт-точности).
pub fn read_snapshot(backups_dir: &Path, r: &SnapshotRef) -> io::Result<Vec<u8>> {
    let bytes = std::fs::read(backups_dir.join(&r.file))?;
    let actual = sha256_hex(&bytes);
    if actual != r.sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "снапшот {} повреждён: sha256 {actual} ≠ ожидаемого {}",
                r.file, r.sha256
            ),
        ));
    }
    Ok(bytes)
}

/// Устанавливает исходный снапшот, ЕСЛИ его ещё нет (до-Obsession `hosts` — бессрочно).
pub fn set_original_if_absent(state: &mut HostsManagedState, snap: SnapshotRef) {
    if state.original.is_none() {
        state.original = Some(snap);
    }
}

/// Фиксирует снапшот как новый last-known-good провайдера: обновляет указатель,
/// `applied_sha256`, добавляет в `confirmed` и обрезает до [`MAX_CONFIRMED`].
pub fn commit_last_known_good(
    state: &mut HostsManagedState,
    provider: &str,
    snap: SnapshotRef,
    backups_dir: &Path,
) {
    {
        let ps = state.providers.entry(provider.to_string()).or_default();
        ps.applied_sha256 = Some(snap.sha256.clone());
        ps.last_known_good = Some(snap.clone());
        ps.confirmed.push(snap);
    }
    prune_confirmed(state, provider, MAX_CONFIRMED, backups_dir);
}

/// Обрезает `confirmed` провайдера до `keep` (удаляя ФАЙЛЫ самых старых). НИКОГДА
/// не удаляет файл original и файл активного `last_known_good` — даже если тот
/// выпал из `confirmed`.
pub fn prune_confirmed(
    state: &mut HostsManagedState,
    provider: &str,
    keep: usize,
    backups_dir: &Path,
) {
    // Защищённые имена: original + активный LKG провайдера.
    let mut protected: BTreeSet<String> = BTreeSet::new();
    if let Some(o) = &state.original {
        protected.insert(o.file.clone());
    }
    if let Some(ps) = state.providers.get(provider) {
        if let Some(lkg) = &ps.last_known_good {
            protected.insert(lkg.file.clone());
        }
    }

    let Some(ps) = state.providers.get_mut(provider) else {
        return;
    };
    if ps.confirmed.len() <= keep {
        return;
    }
    let remove_count = ps.confirmed.len() - keep;
    let removed: Vec<SnapshotRef> = ps.confirmed.drain(0..remove_count).collect();
    for r in removed {
        if !protected.contains(&r.file) {
            let _ = std::fs::remove_file(backups_dir.join(&r.file));
        }
    }
}

// ─── Транзакция apply/rollback (WS1.4 + ядро 1.5) ──────────────────────────────

/// True, если системный `hosts` изменён ВНЕ Obsession с последнего apply (WS1.4).
/// `None` (Obsession ещё не применял) → не с чем сравнивать → не «внешнее».
pub fn is_externally_modified(current: &[u8], applied_sha256: Option<&str>) -> bool {
    match applied_sha256 {
        None => false,
        Some(prev) => sha256_hex(current) != prev,
    }
}

/// Атомарная запись файла: temp рядом + rename, с fallback на прямую запись
/// (System32/антивирус иногда ломают rename). Создаёт родительский каталог.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("hosts");
    let tmp = path.with_file_name(format!("{name}.obsession.tmp"));
    match std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            std::fs::write(path, bytes)
        }
    }
}

/// Результат фазы применения (до probes и commit LKG).
pub struct PreparedApply {
    /// Снимок содержимого `hosts` ДО записи — цель немедленного отката этой операции.
    pub pre_op: SnapshotRef,
    /// Снимок применённого содержимого — станет last-known-good после успешных probes.
    pub applied: SnapshotRef,
}

/// Ядро транзакции: снимает pre-op → атомарно пишет `prepared` → re-read+hash
/// verify. При несовпадении СРАЗУ откатывает к pre-op и возвращает Err. LKG НЕ
/// трогает — commit делает вызывающий после probes (`commit_last_known_good`).
pub fn snapshot_and_apply(
    hosts_path: &Path,
    backups_dir: &Path,
    provider: &str,
    prepared: &[u8],
    operation_id: &str,
    captured_at: &str,
) -> io::Result<PreparedApply> {
    let current = std::fs::read(hosts_path).unwrap_or_default();
    let pre_op = write_snapshot(
        backups_dir,
        &format!("{operation_id}_preop"),
        Some(provider),
        captured_at,
        &current,
    )?;

    write_atomic(hosts_path, prepared)?;

    let readback = std::fs::read(hosts_path)?;
    if sha256_hex(&readback) != sha256_hex(prepared) {
        // Немедленный откат к pre-op: то, что записали, не читается обратно.
        write_atomic(hosts_path, &current)?;
        return Err(io::Error::other(
            "записанный hosts не совпал с подготовленным — выполнен откат к pre-op",
        ));
    }

    let applied = write_snapshot(
        backups_dir,
        operation_id,
        Some(provider),
        captured_at,
        prepared,
    )?;
    Ok(PreparedApply { pre_op, applied })
}

/// Восстанавливает `hosts` из снапшота (с проверкой байт-точности снапшота).
pub fn restore_snapshot(hosts_path: &Path, backups_dir: &Path, r: &SnapshotRef) -> io::Result<()> {
    let bytes = read_snapshot(backups_dir, r)?;
    write_atomic(hosts_path, &bytes)
}

/// Удаляет файл снапшота (транзиентный pre-op после успешного commit).
pub fn remove_snapshot(backups_dir: &Path, r: &SnapshotRef) {
    let _ = std::fs::remove_file(backups_dir.join(&r.file));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Свежая уникальная временная директория под конкретный тест (без rand/time).
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("obsession_hosts_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sref(op: &str, sha: &str, provider: Option<&str>) -> SnapshotRef {
        SnapshotRef {
            operation_id: op.into(),
            file: format!("hosts_snapshot_{op}.txt"),
            sha256: sha.into(),
            size: 3,
            captured_at: "t".into(),
            provider: provider.map(str::to_string),
        }
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn metadata_roundtrip() {
        let dir = temp_dir("roundtrip");
        let state_path = dir.join("hosts-state.json");
        let mut state = HostsManagedState {
            original: Some(sref("orig", "aa", None)),
            ..Default::default()
        };
        let ps = state.providers.entry("malw".into()).or_default();
        ps.last_known_good = Some(sref("op1", "bb", Some("malw")));
        ps.applied_sha256 = Some("bb".into());
        ps.confirmed.push(sref("op1", "bb", Some("malw")));

        save_state(&state_path, &state).unwrap();
        let loaded = load_state(&state_path);
        assert_eq!(loaded, state);
    }

    #[test]
    fn snapshot_exact_byte_restore_even_for_binary() {
        let dir = temp_dir("restore");
        // Снапшот — СЫРЫЕ байты (не валидированный payload): любые байты, вкл. NUL/не-UTF8.
        let bytes: &[u8] = &[0x00, 0x01, 0xff, b'a', b'\n'];
        let r = write_snapshot(&dir, "opX", Some("malw"), "t", bytes).unwrap();
        assert_eq!(r.size, 5);
        let restored = read_snapshot(&dir, &r).unwrap();
        assert_eq!(restored, bytes);
    }

    #[test]
    fn read_snapshot_detects_corruption() {
        let dir = temp_dir("corrupt_snap");
        let r = write_snapshot(&dir, "opC", None, "t", b"original").unwrap();
        std::fs::write(dir.join(&r.file), b"tampered").unwrap();
        assert!(read_snapshot(&dir, &r).is_err());
    }

    #[test]
    fn cleanup_keeps_original_and_active_lkg() {
        let dir = temp_dir("cleanup");
        let mut state = HostsManagedState::default();
        let orig = write_snapshot(&dir, "orig", None, "t", b"orig").unwrap();
        set_original_if_absent(&mut state, orig.clone());

        // 7 подтверждений — commit каждый раз обрезает до MAX_CONFIRMED (5).
        for i in 0..7 {
            let snap = write_snapshot(
                &dir,
                &format!("c{i}"),
                Some("malw"),
                "t",
                format!("v{i}").as_bytes(),
            )
            .unwrap();
            commit_last_known_good(&mut state, "malw", snap, &dir);
        }

        let ps = state.providers.get("malw").unwrap();
        assert!(ps.confirmed.len() <= MAX_CONFIRMED);
        // LKG — последний применённый.
        assert_eq!(ps.last_known_good.as_ref().unwrap().operation_id, "c6");
        // original и активный LKG на диске сохранены.
        assert!(
            dir.join(&orig.file).exists(),
            "original не должен удаляться"
        );
        assert!(dir
            .join(&ps.last_known_good.as_ref().unwrap().file)
            .exists());
        // Старейшие (c0, c1) — вычищены.
        assert!(!dir.join("hosts_snapshot_c0.txt").exists());
        assert!(!dir.join("hosts_snapshot_c1.txt").exists());
    }

    #[test]
    fn corrupt_state_does_not_delete_backups() {
        let dir = temp_dir("corrupt_state");
        let state_path = dir.join("hosts-state.json");
        std::fs::write(&state_path, b"{ not valid json ]").unwrap();
        let backup = dir.join("hosts_snapshot_precious.txt");
        std::fs::write(&backup, b"precious").unwrap();

        let loaded = load_state(&state_path);
        assert_eq!(loaded.schema_version, STATE_SCHEMA_VERSION);
        assert!(loaded.original.is_none());
        assert!(backup.exists(), "load_state НЕ должен удалять backups");
    }

    #[test]
    fn is_externally_modified_detects_change() {
        assert!(
            !is_externally_modified(b"abc", None),
            "без applied_sha256 сравнивать не с чем"
        );
        let sha = sha256_hex(b"abc");
        assert!(!is_externally_modified(b"abc", Some(&sha)));
        assert!(is_externally_modified(b"abcX", Some(&sha)));
    }

    #[test]
    fn snapshot_and_apply_writes_and_snapshots_both_sides() {
        let dir = temp_dir("apply");
        let hosts = dir.join("hosts");
        let backups = dir.join("backups");
        std::fs::write(&hosts, b"old content").unwrap();

        let pa = snapshot_and_apply(&hosts, &backups, "malw", b"new content", "op1", "t").unwrap();
        assert_eq!(std::fs::read(&hosts).unwrap(), b"new content");
        assert_eq!(read_snapshot(&backups, &pa.pre_op).unwrap(), b"old content");
        assert_eq!(
            read_snapshot(&backups, &pa.applied).unwrap(),
            b"new content"
        );
    }

    #[test]
    fn apply_on_missing_hosts_captures_empty_preop() {
        let dir = temp_dir("apply_missing");
        let hosts = dir.join("hosts"); // не существует
        let backups = dir.join("backups");

        let pa = snapshot_and_apply(&hosts, &backups, "malw", b"fresh", "op1", "t").unwrap();
        assert_eq!(std::fs::read(&hosts).unwrap(), b"fresh");
        assert_eq!(read_snapshot(&backups, &pa.pre_op).unwrap(), b"");
    }

    #[test]
    fn restore_snapshot_brings_back_exact_bytes() {
        let dir = temp_dir("restore_txn");
        let hosts = dir.join("hosts");
        let backups = dir.join("backups");
        std::fs::write(&hosts, b"good").unwrap();

        let pa = snapshot_and_apply(&hosts, &backups, "malw", b"bad", "op1", "t").unwrap();
        assert_eq!(std::fs::read(&hosts).unwrap(), b"bad");

        restore_snapshot(&hosts, &backups, &pa.pre_op).unwrap();
        assert_eq!(std::fs::read(&hosts).unwrap(), b"good");
    }
}
