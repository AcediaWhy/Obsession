//! In-app discovery for the custom machine Setup. The release API is only a
//! directory of assets: authorization comes from an Ed25519-signed manifest.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::{Client, Url};
use ring::signature::{UnparsedPublicKey, ED25519};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::state::AppState;

const API_URL: &str = "https://api.github.com/repos/AcediaWhy/Obsession/releases/latest";
const PRODUCT_ID: &str = "com.vlarpsu.obsession";
const MAX_SETUP_BYTES: u64 = 512 * 1024 * 1024;
const MAX_METADATA_BYTES: usize = 16 * 1024;
static INSTALLING: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    body: Option<String>,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u8,
    product_id: String,
    version: String,
    setup_asset: String,
    size: u64,
    sha256: String,
}

#[derive(Clone, Serialize)]
pub struct UpdateCheck {
    current_version: String,
    latest_version: String,
    available: bool,
    notes: String,
    release_url: String,
}

#[derive(Clone, Serialize)]
struct UpdateProgress {
    downloaded: u64,
    total: u64,
}

struct InstallGuard;

impl InstallGuard {
    fn acquire() -> Result<Self, String> {
        INSTALLING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .map_err(|_| "Загрузка обновления уже идёт".to_string())?;
        Ok(Self)
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::Release);
    }
}

fn decode_hex(input: &str, expected_bytes: usize) -> Result<Vec<u8>, String> {
    let value = input.trim();
    if value.len() != expected_bytes * 2 || !value.is_ascii() {
        return Err("Неверный размер криптографической подписи или ключа".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).map_err(|e| e.to_string())?;
            u8::from_str_radix(digits, 16).map_err(|_| "Некорректный hex-код".to_string())
        })
        .collect()
}

fn verify_manifest(
    bytes: &[u8],
    signature_hex: &str,
    public_hex: &str,
) -> Result<Manifest, String> {
    let signature = decode_hex(signature_hex, 64)?;
    let public = decode_hex(public_hex, 32)?;
    UnparsedPublicKey::new(&ED25519, public)
        .verify(bytes, &signature)
        .map_err(|_| "Подпись обновления не прошла проверку".to_string())?;
    serde_json::from_slice(bytes).map_err(|_| "Некорректный манифест обновления".into())
}

fn asset_url(release: &Release, name: &str) -> Result<Url, String> {
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == name)
        .ok_or_else(|| format!("В релизе отсутствует {name}"))?;
    let url = Url::parse(&asset.browser_download_url)
        .map_err(|_| "Некорректная ссылка на файл релиза".to_string())?;
    let expected_path = format!(
        "/AcediaWhy/Obsession/releases/download/{}/{}",
        release.tag_name, name
    );
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || url.path() != expected_path
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Ссылка на файл релиза не принадлежит Obsession".into());
    }
    Ok(url)
}

async fn limited_bytes(client: &Client, url: Url, limit: usize) -> Result<Vec<u8>, String> {
    let mut response = tokio::time::timeout(Duration::from_secs(20), client.get(url).send())
        .await
        .map_err(|_| "Истекло время запроса обновления".to_string())?
        .map_err(|error| format!("Не удалось получить файл обновления: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Сервер обновлений ответил ошибкой: {error}"))?;
    let mut output = Vec::new();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(20), response.chunk())
        .await
        .map_err(|_| "Истекло время загрузки данных обновления".to_string())?
        .map_err(|error| format!("Не удалось прочитать ответ: {error}"))?
    {
        if output.len().saturating_add(chunk.len()) > limit {
            return Err("Данные обновления превышают допустимый размер".into());
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn validate_manifest(manifest: &Manifest, release_version: &Version) -> Result<(), String> {
    let version = Version::parse(&manifest.version)
        .map_err(|_| "Некорректная версия в манифесте".to_string())?;
    let expected_name = format!("Obsession-Setup_{}_x64.exe", manifest.version);
    if manifest.schema_version != 1
        || manifest.product_id != PRODUCT_ID
        || version != *release_version
        || manifest.setup_asset != expected_name
        || manifest.size == 0
        || manifest.size > MAX_SETUP_BYTES
        || decode_hex(&manifest.sha256, 32).is_err()
    {
        return Err("Подписанный манифест не соответствует релизу Obsession".into());
    }
    Ok(())
}

fn verify_and_launch(path: &Path, expected_size: u64, expected_sha256: &str) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Keep a read-only handle that refuses new writers and deleters until
        // CreateProcess has opened the verified image.
        options.share_mode(0x0000_0001); // FILE_SHARE_READ
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("Не удалось открыть сохранённый установщик: {error}"))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Ошибка повторного чтения установщика: {error}"))?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        if size > expected_size {
            return Err("Сохранённый установщик изменился перед запуском".into());
        }
        hasher.update(&buffer[..count]);
    }
    if size != expected_size || format!("{:x}", hasher.finalize()) != expected_sha256 {
        return Err("Сохранённый установщик изменился перед запуском".into());
    }
    std::process::Command::new(path)
        .spawn()
        .map_err(|error| format!("Не удалось запустить Obsession Setup: {error}"))?;
    drop(file);
    Ok(())
}

async fn discover() -> Result<(UpdateCheck, Option<(Manifest, Url)>), String> {
    let client = Client::builder()
        .user_agent(concat!("Obsession-Updater/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(5))
        .connect_timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| format!("Не удалось создать клиент обновлений: {error}"))?;
    let release_bytes = limited_bytes(&client, Url::parse(API_URL).unwrap(), 256 * 1024).await?;
    let release: Release = serde_json::from_slice(&release_bytes)
        .map_err(|_| "Не удалось прочитать последний релиз GitHub".to_string())?;
    let tag = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let latest = Version::parse(tag).map_err(|_| "Некорректная версия релиза".to_string())?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| "Некорректная версия приложения".to_string())?;
    let available = latest > current;
    let check = UpdateCheck {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        available,
        notes: release
            .body
            .as_deref()
            .unwrap_or_default()
            .chars()
            .take(2500)
            .collect(),
        release_url: format!(
            "https://github.com/AcediaWhy/Obsession/releases/tag/{}",
            release.tag_name
        ),
    };
    if !available {
        return Ok((check, None));
    }
    let manifest_bytes = limited_bytes(
        &client,
        asset_url(&release, "update.json")?,
        MAX_METADATA_BYTES,
    )
    .await?;
    let signature_bytes =
        limited_bytes(&client, asset_url(&release, "update.json.sig")?, 256).await?;
    let signature = std::str::from_utf8(&signature_bytes)
        .map_err(|_| "Некорректная кодировка подписи обновления".to_string())?;
    let manifest = verify_manifest(
        &manifest_bytes,
        signature,
        include_str!("update_public_key.hex"),
    )?;
    validate_manifest(&manifest, &latest)?;
    let url = asset_url(&release, &manifest.setup_asset)?;
    Ok((check, Some((manifest, url))))
}

#[tauri::command]
pub async fn update_check() -> Result<UpdateCheck, String> {
    discover().await.map(|(check, _)| check)
}

#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Err("Установка обновлений доступна только в релизной сборке".into());
    }
    let _guard = InstallGuard::acquire()?;
    // Never accept a URL, path, hash, or version from the WebView. Re-fetch the
    // latest signed release immediately before installation.
    let (check, candidate) = discover().await?;
    let (manifest, url) = candidate.ok_or("Новой версии больше нет")?;
    let client = Client::builder()
        .user_agent(concat!("Obsession-Updater/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(5))
        .connect_timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = tokio::time::timeout(Duration::from_secs(20), client.get(url).send())
        .await
        .map_err(|_| "Истекло время подключения к загрузке".to_string())?
        .map_err(|error| format!("Не удалось скачать установщик: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Сервер обновлений ответил ошибкой: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length != manifest.size)
    {
        return Err("Размер установщика не совпадает с подписанным манифестом".into());
    }
    let dir = state.paths.base_dir.join("updates");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| format!("Не удалось подготовить каталог обновлений: {error}"))?;
    let nonce = rand::random::<u64>();
    let partial = dir.join(format!(
        "Obsession-Setup_{}_{nonce:016x}.part",
        check.latest_version
    ));
    let executable = dir.join(format!(
        "Obsession-Setup_{}_{nonce:016x}.exe",
        check.latest_version
    ));
    let result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .await
            .map_err(|error| format!("Не удалось создать файл обновления: {error}"))?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0u64;
        let mut reported = 0u64;
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), response.chunk())
            .await
            .map_err(|_| "Загрузка установщика прервалась по таймауту".to_string())?
            .map_err(|error| format!("Ошибка загрузки установщика: {error}"))?
        {
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            if downloaded > manifest.size || downloaded > MAX_SETUP_BYTES {
                return Err("Размер установщика превышает подписанный размер".into());
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| format!("Ошибка записи установщика: {error}"))?;
            hasher.update(&chunk);
            if downloaded.saturating_sub(reported) >= 1024 * 1024 {
                let _ = app.emit(
                    "update-progress",
                    UpdateProgress {
                        downloaded,
                        total: manifest.size,
                    },
                );
                reported = downloaded;
            }
        }
        if downloaded != manifest.size || format!("{:x}", hasher.finalize()) != manifest.sha256 {
            return Err("Установщик не прошёл проверку SHA-256".into());
        }
        file.sync_all()
            .await
            .map_err(|error| format!("Не удалось сохранить установщик: {error}"))?;
        drop(file);
        tokio::fs::rename(&partial, &executable)
            .await
            .map_err(|error| format!("Не удалось подготовить установщик: {error}"))?;
        let _ = app.emit(
            "update-progress",
            UpdateProgress {
                downloaded,
                total: manifest.size,
            },
        );
        // Re-check while holding the file open through CreateProcess. Setup
        // handles UAC and the app/service transaction after this handoff.
        let sha256 = manifest.sha256.clone();
        let size = manifest.size;
        let launch_path = executable.clone();
        tokio::task::spawn_blocking(move || verify_and_launch(&launch_path, size, &sha256))
            .await
            .map_err(|error| format!("Не удалось проверить установщик: {error}"))??;
        Ok::<(), String>(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
        let _ = tokio::fs::remove_file(&executable).await;
    } else {
        app.exit(0);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    #[test]
    fn verifies_authentic_manifest_and_rejects_modification() {
        let key = Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap();
        let bytes = br#"{"schema_version":1,"product_id":"com.vlarpsu.obsession","version":"1.2.0","setup_asset":"Obsession-Setup_1.2.0_x64.exe","size":42,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        let signature = key.sign(bytes);
        let signature_hex = signature
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let public_hex = key
            .public_key()
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(verify_manifest(bytes, &signature_hex, &public_hex).is_ok());
        let mut modified = bytes.to_vec();
        let index = modified.len() - 3;
        modified[index] = b'b';
        assert!(verify_manifest(&modified, &signature_hex, &public_hex).is_err());
    }

    #[test]
    fn manifest_rejects_wrong_product_and_asset() {
        let mut manifest = Manifest {
            schema_version: 1,
            product_id: PRODUCT_ID.into(),
            version: "1.2.0".into(),
            setup_asset: "other.exe".into(),
            size: 42,
            sha256: "a".repeat(64),
        };
        let version = Version::parse("1.2.0").unwrap();
        assert!(validate_manifest(&manifest, &version).is_err());
        manifest.setup_asset = "Obsession-Setup_1.2.0_x64.exe".into();
        assert!(validate_manifest(&manifest, &version).is_ok());
        manifest.product_id = "another-product".into();
        assert!(validate_manifest(&manifest, &version).is_err());
    }

    #[test]
    fn asset_url_rejects_foreign_downloads() {
        let mut release = Release {
            tag_name: "v1.2.0".into(),
            body: None,
            assets: vec![ReleaseAsset {
                name: "update.json".into(),
                browser_download_url:
                    "https://github.com/AcediaWhy/Obsession/releases/download/v1.2.0/update.json"
                        .into(),
            }],
        };
        assert!(asset_url(&release, "update.json").is_ok());
        release.assets[0].browser_download_url = "https://example.org/update.json".into();
        assert!(asset_url(&release, "update.json").is_err());
    }
}
