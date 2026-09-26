//! Current-user cleanup only. The elevated worker never receives user paths.
use std::{fs, path::{Path, PathBuf}};
use std::os::windows::fs::MetadataExt;
use windows::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT};
use windows::Win32::System::Com::CoTaskMemFree;

const MAX_ENTRIES: usize = 100_000;
const REPARSE: u32 = 0x400;

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CleanupOptions {
    pub settings: bool,
    pub cache: bool,
    pub temporary: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct CleanupLocation {
    path: String,
    category: &'static str,
    exists: bool,
}

fn known_folder(id: &windows::core::GUID) -> Result<PathBuf, String> {
    // Shell resolves the interactive user's folder; no untrusted UI path or
    // elevated-user environment variable participates in deletion.
    unsafe {
        let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).map_err(|e| e.to_string())?;
        let text = raw.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(raw.0.cast()));
        let path = PathBuf::from(text?);
        if !path.is_absolute() { return Err("Windows returned a relative user folder".into()); }
        Ok(path)
    }
}

fn roots() -> Result<Vec<(PathBuf, &'static str)>, String> {
    let local = known_folder(&FOLDERID_LocalAppData)?;
    let roaming = known_folder(&FOLDERID_RoamingAppData)?;
    Ok(vec![
        (local.join("vlarpsu/Obsession"), "data"),
        (roaming.join("Obsession"), "data"),
        (local.join("Obsession"), "data"),
        (local.join("com.vlarpsu.obsession"), "cache"),
        (roaming.join("com.vlarpsu.obsession"), "cache"),
        (local.join("com.vlarpsu.obsession.setup"), "cache"),
        (roaming.join("com.vlarpsu.obsession.setup"), "cache"),
    ])
}

pub(crate) fn locations() -> Result<Vec<CleanupLocation>, String> {
    let mut result: Vec<_> = roots()?.into_iter().map(|(path, category)| CleanupLocation {
        exists: path.exists(), path: path.display().to_string(), category,
    }).collect();
    result.push(CleanupLocation { path: std::env::temp_dir().display().to_string(), category: "temporary", exists: true });
    Ok(result)
}

fn cache_entry(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "logs" | "cache" | "icons" | "netcache.json" |
        "netid_cache.json" | "cfproxy_cache.json" | "legacy-reliability-cache.json" |
        "installer.log" | "installer.log.1" | "onboarding.log")
}

fn owned_temp(name: &str) -> bool {
    // Exact TempArtifacts::write format; never an arbitrary obsession* glob.
    let Some(rest) = name.strip_prefix("obsession-installer-safety-").and_then(|s| s.strip_suffix(".ps1")) else { return false; };
    let parts: Vec<_> = rest.split('-').collect();
    parts.len() == 3 && parts[0].parse::<u32>().is_ok()
        && !parts[1].is_empty() && parts[1].bytes().all(|b| b.is_ascii_hexdigit())
        && parts[2].parse::<u32>().is_ok()
}

#[derive(Default)]
pub(crate) struct Plan { files: Vec<PathBuf>, directories: Vec<PathBuf> }

// Кэш окна удаляется только после остановки WebView; остальные цели остаются
// в заранее проверенном плане и очищаются до экрана завершения.
pub(crate) fn defer_setup_cache(mut plan: Plan, cache: bool) -> Result<(Plan, Vec<PathBuf>), String> {
    let deferred = if cache {
        roots()?.into_iter().map(|(path, _)| path)
            .filter(|path| path.file_name().is_some_and(|name| name == "com.vlarpsu.obsession.setup"))
            .collect::<Vec<_>>()
    } else { Vec::new() };
    plan.files.retain(|path| !deferred.iter().any(|root| path.starts_with(root)));
    plan.directories.retain(|path| !deferred.iter().any(|root| path.starts_with(root)));
    Ok((plan, deferred))
}

pub(crate) fn finish_setup_cache(roots: &[PathBuf]) -> Vec<String> {
    // WebView может дописать файлы при закрытии. После остановки окна строится
    // новый ограниченный план, с повторной проверкой junction и каждого пути.
    let mut errors = Vec::new();
    for root in roots {
        let mut plan = Plan::default();
        match collect(root, root, &mut plan) {
            Ok(()) => errors.extend(execute(plan)),
            Err(error) => errors.push(error),
        }
    }
    errors
}

fn inspect(path: &Path) -> Result<Option<fs::Metadata>, String> {
    // Recheck the whole ancestry, not just the final component.
    for parent in path.ancestors() {
        match fs::symlink_metadata(parent) {
            Ok(meta) if meta.file_attributes() & REPARSE != 0 => return Err(format!("Пропущена ссылка / junction: {}", parent.display())),
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => return Err(format!("{}: {e}", parent.display())),
        }
    }
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn collect(root: &Path, path: &Path, plan: &mut Plan) -> Result<(), String> {
    if !root.is_absolute() || !path.starts_with(root) || root.parent().is_none() {
        return Err("Unsafe cleanup scope".into());
    }
    let mut pending = vec![path.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Some(meta) = inspect(&next)? else { continue; };
        if plan.files.len() + plan.directories.len() >= MAX_ENTRIES { return Err("Слишком много файлов для безопасной очистки".into()); }
        if meta.is_dir() {
            plan.directories.push(next.clone());
            for entry in fs::read_dir(&next).map_err(|e| e.to_string())? {
                pending.push(entry.map_err(|e| e.to_string())?.path());
            }
        } else if meta.is_file() { plan.files.push(next); }
        else { return Err("Unsupported cleanup file type".into()); }
    }
    Ok(())
}

pub(crate) fn plan(options: CleanupOptions) -> Result<Plan, String> {
    let mut plan = Plan::default();
    for (root, category) in roots()? {
        select_root(&root, category, options, &mut plan)?;
    }
    if options.temporary {
        let temp = std::env::temp_dir();
        inspect(&temp)?;
        for entry in fs::read_dir(&temp).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if owned_temp(&entry.file_name().to_string_lossy()) && entry.file_type().map_err(|e| e.to_string())?.is_file() {
                collect(&temp, &entry.path(), &mut plan)?;
            }
        }
    }
    Ok(plan)
}

fn select_root(root: &Path, category: &str, options: CleanupOptions, plan: &mut Plan) -> Result<(), String> {
        if inspect(root)?.is_none() { return Ok(()); }
        if category == "cache" {
            if options.cache { collect(root, root, plan)?; }
        } else if options.settings && options.cache {
            collect(root, root, plan)?;
        } else {
            for entry in fs::read_dir(&root).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let cache = cache_entry(&entry.file_name().to_string_lossy());
                // Retired AppData executables are never personal settings.
                if entry.file_name().to_string_lossy().eq_ignore_ascii_case("bin") || (cache && options.cache) || (!cache && options.settings) {
                    collect(root, &entry.path(), plan)?;
                }
            }
        }
    Ok(())
}

pub(crate) fn execute(mut plan: Plan) -> Vec<String> {
    let mut errors = Vec::new();
    // No recursive delete: links are refused, files deleted individually, and
    // only empty directories removed. New/locked files remain in the report.
    for path in plan.files {
        let result = inspect(&path).and_then(|meta| {
            if meta.is_none() { return Ok(()); }
            fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
        });
        if let Err(error) = result { errors.push(error); }
    }
    plan.directories.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for path in plan.directories {
        let result = inspect(&path).and_then(|meta| {
            if meta.is_none() { return Ok(()); }
            fs::remove_dir(&path).map_err(|e| format!("{}: {e}", path.display()))
        });
        if let Err(error) = result { errors.push(error); }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deferred_cache_retries_after_file_handle_is_released() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::current_dir().unwrap().join("target")
            .join(format!("deferred-cache-{}", std::process::id()));
        let owned = root.join("setup-cache");
        fs::create_dir_all(&owned).unwrap();
        let file = owned.join("lockfile");
        fs::write(&file, b"cache").unwrap();
        fs::write(root.join("neighbor"), b"keep").unwrap();
        let lock = fs::OpenOptions::new().read(true).share_mode(1).open(&file).unwrap();
        assert!(!finish_setup_cache(&[owned.clone()]).is_empty());
        assert!(file.exists());
        drop(lock);
        // При завершении WebView могут появиться новые файлы в том же профиле.
        fs::write(owned.join("shutdown-data"), b"cache").unwrap();
        assert!(finish_setup_cache(&[owned.clone()]).is_empty());
        assert!(!owned.exists());
        assert!(root.join("neighbor").exists());
        fs::remove_file(root.join("neighbor")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn cache_opt_out_keeps_the_original_plan() {
        let mut plan = Plan::default();
        plan.files.push(PathBuf::from(r"C:\fixture\settings.json"));
        let (plan, deferred) = defer_setup_cache(plan, false).unwrap();
        assert!(deferred.is_empty());
        assert_eq!(plan.files, vec![PathBuf::from(r"C:\fixture\settings.json")]);
    }

    #[test]
    fn setup_cache_is_deferred_but_application_data_is_not() {
        let setup_roots: Vec<_> = roots().unwrap().into_iter().map(|(p, _)| p)
            .filter(|p| p.file_name().is_some_and(|n| n == "com.vlarpsu.obsession.setup"))
            .collect();
        let keep = PathBuf::from(r"C:\fixture\settings.json");
        let mut plan = Plan::default();
        plan.files.push(keep.clone());
        for root in &setup_roots {
            plan.files.push(root.join("EBWebView/lockfile"));
            plan.directories.push(root.clone());
        }
        let (plan, deferred) = defer_setup_cache(plan, true).unwrap();
        assert_eq!(deferred, setup_roots);
        assert_eq!(plan.files, vec![keep]);
        assert!(plan.directories.is_empty());
    }

    #[test]
    fn temp_matching_is_narrow() {
        assert!(owned_temp("obsession-installer-safety-123-abc123-0.ps1"));
        for name in ["obsession-notes.txt", "obsession-installer-safety-notes.ps1", "other-123-abc-0.ps1", "obsession-installer-safety-1-xyz-0.ps1"] { assert!(!owned_temp(name)); }
    }
    #[test]
    fn personal_settings_are_not_cache() {
        for name in ["settings.json", "profiles.json", "profiles", "autohosts", "hosts-backups", "adaptive-strategies.json"] { assert!(!cache_entry(name)); }
        assert!(cache_entry("logs")); assert!(cache_entry("netcache.json"));
    }
    #[test]
    fn cleanup_is_confined_to_fixture_and_preserves_neighbors() {
        let root = std::env::current_dir().unwrap().join("target").join(format!("cleanup-fixture-{}", std::process::id()));
        fs::create_dir_all(root.join("owned/sub")).unwrap();
        fs::write(root.join("owned/sub/test"), b"test").unwrap();
        fs::write(root.join("keep"), b"keep").unwrap();
        let mut p = Plan::default();
        collect(&root.join("owned"), &root.join("owned"), &mut p).unwrap();
        assert!(execute(p).is_empty());
        assert!(root.join("keep").is_file());
        fs::remove_file(root.join("keep")).unwrap(); fs::remove_dir(root).unwrap();
    }
    #[test]
    fn switches_select_only_the_requested_categories() {
        let root = std::env::current_dir().unwrap().join("target").join(format!("cleanup-switches-{}", std::process::id()));
        fs::create_dir_all(root.join("bin")).unwrap();
        for name in ["settings.json", "netcache.json", "bin/old.exe"] { fs::write(root.join(name), b"fixture").unwrap(); }
        for settings in [false, true] { for cache in [false, true] {
            let mut p = Plan::default();
            select_root(&root, "data", CleanupOptions {settings, cache, temporary:false}, &mut p).unwrap();
            assert_eq!(p.files.contains(&root.join("settings.json")), settings);
            assert_eq!(p.files.contains(&root.join("netcache.json")), cache);
            assert!(p.files.contains(&root.join("bin/old.exe")));
            let mut web = Plan::default();
            select_root(&root, "cache", CleanupOptions {settings, cache, temporary:false}, &mut web).unwrap();
            assert_eq!(!web.files.is_empty(), cache);
        }}
        let mut p = Plan::default(); collect(&root, &root, &mut p).unwrap(); assert!(execute(p).is_empty());
    }
    #[test]
    fn new_files_are_reported_not_recursively_deleted() {
        let root = std::env::current_dir().unwrap().join("target").join(format!("cleanup-new-files-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let mut p = Plan::default(); collect(&root, &root, &mut p).unwrap();
        fs::write(root.join("new"), b"keep").unwrap();
        assert!(!execute(p).is_empty());
        assert!(root.join("new").is_file());
        fs::remove_file(root.join("new")).unwrap(); fs::remove_dir(root).unwrap();
    }
    #[test]
    fn relative_or_escaped_targets_are_refused() {
        let root = std::env::current_dir().unwrap().join("target/owned");
        assert!(collect(Path::new("relative"), Path::new("relative/file"), &mut Plan::default()).is_err());
        assert!(collect(&root, &root.parent().unwrap().join("other"), &mut Plan::default()).is_err());
    }
}
