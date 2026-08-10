//! Verification of the immutable machine-wide runtime layout.
//!
//! The service deliberately has no API accepting an installation root from a
//! client. It derives the only accepted location from `%ProgramFiles%` and
//! rejects path indirection before it looks at the resource manifest.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};

use obsession_runtime_protocol::{
    DpiCategory, DpiEngine, DpiStartRequest, Zapret2AdaptiveOverride, MAX_SELECTIONS,
    MAX_STRATEGY_ID_BYTES,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const PRODUCT_DIRECTORY: &str = "Obsession";
pub const RESOURCE_MANIFEST: &str = "runtime/runtime-manifest.json";

const RESOURCE_MANIFEST_SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENGINE_GROUPS: usize = 2;
const MAX_FILES_PER_ENGINE: usize = 1024;
const MAX_STRATEGIES_PER_ENGINE: usize = 512;
const MAX_STRATEGY_DEPENDENCIES: usize = 32;
const MAX_RESOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RESOURCE_PATH_BYTES: usize = 240;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedLayout {
    root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRuntimeCatalog {
    root: PathBuf,
    engines: Vec<VerifiedEngine>,
}

impl VerifiedRuntimeCatalog {
    pub fn engine(&self, engine: DpiEngine) -> Option<&VerifiedEngine> {
        self.engines.iter().find(|entry| entry.engine == engine)
    }

    pub fn resolve_strategy(
        &self,
        engine: DpiEngine,
        category: DpiCategory,
        strategy_id: &str,
    ) -> Option<&VerifiedStrategy> {
        self.engine(engine)?
            .strategies
            .iter()
            .find(|strategy| strategy.category == category && strategy.strategy_id == strategy_id)
    }

    /// Converts a typed IPC request into an owned plan containing only paths
    /// that were already hash-checked under the protected root.
    pub fn resolve_dpi_plan(
        &self,
        request: &DpiStartRequest,
    ) -> Result<VerifiedDpiPlan, LayoutError> {
        request
            .validate()
            .map_err(|_| LayoutError::InvalidDpiPlan)?;
        if request.selections.is_empty() || request.selections.len() > MAX_SELECTIONS {
            return Err(LayoutError::InvalidDpiPlan);
        }
        let engine = self
            .engine(request.engine)
            .ok_or(LayoutError::EngineNotAllowlisted(request.engine))?;
        let mut categories = BTreeSet::new();
        let mut strategies = Vec::with_capacity(request.selections.len());
        for selection in &request.selections {
            if !categories.insert(selection.category) {
                return Err(LayoutError::InvalidDpiPlan);
            }
            let strategy = self
                .resolve_strategy(request.engine, selection.category, &selection.strategy_id)
                .ok_or_else(|| LayoutError::StrategyNotAllowlisted {
                    engine: request.engine,
                    category: selection.category,
                    strategy_id: selection.strategy_id.clone(),
                })?;
            strategies.push(strategy.clone());
        }
        Ok(VerifiedDpiPlan {
            root: self.root.clone(),
            engine: request.engine,
            executable: engine.executable.clone(),
            strategies,
            zapret2_level: request.options.zapret2_level,
            legacy_reliability: request.options.legacy_reliability,
            zapret2_overrides: request.options.zapret2_overrides.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedDpiPlan {
    root: PathBuf,
    engine: DpiEngine,
    executable: VerifiedResource,
    strategies: Vec<VerifiedStrategy>,
    zapret2_level: u8,
    legacy_reliability: bool,
    zapret2_overrides: Vec<Zapret2AdaptiveOverride>,
}

impl VerifiedDpiPlan {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn engine(&self) -> DpiEngine {
        self.engine
    }

    pub fn executable(&self) -> &Path {
        self.executable.path()
    }

    pub fn executable_resource(&self) -> &VerifiedResource {
        &self.executable
    }

    pub fn strategies(&self) -> &[VerifiedStrategy] {
        &self.strategies
    }

    pub fn zapret2_level(&self) -> u8 {
        self.zapret2_level
    }

    pub fn legacy_reliability(&self) -> bool {
        self.legacy_reliability
    }

    pub fn zapret2_overrides(&self) -> &[Zapret2AdaptiveOverride] {
        &self.zapret2_overrides
    }

    /// Re-checks every executable/config/data resource immediately before a
    /// launch. The catalog is an allowlist, not a lifetime integrity lease.
    pub fn reverify(&self) -> Result<(), LayoutError> {
        let mut verified = BTreeSet::new();
        for resource in self.resources() {
            if verified.insert(resource.key()) {
                resource.verify()?;
            }
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"obsession/protected-dpi-plan/v1\0");
        digest.update(match self.engine {
            DpiEngine::Legacy => b"legacy".as_slice(),
            DpiEngine::Zapret2 => b"zapret2".as_slice(),
        });
        digest.update([self.zapret2_level, u8::from(self.legacy_reliability)]);
        if let Ok(overrides) = serde_json::to_vec(&self.zapret2_overrides) {
            digest.update((overrides.len() as u64).to_le_bytes());
            digest.update(overrides);
        } else {
            digest.update(u64::MAX.to_le_bytes());
        }
        for strategy in &self.strategies {
            digest.update([category_tag(strategy.category)]);
            digest.update(strategy.strategy_id.as_bytes());
            digest.update([0]);
        }
        let mut resources = self.resources();
        resources.sort_by_key(|resource| resource.key());
        resources.dedup_by_key(|resource| resource.key());
        for resource in resources {
            digest.update(resource.relative.to_string_lossy().as_bytes());
            digest.update([0]);
            digest.update(resource.size.to_le_bytes());
            digest.update(resource.sha256.as_bytes());
        }
        hex_digest(digest.finalize())
    }

    fn resources(&self) -> Vec<&VerifiedResource> {
        let mut resources = vec![&self.executable];
        for strategy in &self.strategies {
            resources.push(&strategy.artifact);
            resources.extend(strategy.dependencies.iter());
        }
        resources
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedEngine {
    engine: DpiEngine,
    executable: VerifiedResource,
    strategies: Vec<VerifiedStrategy>,
}

impl VerifiedEngine {
    pub fn engine(&self) -> DpiEngine {
        self.engine
    }

    pub fn executable(&self) -> &Path {
        self.executable.path()
    }

    pub fn strategies(&self) -> &[VerifiedStrategy] {
        &self.strategies
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedStrategy {
    category: DpiCategory,
    strategy_id: String,
    artifact: VerifiedResource,
    dependencies: Vec<VerifiedResource>,
}

impl VerifiedStrategy {
    pub fn category(&self) -> DpiCategory {
        self.category
    }

    pub fn strategy_id(&self) -> &str {
        &self.strategy_id
    }

    pub fn artifact(&self) -> &Path {
        self.artifact.path()
    }

    pub fn dependencies(&self) -> Vec<&Path> {
        self.dependencies
            .iter()
            .map(VerifiedResource::path)
            .collect()
    }

    pub fn artifact_resource(&self) -> &VerifiedResource {
        &self.artifact
    }

    pub fn dependency_resources(&self) -> &[VerifiedResource] {
        &self.dependencies
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedResource {
    root: PathBuf,
    relative: PathBuf,
    path: PathBuf,
    size: u64,
    sha256: String,
}

impl VerifiedResource {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn verify(&self) -> Result<(), LayoutError> {
        reject_resource_reparse_points(&self.root, &self.relative)?;
        let metadata = fs::metadata(&self.path).map_err(|source| LayoutError::Read {
            path: self.path.clone(),
            source,
        })?;
        if !metadata.is_file() || metadata.len() != self.size {
            return Err(LayoutError::IntegrityMismatch(self.path.clone()));
        }
        if !sha256_file(&self.path)?.eq_ignore_ascii_case(&self.sha256) {
            return Err(LayoutError::IntegrityMismatch(self.path.clone()));
        }
        Ok(())
    }

    fn key(&self) -> String {
        relative_path_key(&self.relative)
    }
}

impl ProtectedLayout {
    /// Discovers the only permitted runtime root. No IPC caller can substitute
    /// a path below a user profile, a temporary directory or a network share.
    pub fn discover() -> Result<Self, LayoutError> {
        let program_files = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .ok_or(LayoutError::ProgramFilesUnavailable)?;
        Self::from_program_files(&program_files)
    }

    pub fn from_program_files(program_files: &Path) -> Result<Self, LayoutError> {
        Self::inspect(program_files, &program_files.join(PRODUCT_DIRECTORY))
    }

    /// Exposed for installer/service tests. Production code must call
    /// [`Self::discover`] or [`Self::from_program_files`].
    pub fn inspect(program_files: &Path, root: &Path) -> Result<Self, LayoutError> {
        let program_files = canonical_directory(program_files)?;
        let root = canonical_directory(root)?;
        let expected = program_files.join(PRODUCT_DIRECTORY);
        if path_key(&root) != path_key(&expected) {
            return Err(LayoutError::UnexpectedRoot {
                expected,
                actual: root,
            });
        }
        reject_reparse_points(&program_files, &root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Verifies every code/config/data dependency named by the protected
    /// runtime manifest. No manifest path or strategy definition comes from
    /// IPC, and the returned catalog contains only absolute protected paths.
    pub fn load_verified_catalog(&self) -> Result<VerifiedRuntimeCatalog, LayoutError> {
        let manifest_relative = Path::new(RESOURCE_MANIFEST);
        reject_resource_reparse_points(&self.root, manifest_relative)?;
        let manifest_path = self.root.join(manifest_relative);
        let metadata = fs::metadata(&manifest_path).map_err(|source| LayoutError::Read {
            path: manifest_path.clone(),
            source,
        })?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_MANIFEST_BYTES {
            return Err(LayoutError::InvalidManifest(manifest_path));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&manifest_path)
            .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|source| LayoutError::Read {
                path: manifest_path.clone(),
                source,
            })?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(LayoutError::InvalidManifest(manifest_path));
        }
        let manifest: ResourceManifest = serde_json::from_slice(&bytes)
            .map_err(|_| LayoutError::InvalidManifest(manifest_path.clone()))?;
        if manifest.schema_version != RESOURCE_MANIFEST_SCHEMA_VERSION
            || manifest.engines.is_empty()
            || manifest.engines.len() > MAX_ENGINE_GROUPS
        {
            return Err(LayoutError::InvalidManifest(manifest_path));
        }

        let mut seen_engines = Vec::new();
        let mut verified_engines = Vec::with_capacity(manifest.engines.len());
        for group in manifest.engines {
            if seen_engines.contains(&group.engine) {
                return Err(LayoutError::DuplicateEngine(group.engine));
            }
            seen_engines.push(group.engine);
            verified_engines.push(self.verify_engine_group(group)?);
        }
        Ok(VerifiedRuntimeCatalog {
            root: self.root.clone(),
            engines: verified_engines,
        })
    }

    /// Compatibility probe used before capabilities are advertised.
    pub fn verify_engine_resources(&self) -> Result<(), LayoutError> {
        self.load_verified_catalog().map(|_| ())
    }

    fn verify_engine_group(&self, group: EngineResources) -> Result<VerifiedEngine, LayoutError> {
        if group.files.is_empty()
            || group.files.len() > MAX_FILES_PER_ENGINE
            || group.strategies.is_empty()
            || group.strategies.len() > MAX_STRATEGIES_PER_ENGINE
        {
            return Err(LayoutError::InvalidEngineGroup(group.engine));
        }

        let mut verified_files = BTreeMap::new();
        for file in group.files {
            let relative = safe_relative(&file.path)
                .ok_or_else(|| LayoutError::UnsafeManifestPath(file.path.clone()))?;
            let key = relative_path_key(relative);
            if verified_files.contains_key(&key) {
                return Err(LayoutError::DuplicateResource(file.path));
            }
            if file.size > MAX_RESOURCE_BYTES || !is_sha256_hex(&file.sha256) {
                return Err(LayoutError::InvalidManifestField(file.path));
            }
            let path = self.root.join(relative);
            reject_resource_reparse_points(&self.root, relative)?;
            let metadata = fs::metadata(&path).map_err(|source| LayoutError::Read {
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file() || metadata.len() != file.size {
                return Err(LayoutError::IntegrityMismatch(path));
            }
            if !sha256_file(&path)?.eq_ignore_ascii_case(&file.sha256) {
                return Err(LayoutError::IntegrityMismatch(path));
            }
            verified_files.insert(
                key,
                VerifiedResource {
                    root: self.root.clone(),
                    relative: relative.to_path_buf(),
                    path,
                    size: file.size,
                    sha256: file.sha256.to_ascii_lowercase(),
                },
            );
        }

        let executable = require_verified_resource(&group.executable, &verified_files)?.clone();
        if executable.size == 0 {
            return Err(LayoutError::InvalidManifestField(group.executable));
        }
        let mut seen_strategies = BTreeSet::new();
        let mut strategies = Vec::with_capacity(group.strategies.len());
        for strategy in group.strategies {
            if !is_safe_strategy_id(&strategy.id)
                || !seen_strategies.insert(strategy.id.clone())
                || strategy.dependencies.len() > MAX_STRATEGY_DEPENDENCIES
            {
                return Err(LayoutError::InvalidStrategy(strategy.id));
            }
            let artifact = require_verified_resource(&strategy.artifact, &verified_files)?.clone();
            let mut seen_dependencies = BTreeSet::new();
            let mut dependencies = Vec::with_capacity(strategy.dependencies.len());
            for dependency in strategy.dependencies {
                let resource = require_verified_resource(&dependency, &verified_files)?;
                if !seen_dependencies.insert(resource.key()) {
                    return Err(LayoutError::DuplicateResource(dependency));
                }
                dependencies.push(resource.clone());
            }
            strategies.push(VerifiedStrategy {
                category: strategy.category,
                strategy_id: strategy.id,
                artifact,
                dependencies,
            });
        }

        Ok(VerifiedEngine {
            engine: group.engine,
            executable,
            strategies,
        })
    }
}

#[derive(Debug)]
pub enum LayoutError {
    ProgramFilesUnavailable,
    MissingOrNotDirectory(PathBuf),
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
    UnexpectedRoot {
        expected: PathBuf,
        actual: PathBuf,
    },
    ReparsePoint(PathBuf),
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidManifest(PathBuf),
    DuplicateEngine(DpiEngine),
    InvalidEngineGroup(DpiEngine),
    UnsafeManifestPath(String),
    InvalidManifestField(String),
    DuplicateResource(String),
    UnverifiedResource(String),
    InvalidStrategy(String),
    EngineNotAllowlisted(DpiEngine),
    StrategyNotAllowlisted {
        engine: DpiEngine,
        category: DpiCategory,
        strategy_id: String,
    },
    InvalidDpiPlan,
    IntegrityMismatch(PathBuf),
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProgramFilesUnavailable => formatter.write_str("ProgramFiles is unavailable"),
            Self::MissingOrNotDirectory(path) => {
                write!(
                    formatter,
                    "protected layout directory is missing: {}",
                    path.display()
                )
            }
            Self::Canonicalize { path, source } => {
                write!(
                    formatter,
                    "could not canonicalize {}: {source}",
                    path.display()
                )
            }
            Self::UnexpectedRoot { expected, actual } => write!(
                formatter,
                "protected layout must be {}, not {}",
                expected.display(),
                actual.display()
            ),
            Self::ReparsePoint(path) => write!(
                formatter,
                "protected layout contains a reparse point: {}",
                path.display()
            ),
            Self::Read { path, source } => {
                write!(formatter, "could not read {}: {source}", path.display())
            }
            Self::InvalidManifest(path) => {
                write!(formatter, "invalid resource manifest: {}", path.display())
            }
            Self::DuplicateEngine(engine) => write!(formatter, "duplicate engine: {engine:?}"),
            Self::InvalidEngineGroup(engine) => {
                write!(formatter, "invalid or unbounded engine group: {engine:?}")
            }
            Self::UnsafeManifestPath(path) => write!(formatter, "unsafe manifest path: {path}"),
            Self::InvalidManifestField(path) => write!(formatter, "invalid manifest entry: {path}"),
            Self::DuplicateResource(path) => write!(formatter, "duplicate resource: {path}"),
            Self::UnverifiedResource(path) => {
                write!(
                    formatter,
                    "strategy references an unverified resource: {path}"
                )
            }
            Self::InvalidStrategy(strategy) => {
                write!(formatter, "invalid or duplicate strategy: {strategy}")
            }
            Self::EngineNotAllowlisted(engine) => {
                write!(formatter, "engine is not allowlisted: {engine:?}")
            }
            Self::StrategyNotAllowlisted {
                engine,
                category,
                strategy_id,
            } => write!(
                formatter,
                "strategy is not allowlisted for {engine:?}/{category:?}: {strategy_id}"
            ),
            Self::InvalidDpiPlan => formatter.write_str("invalid or unbounded DPI plan"),
            Self::IntegrityMismatch(path) => {
                write!(
                    formatter,
                    "protected resource integrity mismatch: {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LayoutError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceManifest {
    schema_version: u32,
    engines: Vec<EngineResources>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineResources {
    engine: DpiEngine,
    executable: String,
    files: Vec<ResourceFile>,
    strategies: Vec<StrategyDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceFile {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyDefinition {
    id: String,
    category: DpiCategory,
    artifact: String,
    #[serde(default)]
    dependencies: Vec<String>,
}

fn canonical_directory(path: &Path) -> Result<PathBuf, LayoutError> {
    if !path.is_dir() {
        return Err(LayoutError::MissingOrNotDirectory(path.to_path_buf()));
    }
    fs::canonicalize(path).map_err(|source| LayoutError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn reject_reparse_points(program_files: &Path, root: &Path) -> Result<(), LayoutError> {
    let mut components = Vec::new();
    let mut current = root;
    while path_key(current) != path_key(program_files) {
        components.push(current.to_path_buf());
        current = current
            .parent()
            .ok_or_else(|| LayoutError::UnexpectedRoot {
                expected: program_files.join(PRODUCT_DIRECTORY),
                actual: root.to_path_buf(),
            })?;
    }
    components.push(program_files.to_path_buf());

    for path in components {
        reject_reparse_point(&path)?;
    }
    Ok(())
}

fn reject_resource_reparse_points(root: &Path, relative: &Path) -> Result<(), LayoutError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(LayoutError::UnsafeManifestPath(
                relative.to_string_lossy().into_owned(),
            ));
        };
        current.push(component);
        reject_reparse_point(&current)?;
    }
    Ok(())
}

fn reject_reparse_point(path: &Path) -> Result<(), LayoutError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| LayoutError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata) {
        Err(LayoutError::ReparsePoint(path.to_path_buf()))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_windows_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn safe_relative(path: &str) -> Option<&Path> {
    if path.is_empty()
        || path.len() > MAX_RESOURCE_PATH_BYTES
        || path.contains(['\\', '\0', ':'])
        || !path.is_ascii()
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
    {
        return None;
    }
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        None
    } else {
        Some(path)
    }
}

fn relative_path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn require_verified_resource<'a>(
    value: &str,
    verified_files: &'a BTreeMap<String, VerifiedResource>,
) -> Result<&'a VerifiedResource, LayoutError> {
    let relative =
        safe_relative(value).ok_or_else(|| LayoutError::UnsafeManifestPath(value.into()))?;
    verified_files
        .get(&relative_path_key(relative))
        .ok_or_else(|| LayoutError::UnverifiedResource(value.into()))
}

fn category_tag(category: DpiCategory) -> u8 {
    match category {
        DpiCategory::Discord => 1,
        DpiCategory::YoutubeTwitch => 2,
        DpiCategory::Gaming => 3,
        DpiCategory::AtRisk => 4,
        DpiCategory::Universal => 5,
    }
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_safe_strategy_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_STRATEGY_ID_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'_')
        })
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

fn sha256_file(path: &Path) -> Result<String, LayoutError> {
    let file = File::open(path).map_err(|source| LayoutError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|source| LayoutError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use obsession_runtime_protocol::{DpiRuntimeOptions, DpiSelection};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("obsession-layout-test-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_layout(program_files: &Path, bytes: &[u8], manifest_path: &str, sha: &str) -> PathBuf {
        let root = program_files.join(PRODUCT_DIRECTORY);
        let resource = root.join(manifest_path);
        fs::create_dir_all(resource.parent().unwrap()).unwrap();
        fs::write(&resource, bytes).unwrap();
        let runtime_manifest = root.join(RESOURCE_MANIFEST);
        fs::create_dir_all(runtime_manifest.parent().unwrap()).unwrap();
        fs::write(
            runtime_manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": RESOURCE_MANIFEST_SCHEMA_VERSION,
                "engines": [{
                    "engine": "legacy",
                    "executable": manifest_path,
                    "files": [{
                        "path": manifest_path,
                        "size": bytes.len(),
                        "sha256": sha
                    }],
                    "strategies": [{
                        "id": "discord_1.conf",
                        "category": "discord",
                        "artifact": manifest_path,
                        "dependencies": []
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn accepts_only_the_exact_program_files_product_root_and_hashes_resources() {
        let parent = temp_root();
        let program_files = parent.join("Program Files");
        fs::create_dir_all(&program_files).unwrap();
        let bytes = b"protected-engine";
        let root = write_layout(&program_files, bytes, "bin/winws.exe", &sha256_hex(bytes));

        let layout = ProtectedLayout::inspect(&program_files, &root).unwrap();
        assert_eq!(layout.root(), fs::canonicalize(root).unwrap());
        let catalog = layout.load_verified_catalog().unwrap();
        let strategy = catalog
            .resolve_strategy(DpiEngine::Legacy, DpiCategory::Discord, "discord_1.conf")
            .unwrap();
        assert_eq!(
            catalog.engine(DpiEngine::Legacy).unwrap().executable(),
            layout.root().join("bin/winws.exe")
        );
        assert_eq!(strategy.artifact(), layout.root().join("bin/winws.exe"));
        assert!(strategy.dependencies().is_empty());
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn dpi_request_resolves_only_to_verified_catalog_entries() {
        let parent = temp_root();
        let program_files = parent.join("Program Files");
        fs::create_dir_all(&program_files).unwrap();
        let bytes = b"protected-engine";
        let root = write_layout(&program_files, bytes, "bin/winws.exe", &sha256_hex(bytes));
        let layout = ProtectedLayout::inspect(&program_files, &root).unwrap();
        let catalog = layout.load_verified_catalog().unwrap();
        let request = DpiStartRequest {
            engine: DpiEngine::Legacy,
            selections: vec![DpiSelection {
                category: DpiCategory::Discord,
                strategy_id: "discord_1.conf".into(),
            }],
            options: DpiRuntimeOptions {
                zapret2_level: 0,
                legacy_reliability: true,
                zapret2_overrides: Vec::new(),
            },
        };
        let plan = catalog.resolve_dpi_plan(&request).unwrap();
        assert_eq!(plan.engine(), DpiEngine::Legacy);
        assert_eq!(plan.executable(), layout.root().join("bin/winws.exe"));
        assert_eq!(plan.strategies()[0].strategy_id(), "discord_1.conf");
        assert!(plan.legacy_reliability());

        let mut unknown = request.clone();
        unknown.selections[0].strategy_id = "discord_unknown.conf".into();
        assert!(matches!(
            catalog.resolve_dpi_plan(&unknown),
            Err(LayoutError::StrategyNotAllowlisted { .. })
        ));

        let mut traversal = request.clone();
        traversal.selections[0].strategy_id = "../../user/payload.conf".into();
        assert!(matches!(
            catalog.resolve_dpi_plan(&traversal),
            Err(LayoutError::InvalidDpiPlan)
        ));

        let mut wrong_category = request.clone();
        wrong_category.selections[0].category = DpiCategory::Gaming;
        assert!(matches!(
            catalog.resolve_dpi_plan(&wrong_category),
            Err(LayoutError::StrategyNotAllowlisted { .. })
        ));

        let mut duplicate = request.clone();
        duplicate.selections.push(duplicate.selections[0].clone());
        assert!(matches!(
            catalog.resolve_dpi_plan(&duplicate),
            Err(LayoutError::InvalidDpiPlan)
        ));

        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn rejects_any_root_other_than_program_files_obsession() {
        let parent = temp_root();
        let program_files = parent.join("Program Files");
        let wrong = parent.join("AppData").join("Obsession");
        fs::create_dir_all(&program_files).unwrap();
        fs::create_dir_all(&wrong).unwrap();

        assert!(matches!(
            ProtectedLayout::inspect(&program_files, &wrong),
            Err(LayoutError::UnexpectedRoot { .. })
        ));
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn rejects_unsafe_manifest_paths_and_hash_mismatches() {
        let parent = temp_root();
        let program_files = parent.join("Program Files");
        fs::create_dir_all(&program_files).unwrap();
        let root = write_layout(&program_files, b"engine", "bin/winws.exe", &"0".repeat(64));
        let layout = ProtectedLayout::inspect(&program_files, &root).unwrap();
        assert!(matches!(
            layout.verify_engine_resources(),
            Err(LayoutError::IntegrityMismatch(_))
        ));

        fs::write(
            root.join(RESOURCE_MANIFEST),
            r#"{"schema_version":1,"engines":[{"engine":"legacy","executable":"../payload.exe","files":[{"path":"../payload.exe","size":1,"sha256":"0000000000000000000000000000000000000000000000000000000000000000"}],"strategies":[{"id":"discord_1.conf","category":"discord","artifact":"../payload.exe"}]}]}"#,
        )
        .unwrap();
        assert!(matches!(
            layout.verify_engine_resources(),
            Err(LayoutError::UnsafeManifestPath(_))
        ));
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn rejects_unverified_dependencies_duplicate_strategies_and_unknown_fields() {
        let parent = temp_root();
        let program_files = parent.join("Program Files");
        fs::create_dir_all(&program_files).unwrap();
        let bytes = b"engine";
        let root = write_layout(&program_files, bytes, "bin/winws.exe", &sha256_hex(bytes));
        let layout = ProtectedLayout::inspect(&program_files, &root).unwrap();

        let manifest_path = root.join(RESOURCE_MANIFEST);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["engines"][0]["strategies"][0]["dependencies"] =
            serde_json::json!(["user/profile.conf"]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(matches!(
            layout.load_verified_catalog(),
            Err(LayoutError::UnverifiedResource(_))
        ));

        manifest["engines"][0]["strategies"][0]["dependencies"] = serde_json::json!([]);
        let duplicate = manifest["engines"][0]["strategies"][0].clone();
        manifest["engines"][0]["strategies"] = serde_json::json!([duplicate.clone(), duplicate]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(matches!(
            layout.load_verified_catalog(),
            Err(LayoutError::InvalidStrategy(_))
        ));

        manifest["engines"][0]["strategies"] = serde_json::json!([{
            "id": "discord_1.conf",
            "category": "discord",
            "artifact": "bin/winws.exe",
            "unexpected": "raw-command"
        }]);
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(matches!(
            layout.load_verified_catalog(),
            Err(LayoutError::InvalidManifest(_))
        ));

        fs::remove_dir_all(parent).unwrap();
    }
}
