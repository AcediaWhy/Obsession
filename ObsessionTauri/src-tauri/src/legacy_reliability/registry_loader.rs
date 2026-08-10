//! Filesystem boundary for immutable Legacy target-registry snapshots.
//!
//! Every candidate remains an independent `.conf` record. This loader only
//! takes a bounded, validated snapshot of the configs and hostlists that are
//! eligible for the active categories; it never rewrites or combines them.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::paths::Paths;

use super::target_registry::{
    parse_legacy_config, ConfigParseError, HostlistKind, LegacyConfigRecord, RegistryBuildError,
    TargetRegistry,
};

const MAX_ACTIVE_CATEGORIES: usize = 32;
const MAX_CANDIDATES_PER_CATEGORY: usize = 256;
const MAX_HOSTLISTS_PER_CONFIG: usize = 64;
const MAX_CONFIG_BYTES: usize = 512 * 1024;
const MAX_HOSTLIST_BYTES: usize = 16 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug)]
pub enum RegistryLoadError {
    NoActiveSelections,
    TooManyActiveCategories {
        limit: usize,
    },
    InvalidCategory {
        category: String,
    },
    InvalidConfigName {
        category: String,
        config_name: String,
    },
    ConflictingSelection {
        category: String,
        first_config: String,
        second_config: String,
    },
    CategoryNotFound {
        category: String,
    },
    TooManyCandidates {
        category: String,
        limit: usize,
    },
    SelectedConfigNotFound {
        category: String,
        selected_config: String,
    },
    TooManyHostlists {
        category: String,
        config_name: String,
        limit: usize,
    },
    UnsafeResolvedPath {
        requested: PathBuf,
        resolved: PathBuf,
    },
    NotARegularFile {
        path: PathBuf,
    },
    FileTooLarge {
        path: PathBuf,
        limit: usize,
    },
    SnapshotTooLarge {
        limit: usize,
    },
    InvalidUtf8 {
        path: PathBuf,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    ParseConfig {
        category: String,
        config_name: String,
        source: ConfigParseError,
    },
    Build(RegistryBuildError),
}

impl fmt::Display for RegistryLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActiveSelections => write!(f, "no active Legacy selections"),
            Self::TooManyActiveCategories { limit } => {
                write!(f, "Legacy snapshot exceeds the limit of {limit} active categories")
            }
            Self::InvalidCategory { category } => {
                write!(f, "invalid Legacy category name {category:?}")
            }
            Self::InvalidConfigName {
                category,
                config_name,
            } => write!(
                f,
                "invalid Legacy config name {config_name:?} in category {category:?}"
            ),
            Self::ConflictingSelection {
                category,
                first_config,
                second_config,
            } => write!(
                f,
                "conflicting Legacy selections for {category:?}: {first_config:?} and {second_config:?}"
            ),
            Self::CategoryNotFound { category } => {
                write!(f, "Legacy category {category:?} does not exist")
            }
            Self::TooManyCandidates { category, limit } => write!(
                f,
                "Legacy category {category:?} exceeds the limit of {limit} config candidates"
            ),
            Self::SelectedConfigNotFound {
                category,
                selected_config,
            } => write!(
                f,
                "selected Legacy config {selected_config:?} is not a candidate of {category:?}"
            ),
            Self::TooManyHostlists {
                category,
                config_name,
                limit,
            } => write!(
                f,
                "Legacy config {category}/{config_name} exceeds the limit of {limit} hostlists"
            ),
            Self::UnsafeResolvedPath {
                requested,
                resolved,
            } => write!(
                f,
                "path {} resolves outside the Legacy resource root: {}",
                requested.display(),
                resolved.display()
            ),
            Self::NotARegularFile { path } => {
                write!(f, "Legacy snapshot path is not a regular file: {}", path.display())
            }
            Self::FileTooLarge { path, limit } => write!(
                f,
                "Legacy snapshot file {} exceeds the limit of {limit} bytes",
                path.display()
            ),
            Self::SnapshotTooLarge { limit } => {
                write!(f, "Legacy registry snapshot exceeds the limit of {limit} bytes")
            }
            Self::InvalidUtf8 { path } => {
                write!(f, "Legacy snapshot file is not UTF-8: {}", path.display())
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "failed to {operation} {}: {source}", path.display()),
            Self::ParseConfig {
                category,
                config_name,
                source,
            } => write!(f, "failed to parse Legacy config {category}/{config_name}: {source}"),
            Self::Build(source) => write!(f, "failed to build Legacy target registry: {source}"),
        }
    }
}

impl Error for RegistryLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::ParseConfig { source, .. } => Some(source),
            Self::Build(source) => Some(source),
            _ => None,
        }
    }
}

impl From<RegistryBuildError> for RegistryLoadError {
    fn from(source: RegistryBuildError) -> Self {
        Self::Build(source)
    }
}

/// Builds one immutable registry from all bundled candidates of active Legacy
/// categories. Identical duplicate selections are collapsed; conflicting ones
/// are rejected before any files are read.
pub fn load_target_registry(
    paths: &Paths,
    active_selections: &[(String, String)],
) -> Result<TargetRegistry, RegistryLoadError> {
    let selections = validated_selections(active_selections)?;
    let canonical_base = canonicalize_directory(&paths.base_dir, "canonicalize base directory")?;
    let requested_configs = paths.configs_dir();
    let canonical_configs =
        canonicalize_directory(&requested_configs, "canonicalize configs directory")?;
    require_descendant(&canonical_base, &requested_configs, &canonical_configs)?;

    let mut records = Vec::new();
    let mut snapshot_bytes = 0usize;
    for (category, selected_config) in &selections {
        let requested_category = requested_configs.join(category);
        let canonical_category = match fs::canonicalize(&requested_category) {
            Ok(path) => path,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Err(RegistryLoadError::CategoryNotFound {
                    category: category.clone(),
                });
            }
            Err(source) => {
                return Err(RegistryLoadError::Io {
                    operation: "canonicalize category directory",
                    path: requested_category,
                    source,
                });
            }
        };
        require_descendant(&canonical_configs, &requested_category, &canonical_category)?;
        if !metadata(&canonical_category)?.is_dir() {
            return Err(RegistryLoadError::CategoryNotFound {
                category: category.clone(),
            });
        }

        let candidates = candidate_files(category, &canonical_category)?;
        if !candidates.iter().any(|(name, _)| name == selected_config) {
            return Err(RegistryLoadError::SelectedConfigNotFound {
                category: category.clone(),
                selected_config: selected_config.clone(),
            });
        }

        for (config_name, requested_config) in candidates {
            let canonical_config = canonicalize_existing(&requested_config, "canonicalize config")?;
            require_descendant(&canonical_category, &requested_config, &canonical_config)?;
            require_regular_file(&canonical_config)?;
            let config_content = read_bounded(&canonical_config, MAX_CONFIG_BYTES)?;
            add_snapshot_bytes(&mut snapshot_bytes, config_content.len())?;

            let parsed = parse_legacy_config(&config_content).map_err(|source| {
                RegistryLoadError::ParseConfig {
                    category: category.clone(),
                    config_name: config_name.clone(),
                    source,
                }
            })?;
            if parsed.hostlists.len() > MAX_HOSTLISTS_PER_CONFIG {
                return Err(RegistryLoadError::TooManyHostlists {
                    category: category.clone(),
                    config_name,
                    limit: MAX_HOSTLISTS_PER_CONFIG,
                });
            }

            let mut record =
                LegacyConfigRecord::new(category.clone(), config_name.clone(), config_content);
            for hostlist in parsed.hostlists {
                // Auto-hostlists are mutable winws runtime state. Their safe
                // references are validated by the parser above, but their
                // filesystem state must not poison or expand this immutable
                // static snapshot.
                if hostlist.kind == HostlistKind::AutoInclude {
                    continue;
                }
                let requested_hostlist = paths.base_dir.join(&hostlist.reference);
                let canonical_hostlist = match fs::canonicalize(&requested_hostlist) {
                    Ok(path) => path,
                    Err(source) if source.kind() == io::ErrorKind::NotFound => {
                        // The registry builder owns required-vs-optional hostlist
                        // semantics and will reject Include/Exclude omissions.
                        continue;
                    }
                    Err(source) => {
                        return Err(RegistryLoadError::Io {
                            operation: "canonicalize hostlist",
                            path: requested_hostlist,
                            source,
                        });
                    }
                };
                require_descendant(&canonical_base, &requested_hostlist, &canonical_hostlist)?;
                require_regular_file(&canonical_hostlist)?;
                let content = read_bounded(&canonical_hostlist, MAX_HOSTLIST_BYTES)?;
                add_snapshot_bytes(&mut snapshot_bytes, content.len())?;
                record = record.with_hostlist(hostlist.reference, content);
            }
            records.push(record);
        }
    }

    TargetRegistry::from_records_with_active_selections(records, selections)
        .map_err(RegistryLoadError::Build)
}

fn validated_selections(
    active_selections: &[(String, String)],
) -> Result<BTreeMap<String, String>, RegistryLoadError> {
    if active_selections.is_empty() {
        return Err(RegistryLoadError::NoActiveSelections);
    }

    let mut selections = BTreeMap::new();
    for (category, config_name) in active_selections {
        if !is_safe_component(category) {
            return Err(RegistryLoadError::InvalidCategory {
                category: category.clone(),
            });
        }
        if !is_safe_component(config_name)
            || Path::new(config_name)
                .extension()
                .and_then(|value| value.to_str())
                != Some("conf")
        {
            return Err(RegistryLoadError::InvalidConfigName {
                category: category.clone(),
                config_name: config_name.clone(),
            });
        }

        if let Some(previous) = selections.insert(category.clone(), config_name.clone()) {
            if previous != *config_name {
                return Err(RegistryLoadError::ConflictingSelection {
                    category: category.clone(),
                    first_config: previous,
                    second_config: config_name.clone(),
                });
            }
        }
    }
    if selections.len() > MAX_ACTIVE_CATEGORIES {
        return Err(RegistryLoadError::TooManyActiveCategories {
            limit: MAX_ACTIVE_CATEGORIES,
        });
    }
    Ok(selections)
}

fn candidate_files(
    category: &str,
    category_dir: &Path,
) -> Result<Vec<(String, PathBuf)>, RegistryLoadError> {
    let entries = fs::read_dir(category_dir).map_err(|source| RegistryLoadError::Io {
        operation: "read category directory",
        path: category_dir.to_path_buf(),
        source,
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| RegistryLoadError::Io {
            operation: "read category entry",
            path: category_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("conf") {
            continue;
        }
        let Some(config_name) = entry.file_name().to_str().map(str::to_owned) else {
            return Err(RegistryLoadError::InvalidConfigName {
                category: category.to_string(),
                config_name: entry.file_name().to_string_lossy().into_owned(),
            });
        };
        if !is_safe_component(&config_name) {
            return Err(RegistryLoadError::InvalidConfigName {
                category: category.to_string(),
                config_name,
            });
        }
        candidates.push((config_name, path));
        if candidates.len() > MAX_CANDIDATES_PER_CATEGORY {
            return Err(RegistryLoadError::TooManyCandidates {
                category: category.to_string(),
                limit: MAX_CANDIDATES_PER_CATEGORY,
            });
        }
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(candidates)
}

fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.ends_with([' ', '.'])
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\' | ':'))
        && Path::new(value)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn canonicalize_directory(
    path: &Path,
    operation: &'static str,
) -> Result<PathBuf, RegistryLoadError> {
    let canonical = canonicalize_existing(path, operation)?;
    let metadata = metadata(&canonical)?;
    if !metadata.is_dir() {
        return Err(RegistryLoadError::Io {
            operation: "open directory",
            path: canonical,
            source: io::Error::new(io::ErrorKind::NotADirectory, "path is not a directory"),
        });
    }
    Ok(canonical)
}

fn canonicalize_existing(
    path: &Path,
    operation: &'static str,
) -> Result<PathBuf, RegistryLoadError> {
    fs::canonicalize(path).map_err(|source| RegistryLoadError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })
}

fn metadata(path: &Path) -> Result<fs::Metadata, RegistryLoadError> {
    fs::metadata(path).map_err(|source| RegistryLoadError::Io {
        operation: "read metadata for",
        path: path.to_path_buf(),
        source,
    })
}

fn require_descendant(
    root: &Path,
    requested: &Path,
    resolved: &Path,
) -> Result<(), RegistryLoadError> {
    if resolved != root && resolved.starts_with(root) {
        Ok(())
    } else {
        Err(RegistryLoadError::UnsafeResolvedPath {
            requested: requested.to_path_buf(),
            resolved: resolved.to_path_buf(),
        })
    }
}

fn require_regular_file(path: &Path) -> Result<(), RegistryLoadError> {
    if metadata(path)?.is_file() {
        Ok(())
    } else {
        Err(RegistryLoadError::NotARegularFile {
            path: path.to_path_buf(),
        })
    }
}

fn read_bounded(path: &Path, limit: usize) -> Result<String, RegistryLoadError> {
    let file_len = metadata(path)?.len();
    if file_len > limit as u64 {
        return Err(RegistryLoadError::FileTooLarge {
            path: path.to_path_buf(),
            limit,
        });
    }

    let mut file = File::open(path).map_err(|source| RegistryLoadError::Io {
        operation: "open",
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(file_len as usize);
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| RegistryLoadError::Io {
            operation: "read",
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > limit {
        return Err(RegistryLoadError::FileTooLarge {
            path: path.to_path_buf(),
            limit,
        });
    }
    String::from_utf8(bytes).map_err(|_| RegistryLoadError::InvalidUtf8 {
        path: path.to_path_buf(),
    })
}

fn add_snapshot_bytes(total: &mut usize, additional: usize) -> Result<(), RegistryLoadError> {
    *total = total
        .checked_add(additional)
        .filter(|value| *value <= MAX_SNAPSHOT_BYTES)
        .ok_or(RegistryLoadError::SnapshotTooLarge {
            limit: MAX_SNAPSHOT_BYTES,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::legacy_reliability::target_registry::{
        Attribution, HostlistKind, HostlistReferenceError, RegistryBuildError,
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(1);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let nonce = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "obsession-registry-loader-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn paths(&self) -> Paths {
            Paths {
                resource_dir: self.path.clone(),
                base_dir: self.path.clone(),
            }
        }

        fn write(&self, relative: &str, content: &str) {
            self.write_bytes(relative, content.as_bytes());
        }

        fn write_bytes(&self, relative: &str, content: &[u8]) {
            let path = self.path.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn create_file_with_len(&self, relative: &str, len: u64) {
            let path = self.path.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            File::create(path).unwrap().set_len(len).unwrap();
        }

        fn create_dir(&self, relative: &str) {
            fs::create_dir_all(self.path.join(relative)).unwrap();
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn selection(category: &str, config: &str) -> Vec<(String, String)> {
        vec![(category.to_string(), config.to_string())]
    }

    #[test]
    fn active_category_snapshot_contains_selected_and_eligible_candidates() {
        let dir = TestDir::new("candidate-union");
        dir.write("lists/selected.txt", "selected.example\n");
        dir.write("lists/candidate.txt", "candidate.example\n");
        dir.write(
            "configs/video/video_1.conf",
            "--wf-tcp=443 --hostlist=lists/selected.txt",
        );
        dir.write(
            "configs/video/video_2.conf",
            "--wf-tcp=80 --hostlist=lists/candidate.txt",
        );
        // An inactive category must not poison or expand this snapshot.
        dir.write(
            "configs/inactive/broken.conf",
            "--wf-tcp=6553 --hostlist=lists/missing.txt",
        );

        let registry =
            load_target_registry(&dir.paths(), &selection("video", "video_1.conf")).unwrap();

        assert_eq!(registry.active_config("video"), Some("video_1.conf"));
        assert!(registry.port_plan().contains(443));
        assert!(registry.port_plan().contains(80));
        let active_plan = registry.active_capture_plan().unwrap();
        assert!(active_plan.contains(443));
        assert!(!active_plan.contains(80));
        assert_eq!(
            registry.active_target_suffixes().collect::<Vec<_>>(),
            ["selected.example"]
        );
        match registry.attribute("www.candidate.example") {
            Attribution::Matched { owner, .. } => {
                assert_eq!(owner.category, "video");
                assert_eq!(owner.config_names, ["video_2.conf"]);
                assert_eq!(owner.active_config.as_deref(), Some("video_1.conf"));
            }
            other => panic!("expected candidate attribution, got {other:?}"),
        }
        assert_eq!(
            registry.attribute("inactive.example"),
            Attribution::Unmatched
        );
    }

    #[test]
    fn traversal_references_and_names_are_rejected() {
        let dir = TestDir::new("traversal");
        dir.write("outside.txt", "outside.example\n");
        dir.write(
            "configs/video/video.conf",
            "--wf-tcp=443 --hostlist=../outside.txt",
        );

        let error =
            load_target_registry(&dir.paths(), &selection("video", "video.conf")).unwrap_err();
        assert!(matches!(
            error,
            RegistryLoadError::ParseConfig {
                source: ConfigParseError::InvalidHostlistReference {
                    source: HostlistReferenceError::Traversal,
                    ..
                },
                ..
            }
        ));

        let error =
            load_target_registry(&dir.paths(), &selection("video", "../video.conf")).unwrap_err();
        assert!(matches!(error, RegistryLoadError::InvalidConfigName { .. }));
    }

    #[test]
    fn missing_required_hostlists_fail_but_missing_auto_hostlist_is_allowed() {
        let required = TestDir::new("missing-required");
        required.write(
            "configs/video/video.conf",
            "--wf-tcp=443 --hostlist=lists/missing.txt",
        );
        let error =
            load_target_registry(&required.paths(), &selection("video", "video.conf")).unwrap_err();
        assert!(matches!(
            error,
            RegistryLoadError::Build(RegistryBuildError::MissingHostlist {
                kind: HostlistKind::Include,
                ..
            })
        ));

        let excluded = TestDir::new("missing-exclude");
        excluded.write("lists/video.txt", "video.example\n");
        excluded.write(
            "configs/video/video.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt --hostlist-exclude=lists/missing.txt",
        );
        let error =
            load_target_registry(&excluded.paths(), &selection("video", "video.conf")).unwrap_err();
        assert!(matches!(
            error,
            RegistryLoadError::Build(RegistryBuildError::MissingHostlist {
                kind: HostlistKind::Exclude,
                ..
            })
        ));

        let automatic = TestDir::new("missing-auto");
        automatic.write("lists/video.txt", "video.example\n");
        automatic.write(
            "configs/video/video.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt --hostlist-auto=autohosts/video.txt",
        );
        let registry =
            load_target_registry(&automatic.paths(), &selection("video", "video.conf")).unwrap();
        assert_eq!(registry.target_count(), 1);
    }

    #[test]
    fn mutable_auto_hostlists_are_not_read_into_the_static_snapshot() {
        let dir = TestDir::new("mutable-auto");
        dir.write("lists/video.txt", "video.example\n");
        dir.create_file_with_len("autohosts/oversized.txt", MAX_HOSTLIST_BYTES as u64 + 1);
        dir.write_bytes("autohosts/invalid-utf8.txt", &[0xff, 0xfe, 0xfd]);
        dir.create_dir("autohosts/directory.txt");
        dir.write("autohosts/valid.txt", "learned.other.example\n");
        dir.write(
            "configs/video/video.conf",
            "--wf-tcp=443 \
             --hostlist=lists/video.txt \
             --hostlist-auto=autohosts/oversized.txt \
             --hostlist-auto=autohosts/invalid-utf8.txt \
             --hostlist-auto=autohosts/directory.txt \
             --hostlist-auto=autohosts/valid.txt",
        );

        let registry =
            load_target_registry(&dir.paths(), &selection("video", "video.conf")).unwrap();

        assert_eq!(registry.target_count(), 1);
        assert!(matches!(
            registry.attribute("www.video.example"),
            Attribution::Matched { .. }
        ));
        assert_eq!(
            registry.attribute("learned.other.example"),
            Attribution::Unmatched
        );
    }

    #[test]
    fn oversized_static_include_is_still_rejected() {
        let dir = TestDir::new("oversized-static");
        dir.create_file_with_len("lists/video.txt", MAX_HOSTLIST_BYTES as u64 + 1);
        dir.write(
            "configs/video/video.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt",
        );

        let error =
            load_target_registry(&dir.paths(), &selection("video", "video.conf")).unwrap_err();

        assert!(matches!(
            error,
            RegistryLoadError::FileTooLarge {
                limit: MAX_HOSTLIST_BYTES,
                ..
            }
        ));
    }

    #[test]
    fn invalid_utf8_static_include_is_still_rejected() {
        let dir = TestDir::new("invalid-static-utf8");
        dir.write_bytes("lists/video.txt", &[0xff, 0xfe, 0xfd]);
        dir.write(
            "configs/video/video.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt",
        );

        let error =
            load_target_registry(&dir.paths(), &selection("video", "video.conf")).unwrap_err();

        assert!(matches!(error, RegistryLoadError::InvalidUtf8 { .. }));
    }

    #[test]
    fn snapshot_version_is_deterministic_and_changes_with_candidate_content() {
        let dir = TestDir::new("version");
        dir.write("lists/video.txt", "video.example\n");
        dir.write(
            "configs/video/video_1.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt",
        );
        dir.write(
            "configs/video/video_2.conf",
            "--wf-tcp=80 --hostlist=lists/video.txt",
        );
        let selections = selection("video", "video_1.conf");

        let first = load_target_registry(&dir.paths(), &selections).unwrap();
        let repeated = load_target_registry(&dir.paths(), &selections).unwrap();
        assert_eq!(first.version(), repeated.version());
        assert_eq!(first.content_hash(), repeated.content_hash());

        dir.write(
            "configs/video/video_2.conf",
            "--wf-tcp=80  --hostlist=lists/video.txt",
        );
        let changed = load_target_registry(&dir.paths(), &selections).unwrap();
        assert_ne!(first.version(), changed.version());
        assert_ne!(first.content_hash(), changed.content_hash());
    }

    #[test]
    fn selection_only_change_updates_registry_version() {
        let dir = TestDir::new("selection-version");
        dir.write("lists/video.txt", "video.example\n");
        dir.write(
            "configs/video/video_1.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt",
        );
        dir.write(
            "configs/video/video_2.conf",
            "--wf-tcp=8443 --hostlist=lists/video.txt",
        );

        let first =
            load_target_registry(&dir.paths(), &selection("video", "video_1.conf")).unwrap();
        let second =
            load_target_registry(&dir.paths(), &selection("video", "video_2.conf")).unwrap();

        assert_eq!(first.content_hash(), second.content_hash());
        assert_ne!(first.version(), second.version());
        assert_eq!(first.active_config("video"), Some("video_1.conf"));
        assert_eq!(second.active_config("video"), Some("video_2.conf"));

        let first_plan = first.active_capture_plan().unwrap();
        assert!(first_plan.contains(443));
        assert!(!first_plan.contains(8443));
        let second_plan = second.active_capture_plan().unwrap();
        assert!(!second_plan.contains(443));
        assert!(second_plan.contains(8443));
    }

    #[test]
    fn duplicate_selection_is_idempotent_but_conflict_is_rejected() {
        let dir = TestDir::new("duplicate-selection");
        dir.write("lists/video.txt", "video.example\n");
        dir.write(
            "configs/video/video_1.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt",
        );
        dir.write(
            "configs/video/video_2.conf",
            "--wf-tcp=80 --hostlist=lists/video.txt",
        );

        let duplicate = vec![
            ("video".to_string(), "video_1.conf".to_string()),
            ("video".to_string(), "video_1.conf".to_string()),
        ];
        load_target_registry(&dir.paths(), &duplicate).unwrap();

        let conflict = vec![
            ("video".to_string(), "video_1.conf".to_string()),
            ("video".to_string(), "video_2.conf".to_string()),
        ];
        assert!(matches!(
            load_target_registry(&dir.paths(), &conflict),
            Err(RegistryLoadError::ConflictingSelection { .. })
        ));
    }

    #[test]
    fn bundled_candidates_form_a_usable_snapshot() {
        let resource_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let paths = Paths {
            base_dir: resource_dir.clone(),
            resource_dir,
        };
        let selections = [
            ("atrisk".to_string(), "atrisk_1.conf".to_string()),
            ("discord".to_string(), "discord_1.conf".to_string()),
            ("gaming".to_string(), "gaming_1.conf".to_string()),
            ("universal".to_string(), "universal_1.conf".to_string()),
            (
                "youtube_twitch".to_string(),
                "youtube_twitch_1.conf".to_string(),
            ),
        ];

        let registry = load_target_registry(&paths, &selections).unwrap();

        assert!(registry.is_usable());
        assert!(registry.port_plan().contains(80));
        assert!(registry.port_plan().contains(443));
    }
}
