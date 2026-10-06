//! Bounded local evidence archive. It never reconstructs application tickets.
use crate::util::LockExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

pub const MAX_RECORDS: usize = 40;
const MAX_BYTES: u64 = 4 * 1024 * 1024;
const FILE: &str = "zapret2-trials.json";
static IO: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Tournament,
    Scenario,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub recorded_at_unix_ms: u64,
    pub kind: Kind,
    pub subject: String,
    pub outcome: String,
    pub report: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Archive {
    schema_version: u8,
    records: Vec<Record>,
}

fn read(path: &Path) -> Result<Vec<Record>, String> {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Не удалось прочитать историю испытаний Zapret2.".into()),
    };
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err("История испытаний превышает допустимый размер.".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("История испытаний превышает допустимый размер.".into());
    }
    let archive: Archive = serde_json::from_slice(&bytes)
        .map_err(|_| "История испытаний повреждена; файл сохранён для диагностики.".to_owned())?;
    if archive.schema_version != 1
        || archive.records.len() > MAX_RECORDS
        || archive
            .records
            .iter()
            .any(|record| record.subject.len() > 256 || record.outcome.len() > 32)
    {
        return Err("Формат истории испытаний не поддерживается.".into());
    }
    Ok(archive.records)
}

pub fn load(base: &Path) -> Result<Vec<Record>, String> {
    let _lock = IO.lock_recover();
    read(&base.join(FILE))
}

pub fn append(
    base: &Path,
    kind: Kind,
    subject: String,
    outcome: String,
    report: serde_json::Value,
) -> Result<(), String> {
    use std::io::Write;
    let _lock = IO.lock_recover();
    let path = base.join(FILE);
    let mut records = read(&path)?;
    if subject.len() > 256
        || outcome.len() > 32
        || serde_json::to_vec(&report)
            .map_err(|e| e.to_string())?
            .len() as u64
            > MAX_BYTES / 2
    {
        return Err("Результат слишком велик для истории испытаний.".into());
    }
    records.push(Record {
        recorded_at_unix_ms: super::zapret2_scenario::unix_ms(),
        kind,
        subject,
        outcome,
        report,
    });
    if records.len() > MAX_RECORDS {
        records.drain(..records.len() - MAX_RECORDS);
    }
    let mut archive = Archive {
        schema_version: 1,
        records,
    };
    let bytes = loop {
        let bytes = serde_json::to_vec(&archive).map_err(|e| e.to_string())?;
        if bytes.len() as u64 <= MAX_BYTES {
            break bytes;
        }
        archive.records.remove(0);
    };
    std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
    let temporary: PathBuf = base.join(format!(
        "zapret2-trials-{:032x}.tmp",
        rand::random::<u128>()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|_| "Не удалось сохранить историю испытаний Zapret2.".into())
}

pub(super) async fn save(
    app: &tauri::AppHandle,
    kind: Kind,
    subject: String,
    outcome: &str,
    report: serde_json::Value,
) -> Option<String> {
    use tauri::Manager;
    let base = app.state::<crate::state::AppState>().paths.base_dir.clone();
    let outcome = outcome.to_owned();
    tauri::async_runtime::spawn_blocking(move || append(&base, kind, subject, outcome, report))
        .await
        .unwrap_or_else(|error| Err(error.to_string()))
        .err()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "obsession-history-test-{:032x}",
                rand::random::<u128>()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join(FILE));
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    #[test]
    fn retains_latest_evidence_without_transforming_user_reports_or_failure_outcomes() {
        let fixture = Fixture::new();
        for index in 0..MAX_RECORDS + 3 {
            append(&fixture.0, Kind::Scenario, format!("trial-{index}"), "failed".into(), serde_json::json!({"source":"user_report","actualTransport":"unverified","index":index})).unwrap();
        }
        let records = load(&fixture.0).unwrap();
        assert_eq!(records.len(), MAX_RECORDS);
        assert_eq!(records[0].report["index"], 3);
        assert!(records
            .iter()
            .all(|record| record.report["source"] == "user_report"
                && record.report["actualTransport"] == "unverified"
                && record.outcome == "failed"));
    }
    #[test]
    fn rejects_corrupt_unknown_and_oversized_archives_without_overwriting_them() {
        let fixture = Fixture::new();
        for bytes in [
            b"broken-json".to_vec(),
            br#"{"schemaVersion":99,"records":[]}"#.to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            std::fs::write(fixture.0.join(FILE), &bytes).unwrap();
            assert!(load(&fixture.0).is_err());
            assert!(append(
                &fixture.0,
                Kind::Tournament,
                "trial".into(),
                "passed".into(),
                serde_json::json!({})
            )
            .is_err());
            assert_eq!(std::fs::read(fixture.0.join(FILE)).unwrap(), bytes);
        }
    }
}
