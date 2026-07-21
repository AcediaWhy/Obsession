//! Isolation boundary for mutable Legacy `--hostlist-auto` files.
//!
//! Static hostlists remain the only semantic ownership source. This module
//! provides three deliberately separate layers:
//!
//! - a pure migration planner for already captured auto-hostlist snapshots;
//! - a bounded transactional filesystem executor with recoverable backups;
//! - an effective-config generator that injects foreign static exclusions
//!   without ever rewriting the selected source `.conf`.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::target_registry::{
    normalize_domain, parse_legacy_config, ConfigParseError, HostlistKind,
};

const MAX_SELECTIONS: usize = 32;
const MAX_CONFIGS_PER_CATEGORY: usize = 256;
const MAX_AUTO_FILES: usize = 128;
const MAX_HOSTLISTS_PER_CONFIG: usize = 64;
const MAX_CONFIG_BYTES: usize = 512 * 1024;
const MAX_HOSTLIST_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;
const MAX_HOSTS_PER_FILE: usize = 250_000;
const MAX_TOTAL_HOSTS: usize = 1_000_000;
const MAX_CONFIG_TOKENS: usize = 65_536;
const MAX_MIGRATION_BACKUPS: usize = 3;
const TEMP_CREATE_ATTEMPTS: usize = 32;
const REPLACE_ATTEMPTS: usize = 50;
const BACKUP_SCHEMA_VERSION: u32 = 2;
const BACKUP_PREFIX: &str = "migration-v2-";
const BACKUP_COMMITTED_MARKER: &str = "COMMITTED";
const BACKUP_COMMITTED_CONTENT: &[u8] = b"committed\n";

static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// One selected source config. The source path and bytes are observation-only;
/// generated isolation files are always written beneath `runtime_root`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectedLegacyConfig {
    pub category: String,
    pub config_name: String,
    pub source_path: PathBuf,
    pub source_content: String,
    /// The single category-wide auto store discovered across all candidate
    /// configs. Repeated references to the same path collapse to one entry.
    pub auto_host_reference: Option<PathBuf>,
}

/// Detailed preparation input used by tests and by the disk convenience
/// wrapper. `static_ownership` must contain only static `--hostlist` domains.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IsolationPrepareRequest<'a> {
    pub base_dir: &'a Path,
    pub backup_root: &'a Path,
    pub runtime_root: &'a Path,
    pub static_ownership: &'a BTreeMap<String, BTreeSet<String>>,
    pub selections: &'a [SelectedLegacyConfig],
    pub migrate_existing: bool,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IsolationPrepareResult {
    /// Exact launch path keyed by the original `(category, config_name)`.
    /// Entries point either to the immutable source or to a generated overlay.
    pub effective_paths: BTreeMap<(String, String), PathBuf>,
    pub migration: MigrationSummary,
}

/// Privacy-safe aggregate suitable for application logs and UI diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationSummary {
    pub files_changed: usize,
    pub hosts_moved: usize,
    pub ambiguous_hosts_quarantined: usize,
    pub unknown_hosts_retained: usize,
    pub rollback_performed: bool,
}

impl fmt::Display for MigrationSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "files_changed={}, hosts_moved={}, ambiguous_hosts_quarantined={}, unknown_hosts_retained={}, rollback_performed={}",
            self.files_changed,
            self.hosts_moved,
            self.ambiguous_hosts_quarantined,
            self.unknown_hosts_retained,
            self.rollback_performed
        )
    }
}

/// Byte-exact snapshot captured before calling the pure planner. `relative_path`
/// is relative to the Obsession data directory and is validated again by the
/// executor before every filesystem operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutoHostSnapshot {
    pub category: String,
    pub relative_path: PathBuf,
    pub original: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MigrationFileChange {
    category: String,
    relative_path: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MigrationPlan {
    changes: Vec<MigrationFileChange>,
    summary: MigrationSummary,
}

impl MigrationPlan {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn summary(&self) -> MigrationSummary {
        self.summary
    }
}

#[derive(Debug)]
pub(crate) enum MigrationPlanError {
    TooManyFiles { limit: usize },
    SnapshotTooLarge { limit: usize },
    TooManyHosts { category: String, limit: usize },
    HostlistTooLarge { category: String, limit: usize },
    TooManyTotalHosts { limit: usize },
    InvalidCategory { category: String },
    InvalidRelativePath,
    DuplicatePath,
    InvalidUtf8 { category: String },
    InvalidDomain { category: String, line: usize },
    InvalidStaticDomain { category: String },
    MultipleDestinationAutoHostlists { category: String },
}

impl fmt::Display for MigrationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyFiles { limit } => {
                write!(formatter, "auto-hostlist snapshot exceeds {limit} files")
            }
            Self::SnapshotTooLarge { limit } => {
                write!(formatter, "auto-hostlist snapshot exceeds {limit} bytes")
            }
            Self::TooManyHosts { category, limit } => write!(
                formatter,
                "auto-hostlist for category {category:?} exceeds {limit} hosts"
            ),
            Self::HostlistTooLarge { category, limit } => write!(
                formatter,
                "auto-hostlist for category {category:?} exceeds {limit} bytes"
            ),
            Self::TooManyTotalHosts { limit } => {
                write!(
                    formatter,
                    "auto-hostlist snapshot exceeds {limit} total hosts"
                )
            }
            Self::InvalidCategory { category } => {
                write!(formatter, "invalid Legacy category component {category:?}")
            }
            Self::InvalidRelativePath => {
                write!(
                    formatter,
                    "auto-hostlist path is not a confined relative path"
                )
            }
            Self::DuplicatePath => {
                write!(formatter, "the same auto-hostlist path has multiple owners")
            }
            Self::InvalidUtf8 { category } => write!(
                formatter,
                "auto-hostlist for category {category:?} is not valid UTF-8"
            ),
            Self::InvalidDomain { category, line } => write!(
                formatter,
                "auto-hostlist for category {category:?} has an invalid domain on line {line}"
            ),
            Self::InvalidStaticDomain { category } => write!(
                formatter,
                "static ownership for category {category:?} contains an invalid domain"
            ),
            Self::MultipleDestinationAutoHostlists { category } => write!(
                formatter,
                "category {category:?} has multiple distinct auto-hostlist destinations"
            ),
        }
    }
}

impl Error for MigrationPlanError {}

#[derive(Debug)]
pub(crate) struct MigrationExecutionError {
    operation: &'static str,
    path: PathBuf,
    source: io::Error,
    summary: MigrationSummary,
    rollback_failed: bool,
}

impl MigrationExecutionError {
    pub fn summary(&self) -> MigrationSummary {
        self.summary
    }

    pub fn rollback_failed(&self) -> bool {
        self.rollback_failed
    }
}

impl fmt::Display for MigrationExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} failed for {}: {} ({}; rollback_failed={})",
            self.operation,
            self.path.display(),
            self.source,
            self.summary,
            self.rollback_failed
        )
    }
}

impl Error for MigrationExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Debug)]
pub(crate) enum OverlayError {
    Config(ConfigParseError),
    TooLarge { limit: usize },
    TooManyTokens { limit: usize },
    InvalidExclusionReference,
    MissingOptionValue,
}

impl fmt::Display for OverlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(source) => write!(formatter, "invalid Legacy config: {source}"),
            Self::TooLarge { limit } => {
                write!(formatter, "effective Legacy config exceeds {limit} bytes")
            }
            Self::TooManyTokens { limit } => {
                write!(formatter, "effective Legacy config exceeds {limit} tokens")
            }
            Self::InvalidExclusionReference => {
                write!(
                    formatter,
                    "generated exclusion reference is not a safe relative path"
                )
            }
            Self::MissingOptionValue => {
                write!(formatter, "Legacy hostlist option is missing its value")
            }
        }
    }
}

impl Error for OverlayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config(source) => Some(source),
            _ => None,
        }
    }
}

impl From<ConfigParseError> for OverlayError {
    fn from(source: ConfigParseError) -> Self {
        Self::Config(source)
    }
}

#[derive(Debug)]
pub(crate) enum IsolationError {
    NoSelections,
    TooManySelections {
        limit: usize,
    },
    InvalidSelection {
        category: String,
        config_name: String,
    },
    TargetNotSelected {
        category: String,
        config_name: String,
    },
    ConflictingSelection {
        category: String,
    },
    TooManyConfigs {
        category: String,
        limit: usize,
    },
    TooManyHostlists {
        category: String,
        config_name: String,
        limit: usize,
    },
    InvalidStaticDomain {
        category: String,
    },
    Config {
        category: String,
        config_name: String,
        source: ConfigParseError,
    },
    Hostlist {
        category: String,
        line: usize,
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
    UnsafePath {
        operation: &'static str,
        path: PathBuf,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Plan(MigrationPlanError),
    Migration(MigrationExecutionError),
    Overlay {
        category: String,
        config_name: String,
        source: OverlayError,
    },
}

impl fmt::Display for IsolationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSelections => {
                write!(formatter, "Legacy isolation requires an active selection")
            }
            Self::TooManySelections { limit } => {
                write!(
                    formatter,
                    "Legacy isolation exceeds {limit} active selections"
                )
            }
            Self::InvalidSelection {
                category,
                config_name,
            } => write!(
                formatter,
                "invalid Legacy selection {category:?}/{config_name:?}"
            ),
            Self::TargetNotSelected {
                category,
                config_name,
            } => write!(
                formatter,
                "scoped Legacy isolation target {category:?}/{config_name:?} is not in the tentative selection"
            ),
            Self::ConflictingSelection { category } => write!(
                formatter,
                "Legacy category {category:?} has conflicting selected configs"
            ),
            Self::TooManyConfigs { category, limit } => write!(
                formatter,
                "Legacy category {category:?} exceeds {limit} config files"
            ),
            Self::TooManyHostlists {
                category,
                config_name,
                limit,
            } => write!(
                formatter,
                "Legacy config {category:?}/{config_name:?} exceeds {limit} hostlist references"
            ),
            Self::InvalidStaticDomain { category } => write!(
                formatter,
                "static ownership for category {category:?} contains an invalid domain"
            ),
            Self::Config {
                category,
                config_name,
                source,
            } => write!(
                formatter,
                "could not parse Legacy selection {category:?}/{config_name:?}: {source}"
            ),
            Self::Hostlist { category, line } => write!(
                formatter,
                "static hostlist for category {category:?} has an invalid domain on line {line}"
            ),
            Self::FileTooLarge { path, limit } => {
                write!(formatter, "{} exceeds {limit} bytes", path.display())
            }
            Self::SnapshotTooLarge { limit } => {
                write!(formatter, "Legacy isolation snapshot exceeds {limit} bytes")
            }
            Self::InvalidUtf8 { path } => {
                write!(formatter, "{} is not valid UTF-8", path.display())
            }
            Self::UnsafePath { operation, path } => {
                write!(
                    formatter,
                    "{operation} rejected unsafe path {}",
                    path.display()
                )
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "{operation} failed for {}: {source}",
                path.display()
            ),
            Self::Plan(source) => {
                write!(formatter, "auto-hostlist migration plan failed: {source}")
            }
            Self::Migration(source) => {
                write!(formatter, "auto-hostlist migration failed: {source}")
            }
            Self::Overlay {
                category,
                config_name,
                source,
            } => write!(
                formatter,
                "Legacy overlay failed for {category:?}/{config_name:?}: {source}"
            ),
        }
    }
}

impl Error for IsolationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::Plan(source) => Some(source),
            Self::Migration(source) => Some(source),
            Self::Overlay { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<MigrationPlanError> for IsolationError {
    fn from(source: MigrationPlanError) -> Self {
        Self::Plan(source)
    }
}

impl From<MigrationExecutionError> for IsolationError {
    fn from(source: MigrationExecutionError) -> Self {
        Self::Migration(source)
    }
}

#[derive(Clone, Debug, Default)]
struct StaticOwnershipIndex {
    categories: BTreeSet<String>,
    owners_by_suffix: BTreeMap<String, BTreeSet<String>>,
}

impl StaticOwnershipIndex {
    fn build(ownership: &BTreeMap<String, BTreeSet<String>>) -> Result<Self, MigrationPlanError> {
        let mut index = Self::default();
        for (raw_category, domains) in ownership {
            let category = normalize_category(raw_category).ok_or_else(|| {
                MigrationPlanError::InvalidCategory {
                    category: raw_category.clone(),
                }
            })?;
            index.categories.insert(category.clone());
            for raw_domain in domains {
                let domain = normalize_domain(raw_domain).ok_or_else(|| {
                    MigrationPlanError::InvalidStaticDomain {
                        category: category.clone(),
                    }
                })?;
                index
                    .owners_by_suffix
                    .entry(domain)
                    .or_default()
                    .insert(category.clone());
            }
        }
        Ok(index)
    }

    fn owners_for_host(&self, host: &str) -> BTreeSet<String> {
        let mut suffix = host;
        loop {
            if let Some(suffix_owners) = self.owners_by_suffix.get(suffix) {
                // Static ownership follows the same longest/deepest suffix
                // rule as TargetRegistry. A specific foreign suffix must beat
                // a broader source-category suffix.
                return suffix_owners.clone();
            }
            let Some(dot) = suffix.find('.') else {
                break;
            };
            suffix = &suffix[dot + 1..];
        }
        BTreeSet::new()
    }
}

/// Produces a deterministic, privacy-preserving migration plan without touching
/// the filesystem. Ambiguous hosts and foreign hosts whose owner has no mutable
/// destination are intentionally absent from every output; their byte-exact
/// originals remain recoverable through the executor backup.
pub(crate) fn plan_migration(
    static_ownership: &BTreeMap<String, BTreeSet<String>>,
    snapshots: &[AutoHostSnapshot],
) -> Result<MigrationPlan, MigrationPlanError> {
    plan_migration_with_limits(
        static_ownership,
        snapshots,
        HostlistLimits {
            max_hosts: MAX_HOSTS_PER_FILE,
            max_bytes: MAX_HOSTLIST_BYTES,
        },
    )
}

#[derive(Clone, Copy, Debug)]
struct HostlistLimits {
    max_hosts: usize,
    max_bytes: usize,
}

fn plan_migration_with_limits(
    static_ownership: &BTreeMap<String, BTreeSet<String>>,
    snapshots: &[AutoHostSnapshot],
    limits: HostlistLimits,
) -> Result<MigrationPlan, MigrationPlanError> {
    if snapshots.len() > MAX_AUTO_FILES {
        return Err(MigrationPlanError::TooManyFiles {
            limit: MAX_AUTO_FILES,
        });
    }
    let ownership = StaticOwnershipIndex::build(static_ownership)?;
    let mut seen_paths = BTreeSet::new();
    let mut category_files: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut normalized_files = Vec::with_capacity(snapshots.len());
    let mut total_bytes = 0usize;
    let mut total_hosts = 0usize;

    for (index, snapshot) in snapshots.iter().enumerate() {
        let category = normalize_category(&snapshot.category).ok_or_else(|| {
            MigrationPlanError::InvalidCategory {
                category: snapshot.category.clone(),
            }
        })?;
        if !is_confined_relative(&snapshot.relative_path) {
            return Err(MigrationPlanError::InvalidRelativePath);
        }
        let normalized_path = normalize_relative_path(&snapshot.relative_path)
            .ok_or(MigrationPlanError::InvalidRelativePath)?;
        if normalized_path
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            != Some("autohosts")
        {
            return Err(MigrationPlanError::InvalidRelativePath);
        }
        let path_key =
            relative_path_key(&normalized_path).ok_or(MigrationPlanError::InvalidRelativePath)?;
        if !seen_paths.insert(path_key) {
            return Err(MigrationPlanError::DuplicatePath);
        }

        let bytes = snapshot.original.as_deref().unwrap_or_default();
        total_bytes =
            total_bytes
                .checked_add(bytes.len())
                .ok_or(MigrationPlanError::SnapshotTooLarge {
                    limit: MAX_TOTAL_SNAPSHOT_BYTES,
                })?;
        if bytes.len() > limits.max_bytes {
            return Err(MigrationPlanError::HostlistTooLarge {
                category,
                limit: limits.max_bytes,
            });
        }
        if total_bytes > MAX_TOTAL_SNAPSHOT_BYTES {
            return Err(MigrationPlanError::SnapshotTooLarge {
                limit: MAX_TOTAL_SNAPSHOT_BYTES,
            });
        }
        let text = std::str::from_utf8(bytes).map_err(|_| MigrationPlanError::InvalidUtf8 {
            category: category.clone(),
        })?;
        let domains = parse_domains(text, &category, |category, line| {
            MigrationPlanError::InvalidDomain { category, line }
        })?;
        if domains.len() > limits.max_hosts {
            return Err(MigrationPlanError::TooManyHosts {
                category,
                limit: limits.max_hosts,
            });
        }
        total_hosts = total_hosts.checked_add(domains.len()).ok_or(
            MigrationPlanError::TooManyTotalHosts {
                limit: MAX_TOTAL_HOSTS,
            },
        )?;
        if total_hosts > MAX_TOTAL_HOSTS {
            return Err(MigrationPlanError::TooManyTotalHosts {
                limit: MAX_TOTAL_HOSTS,
            });
        }
        category_files
            .entry(category.clone())
            .or_default()
            .push(index);
        normalized_files.push((category, normalized_path, domains));
    }
    if let Some((category, _)) = category_files.iter().find(|(_, files)| files.len() > 1) {
        return Err(MigrationPlanError::MultipleDestinationAutoHostlists {
            category: category.clone(),
        });
    }

    let mut outputs = normalized_files
        .iter()
        .map(|(_, _, domains)| domains.clone())
        .collect::<Vec<_>>();
    let mut moved = BTreeSet::new();
    let mut ambiguous = BTreeSet::new();
    let mut unknown = BTreeSet::new();

    for (source_index, (source_category, _, domains)) in normalized_files.iter().enumerate() {
        for host in domains {
            let owners = ownership.owners_for_host(host);
            match owners.len() {
                0 => {
                    unknown.insert((source_category.clone(), host.clone()));
                }
                1 if owners.contains(source_category) => {}
                1 => {
                    let destination = owners.iter().next().expect("single-owner set is non-empty");
                    outputs[source_index].remove(host);
                    if let Some(destination_files) = category_files.get(destination) {
                        for destination_index in destination_files {
                            outputs[*destination_index].insert(host.clone());
                        }
                        moved.insert((source_category.clone(), destination.clone(), host.clone()));
                    } else {
                        // Some valid static-only categories (for example
                        // `atrisk`) deliberately have no auto-hostlist. Keeping
                        // their learned domains in a foreign mutable file would
                        // preserve the cross-category leak; failing startup
                        // would make an otherwise valid category combination
                        // unusable. Quarantine is recoverable from the migration
                        // backup and is therefore the only safe bounded result.
                        ambiguous.insert(host.clone());
                    }
                }
                _ => {
                    outputs[source_index].remove(host);
                    ambiguous.insert(host.clone());
                }
            }
        }
    }

    let rendered_outputs = outputs
        .iter()
        .enumerate()
        .map(|(index, domains)| render_bounded_domains(domains, &normalized_files[index].0, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let mut changes = Vec::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        let (_, relative_path, _) = &normalized_files[index];
        let after = rendered_outputs[index].clone();
        let before_bytes = snapshot.original.as_deref().unwrap_or_default();
        let absent_and_empty = snapshot.original.is_none() && after.is_empty();
        if !absent_and_empty && before_bytes != after.as_slice() {
            changes.push(MigrationFileChange {
                category: normalized_files[index].0.clone(),
                relative_path: relative_path.clone(),
                before: snapshot.original.clone(),
                after,
            });
        }
    }
    changes.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let summary = MigrationSummary {
        files_changed: changes.len(),
        hosts_moved: moved.len(),
        ambiguous_hosts_quarantined: ambiguous.len(),
        unknown_hosts_retained: unknown.len(),
        rollback_performed: false,
    };
    Ok(MigrationPlan { changes, summary })
}

/// Returns only static suffixes uniquely foreign to `category` among the full
/// selected set. Static suffixes shared with any other selected category are
/// deliberately not excluded.
pub(crate) fn foreign_only_exclusions(
    category: &str,
    selected_categories: &BTreeSet<String>,
    static_ownership: &BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeSet<String>, MigrationPlanError> {
    let category =
        normalize_category(category).ok_or_else(|| MigrationPlanError::InvalidCategory {
            category: category.to_string(),
        })?;
    let selected = selected_categories
        .iter()
        .map(|value| {
            normalize_category(value).ok_or_else(|| MigrationPlanError::InvalidCategory {
                category: value.clone(),
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut filtered: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (raw_owner, domains) in static_ownership {
        let owner =
            normalize_category(raw_owner).ok_or_else(|| MigrationPlanError::InvalidCategory {
                category: raw_owner.clone(),
            })?;
        if selected.contains(&owner) {
            filtered
                .entry(owner)
                .or_default()
                .extend(domains.iter().cloned());
        }
    }
    let index = StaticOwnershipIndex::build(&filtered)?;

    let mut exclusions = BTreeSet::new();
    for (owner, domains) in &filtered {
        if owner == &category {
            continue;
        }
        for raw_domain in domains {
            let domain = normalize_domain(raw_domain).ok_or_else(|| {
                MigrationPlanError::InvalidStaticDomain {
                    category: owner.clone(),
                }
            })?;
            let owners = index.owners_for_host(&domain);
            let covers_source_specific = filtered.get(&category).is_some_and(|source_domains| {
                source_domains.iter().any(|source_domain| {
                    normalize_domain(source_domain).is_some_and(|source_domain| {
                        source_domain == domain || source_domain.ends_with(&format!(".{domain}"))
                    })
                })
            });
            if owners.len() == 1 && !owners.contains(&category) && !covers_source_specific {
                exclusions.insert(domain);
            }
        }
    }
    Ok(exclusions)
}

fn parse_domains<E>(
    content: &str,
    category: &str,
    invalid: impl Fn(String, usize) -> E,
) -> Result<BTreeSet<String>, E> {
    let mut domains = BTreeSet::new();
    for (index, raw_line) in content.lines().enumerate() {
        let without_bom = raw_line.trim_start_matches('\u{feff}');
        let value = without_bom
            .split_once('#')
            .map_or(without_bom, |(value, _)| value)
            .trim();
        if value.is_empty() {
            continue;
        }
        let Some(domain) = normalize_domain(value) else {
            return Err(invalid(category.to_string(), index + 1));
        };
        domains.insert(domain);
    }
    Ok(domains)
}

fn render_domains(domains: &BTreeSet<String>) -> Vec<u8> {
    if domains.is_empty() {
        Vec::new()
    } else {
        let mut rendered = domains.iter().cloned().collect::<Vec<_>>().join("\n");
        rendered.push('\n');
        rendered.into_bytes()
    }
}

fn render_bounded_domains(
    domains: &BTreeSet<String>,
    category: &str,
    limits: HostlistLimits,
) -> Result<Vec<u8>, MigrationPlanError> {
    if domains.len() > limits.max_hosts {
        return Err(MigrationPlanError::TooManyHosts {
            category: category.to_string(),
            limit: limits.max_hosts,
        });
    }
    let rendered = render_domains(domains);
    if rendered.len() > limits.max_bytes {
        return Err(MigrationPlanError::HostlistTooLarge {
            category: category.to_string(),
            limit: limits.max_bytes,
        });
    }
    Ok(rendered)
}

fn normalize_category(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    is_safe_component(&value).then_some(value)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BackupManifest {
    schema_version: u32,
    timestamp_ms: u64,
    files: Vec<BackupManifestEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifestEntry {
    relative_path: String,
    existed: bool,
    backup_file: Option<String>,
}

/// Applies a pure migration plan transactionally. Every original is persisted
/// in a snapshot before the first destination replacement. A failed replacement
/// restores already-mutated destinations in reverse order.
pub(crate) fn execute_migration(
    base_dir: &Path,
    backup_root: &Path,
    plan: &MigrationPlan,
    timestamp_ms: u64,
) -> Result<MigrationSummary, MigrationExecutionError> {
    execute_migration_inner(base_dir, backup_root, plan, timestamp_ms, None)
}

fn execute_migration_inner(
    base_dir: &Path,
    backup_root: &Path,
    plan: &MigrationPlan,
    timestamp_ms: u64,
    #[cfg_attr(not(test), allow(unused_variables))] fail_after_replacements: Option<usize>,
) -> Result<MigrationSummary, MigrationExecutionError> {
    if plan.is_empty() {
        return Ok(plan.summary());
    }

    let canonical_base = canonical_directory(base_dir).map_err(|source| {
        migration_io_error(
            "canonicalize isolation base directory",
            base_dir,
            source,
            plan.summary(),
            false,
        )
    })?;
    let requested_backup_root = rebase_requested_path(base_dir, &canonical_base, backup_root)
        .unwrap_or_else(|| backup_root.to_path_buf());
    let canonical_backup_root = create_confined_directory(
        &canonical_base,
        &requested_backup_root,
        "create migration backup directory",
    )
    .map_err(|(operation, path, source)| {
        migration_io_error(operation, &path, source, plan.summary(), false)
    })?;
    let snapshot =
        create_backup_snapshot(&canonical_base, &canonical_backup_root, plan, timestamp_ms)?;

    let mut applied = 0usize;
    let mut failure = None;
    for change in &plan.changes {
        #[cfg(test)]
        if fail_after_replacements.is_some_and(|limit| applied >= limit) {
            failure = Some((
                "replace migrated auto-hostlist",
                safe_join(&canonical_base, &change.relative_path)
                    .unwrap_or_else(|| canonical_base.clone()),
                io::Error::other("simulated replacement failure"),
            ));
            break;
        }

        let destination = match validate_destination(&canonical_base, &change.relative_path) {
            Ok(path) => path,
            Err((operation, path, source)) => {
                failure = Some((operation, path, source));
                break;
            }
        };
        match read_optional_bounded(&destination, MAX_HOSTLIST_BYTES) {
            Ok(current) if current == change.before => {}
            Ok(_) => {
                failure = Some((
                    "verify auto-hostlist snapshot",
                    destination,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "auto-hostlist changed after migration planning",
                    ),
                ));
                break;
            }
            Err(source) => {
                failure = Some(("read auto-hostlist before replacement", destination, source));
                break;
            }
        }
        if let Err(source) = write_atomic(&destination, &change.after) {
            failure = Some(("replace migrated auto-hostlist", destination, source));
            break;
        }
        applied += 1;
    }

    if let Some((operation, path, source)) = failure {
        let rollback_result = rollback_from_snapshot(&canonical_base, &snapshot, applied);
        let rollback_failed = rollback_result.is_err();
        if !rollback_failed {
            let _ = fs::remove_dir_all(&snapshot.path);
        }
        let mut summary = plan.summary();
        summary.rollback_performed = applied > 0;
        let source = match rollback_result {
            Ok(()) => source,
            Err(rollback) => io::Error::new(
                rollback.kind(),
                format!("{source}; rollback also failed: {rollback}"),
            ),
        };
        return Err(migration_io_error(
            operation,
            &path,
            source,
            summary,
            rollback_failed,
        ));
    }

    let committed_marker = snapshot.path.join(BACKUP_COMMITTED_MARKER);
    if let Err(source) = write_atomic(&committed_marker, BACKUP_COMMITTED_CONTENT) {
        // A failed commit must never leave a marker that a later recovery can
        // mistake for a durable transaction. `write_atomic` publishes only
        // after syncing its temporary file; this removal also covers an
        // ambiguous platform rename failure before rollback begins.
        let _ = fs::remove_file(&committed_marker);
        let rollback_result = rollback_from_snapshot(&canonical_base, &snapshot, applied);
        let rollback_failed = rollback_result.is_err();
        if !rollback_failed {
            let _ = fs::remove_dir_all(&snapshot.path);
        }
        let mut summary = plan.summary();
        summary.rollback_performed = applied > 0;
        let source = match rollback_result {
            Ok(()) => source,
            Err(rollback) => io::Error::new(
                rollback.kind(),
                format!("{source}; rollback also failed: {rollback}"),
            ),
        };
        return Err(migration_io_error(
            "commit migration backup",
            &committed_marker,
            source,
            summary,
            rollback_failed,
        ));
    }
    if let Err((operation, path, source)) = prune_backup_directories(
        &canonical_backup_root,
        MAX_MIGRATION_BACKUPS,
        Some(&snapshot.path),
    ) {
        // The marker is durable and all replacements succeeded. This is a
        // committed migration, not a partial transaction; fail closed so the
        // next preparation retries bounded pruning before any new snapshot.
        return Err(migration_io_error(
            operation,
            &path,
            source,
            plan.summary(),
            false,
        ));
    }

    Ok(plan.summary())
}

struct CreatedBackup {
    path: PathBuf,
    manifest: BackupManifest,
}

fn create_backup_snapshot(
    canonical_base: &Path,
    canonical_backup_root: &Path,
    plan: &MigrationPlan,
    timestamp_ms: u64,
) -> Result<CreatedBackup, MigrationExecutionError> {
    let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let snapshot_name = format!(
        "{BACKUP_PREFIX}{timestamp_ms:020}-{}-{sequence:020}",
        std::process::id()
    );
    let snapshot_path = canonical_backup_root.join(snapshot_name);
    fs::create_dir(&snapshot_path).map_err(|source| {
        migration_io_error(
            "create migration snapshot",
            &snapshot_path,
            source,
            plan.summary(),
            false,
        )
    })?;

    let result = (|| {
        let mut files = Vec::with_capacity(plan.changes.len());
        for (index, change) in plan.changes.iter().enumerate() {
            let destination = validate_destination(canonical_base, &change.relative_path)?;
            let current = read_optional_bounded(&destination, MAX_HOSTLIST_BYTES)
                .map_err(|source| ("read auto-hostlist for backup", destination.clone(), source))?;
            if current != change.before {
                return Err((
                    "verify auto-hostlist before backup",
                    destination,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "auto-hostlist changed after migration planning",
                    ),
                ));
            }
            let backup_file = current.as_ref().map(|bytes| {
                let file_name = format!("{index:04}.hostlist");
                (file_name, bytes)
            });
            if let Some((file_name, bytes)) = &backup_file {
                write_new_synced(&snapshot_path.join(file_name), bytes).map_err(|source| {
                    (
                        "write migration backup",
                        snapshot_path.join(file_name),
                        source,
                    )
                })?;
            }
            files.push(BackupManifestEntry {
                relative_path: relative_path_key(&change.relative_path).ok_or_else(|| {
                    (
                        "validate migration backup path",
                        change.relative_path.clone(),
                        io::Error::new(io::ErrorKind::InvalidInput, "unsafe relative path"),
                    )
                })?,
                existed: current.is_some(),
                backup_file: backup_file.map(|(name, _)| name),
            });
        }
        let manifest = BackupManifest {
            schema_version: BACKUP_SCHEMA_VERSION,
            timestamp_ms,
            files,
        };
        let encoded = serde_json::to_vec_pretty(&manifest).map_err(|source| {
            (
                "encode migration backup manifest",
                snapshot_path.clone(),
                io::Error::new(io::ErrorKind::InvalidData, source),
            )
        })?;
        write_new_synced(&snapshot_path.join("manifest.json"), &encoded).map_err(|source| {
            (
                "write migration backup manifest",
                snapshot_path.join("manifest.json"),
                source,
            )
        })?;
        Ok::<_, (&'static str, PathBuf, io::Error)>(manifest)
    })();

    match result {
        Ok(manifest) => Ok(CreatedBackup {
            path: snapshot_path,
            manifest,
        }),
        Err((operation, path, source)) => {
            let _ = fs::remove_dir_all(&snapshot_path);
            Err(migration_io_error(
                operation,
                &path,
                source,
                plan.summary(),
                false,
            ))
        }
    }
}

fn rollback_from_snapshot(
    canonical_base: &Path,
    snapshot: &CreatedBackup,
    applied: usize,
) -> io::Result<()> {
    let mut first_error = None;
    for entry in snapshot.manifest.files.iter().take(applied).rev() {
        let relative = PathBuf::from(
            entry
                .relative_path
                .replace('/', std::path::MAIN_SEPARATOR_STR),
        );
        let destination = match validate_destination(canonical_base, &relative) {
            Ok(path) => path,
            Err((_, _, source)) => {
                if first_error.is_none() {
                    first_error = Some(source);
                }
                continue;
            }
        };
        let result = if entry.existed {
            let Some(backup_file) = entry.backup_file.as_deref() else {
                if first_error.is_none() {
                    first_error = Some(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "migration backup is missing a file reference",
                    ));
                }
                continue;
            };
            if !is_safe_component(backup_file) || Path::new(backup_file).components().count() != 1 {
                if first_error.is_none() {
                    first_error = Some(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "migration backup has an unsafe file reference",
                    ));
                }
                continue;
            }
            match fs::read(snapshot.path.join(backup_file)) {
                Ok(bytes) => write_atomic(&destination, &bytes),
                Err(source) => Err(source),
            }
        } else {
            match fs::remove_file(&destination) {
                Ok(()) => Ok(()),
                Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(source),
            }
        };
        if let Err(source) = result {
            if first_error.is_none() {
                first_error = Some(source);
            }
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn prune_backup_directories(
    backup_root: &Path,
    limit: usize,
    protected_snapshot: Option<&Path>,
) -> Result<(), (&'static str, PathBuf, io::Error)> {
    let mut backups = Vec::new();
    for entry in fs::read_dir(backup_root)
        .map_err(|source| ("read migration backups", backup_root.to_path_buf(), source))?
    {
        let entry = entry.map_err(|source| {
            (
                "read migration backup entry",
                backup_root.to_path_buf(),
                source,
            )
        })?;
        let file_type = entry
            .file_type()
            .map_err(|source| ("inspect migration backup entry", entry.path(), source))?;
        if file_type.is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(BACKUP_PREFIX))
            && is_committed_backup(&entry.path())
        {
            backups.push(entry.path());
        }
    }
    backups.sort();
    let remove_count = backups.len().saturating_sub(limit);
    for path in backups
        .into_iter()
        .filter(|path| protected_snapshot != Some(path.as_path()))
        .take(remove_count)
    {
        fs::remove_dir_all(&path).map_err(|source| ("prune migration backup", path, source))?;
    }
    Ok(())
}

fn is_committed_backup(path: &Path) -> bool {
    fs::read(path.join(BACKUP_COMMITTED_MARKER))
        .is_ok_and(|content| content == BACKUP_COMMITTED_CONTENT)
}

/// Restores the sole uncommitted transaction before a new migration snapshot
/// is planned. All manifest entries are restored, including entries that were
/// never replaced; this makes crash recovery idempotent without an applied
/// counter that could itself become stale.
fn recover_incomplete_migration(
    canonical_base: &Path,
    backup_root: &Path,
) -> Result<bool, MigrationExecutionError> {
    let mut incomplete = Vec::new();
    for entry in fs::read_dir(backup_root).map_err(|source| {
        migration_io_error(
            "scan migration backups for recovery",
            backup_root,
            source,
            MigrationSummary::default(),
            false,
        )
    })? {
        let entry = entry.map_err(|source| {
            migration_io_error(
                "read migration backup during recovery",
                backup_root,
                source,
                MigrationSummary::default(),
                false,
            )
        })?;
        let path = entry.path();
        let is_snapshot = entry
            .file_type()
            .map_err(|source| {
                migration_io_error(
                    "inspect migration backup during recovery",
                    &path,
                    source,
                    MigrationSummary::default(),
                    false,
                )
            })?
            .is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(BACKUP_PREFIX));
        if is_snapshot && !is_committed_backup(&path) {
            incomplete.push(path);
        }
    }
    if incomplete.len() > 1 {
        return Err(migration_io_error(
            "recover incomplete migration",
            backup_root,
            io::Error::new(
                io::ErrorKind::InvalidData,
                "multiple incomplete migration snapshots require manual recovery",
            ),
            MigrationSummary::default(),
            false,
        ));
    }

    let mut recovered = false;
    if let Some(snapshot_path) = incomplete.pop() {
        let manifest_path = snapshot_path.join("manifest.json");
        let encoded = match read_bounded(&manifest_path, 1024 * 1024) {
            Ok(encoded) => Some(encoded),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                // Replacements begin only after a complete synced manifest.
                fs::remove_dir_all(&snapshot_path).map_err(|source| {
                    migration_io_error(
                        "remove pre-manifest migration snapshot",
                        &snapshot_path,
                        source,
                        MigrationSummary::default(),
                        false,
                    )
                })?;
                None
            }
            Err(source) => {
                return Err(migration_io_error(
                    "read incomplete migration manifest",
                    &manifest_path,
                    source,
                    MigrationSummary::default(),
                    false,
                ));
            }
        };
        if let Some(encoded) = encoded {
            let manifest: BackupManifest = serde_json::from_slice(&encoded).map_err(|source| {
                migration_io_error(
                    "decode incomplete migration manifest",
                    &manifest_path,
                    io::Error::new(io::ErrorKind::InvalidData, source),
                    MigrationSummary::default(),
                    false,
                )
            })?;
            if manifest.schema_version != BACKUP_SCHEMA_VERSION {
                return Err(migration_io_error(
                    "validate incomplete migration manifest",
                    &manifest_path,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unsupported migration backup schema",
                    ),
                    MigrationSummary::default(),
                    false,
                ));
            }
            let snapshot = CreatedBackup {
                path: snapshot_path.clone(),
                manifest,
            };
            rollback_from_snapshot(canonical_base, &snapshot, snapshot.manifest.files.len())
                .map_err(|source| {
                    let summary = MigrationSummary {
                        rollback_performed: true,
                        ..MigrationSummary::default()
                    };
                    migration_io_error(
                        "restore incomplete migration snapshot",
                        &snapshot_path,
                        source,
                        summary,
                        true,
                    )
                })?;
            fs::remove_dir_all(&snapshot_path).map_err(|source| {
                let summary = MigrationSummary {
                    rollback_performed: true,
                    ..MigrationSummary::default()
                };
                migration_io_error(
                    "remove recovered migration snapshot",
                    &snapshot_path,
                    source,
                    summary,
                    false,
                )
            })?;
            recovered = true;
        }
    }

    prune_backup_directories(backup_root, MAX_MIGRATION_BACKUPS, None).map_err(
        |(operation, path, source)| {
            migration_io_error(operation, &path, source, MigrationSummary::default(), false)
        },
    )?;
    Ok(recovered)
}

fn migration_io_error(
    operation: &'static str,
    path: &Path,
    source: io::Error,
    summary: MigrationSummary,
    rollback_failed: bool,
) -> MigrationExecutionError {
    MigrationExecutionError {
        operation,
        path: path.to_path_buf(),
        source,
        summary,
        rollback_failed,
    }
}

#[derive(Clone, Debug)]
struct ConfigTokenSpan {
    start: usize,
    end: usize,
    value: String,
}

/// Token-aware overlay transformation. Exactly one generated exclusion is
/// added to every `--new` profile containing at least one `--hostlist-auto`.
/// Existing unrelated exclusions and all comments/line endings are preserved.
///
/// `None` means that the config has no auto-learning profile and may be launched
/// directly. `Some` is returned even when the exact generated reference was
/// already present, making repeated preparation idempotent.
pub(crate) fn inject_exclusion_overlay(
    source: &str,
    exclusion_reference: &str,
) -> Result<Option<String>, OverlayError> {
    if source.len() > MAX_CONFIG_BYTES {
        return Err(OverlayError::TooLarge {
            limit: MAX_CONFIG_BYTES,
        });
    }
    // Reuse the production parser as the syntax authority before preserving
    // spans for injection. This also rejects malformed quoted option values.
    parse_legacy_config(source)?;
    let normalized_reference =
        normalize_reference(exclusion_reference).ok_or(OverlayError::InvalidExclusionReference)?;
    let rendered_reference = normalized_reference.replace('/', "\\");
    let tokens = tokenize_with_spans(source)?;
    if tokens.len() > MAX_CONFIG_TOKENS {
        return Err(OverlayError::TooManyTokens {
            limit: MAX_CONFIG_TOKENS,
        });
    }

    let mut profile_start = 0usize;
    let mut insertions = BTreeSet::new();
    let mut has_auto_profile = false;
    for boundary in 0..=tokens.len() {
        let at_boundary = boundary == tokens.len() || tokens[boundary].value == "--new";
        if !at_boundary {
            continue;
        }
        if profile_start < boundary {
            let profile = &tokens[profile_start..boundary];
            let analysis = analyze_profile(profile, &normalized_reference)?;
            if analysis.has_auto {
                has_auto_profile = true;
                if !analysis.has_exact_exclusion {
                    insertions.insert(analysis.insertion_position);
                }
            }
        }
        profile_start = boundary.saturating_add(1);
    }

    if !has_auto_profile {
        return Ok(None);
    }
    if insertions.is_empty() {
        return Ok(Some(source.to_string()));
    }

    let injection = format!(" --hostlist-exclude=\"{rendered_reference}\"");
    let projected = source
        .len()
        .checked_add(injection.len().saturating_mul(insertions.len()))
        .ok_or(OverlayError::TooLarge {
            limit: MAX_CONFIG_BYTES,
        })?;
    if projected > MAX_CONFIG_BYTES {
        return Err(OverlayError::TooLarge {
            limit: MAX_CONFIG_BYTES,
        });
    }
    let mut effective = source.to_string();
    for position in insertions.into_iter().rev() {
        effective.insert_str(position, &injection);
    }
    Ok(Some(effective))
}

#[derive(Clone, Copy, Debug, Default)]
struct ProfileAnalysis {
    has_auto: bool,
    has_exact_exclusion: bool,
    insertion_position: usize,
}

fn analyze_profile(
    profile: &[ConfigTokenSpan],
    normalized_reference: &str,
) -> Result<ProfileAnalysis, OverlayError> {
    let mut analysis = ProfileAnalysis::default();
    let mut index = 0usize;
    while index < profile.len() {
        let token = &profile[index];
        let (option, inline) = token
            .value
            .split_once('=')
            .map_or((token.value.as_str(), None), |(option, value)| {
                (option, Some(value))
            });
        if matches!(option, "--hostlist-auto" | "--hostlist-exclude") {
            let (value, value_end, consumed_next) = if let Some(value) = inline {
                if value.is_empty() {
                    return Err(OverlayError::MissingOptionValue);
                }
                (value, token.end, false)
            } else {
                let next = profile
                    .get(index + 1)
                    .filter(|next| !next.value.starts_with("--"))
                    .ok_or(OverlayError::MissingOptionValue)?;
                (next.value.as_str(), next.end, true)
            };
            if option == "--hostlist-auto" {
                analysis.has_auto = true;
                analysis.insertion_position = analysis.insertion_position.max(value_end);
            } else if normalize_reference(value).as_deref() == Some(normalized_reference) {
                analysis.has_exact_exclusion = true;
            }
            if consumed_next {
                index += 1;
            }
        }
        index += 1;
    }
    Ok(analysis)
}

fn tokenize_with_spans(source: &str) -> Result<Vec<ConfigTokenSpan>, OverlayError> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    let mut token_value = String::new();
    let mut quote = None;
    let mut in_comment = false;
    for (offset, character) in source.char_indices() {
        if in_comment {
            if character == '\n' {
                in_comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                token_value.push(character);
            }
            continue;
        }
        match character {
            '\u{feff}' if offset == 0 && token_start.is_none() => {}
            '"' | '\'' => {
                token_start.get_or_insert(offset);
                quote = Some(character);
            }
            '#' => {
                flush_token(&mut tokens, &mut token_start, &mut token_value, offset);
                in_comment = true;
            }
            character if character.is_whitespace() => {
                flush_token(&mut tokens, &mut token_start, &mut token_value, offset);
            }
            _ => {
                token_start.get_or_insert(offset);
                token_value.push(character);
            }
        }
        if tokens.len() > MAX_CONFIG_TOKENS {
            return Err(OverlayError::TooManyTokens {
                limit: MAX_CONFIG_TOKENS,
            });
        }
    }
    if quote.is_some() {
        // The production parser above normally catches this. Keep the span
        // tokenizer independently fail-closed if its contract changes later.
        return Err(OverlayError::Config(ConfigParseError::UnterminatedQuote {
            line: source.lines().count().max(1),
        }));
    }
    flush_token(
        &mut tokens,
        &mut token_start,
        &mut token_value,
        source.len(),
    );
    Ok(tokens)
}

fn flush_token(
    tokens: &mut Vec<ConfigTokenSpan>,
    token_start: &mut Option<usize>,
    token_value: &mut String,
    end: usize,
) {
    if let Some(start) = token_start.take() {
        tokens.push(ConfigTokenSpan {
            start,
            end,
            value: std::mem::take(token_value),
        });
    }
}

fn normalize_reference(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let replaced = value.replace('\\', "/");
    if replaced.starts_with('/') || replaced.contains(':') {
        return None;
    }
    let mut parts = Vec::new();
    for part in replaced.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/").to_ascii_lowercase())
}

/// Production convenience boundary. It snapshots the exact selected configs,
/// follows all of their static and auto-hostlist references under `base_dir`,
/// optionally migrates existing learned hosts, and returns launch-ready paths.
/// No source `.conf` is ever opened for writing.
pub(crate) fn prepare_launches_from_disk(
    base_dir: &Path,
    selections: &[(String, String)],
    migrate_existing: bool,
) -> Result<IsolationPrepareResult, IsolationError> {
    prepare_launches_from_disk_inner(
        base_dir,
        selections,
        migrate_existing,
        &Materialization::All,
        false,
    )
}

/// Read-only safety gate for `start_many`: validates the exact disk snapshot,
/// migration outputs, and generated overlays before any running Legacy process
/// is stopped. It never creates runtime/backup/auto directories and never
/// restores an incomplete transaction.
pub(crate) fn preflight_launches_from_disk(
    base_dir: &Path,
    selections: &[(String, String)],
) -> Result<(), IsolationError> {
    prepare_launches_from_disk_inner(base_dir, selections, true, &Materialization::All, true)
        .map(|_| ())
}

/// Read-only counterpart of scoped materialization. Both candidate and exact
/// rollback configs pass through this boundary before the executor restarts
/// Eyes or stops the currently working target lane.
pub(crate) fn preflight_scoped_launch_from_disk(
    base_dir: &Path,
    full_selections: &[(String, String)],
    target_category: &str,
    target_config: &str,
) -> Result<(), IsolationError> {
    let category =
        normalize_category(target_category).ok_or_else(|| IsolationError::InvalidSelection {
            category: target_category.to_string(),
            config_name: target_config.to_string(),
        })?;
    if !is_safe_config_name(target_config) {
        return Err(IsolationError::InvalidSelection {
            category,
            config_name: target_config.to_string(),
        });
    }
    prepare_launches_from_disk_inner(
        base_dir,
        full_selections,
        false,
        &Materialization::One((category, target_config.to_string())),
        true,
    )
    .map(|_| ())
}

/// Scoped recovery observes the full tentative selection for ownership and
/// exclusions, but persists only the exact target's generated files.
pub(crate) fn prepare_scoped_launch_from_disk(
    base_dir: &Path,
    full_selections: &[(String, String)],
    target_category: &str,
    target_config: &str,
) -> Result<PathBuf, IsolationError> {
    let category =
        normalize_category(target_category).ok_or_else(|| IsolationError::InvalidSelection {
            category: target_category.to_string(),
            config_name: target_config.to_string(),
        })?;
    if !is_safe_config_name(target_config) {
        return Err(IsolationError::InvalidSelection {
            category,
            config_name: target_config.to_string(),
        });
    }
    let key = (category, target_config.to_string());
    let mut result = prepare_launches_from_disk_inner(
        base_dir,
        full_selections,
        false,
        &Materialization::One(key.clone()),
        false,
    )?;
    result
        .effective_paths
        .remove(&key)
        .ok_or(IsolationError::TargetNotSelected {
            category: key.0,
            config_name: key.1,
        })
}

#[derive(Clone, Debug)]
enum Materialization {
    All,
    One((String, String)),
}

impl Materialization {
    fn includes(&self, category: &str, config_name: &str) -> bool {
        match self {
            Self::All => true,
            Self::One((target_category, target_config)) => {
                target_category == category && target_config == config_name
            }
        }
    }
}

fn prepare_launches_from_disk_inner(
    base_dir: &Path,
    selections: &[(String, String)],
    migrate_existing: bool,
    materialization: &Materialization,
    read_only: bool,
) -> Result<IsolationPrepareResult, IsolationError> {
    let canonical_base = canonical_directory(base_dir).map_err(|source| IsolationError::Io {
        operation: "canonicalize isolation base directory",
        path: base_dir.to_path_buf(),
        source,
    })?;
    let validated = validate_selection_pairs(selections)?;
    let configs_root = canonical_directory(&canonical_base.join("configs")).map_err(|source| {
        IsolationError::Io {
            operation: "canonicalize Legacy configs directory",
            path: canonical_base.join("configs"),
            source,
        }
    })?;
    require_descendant(
        &canonical_base,
        &configs_root,
        "resolve Legacy configs directory",
    )?;

    let mut selected_configs = Vec::with_capacity(validated.len());
    let mut static_ownership: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut snapshot_bytes = 0usize;
    for (category, config_name) in validated {
        let category_dir =
            canonical_directory(&configs_root.join(&category)).map_err(|source| {
                IsolationError::Io {
                    operation: "canonicalize Legacy category directory",
                    path: configs_root.join(&category),
                    source,
                }
            })?;
        require_descendant(
            &configs_root,
            &category_dir,
            "resolve Legacy category directory",
        )?;
        let inventory = scan_category_inventory(
            &canonical_base,
            &category_dir,
            &category,
            &mut snapshot_bytes,
        )?;
        static_ownership.insert(category.clone(), inventory.static_domains);
        let source_path =
            canonical_regular_file(&category_dir.join(&config_name)).map_err(|source| {
                IsolationError::Io {
                    operation: "open selected Legacy config",
                    path: category_dir.join(&config_name),
                    source,
                }
            })?;
        require_descendant(
            &category_dir,
            &source_path,
            "resolve selected Legacy config",
        )?;
        let source_bytes = read_bounded(&source_path, MAX_CONFIG_BYTES).map_err(|source| {
            map_bounded_read_error(
                "read selected Legacy config",
                &source_path,
                source,
                MAX_CONFIG_BYTES,
            )
        })?;
        add_snapshot_bytes(&mut snapshot_bytes, source_bytes.len())?;
        let source_content =
            String::from_utf8(source_bytes).map_err(|_| IsolationError::InvalidUtf8 {
                path: source_path.clone(),
            })?;
        let parsed =
            parse_legacy_config(&source_content).map_err(|source| IsolationError::Config {
                category: category.clone(),
                config_name: config_name.clone(),
                source,
            })?;
        if parsed.hostlists.len() > MAX_HOSTLISTS_PER_CONFIG {
            return Err(IsolationError::TooManyHostlists {
                category: category.clone(),
                config_name: config_name.clone(),
                limit: MAX_HOSTLISTS_PER_CONFIG,
            });
        }
        selected_configs.push(SelectedLegacyConfig {
            category,
            config_name,
            source_path,
            source_content,
            auto_host_reference: inventory.auto_host_reference,
        });
    }

    if read_only {
        preflight_preparation(
            &canonical_base,
            &static_ownership,
            &selected_configs,
            materialization,
            migrate_existing,
        )?;
        return Ok(IsolationPrepareResult::default());
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    let backup_root = canonical_base.join("legacy-isolation-backups");
    let runtime_root = canonical_base.join("runtime").join("legacy-isolation");
    prepare_launches_inner(
        IsolationPrepareRequest {
            base_dir: &canonical_base,
            backup_root: &backup_root,
            runtime_root: &runtime_root,
            static_ownership: &static_ownership,
            selections: &selected_configs,
            migrate_existing,
            timestamp_ms,
        },
        materialization,
    )
}

struct CategoryInventory {
    static_domains: BTreeSet<String>,
    auto_host_reference: Option<PathBuf>,
}

fn preflight_preparation(
    canonical_base: &Path,
    static_ownership: &BTreeMap<String, BTreeSet<String>>,
    selections: &[SelectedLegacyConfig],
    materialization: &Materialization,
    validate_migration: bool,
) -> Result<(), IsolationError> {
    let mut selected = BTreeMap::new();
    let mut selected_categories = BTreeSet::new();
    for selection in selections {
        let category = normalize_category(&selection.category).ok_or_else(|| {
            IsolationError::InvalidSelection {
                category: selection.category.clone(),
                config_name: selection.config_name.clone(),
            }
        })?;
        if let Some(previous) = selected.insert(category.clone(), selection.config_name.clone()) {
            if previous != selection.config_name {
                return Err(IsolationError::ConflictingSelection { category });
            }
        }
        selected_categories.insert(category);
    }
    if let Materialization::One((category, config_name)) = materialization {
        if selected.get(category) != Some(config_name) {
            return Err(IsolationError::TargetNotSelected {
                category: category.clone(),
                config_name: config_name.clone(),
            });
        }
    }

    // The mutating preparation always creates the runtime root and creates the
    // backup root whenever migration is enabled. Validate those exact directory
    // chains even when no generated overlay happens to be needed, so a regular
    // file or reparse/symlink component cannot fail only after running lanes
    // have already been stopped.
    validate_read_only_directory_path(
        canonical_base,
        &PathBuf::from("runtime").join("legacy-isolation"),
    )?;
    if validate_migration {
        validate_read_only_directory_path(canonical_base, Path::new("legacy-isolation-backups"))?;
    }

    if validate_migration {
        let snapshots = load_auto_snapshots_read_only(canonical_base, selections)?;
        let _ = plan_migration(static_ownership, &snapshots)?;
    }

    for selection in selections {
        let category = normalize_category(&selection.category).expect("selection was validated");
        if !materialization.includes(&category, &selection.config_name) {
            continue;
        }
        let source_path = canonical_regular_file(&selection.source_path).map_err(|source| {
            IsolationError::Io {
                operation: "open immutable source Legacy config during preflight",
                path: selection.source_path.clone(),
                source,
            }
        })?;
        require_descendant(
            canonical_base,
            &source_path,
            "resolve immutable source Legacy config during preflight",
        )?;
        let current = read_bounded(&source_path, MAX_CONFIG_BYTES).map_err(|source| {
            map_bounded_read_error(
                "verify immutable source Legacy config during preflight",
                &source_path,
                source,
                MAX_CONFIG_BYTES,
            )
        })?;
        if current != selection.source_content.as_bytes() {
            return Err(IsolationError::Io {
                operation: "verify immutable source Legacy config during preflight",
                path: source_path,
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source config changed during read-only preflight",
                ),
            });
        }

        let exclusions =
            foreign_only_exclusions(&category, &selected_categories, static_ownership)?;
        if exclusions.is_empty() {
            continue;
        }
        let _ = render_bounded_domains(
            &exclusions,
            &category,
            HostlistLimits {
                max_hosts: MAX_HOSTS_PER_FILE,
                max_bytes: MAX_HOSTLIST_BYTES,
            },
        )?;
        let category_runtime = PathBuf::from("runtime")
            .join("legacy-isolation")
            .join(&category);
        let exclusion_relative = category_runtime.join("foreign-static.txt");
        let exclusion_reference =
            relative_path_key(&exclusion_relative).ok_or_else(|| IsolationError::UnsafePath {
                operation: "build read-only generated exclusion reference",
                path: exclusion_relative.clone(),
            })?;
        let generated = inject_exclusion_overlay(&selection.source_content, &exclusion_reference)
            .map_err(|source| IsolationError::Overlay {
            category: category.clone(),
            config_name: selection.config_name.clone(),
            source,
        })?;
        if generated.is_some() {
            validate_read_only_output_path(canonical_base, &exclusion_relative)?;
            validate_read_only_output_path(
                canonical_base,
                &category_runtime.join("effective.conf"),
            )?;
        }
    }
    Ok(())
}

fn scan_category_inventory(
    canonical_base: &Path,
    category_dir: &Path,
    category: &str,
    snapshot_bytes: &mut usize,
) -> Result<CategoryInventory, IsolationError> {
    let entries = fs::read_dir(category_dir).map_err(|source| IsolationError::Io {
        operation: "scan Legacy category configs",
        path: category_dir.to_path_buf(),
        source,
    })?;
    let mut configs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| IsolationError::Io {
            operation: "read Legacy category config entry",
            path: category_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("conf") {
            continue;
        }
        let config_name = entry
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| IsolationError::InvalidSelection {
                category: category.to_string(),
                config_name: entry.file_name().to_string_lossy().into_owned(),
            })?;
        if !is_safe_config_name(&config_name) {
            return Err(IsolationError::InvalidSelection {
                category: category.to_string(),
                config_name,
            });
        }
        configs.push((config_name, path));
        if configs.len() > MAX_CONFIGS_PER_CATEGORY {
            return Err(IsolationError::TooManyConfigs {
                category: category.to_string(),
                limit: MAX_CONFIGS_PER_CATEGORY,
            });
        }
    }
    configs.sort_by(|left, right| left.0.cmp(&right.0));

    let mut static_references = BTreeSet::new();
    let mut auto_references = BTreeSet::new();
    for (config_name, requested) in configs {
        let resolved = canonical_regular_file(&requested).map_err(|source| IsolationError::Io {
            operation: "open Legacy category config",
            path: requested,
            source,
        })?;
        require_descendant(category_dir, &resolved, "resolve Legacy category config")?;
        let bytes = read_bounded(&resolved, MAX_CONFIG_BYTES).map_err(|source| {
            map_bounded_read_error(
                "read Legacy category config",
                &resolved,
                source,
                MAX_CONFIG_BYTES,
            )
        })?;
        add_snapshot_bytes(snapshot_bytes, bytes.len())?;
        let content = std::str::from_utf8(&bytes)
            .map_err(|_| IsolationError::InvalidUtf8 { path: resolved })?;
        let parsed = parse_legacy_config(content).map_err(|source| IsolationError::Config {
            category: category.to_string(),
            config_name: config_name.clone(),
            source,
        })?;
        if parsed.hostlists.len() > MAX_HOSTLISTS_PER_CONFIG {
            return Err(IsolationError::TooManyHostlists {
                category: category.to_string(),
                config_name,
                limit: MAX_HOSTLISTS_PER_CONFIG,
            });
        }
        for reference in parsed.hostlists {
            match reference.kind {
                HostlistKind::Include => {
                    static_references.insert(reference.reference);
                }
                HostlistKind::AutoInclude => {
                    let relative = reference_path_for_root(&reference.reference, "autohosts")
                        .ok_or_else(|| IsolationError::UnsafePath {
                            operation: "resolve category auto-hostlist",
                            path: PathBuf::from(&reference.reference),
                        })?;
                    auto_references.insert(relative);
                    if auto_references.len() > 1 {
                        return Err(IsolationError::Plan(
                            MigrationPlanError::MultipleDestinationAutoHostlists {
                                category: category.to_string(),
                            },
                        ));
                    }
                }
                HostlistKind::Exclude => {}
            }
        }
    }

    let mut domains = BTreeSet::new();
    for reference in static_references {
        let relative = reference_path_for_root(&reference, "lists").ok_or_else(|| {
            IsolationError::UnsafePath {
                operation: "resolve static Legacy hostlist",
                path: PathBuf::from(&reference),
            }
        })?;
        let requested = canonical_base.join(&relative);
        let resolved = canonical_regular_file(&requested).map_err(|source| IsolationError::Io {
            operation: "open static Legacy hostlist",
            path: requested,
            source,
        })?;
        require_descendant(canonical_base, &resolved, "resolve static Legacy hostlist")?;
        let bytes = read_bounded(&resolved, MAX_HOSTLIST_BYTES).map_err(|source| {
            map_bounded_read_error(
                "read static Legacy hostlist",
                &resolved,
                source,
                MAX_HOSTLIST_BYTES,
            )
        })?;
        add_snapshot_bytes(snapshot_bytes, bytes.len())?;
        let content = std::str::from_utf8(&bytes)
            .map_err(|_| IsolationError::InvalidUtf8 { path: resolved })?;
        domains.extend(parse_domains(content, category, |category, line| {
            IsolationError::Hostlist { category, line }
        })?);
    }
    Ok(CategoryInventory {
        static_domains: domains,
        auto_host_reference: auto_references.into_iter().next(),
    })
}

/// Detailed preparation API. Initial startup passes all active selections with
/// `migrate_existing=true`; scoped recovery passes the full tentative selection
/// with `false` and takes the target entry from the returned map.
pub(crate) fn prepare_launches(
    request: IsolationPrepareRequest<'_>,
) -> Result<IsolationPrepareResult, IsolationError> {
    prepare_launches_inner(request, &Materialization::All)
}

fn prepare_launches_inner(
    request: IsolationPrepareRequest<'_>,
    materialization: &Materialization,
) -> Result<IsolationPrepareResult, IsolationError> {
    if request.selections.is_empty() {
        return Err(IsolationError::NoSelections);
    }
    if request.selections.len() > MAX_SELECTIONS {
        return Err(IsolationError::TooManySelections {
            limit: MAX_SELECTIONS,
        });
    }
    let canonical_base =
        canonical_directory(request.base_dir).map_err(|source| IsolationError::Io {
            operation: "canonicalize isolation base directory",
            path: request.base_dir.to_path_buf(),
            source,
        })?;
    let mut unique_selections = BTreeMap::new();
    let mut selected_categories = BTreeSet::new();
    for selection in request.selections {
        let category = normalize_category(&selection.category).ok_or_else(|| {
            IsolationError::InvalidSelection {
                category: selection.category.clone(),
                config_name: selection.config_name.clone(),
            }
        })?;
        if !is_safe_config_name(&selection.config_name) {
            return Err(IsolationError::InvalidSelection {
                category,
                config_name: selection.config_name.clone(),
            });
        }
        if let Some(previous) =
            unique_selections.insert(category.clone(), selection.config_name.clone())
        {
            if previous != selection.config_name {
                return Err(IsolationError::ConflictingSelection { category });
            }
        }
        selected_categories.insert(category);
    }
    if let Materialization::One((category, config_name)) = materialization {
        if unique_selections.get(category) != Some(config_name) {
            return Err(IsolationError::TargetNotSelected {
                category: category.clone(),
                config_name: config_name.clone(),
            });
        }
    }

    let requested_runtime_root =
        rebase_requested_path(request.base_dir, &canonical_base, request.runtime_root)
            .unwrap_or_else(|| request.runtime_root.to_path_buf());
    let canonical_runtime_root = create_confined_directory(
        &canonical_base,
        &requested_runtime_root,
        "create Legacy isolation runtime directory",
    )
    .map_err(|(operation, path, source)| IsolationError::Io {
        operation,
        path,
        source,
    })?;

    let migration = if request.migrate_existing {
        let requested_backup_root =
            rebase_requested_path(request.base_dir, &canonical_base, request.backup_root)
                .unwrap_or_else(|| request.backup_root.to_path_buf());
        let canonical_backup_root = create_confined_directory(
            &canonical_base,
            &requested_backup_root,
            "create migration backup directory",
        )
        .map_err(|(operation, path, source)| IsolationError::Io {
            operation,
            path,
            source,
        })?;
        let recovered = recover_incomplete_migration(&canonical_base, &canonical_backup_root)?;
        let snapshots = load_auto_snapshots(&canonical_base, request.selections)?;
        let plan = plan_migration(request.static_ownership, &snapshots)?;
        let mut summary = execute_migration(
            &canonical_base,
            &canonical_backup_root,
            &plan,
            request.timestamp_ms,
        )?;
        summary.rollback_performed |= recovered;
        summary
    } else {
        MigrationSummary::default()
    };

    let mut prepared = Vec::with_capacity(request.selections.len());
    for selection in request.selections {
        let category = normalize_category(&selection.category).expect("selection was validated");
        if !materialization.includes(&category, &selection.config_name) {
            continue;
        }
        let source_path = canonical_regular_file(&selection.source_path).map_err(|source| {
            IsolationError::Io {
                operation: "open immutable source Legacy config",
                path: selection.source_path.clone(),
                source,
            }
        })?;
        require_descendant(
            &canonical_base,
            &source_path,
            "resolve immutable source Legacy config",
        )?;
        let current = read_bounded(&source_path, MAX_CONFIG_BYTES).map_err(|source| {
            map_bounded_read_error(
                "verify immutable source Legacy config",
                &source_path,
                source,
                MAX_CONFIG_BYTES,
            )
        })?;
        if current != selection.source_content.as_bytes() {
            return Err(IsolationError::Io {
                operation: "verify immutable source Legacy config",
                path: source_path,
                source: io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source config changed during isolation preparation",
                ),
            });
        }

        let exclusions = foreign_only_exclusions(
            &selection.category,
            &selected_categories,
            request.static_ownership,
        )?;
        if exclusions.is_empty() {
            prepared.push(PreparedOverlay::Source {
                key: (category.clone(), selection.config_name.clone()),
                path: source_path,
            });
            continue;
        }
        let exclusion_content = render_bounded_domains(
            &exclusions,
            &category,
            HostlistLimits {
                max_hosts: MAX_HOSTS_PER_FILE,
                max_bytes: MAX_HOSTLIST_BYTES,
            },
        )?;
        let category_runtime = create_confined_directory(
            &canonical_base,
            &canonical_runtime_root.join(&category),
            "create category isolation runtime directory",
        )
        .map_err(|(operation, path, source)| IsolationError::Io {
            operation,
            path,
            source,
        })?;
        let exclusion_path = category_runtime.join("foreign-static.txt");
        let relative_exclusion = exclusion_path
            .strip_prefix(&canonical_base)
            .ok()
            .and_then(relative_path_key)
            .ok_or_else(|| IsolationError::UnsafePath {
                operation: "build generated exclusion reference",
                path: exclusion_path.clone(),
            })?;
        let Some(effective_content) =
            inject_exclusion_overlay(&selection.source_content, &relative_exclusion).map_err(
                |source| IsolationError::Overlay {
                    category: selection.category.clone(),
                    config_name: selection.config_name.clone(),
                    source,
                },
            )?
        else {
            prepared.push(PreparedOverlay::Source {
                key: (category.clone(), selection.config_name.clone()),
                path: source_path,
            });
            continue;
        };
        prepared.push(PreparedOverlay::Generated {
            key: (category, selection.config_name.clone()),
            exclusion_path,
            exclusion_content,
            effective_path: category_runtime.join("effective.conf"),
            effective_content: effective_content.into_bytes(),
        });
    }

    let mut effective_paths = BTreeMap::new();
    for overlay in prepared {
        match overlay {
            PreparedOverlay::Source { key, path } => {
                effective_paths.insert(key, path);
            }
            PreparedOverlay::Generated {
                key,
                exclusion_path,
                exclusion_content,
                effective_path,
                effective_content,
            } => {
                write_atomic(&exclusion_path, &exclusion_content).map_err(|source| {
                    IsolationError::Io {
                        operation: "write generated foreign exclusion list",
                        path: exclusion_path,
                        source,
                    }
                })?;
                write_atomic(&effective_path, &effective_content).map_err(|source| {
                    IsolationError::Io {
                        operation: "write generated effective Legacy config",
                        path: effective_path.clone(),
                        source,
                    }
                })?;
                effective_paths.insert(key, effective_path);
            }
        }
    }

    Ok(IsolationPrepareResult {
        effective_paths,
        migration,
    })
}

enum PreparedOverlay {
    Source {
        key: (String, String),
        path: PathBuf,
    },
    Generated {
        key: (String, String),
        exclusion_path: PathBuf,
        exclusion_content: Vec<u8>,
        effective_path: PathBuf,
        effective_content: Vec<u8>,
    },
}

fn load_auto_snapshots(
    canonical_base: &Path,
    selections: &[SelectedLegacyConfig],
) -> Result<Vec<AutoHostSnapshot>, IsolationError> {
    let auto_root = create_confined_directory(
        canonical_base,
        &canonical_base.join("autohosts"),
        "create Legacy auto-hostlist directory",
    )
    .map_err(|(operation, path, source)| IsolationError::Io {
        operation,
        path,
        source,
    })?;
    let references = collect_auto_references(selections)?;

    let mut total = 0usize;
    let mut snapshots = Vec::with_capacity(references.len());
    for (_, (category, relative)) in references {
        let requested =
            safe_join(canonical_base, &relative).ok_or_else(|| IsolationError::UnsafePath {
                operation: "resolve Legacy auto-hostlist",
                path: relative.clone(),
            })?;
        let parent = requested
            .parent()
            .ok_or_else(|| IsolationError::UnsafePath {
                operation: "resolve Legacy auto-hostlist parent",
                path: requested.clone(),
            })?;
        let canonical_parent =
            create_confined_directory(&auto_root, parent, "create Legacy auto-hostlist parent")
                .map_err(|(operation, path, source)| IsolationError::Io {
                    operation,
                    path,
                    source,
                })?;
        require_descendant(
            &auto_root,
            &canonical_parent,
            "resolve Legacy auto-hostlist parent",
        )?;
        let destination = canonical_parent.join(requested.file_name().ok_or_else(|| {
            IsolationError::UnsafePath {
                operation: "resolve Legacy auto-hostlist filename",
                path: requested.clone(),
            }
        })?);
        let original =
            read_optional_bounded(&destination, MAX_HOSTLIST_BYTES).map_err(|source| {
                map_bounded_read_error(
                    "read Legacy auto-hostlist",
                    &destination,
                    source,
                    MAX_HOSTLIST_BYTES,
                )
            })?;
        if let Some(bytes) = &original {
            add_snapshot_bytes(&mut total, bytes.len())?;
        }
        let relative_path = destination
            .strip_prefix(canonical_base)
            .map(Path::to_path_buf)
            .map_err(|_| IsolationError::UnsafePath {
                operation: "confine Legacy auto-hostlist",
                path: destination,
            })?;
        snapshots.push(AutoHostSnapshot {
            category,
            relative_path,
            original,
        });
    }
    Ok(snapshots)
}

fn collect_auto_references(
    selections: &[SelectedLegacyConfig],
) -> Result<BTreeMap<String, (String, PathBuf)>, IsolationError> {
    let mut references = BTreeMap::<String, (String, PathBuf)>::new();
    for selection in selections {
        let Some(relative) = selection.auto_host_reference.clone() else {
            continue;
        };
        let key = relative_path_key(&relative).ok_or_else(|| IsolationError::UnsafePath {
            operation: "resolve Legacy auto-hostlist",
            path: relative.clone(),
        })?;
        let category = normalize_category(&selection.category).ok_or_else(|| {
            IsolationError::InvalidSelection {
                category: selection.category.clone(),
                config_name: selection.config_name.clone(),
            }
        })?;
        if let Some((owner, _)) = references.get(&key) {
            if owner != &category {
                return Err(IsolationError::Plan(MigrationPlanError::DuplicatePath));
            }
        } else {
            references.insert(key, (category, relative));
        }
    }
    if references.len() > MAX_AUTO_FILES {
        return Err(IsolationError::Plan(MigrationPlanError::TooManyFiles {
            limit: MAX_AUTO_FILES,
        }));
    }
    Ok(references)
}

fn load_auto_snapshots_read_only(
    canonical_base: &Path,
    selections: &[SelectedLegacyConfig],
) -> Result<Vec<AutoHostSnapshot>, IsolationError> {
    let references = collect_auto_references(selections)?;
    let requested_auto_root = canonical_base.join("autohosts");
    let auto_root = match canonical_directory(&requested_auto_root) {
        Ok(path) => {
            require_descendant(
                canonical_base,
                &path,
                "resolve read-only auto-hostlist root",
            )?;
            Some(path)
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(IsolationError::Io {
                operation: "open read-only auto-hostlist root",
                path: requested_auto_root,
                source,
            });
        }
    };

    let mut total = 0usize;
    let mut snapshots = Vec::with_capacity(references.len());
    for (_, (category, relative_path)) in references {
        let original = if let Some(auto_root) = auto_root.as_deref() {
            let requested = safe_join(canonical_base, &relative_path).ok_or_else(|| {
                IsolationError::UnsafePath {
                    operation: "resolve read-only auto-hostlist",
                    path: relative_path.clone(),
                }
            })?;
            let parent = requested
                .parent()
                .ok_or_else(|| IsolationError::UnsafePath {
                    operation: "resolve read-only auto-hostlist parent",
                    path: requested.clone(),
                })?;
            match canonical_directory(parent) {
                Ok(parent) => {
                    require_descendant(
                        auto_root,
                        &parent,
                        "resolve read-only auto-hostlist parent",
                    )?;
                    let destination = parent.join(requested.file_name().ok_or_else(|| {
                        IsolationError::UnsafePath {
                            operation: "resolve read-only auto-hostlist filename",
                            path: requested.clone(),
                        }
                    })?);
                    read_optional_bounded(&destination, MAX_HOSTLIST_BYTES).map_err(|source| {
                        map_bounded_read_error(
                            "read Legacy auto-hostlist during preflight",
                            &destination,
                            source,
                            MAX_HOSTLIST_BYTES,
                        )
                    })?
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => None,
                Err(source) => {
                    return Err(IsolationError::Io {
                        operation: "open read-only auto-hostlist parent",
                        path: parent.to_path_buf(),
                        source,
                    });
                }
            }
        } else {
            None
        };
        if let Some(bytes) = &original {
            add_snapshot_bytes(&mut total, bytes.len())?;
        }
        snapshots.push(AutoHostSnapshot {
            category,
            relative_path,
            original,
        });
    }
    Ok(snapshots)
}

fn validate_selection_pairs(
    selections: &[(String, String)],
) -> Result<BTreeMap<String, String>, IsolationError> {
    if selections.is_empty() {
        return Err(IsolationError::NoSelections);
    }
    if selections.len() > MAX_SELECTIONS {
        return Err(IsolationError::TooManySelections {
            limit: MAX_SELECTIONS,
        });
    }
    let mut validated = BTreeMap::new();
    for (raw_category, config_name) in selections {
        let category =
            normalize_category(raw_category).ok_or_else(|| IsolationError::InvalidSelection {
                category: raw_category.clone(),
                config_name: config_name.clone(),
            })?;
        if !is_safe_config_name(config_name) {
            return Err(IsolationError::InvalidSelection {
                category,
                config_name: config_name.clone(),
            });
        }
        if let Some(previous) = validated.insert(category.clone(), config_name.clone()) {
            if previous != *config_name {
                return Err(IsolationError::ConflictingSelection { category });
            }
        }
    }
    Ok(validated)
}

fn is_safe_config_name(value: &str) -> bool {
    is_safe_component(value)
        && Path::new(value)
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("conf")
}

fn is_safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.ends_with([' ', '.'])
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn reference_path_for_root(reference: &str, required_root: &str) -> Option<PathBuf> {
    let normalized = normalize_reference(reference)?;
    let mut parts = normalized.split('/');
    if parts.next()? != required_root {
        return None;
    }
    let remaining = parts.collect::<Vec<_>>();
    if remaining.is_empty() || remaining.iter().any(|part| !is_safe_component(part)) {
        return None;
    }
    let mut path = PathBuf::from(required_root);
    for part in remaining {
        path.push(part);
    }
    (path.extension().and_then(|extension| extension.to_str()) == Some("txt")).then_some(path)
}

fn is_confined_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn normalize_relative_path(path: &Path) -> Option<PathBuf> {
    if !is_confined_relative(path) {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return None;
        };
        let value = value.to_str()?;
        if !is_safe_component(value) {
            return None;
        }
        normalized.push(value);
    }
    Some(normalized)
}

fn relative_path_key(path: &Path) -> Option<String> {
    let normalized = normalize_relative_path(path)?;
    Some(
        normalized
            .components()
            .map(|component| component.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()?
            .join("/")
            .to_ascii_lowercase(),
    )
}

fn safe_join(root: &Path, relative: &Path) -> Option<PathBuf> {
    normalize_relative_path(relative).map(|relative| root.join(relative))
}

fn rebase_requested_path(
    original_base: &Path,
    canonical_base: &Path,
    requested: &Path,
) -> Option<PathBuf> {
    if !requested.is_absolute() {
        return normalize_relative_path(requested).map(|relative| canonical_base.join(relative));
    }
    if requested.starts_with(canonical_base) {
        return Some(requested.to_path_buf());
    }
    let original_base = if original_base.is_absolute() {
        original_base.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(original_base)
    };
    let relative = requested.strip_prefix(original_base).ok()?;
    normalize_relative_path_or_empty(relative).map(|relative| canonical_base.join(relative))
}

fn validate_read_only_output_path(
    canonical_base: &Path,
    relative_path: &Path,
) -> Result<PathBuf, IsolationError> {
    let normalized =
        normalize_relative_path(relative_path).ok_or_else(|| IsolationError::UnsafePath {
            operation: "validate generated preflight path",
            path: relative_path.to_path_buf(),
        })?;
    let file_name = normalized
        .file_name()
        .ok_or_else(|| IsolationError::UnsafePath {
            operation: "validate generated preflight filename",
            path: normalized.clone(),
        })?
        .to_os_string();
    let parent = normalized
        .parent()
        .ok_or_else(|| IsolationError::UnsafePath {
            operation: "validate generated preflight parent",
            path: normalized.clone(),
        })?;
    let mut current = canonical_base.to_path_buf();
    for component in parent.components() {
        let next = current.join(component.as_os_str());
        match fs::symlink_metadata(&next) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(IsolationError::UnsafePath {
                    operation: "validate generated preflight directory",
                    path: next,
                });
            }
            Ok(_) => {
                current = fs::canonicalize(&next).map_err(|source| IsolationError::Io {
                    operation: "canonicalize generated preflight directory",
                    path: next,
                    source,
                })?;
                require_descendant(
                    canonical_base,
                    &current,
                    "confine generated preflight directory",
                )?;
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(canonical_base.join(normalized));
            }
            Err(source) => {
                return Err(IsolationError::Io {
                    operation: "inspect generated preflight directory",
                    path: next,
                    source,
                });
            }
        }
    }
    let output = current.join(file_name);
    match fs::symlink_metadata(&output) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(IsolationError::UnsafePath {
                operation: "validate generated preflight output",
                path: output,
            })
        }
        Ok(_) => Ok(output),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(output),
        Err(source) => Err(IsolationError::Io {
            operation: "inspect generated preflight output",
            path: output,
            source,
        }),
    }
}

fn validate_read_only_directory_path(
    canonical_base: &Path,
    relative_path: &Path,
) -> Result<PathBuf, IsolationError> {
    let normalized =
        normalize_relative_path(relative_path).ok_or_else(|| IsolationError::UnsafePath {
            operation: "validate preflight directory path",
            path: relative_path.to_path_buf(),
        })?;
    let mut current = canonical_base.to_path_buf();
    for component in normalized.components() {
        let next = current.join(component.as_os_str());
        match fs::symlink_metadata(&next) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(IsolationError::UnsafePath {
                    operation: "validate preflight directory",
                    path: next,
                });
            }
            Ok(_) => {
                current = fs::canonicalize(&next).map_err(|source| IsolationError::Io {
                    operation: "canonicalize preflight directory",
                    path: next,
                    source,
                })?;
                require_descendant(canonical_base, &current, "confine preflight directory")?;
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(canonical_base.join(normalized));
            }
            Err(source) => {
                return Err(IsolationError::Io {
                    operation: "inspect preflight directory",
                    path: next,
                    source,
                });
            }
        }
    }
    Ok(current)
}

fn canonical_directory(path: &Path) -> io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "symbolic-link directories are not accepted at this boundary",
        ));
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "path is not a directory",
        ));
    }
    fs::canonicalize(path)
}

fn canonical_regular_file(path: &Path) -> io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "symbolic-link files are not accepted at this boundary",
        ));
    }
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    fs::canonicalize(path)
}

fn require_descendant(
    root: &Path,
    resolved: &Path,
    operation: &'static str,
) -> Result<(), IsolationError> {
    if resolved.starts_with(root) {
        Ok(())
    } else {
        Err(IsolationError::UnsafePath {
            operation,
            path: resolved.to_path_buf(),
        })
    }
}

fn create_confined_directory(
    canonical_boundary: &Path,
    requested: &Path,
    operation: &'static str,
) -> Result<PathBuf, (&'static str, PathBuf, io::Error)> {
    let requested = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        canonical_boundary.join(requested)
    };
    let relative = requested.strip_prefix(canonical_boundary).map_err(|_| {
        (
            operation,
            requested.clone(),
            io::Error::new(io::ErrorKind::PermissionDenied, "path escapes its boundary"),
        )
    })?;
    let relative = normalize_relative_path_or_empty(relative).ok_or_else(|| {
        (
            operation,
            requested.clone(),
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path contains unsafe components",
            ),
        )
    })?;
    let mut current = canonical_boundary.to_path_buf();
    for component in relative.components() {
        let next = current.join(component.as_os_str());
        match fs::symlink_metadata(&next) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err((
                        operation,
                        next,
                        io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "directory component is not a real directory",
                        ),
                    ));
                }
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&next).map_err(|source| (operation, next.clone(), source))?;
            }
            Err(source) => return Err((operation, next, source)),
        }
        current = fs::canonicalize(&next).map_err(|source| (operation, next.clone(), source))?;
        if !current.starts_with(canonical_boundary) {
            return Err((
                operation,
                current,
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "directory escapes its boundary",
                ),
            ));
        }
    }
    Ok(current)
}

fn normalize_relative_path_or_empty(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return Some(PathBuf::new());
    }
    normalize_relative_path(path)
}

fn validate_destination(
    canonical_base: &Path,
    relative_path: &Path,
) -> Result<PathBuf, (&'static str, PathBuf, io::Error)> {
    let normalized = normalize_relative_path(relative_path).ok_or_else(|| {
        (
            "resolve confined auto-hostlist destination",
            relative_path.to_path_buf(),
            io::Error::new(io::ErrorKind::PermissionDenied, "unsafe relative path"),
        )
    })?;
    if normalized
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        != Some("autohosts")
    {
        return Err((
            "resolve confined auto-hostlist destination",
            relative_path.to_path_buf(),
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "migration destination is outside autohosts",
            ),
        ));
    }
    let requested = canonical_base.join(normalized);
    let parent = requested.parent().ok_or_else(|| {
        (
            "resolve auto-hostlist parent",
            requested.clone(),
            io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"),
        )
    })?;
    let canonical_parent =
        create_confined_directory(canonical_base, parent, "resolve auto-hostlist parent")?;
    let file_name = requested.file_name().ok_or_else(|| {
        (
            "resolve auto-hostlist filename",
            requested.clone(),
            io::Error::new(io::ErrorKind::InvalidInput, "destination has no filename"),
        )
    })?;
    let destination = canonical_parent.join(file_name);
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err((
            "resolve auto-hostlist destination",
            destination,
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "destination is not a real regular file",
            ),
        )),
        Ok(_) => Ok(destination),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(destination),
        Err(source) => Err(("resolve auto-hostlist destination", destination, source)),
    }
}

fn read_optional_bounded(path: &Path, limit: usize) -> io::Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "path is not a real regular file",
                ));
            }
            read_bounded(path, limit).map(Some)
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(source),
    }
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > limit as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds isolation size limit",
        ));
    }
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len().min(limit as u64) as usize);
    Read::by_ref(&mut file)
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds isolation size limit",
        ));
    }
    Ok(bytes)
}

fn map_bounded_read_error(
    operation: &'static str,
    path: &Path,
    source: io::Error,
    limit: usize,
) -> IsolationError {
    if source.kind() == io::ErrorKind::InvalidData && source.to_string().contains("size limit") {
        IsolationError::FileTooLarge {
            path: path.to_path_buf(),
            limit,
        }
    } else {
        IsolationError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}

fn add_snapshot_bytes(total: &mut usize, amount: usize) -> Result<(), IsolationError> {
    *total = total
        .checked_add(amount)
        .ok_or(IsolationError::SnapshotTooLarge {
            limit: MAX_TOTAL_SNAPSHOT_BYTES,
        })?;
    if *total > MAX_TOTAL_SNAPSHOT_BYTES {
        Err(IsolationError::SnapshotTooLarge {
            limit: MAX_TOTAL_SNAPSHOT_BYTES,
        })
    } else {
        Ok(())
    }
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "destination filename is not UTF-8",
            )
        })?;
    let mut temporary = None;
    for _ in 0..TEMP_CREATE_ATTEMPTS {
        let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{file_name}.isolation.tmp.{}.{sequence}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(source),
        }
    }
    let Some((temporary_path, mut file)) = temporary else {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique isolation temporary file",
        ));
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        atomic_replace_with_retry(&temporary_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn atomic_replace_with_retry(source: &Path, destination: &Path) -> io::Result<()> {
    for attempt in 0..REPLACE_ATTEMPTS {
        match atomic_replace(source, destination) {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt + 1 < REPLACE_ATTEMPTS
                    && (matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock
                    ) || matches!(error.raw_os_error(), Some(5 | 32))) =>
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "obsession-autohost-isolation-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ownership(entries: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        entries
            .iter()
            .map(|(category, domains)| {
                (
                    (*category).to_string(),
                    domains.iter().map(|domain| (*domain).to_string()).collect(),
                )
            })
            .collect()
    }

    fn snapshot(category: &str, file: &str, content: Option<&str>) -> AutoHostSnapshot {
        AutoHostSnapshot {
            category: category.to_string(),
            relative_path: PathBuf::from("autohosts").join(file),
            original: content.map(|content| content.as_bytes().to_vec()),
        }
    }

    fn after_for<'a>(plan: &'a MigrationPlan, file: &str) -> &'a [u8] {
        &plan
            .changes
            .iter()
            .find(|change| change.relative_path.ends_with(file))
            .unwrap()
            .after
    }

    #[test]
    fn migration_applies_static_ownership_rules_and_normalizes_outputs() {
        let static_ownership = ownership(&[
            ("youtube", &["youtube.com", "shared.example"]),
            ("discord", &["discord.com", "shared.example"]),
        ]);
        let snapshots = [
            snapshot(
                "youtube",
                "youtube.txt",
                Some(
                    "WWW.YOUTUBE.COM.\ncdn.discord.com\namb.shared.example\nmystery.example\nMYSTERY.EXAMPLE\n",
                ),
            ),
            snapshot("discord", "discord.txt", Some("discord.com\n")),
        ];

        let plan = plan_migration(&static_ownership, &snapshots).unwrap();

        assert_eq!(plan.summary.files_changed, 2);
        assert_eq!(plan.summary.hosts_moved, 1);
        assert_eq!(plan.summary.ambiguous_hosts_quarantined, 1);
        assert_eq!(plan.summary.unknown_hosts_retained, 1);
        assert_eq!(
            after_for(&plan, "youtube.txt"),
            b"mystery.example\nwww.youtube.com\n"
        );
        assert_eq!(
            after_for(&plan, "discord.txt"),
            b"cdn.discord.com\ndiscord.com\n"
        );
        assert!(!plan
            .changes
            .iter()
            .any(|change| String::from_utf8_lossy(&change.after).contains("shared.example")));
        let rendered = plan.summary.to_string();
        assert!(!rendered.contains("discord.com"));
        assert!(!rendered.contains("youtube.com"));
    }

    #[test]
    fn normalized_migration_is_idempotent() {
        let static_ownership =
            ownership(&[("youtube", &["youtube.com"]), ("discord", &["discord.com"])]);
        let first_snapshots = [
            snapshot(
                "youtube",
                "youtube.txt",
                Some("cdn.discord.com\nUNKNOWN.EXAMPLE\n"),
            ),
            snapshot("discord", "discord.txt", Some("discord.com\n")),
        ];
        let first = plan_migration(&static_ownership, &first_snapshots).unwrap();
        let second_snapshots = first_snapshots
            .iter()
            .map(|snapshot| {
                let replacement = first
                    .changes
                    .iter()
                    .find(|change| change.relative_path == snapshot.relative_path)
                    .map(|change| change.after.clone())
                    .or_else(|| snapshot.original.clone());
                AutoHostSnapshot {
                    category: snapshot.category.clone(),
                    relative_path: snapshot.relative_path.clone(),
                    original: replacement,
                }
            })
            .collect::<Vec<_>>();

        let second = plan_migration(&static_ownership, &second_snapshots).unwrap();

        assert!(second.is_empty());
        assert_eq!(second.summary.files_changed, 0);
        assert_eq!(second.summary.hosts_moved, 0);
        assert_eq!(second.summary.ambiguous_hosts_quarantined, 0);
    }

    #[test]
    fn moved_hosts_cannot_overflow_a_single_destination_output() {
        let static_ownership = ownership(&[
            ("source_a", &[]),
            ("source_b", &[]),
            ("destination", &["dest.example"]),
        ]);
        let snapshots = vec![
            snapshot(
                "source_a",
                "source-a.txt",
                Some("a.dest.example\nb.dest.example\n"),
            ),
            snapshot(
                "source_b",
                "source-b.txt",
                Some("c.dest.example\nd.dest.example\n"),
            ),
            snapshot("destination", "destination.txt", Some("")),
        ];
        let originals = snapshots.clone();

        assert!(matches!(
            plan_migration_with_limits(
                &static_ownership,
                &snapshots,
                HostlistLimits {
                    max_hosts: 3,
                    max_bytes: 1_024,
                },
            ),
            Err(MigrationPlanError::TooManyHosts { category, limit: 3 })
                if category == "destination"
        ));
        assert!(matches!(
            plan_migration_with_limits(
                &static_ownership,
                &snapshots,
                HostlistLimits {
                    max_hosts: 10,
                    max_bytes: 40,
                },
            ),
            Err(MigrationPlanError::HostlistTooLarge { category, limit: 40 })
                if category == "destination"
        ));
        assert_eq!(snapshots, originals);
    }

    #[test]
    fn deepest_foreign_suffix_beats_broad_source_suffix() {
        let static_ownership = ownership(&[
            ("source", &["example.com"]),
            ("foreign", &["video.example.com"]),
        ]);
        let snapshots = [
            snapshot("source", "source.txt", Some("cdn.video.example.com\n")),
            snapshot("foreign", "foreign.txt", Some("")),
        ];

        let plan = plan_migration(&static_ownership, &snapshots).unwrap();
        assert_eq!(plan.summary.hosts_moved, 1);
        assert_eq!(after_for(&plan, "source.txt"), b"");
        assert_eq!(after_for(&plan, "foreign.txt"), b"cdn.video.example.com\n");

        let selected = BTreeSet::from(["source".to_string(), "foreign".to_string()]);
        assert_eq!(
            foreign_only_exclusions("source", &selected, &static_ownership).unwrap(),
            BTreeSet::from(["video.example.com".to_string()])
        );
    }

    #[test]
    fn broad_foreign_suffix_does_not_mask_specific_source_ownership() {
        let static_ownership = ownership(&[
            ("source", &["video.example.com"]),
            ("foreign", &["example.com"]),
        ]);
        let selected = BTreeSet::from(["source".to_string(), "foreign".to_string()]);

        let exclusions = foreign_only_exclusions("source", &selected, &static_ownership).unwrap();

        assert!(!exclusions.contains("example.com"));
    }

    #[test]
    fn missing_mutable_destination_is_quarantined_and_multiple_destinations_fail_closed() {
        let static_ownership = ownership(&[("video", &["video.example"])]);
        let no_destination = [snapshot("other", "other.txt", Some("cdn.video.example\n"))];
        let plan = plan_migration(&static_ownership, &no_destination).unwrap();
        assert_eq!(after_for(&plan, "other.txt"), b"");
        assert_eq!(plan.summary.hosts_moved, 0);
        assert_eq!(plan.summary.ambiguous_hosts_quarantined, 1);

        let multiple = [
            snapshot("video", "first.txt", Some("video.example\n")),
            snapshot("video", "second.txt", Some("")),
        ];
        assert!(matches!(
            plan_migration(&static_ownership, &multiple),
            Err(MigrationPlanError::MultipleDestinationAutoHostlists { .. })
        ));
    }

    #[test]
    fn preflight_rejects_blocked_runtime_and_backup_roots_without_writes() {
        for blocked_root in ["runtime/legacy-isolation", "legacy-isolation-backups"] {
            let directory = TestDirectory::new("blocked-preflight-root");
            let base = directory.path();
            for path in ["configs/video", "lists", "autohosts"] {
                fs::create_dir_all(base.join(path)).unwrap();
            }
            let config = "--wf-tcp=443 --hostlist=lists/video.txt --hostlist-auto=autohosts/video.txt --dpi-desync=fake";
            fs::write(base.join("configs/video/selected.conf"), config).unwrap();
            fs::write(base.join("lists/video.txt"), "video.example\n").unwrap();
            fs::write(base.join("autohosts/video.txt"), "cdn.video.example\n").unwrap();
            let blocked = base.join(blocked_root);
            fs::create_dir_all(blocked.parent().unwrap()).unwrap();
            fs::write(&blocked, "not a directory").unwrap();
            let selections = vec![("video".to_string(), "selected.conf".to_string())];

            assert!(matches!(
                preflight_launches_from_disk(base, &selections),
                Err(IsolationError::UnsafePath { .. })
            ));
            assert_eq!(
                fs::read_to_string(base.join("configs/video/selected.conf")).unwrap(),
                config
            );
            assert_eq!(
                fs::read_to_string(base.join("autohosts/video.txt")).unwrap(),
                "cdn.video.example\n"
            );
        }
    }

    #[test]
    fn overlay_injects_once_per_auto_profile_and_preserves_source_text() {
        let source = "# leading comment\r\n\
            --wf-tcp=443\r\n\
            --filter-tcp=443 --hostlist-auto=\"autohosts\\video.txt\" --dpi-desync=fake # first\r\n\
            --new --filter-tcp=443 --hostlist-auto autohosts/video.txt --hostlist-exclude=\"lists\\existing.txt\" --new\r\n\
            --filter-tcp=80 --hostlist=lists/video.txt # no auto\r\n";
        let original = source.to_string();
        let reference = "runtime/legacy-isolation/video/foreign-static.txt";

        let effective = inject_exclusion_overlay(source, reference)
            .unwrap()
            .expect("config contains auto profiles");

        let injected =
            "--hostlist-exclude=\"runtime\\legacy-isolation\\video\\foreign-static.txt\"";
        assert_eq!(effective.matches(injected).count(), 2);
        assert!(effective.contains("--hostlist-exclude=\"lists\\existing.txt\""));
        assert!(effective.contains("# first\r\n"));
        assert_eq!(source, original);
        assert_eq!(
            inject_exclusion_overlay(&effective, reference).unwrap(),
            Some(effective)
        );
    }

    #[test]
    fn overlay_handles_no_auto_existing_exact_and_invalid_syntax() {
        assert_eq!(
            inject_exclusion_overlay(
                "--wf-tcp=443 --hostlist=lists/video.txt",
                "runtime/legacy-isolation/video/foreign-static.txt"
            )
            .unwrap(),
            None
        );
        let existing = "--wf-tcp=443 --hostlist-auto=autohosts/video.txt --hostlist-exclude=runtime/legacy-isolation/video/foreign-static.txt";
        assert_eq!(
            inject_exclusion_overlay(
                existing,
                "runtime/legacy-isolation/video/foreign-static.txt"
            )
            .unwrap(),
            Some(existing.to_string())
        );
        assert!(matches!(
            inject_exclusion_overlay(
                "--wf-tcp=443 --hostlist-auto=\"unterminated",
                "runtime/legacy-isolation/video/foreign-static.txt"
            ),
            Err(OverlayError::Config(
                ConfigParseError::UnterminatedQuote { .. }
            ))
        ));
        assert!(matches!(
            inject_exclusion_overlay(
                "--wf-tcp=443 --hostlist-auto=autohosts/video.txt",
                "../outside.txt"
            ),
            Err(OverlayError::InvalidExclusionReference)
        ));
    }

    fn manual_plan(changes: Vec<MigrationFileChange>) -> MigrationPlan {
        MigrationPlan {
            summary: MigrationSummary {
                files_changed: changes.len(),
                ..MigrationSummary::default()
            },
            changes,
        }
    }

    #[test]
    fn executor_rolls_back_all_applied_files_after_simulated_failure() {
        let directory = TestDirectory::new("rollback");
        let auto = directory.path().join("autohosts");
        fs::create_dir(&auto).unwrap();
        fs::write(auto.join("one.txt"), b"one.example\n").unwrap();
        fs::write(auto.join("two.txt"), b"two.example\n").unwrap();
        let plan = manual_plan(vec![
            MigrationFileChange {
                category: "one".into(),
                relative_path: PathBuf::from("autohosts/one.txt"),
                before: Some(b"one.example\n".to_vec()),
                after: b"changed-one.example\n".to_vec(),
            },
            MigrationFileChange {
                category: "two".into(),
                relative_path: PathBuf::from("autohosts/two.txt"),
                before: Some(b"two.example\n".to_vec()),
                after: b"changed-two.example\n".to_vec(),
            },
        ]);

        let error = execute_migration_inner(
            directory.path(),
            &directory.path().join("legacy-isolation-backups"),
            &plan,
            1,
            Some(1),
        )
        .unwrap_err();

        assert!(error.summary().rollback_performed);
        assert!(!error.rollback_failed());
        assert_eq!(fs::read(auto.join("one.txt")).unwrap(), b"one.example\n");
        assert_eq!(fs::read(auto.join("two.txt")).unwrap(), b"two.example\n");
    }

    #[test]
    fn next_start_restores_an_incomplete_multi_file_snapshot_before_replanning() {
        let directory = TestDirectory::new("crash-recovery");
        let auto = directory.path().join("autohosts");
        fs::create_dir(&auto).unwrap();
        fs::write(auto.join("one.txt"), b"one.example\n").unwrap();
        fs::write(auto.join("two.txt"), b"two.example\n").unwrap();
        let plan = manual_plan(vec![
            MigrationFileChange {
                category: "one".into(),
                relative_path: PathBuf::from("autohosts/one.txt"),
                before: Some(b"one.example\n".to_vec()),
                after: b"changed-one.example\n".to_vec(),
            },
            MigrationFileChange {
                category: "two".into(),
                relative_path: PathBuf::from("autohosts/two.txt"),
                before: Some(b"two.example\n".to_vec()),
                after: b"changed-two.example\n".to_vec(),
            },
        ]);
        let canonical_base = canonical_directory(directory.path()).unwrap();
        let backup_root = create_confined_directory(
            &canonical_base,
            &canonical_base.join("legacy-isolation-backups"),
            "test backup root",
        )
        .unwrap();
        let _snapshot = create_backup_snapshot(&canonical_base, &backup_root, &plan, 10).unwrap();
        write_atomic(&auto.join("one.txt"), b"changed-one.example\n").unwrap();

        assert!(recover_incomplete_migration(&canonical_base, &backup_root).unwrap());
        assert_eq!(fs::read(auto.join("one.txt")).unwrap(), b"one.example\n");
        assert_eq!(fs::read(auto.join("two.txt")).unwrap(), b"two.example\n");
        assert_eq!(fs::read_dir(backup_root).unwrap().count(), 0);
    }

    #[test]
    fn executor_bounds_backups_and_protects_current_snapshot_when_clock_moves_back() {
        let directory = TestDirectory::new("backup-bound");
        let auto = directory.path().join("autohosts");
        fs::create_dir(&auto).unwrap();
        let target = auto.join("video.txt");
        fs::write(&target, b"version0.example\n").unwrap();
        let backup_root = directory.path().join("legacy-isolation-backups");

        for (version, timestamp) in [100_u64, 200, 300, 1].into_iter().enumerate() {
            let version = version + 1;
            let before = format!("version{}.example\n", version - 1).into_bytes();
            let after = format!("version{version}.example\n").into_bytes();
            let plan = manual_plan(vec![MigrationFileChange {
                category: "video".into(),
                relative_path: PathBuf::from("autohosts/video.txt"),
                before: Some(before),
                after,
            }]);
            execute_migration(directory.path(), &backup_root, &plan, timestamp).unwrap();
        }

        let backups = fs::read_dir(&backup_root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), MAX_MIGRATION_BACKUPS);
        assert!(backups.iter().any(|entry| {
            let manifest = fs::read(entry.path().join("manifest.json")).unwrap();
            serde_json::from_slice::<BackupManifest>(&manifest)
                .is_ok_and(|manifest| manifest.timestamp_ms == 1)
        }));
    }

    #[test]
    fn planner_rejects_paths_outside_autohosts() {
        let result = plan_migration(
            &ownership(&[("video", &["video.example"])]),
            &[AutoHostSnapshot {
                category: "video".into(),
                relative_path: PathBuf::from("../outside.txt"),
                original: Some(b"video.example\n".to_vec()),
            }],
        );
        assert!(matches!(
            result,
            Err(MigrationPlanError::InvalidRelativePath)
        ));
    }

    #[test]
    fn disk_wrapper_scans_all_category_candidates_and_preserves_sources() {
        let directory = TestDirectory::new("disk-wrapper");
        let base = directory.path();
        for path in [
            "configs/youtube",
            "configs/discord",
            "configs/universal",
            "lists",
            "autohosts",
        ] {
            fs::create_dir_all(base.join(path)).unwrap();
        }
        let youtube_selected = "--wf-tcp=443\n\
            --filter-tcp=443 --hostlist=lists/youtube.txt --hostlist-auto=autohosts/youtube.txt --dpi-desync=fake --new\n\
            --filter-tcp=80 --hostlist=lists/youtube.txt --hostlist-auto=autohosts/youtube.txt --dpi-desync=fake --new\n";
        let youtube_candidate = "--wf-tcp=443 --hostlist=lists/youtube-candidate.txt --hostlist-auto=autohosts/youtube.txt --dpi-desync=fake";
        let discord_selected = "--wf-tcp=443 --hostlist=lists/discord.txt --hostlist-auto=autohosts/discord.txt --dpi-desync=fake";
        let universal_selected =
            "--wf-tcp=443 --hostlist=lists/universal-selected.txt --dpi-desync=fake";
        let universal_candidate = "--wf-tcp=443 --hostlist=lists/universal-candidate.txt --hostlist-auto=autohosts/universal-alt.txt --dpi-desync=fake";
        fs::write(base.join("configs/youtube/selected.conf"), youtube_selected).unwrap();
        fs::write(
            base.join("configs/youtube/candidate.conf"),
            youtube_candidate,
        )
        .unwrap();
        fs::write(base.join("configs/discord/selected.conf"), discord_selected).unwrap();
        fs::write(
            base.join("configs/universal/selected.conf"),
            universal_selected,
        )
        .unwrap();
        fs::write(
            base.join("configs/universal/candidate.conf"),
            universal_candidate,
        )
        .unwrap();
        fs::write(base.join("lists/youtube.txt"), "youtube.com\n").unwrap();
        fs::write(
            base.join("lists/youtube-candidate.txt"),
            "candidate-only.example\n",
        )
        .unwrap();
        fs::write(base.join("lists/discord.txt"), "discord.com\n").unwrap();
        fs::write(
            base.join("lists/universal-selected.txt"),
            "selected-universal.example\n",
        )
        .unwrap();
        fs::write(
            base.join("lists/universal-candidate.txt"),
            "candidate-universal.example\n",
        )
        .unwrap();
        fs::write(base.join("autohosts/youtube.txt"), "").unwrap();
        fs::write(base.join("autohosts/universal-alt.txt"), "").unwrap();
        fs::write(
            base.join("autohosts/discord.txt"),
            "www.youtube.com\ncdn.candidate-only.example\ncdn.candidate-universal.example\nunknown.example\n",
        )
        .unwrap();
        let selections = vec![
            ("youtube".to_string(), "selected.conf".to_string()),
            ("discord".to_string(), "selected.conf".to_string()),
            ("universal".to_string(), "selected.conf".to_string()),
        ];

        let discord_before_preflight = fs::read(base.join("autohosts/discord.txt")).unwrap();
        preflight_launches_from_disk(base, &selections).unwrap();
        assert_eq!(
            fs::read(base.join("autohosts/discord.txt")).unwrap(),
            discord_before_preflight
        );
        assert!(!base.join("runtime").exists());
        assert!(!base.join("legacy-isolation-backups").exists());

        let prepared = prepare_launches_from_disk(base, &selections, true).unwrap();

        assert_eq!(prepared.migration.hosts_moved, 3);
        assert_eq!(
            fs::read_to_string(base.join("autohosts/youtube.txt")).unwrap(),
            "cdn.candidate-only.example\nwww.youtube.com\n"
        );
        assert_eq!(
            fs::read_to_string(base.join("autohosts/discord.txt")).unwrap(),
            "unknown.example\n"
        );
        assert_eq!(
            fs::read_to_string(base.join("autohosts/universal-alt.txt")).unwrap(),
            "cdn.candidate-universal.example\n"
        );
        assert_eq!(
            fs::read_to_string(base.join("configs/youtube/selected.conf")).unwrap(),
            youtube_selected
        );
        assert_eq!(
            fs::read_to_string(base.join("configs/discord/selected.conf")).unwrap(),
            discord_selected
        );
        let youtube_effective = prepared
            .effective_paths
            .get(&("youtube".into(), "selected.conf".into()))
            .unwrap();
        let effective = fs::read_to_string(youtube_effective).unwrap();
        assert_eq!(effective.matches("foreign-static.txt").count(), 2);
        assert!(fs::read_to_string(
            base.join("runtime/legacy-isolation/youtube/foreign-static.txt")
        )
        .unwrap()
        .contains("discord.com"));

        let repeated = prepare_launches_from_disk(base, &selections, true).unwrap();
        assert_eq!(repeated.migration.files_changed, 0);
        assert_eq!(repeated.migration.hosts_moved, 0);

        let neighbor_effective = base.join("runtime/legacy-isolation/discord/effective.conf");
        let neighbor_exclusion = base.join("runtime/legacy-isolation/discord/foreign-static.txt");
        let target_effective = base.join("runtime/legacy-isolation/youtube/effective.conf");
        let neighbor_effective_before = fs::read(&neighbor_effective).unwrap();
        let neighbor_exclusion_before = fs::read(&neighbor_exclusion).unwrap();
        let target_effective_before = fs::read(&target_effective).unwrap();
        let neighbor_effective_mtime = fs::metadata(&neighbor_effective)
            .unwrap()
            .modified()
            .unwrap();
        let neighbor_exclusion_mtime = fs::metadata(&neighbor_exclusion)
            .unwrap()
            .modified()
            .unwrap();
        let target_effective_mtime = fs::metadata(&target_effective).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let tentative = vec![
            ("youtube".to_string(), "candidate.conf".to_string()),
            ("discord".to_string(), "selected.conf".to_string()),
            ("universal".to_string(), "selected.conf".to_string()),
        ];

        preflight_scoped_launch_from_disk(base, &tentative, "youtube", "candidate.conf").unwrap();
        assert_eq!(
            fs::read(&target_effective).unwrap(),
            target_effective_before
        );
        assert_eq!(
            fs::metadata(&target_effective).unwrap().modified().unwrap(),
            target_effective_mtime
        );
        assert_eq!(
            fs::metadata(&neighbor_effective)
                .unwrap()
                .modified()
                .unwrap(),
            neighbor_effective_mtime
        );

        let scoped =
            prepare_scoped_launch_from_disk(base, &tentative, "youtube", "candidate.conf").unwrap();

        assert!(fs::read_to_string(scoped)
            .unwrap()
            .contains("foreign-static.txt"));
        assert_eq!(
            fs::read(&neighbor_effective).unwrap(),
            neighbor_effective_before
        );
        assert_eq!(
            fs::read(&neighbor_exclusion).unwrap(),
            neighbor_exclusion_before
        );
        assert_eq!(
            fs::metadata(&neighbor_effective)
                .unwrap()
                .modified()
                .unwrap(),
            neighbor_effective_mtime
        );
        assert_eq!(
            fs::metadata(&neighbor_exclusion)
                .unwrap()
                .modified()
                .unwrap(),
            neighbor_exclusion_mtime
        );
    }
}
