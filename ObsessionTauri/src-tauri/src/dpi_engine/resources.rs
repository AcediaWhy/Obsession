//! Проверка целостности бинарных ресурсов движков по `resources/manifest.json`.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct ResourceManifest {
    schema_version: u32,
    engines: BTreeMap<String, EngineResources>,
}

#[derive(Debug, Deserialize)]
struct EngineResources {
    files: Vec<ResourceFile>,
}

#[derive(Debug, Deserialize)]
struct ResourceFile {
    path: String,
    role: String,
    size: u64,
    sha256: String,
}

fn safe_relative(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn validate_engine_resources(base_dir: &Path, engine: &str) -> Result<(), String> {
    let manifest_path = base_dir.join("manifest.json");
    let bytes = std::fs::read(&manifest_path)
        .map_err(|e| format!("не удалось прочитать {}: {e}", manifest_path.display()))?;
    let manifest: ResourceManifest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("не удалось разобрать {}: {e}", manifest_path.display()))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "несовместимая версия resource manifest: {}",
            manifest.schema_version
        ));
    }
    let group = manifest
        .engines
        .get(engine)
        .ok_or_else(|| format!("движок {engine} отсутствует в resource manifest"))?;
    if group.files.is_empty() {
        return Err(format!(
            "resource manifest не содержит файлов движка {engine}"
        ));
    }

    for file in &group.files {
        if !safe_relative(&file.path) {
            return Err(format!(
                "небезопасный путь {} ресурса {}",
                file.path, file.role
            ));
        }
        let path = base_dir.join(&file.path);
        let metadata = std::fs::metadata(&path)
            .map_err(|e| format!("ресурс {} отсутствует: {e}", path.display()))?;
        if metadata.len() != file.size {
            return Err(format!(
                "размер {} не совпал: {} != {}",
                path.display(),
                metadata.len(),
                file.size
            ));
        }
        let actual = sha256_hex(
            &std::fs::read(&path)
                .map_err(|e| format!("не удалось прочитать {}: {e}", path.display()))?,
        );
        if !actual.eq_ignore_ascii_case(&file.sha256) {
            return Err(format!(
                "SHA-256 {} не совпал: {actual} != {}",
                path.display(),
                file.sha256
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("obsession-resource-test-{nonce}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_manifest(base: &Path, path: &str, bytes: &[u8], size: u64, sha: &str) {
        let manifest = serde_json::json!({
            "schema_version": 1,
            "engines": {
                "zapret2": {
                    "files": [{
                        "path": path,
                        "role": "engine",
                        "size": size,
                        "sha256": sha
                    }]
                }
            }
        });
        std::fs::write(
            base.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        if safe_relative(path) {
            let target = base.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(target, bytes).unwrap();
        }
    }

    #[test]
    fn accepts_valid_engine_resources() {
        let dir = temp_dir();
        let bytes = b"winws2";
        write_manifest(
            &dir,
            "bin/zapret2/winws2.exe",
            bytes,
            bytes.len() as u64,
            &sha256_hex(bytes),
        );
        assert!(validate_engine_resources(&dir, "zapret2").is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_missing_size_hash_path_and_engine() {
        let dir = temp_dir();
        let bytes = b"winws2";
        write_manifest(&dir, "bin/winws2.exe", bytes, 999, &sha256_hex(bytes));
        assert!(validate_engine_resources(&dir, "zapret2")
            .unwrap_err()
            .contains("размер"));

        write_manifest(&dir, "bin/winws2.exe", bytes, bytes.len() as u64, "00");
        assert!(validate_engine_resources(&dir, "zapret2")
            .unwrap_err()
            .contains("SHA-256"));

        write_manifest(&dir, "../winws2.exe", bytes, bytes.len() as u64, "00");
        assert!(validate_engine_resources(&dir, "zapret2")
            .unwrap_err()
            .contains("небезопасный путь"));
        assert!(validate_engine_resources(&dir, "legacy")
            .unwrap_err()
            .contains("отсутствует"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn validates_real_bundled_legacy_and_zapret2_resources() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        validate_engine_resources(&base, "zapret1").expect("Legacy resources must be valid");
        validate_engine_resources(&base, "zapret2").expect("Zapret2 resources must be valid");
    }
}
