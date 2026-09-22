//! Service-owned DPI configuration materialization.
//!
//! Signed configs below Program Files are never handed to the engine verbatim:
//! every path-bearing value is resolved against the verified manifest and
//! rewritten to either an immutable protected resource or a service-owned
//! mutable auto-hostlist below ProgramData. The wire request contributes only
//! typed strategy selections; it can never contribute a path or argument.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use obsession_runtime_protocol::{DpiCategory, DpiEngine};
use sha2::{Digest, Sha256};

use crate::protected_layout::{LayoutError, VerifiedDpiPlan, VerifiedResource, VerifiedStrategy};
#[cfg(windows)]
use crate::zapret2_pack;

pub const RUNTIME_STATE_RELATIVE: &str = "Obsession/Runtime";

const DPI_DIRECTORY: &str = "dpi";
const GENERATIONS_DIRECTORY: &str = "generations";
const AUTOHOSTS_DIRECTORY: &str = "autohosts";
const MAX_CONFIG_BYTES: u64 = 512 * 1024;
const MAX_CONFIG_TOKENS: usize = 65_536;
const MAX_CONFIG_TOKEN_BYTES: usize = 8 * 1024;
const MAX_AUTOHOST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AUTOHOST_DOMAINS: usize = 250_000;
const MAX_LAUNCH_ARGUMENTS: usize = 2048;
const MAX_LAUNCH_ARGUMENT_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedDataLayout {
    program_data: PathBuf,
    root: PathBuf,
    enforce_acl: bool,
}

impl ProtectedDataLayout {
    /// Discovers a state directory that the machine installer has already
    /// created and ACLed. Missing directories fail closed; the LocalSystem
    /// service must not adopt a user-created substitute during startup.
    pub fn discover() -> Result<Self, MaterializationError> {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .ok_or(MaterializationError::ProgramDataUnavailable)?;
        Self::from_program_data(&program_data)
    }

    pub fn from_program_data(program_data: &Path) -> Result<Self, MaterializationError> {
        let mut layout = Self::inspect(program_data, &program_data.join(RUNTIME_STATE_RELATIVE))?;
        layout.enforce_acl = true;
        layout.verify_root()?;
        Ok(layout)
    }

    /// Test/installer seam. Production discovery never accepts a caller path.
    pub fn inspect(program_data: &Path, root: &Path) -> Result<Self, MaterializationError> {
        let program_data = canonical_directory(program_data)?;
        let root = canonical_directory(root)?;
        let expected = program_data.join(RUNTIME_STATE_RELATIVE);
        if path_key(&root) != path_key(&expected) {
            return Err(MaterializationError::UnexpectedStateRoot {
                expected,
                actual: root,
            });
        }
        reject_reparse_chain(&program_data, &root)?;
        Ok(Self {
            program_data,
            root,
            enforce_acl: false,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn materialize(
        &self,
        plan: &VerifiedDpiPlan,
        generation: u64,
    ) -> Result<MaterializedGeneration, MaterializationError> {
        if generation == 0 {
            return Err(MaterializationError::InvalidGeneration);
        }
        self.verify_root()?;
        plan.reverify()?;

        let dpi_root = self.ensure_directory(Path::new(DPI_DIRECTORY))?;
        let generations_root = self.ensure_directory(
            Path::new(DPI_DIRECTORY)
                .join(GENERATIONS_DIRECTORY)
                .as_path(),
        )?;
        let autohosts_root =
            self.ensure_directory(Path::new(DPI_DIRECTORY).join(AUTOHOSTS_DIRECTORY).as_path())?;
        let generation_dir = generations_root.join(generation.to_string());
        fs::create_dir(&generation_dir).map_err(|source| MaterializationError::Io {
            operation: "create generation directory",
            path: generation_dir.clone(),
            source,
        })?;
        reject_reparse_point(&generation_dir)?;
        self.verify_service_owned_acl(&generation_dir)?;

        let result = self.materialize_into(
            plan,
            generation,
            &dpi_root,
            &autohosts_root,
            &generation_dir,
        );
        if result.is_err() {
            let _ = remove_exact_generation(&self.root, &generations_root, &generation_dir);
        }
        result
    }

    pub fn cleanup_generation(
        &self,
        generation: &MaterializedGeneration,
    ) -> Result<(), MaterializationError> {
        self.verify_root()?;
        let generations_root = self.root.join(DPI_DIRECTORY).join(GENERATIONS_DIRECTORY);
        remove_exact_generation(&self.root, &generations_root, &generation.directory)
    }

    fn materialize_into(
        &self,
        plan: &VerifiedDpiPlan,
        generation: u64,
        _dpi_root: &Path,
        autohosts_root: &Path,
        generation_dir: &Path,
    ) -> Result<MaterializedGeneration, MaterializationError> {
        if plan.engine() == DpiEngine::Zapret2 {
            return self.materialize_zapret2_into(plan, generation, generation_dir);
        }
        // winws opens WinDivert at priority 0: overlapping category processes
        // compete for packets. Hostlists do not make kernel filters exclusive.
        let mut strategies: Vec<_> = plan.strategies().iter().collect();
        strategies.sort_by_key(|strategy| category_tag(strategy.category()));
        let first = *strategies
            .first()
            .ok_or(MaterializationError::InvalidConfig(
                "verified plan contains no launch lanes",
            ))?;
        let configs = strategies
            .iter()
            .map(|strategy| self.render_legacy_config(strategy, autohosts_root))
            .collect::<Result<Vec<_>, _>>()?;
        let rendered = if configs.len() == 1 {
            configs.into_iter().next().expect("checked nonempty plan")
        } else {
            crate::legacy_pack::compile(&configs).map_err(MaterializationError::InvalidConfig)?
        };
        if rendered.len() as u64 > MAX_CONFIG_BYTES {
            return Err(MaterializationError::InvalidConfig(
                "combined Legacy config is oversized",
            ));
        }
        let response_file = generation_dir.join("effective.conf");
        write_new_synced(&response_file, rendered.as_bytes())?;
        self.verify_service_owned_acl(&response_file)?;
        let config_sha = sha256_bytes(rendered.as_bytes());
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"obsession/materialized-legacy-single-process/v2\0");
        fingerprint.update(plan.fingerprint().as_bytes());
        fingerprint.update(generation.to_le_bytes());
        fingerprint.update(config_sha.as_bytes());
        Ok(MaterializedGeneration {
            generation,
            directory: generation_dir.to_path_buf(),
            fingerprint: hex_digest(fingerprint.finalize()),
            launches: vec![MaterializedLaunch {
                executable: plan.executable().to_path_buf(),
                executable_sha256: plan.executable_resource().sha256().to_owned(),
                working_directory: plan.root().to_path_buf(),
                arguments: vec![format!("@{}", engine_file_path(&response_file))],
                response_file: Some(response_file),
                engine: plan.engine(),
                // Executor snapshots/observers retain ALL plan selections.
                category: first.category(),
                strategy_id: first.strategy_id().to_owned(),
                config_sha256: config_sha,
            }],
        })
    }

    #[cfg(windows)]
    fn materialize_zapret2_into(
        &self,
        plan: &VerifiedDpiPlan,
        generation: u64,
        generation_dir: &Path,
    ) -> Result<MaterializedGeneration, MaterializationError> {
        let arguments = zapret2_pack::compile(plan)
            .map_err(|_| MaterializationError::InvalidConfig("invalid Zapret2 Strategy Pack"))?;
        validate_launch_arguments(&arguments)?;
        let config_sha256 = launch_arguments_sha256(&arguments);
        let first = plan
            .strategies()
            .first()
            .ok_or(MaterializationError::InvalidConfig(
                "verified Zapret2 plan contains no categories",
            ))?;
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"obsession/materialized-zapret2-generation/v1\0");
        fingerprint.update(plan.fingerprint().as_bytes());
        fingerprint.update(generation.to_le_bytes());
        fingerprint.update(config_sha256.as_bytes());

        Ok(MaterializedGeneration {
            generation,
            directory: generation_dir.to_path_buf(),
            fingerprint: hex_digest(fingerprint.finalize()),
            launches: vec![MaterializedLaunch {
                executable: plan.executable().to_path_buf(),
                executable_sha256: plan.executable_resource().sha256().to_owned(),
                working_directory: plan.root().to_path_buf(),
                arguments,
                response_file: None,
                engine: plan.engine(),
                category: first.category(),
                strategy_id: first.strategy_id().to_owned(),
                config_sha256,
            }],
        })
    }

    fn render_legacy_config(
        &self,
        strategy: &VerifiedStrategy,
        autohosts_root: &Path,
    ) -> Result<String, MaterializationError> {
        let source = read_bounded(strategy.artifact_resource(), MAX_CONFIG_BYTES)?;
        let source = std::str::from_utf8(&source)
            .map_err(|_| MaterializationError::InvalidConfig("config is not UTF-8"))?;
        let tokens = tokenize_config(source)?;
        let mut dependencies = BTreeMap::new();
        for dependency in strategy.dependency_resources() {
            let key = relative_reference_key(dependency.relative_path()).ok_or({
                MaterializationError::InvalidConfig("manifest dependency is not relative")
            })?;
            if dependencies.insert(key, dependency).is_some() {
                return Err(MaterializationError::InvalidConfig(
                    "strategy contains duplicate dependencies",
                ));
            }
        }

        let mut rendered = String::new();
        let mut cursor = 0;
        while cursor < tokens.len() {
            let token = &tokens[cursor];
            if !token.starts_with("--") {
                return Err(MaterializationError::InvalidConfig(
                    "every response-file token must be an option",
                ));
            }
            let (option, mut value) = match token.split_once('=') {
                Some((option, value)) => (option, Some(value.to_owned())),
                None => (token.as_str(), None),
            };
            validate_option_name(option)?;
            if value.is_none()
                && tokens
                    .get(cursor + 1)
                    .is_some_and(|next| !next.starts_with("--"))
            {
                cursor += 1;
                value = Some(tokens[cursor].clone());
            }

            let value = match value {
                Some(value) => Some(self.resolve_option_value(
                    option,
                    &value,
                    strategy.category(),
                    &dependencies,
                    autohosts_root,
                )?),
                None if option_requires_resource(option) => {
                    return Err(MaterializationError::InvalidConfig(
                        "path-bearing option is missing its value",
                    ));
                }
                None => None,
            };
            rendered.push_str(option);
            if let Some(value) = value {
                rendered.push_str("=\"");
                rendered.push_str(&value);
                rendered.push('"');
            }
            rendered.push('\n');
            cursor += 1;
        }
        if rendered.is_empty() || rendered.len() as u64 > MAX_CONFIG_BYTES {
            return Err(MaterializationError::InvalidConfig(
                "materialized config is empty or oversized",
            ));
        }
        Ok(rendered)
    }

    fn resolve_option_value(
        &self,
        option: &str,
        value: &str,
        category: DpiCategory,
        dependencies: &BTreeMap<String, &VerifiedResource>,
        autohosts_root: &Path,
    ) -> Result<String, MaterializationError> {
        validate_scalar(value)?;
        let reference = normalize_reference(value);
        if let Some(reference) = reference.as_ref() {
            if let Some(resource) = dependencies.get(reference) {
                if option == "--hostlist-auto" {
                    let path =
                        self.materialize_auto_hostlist(category, resource, autohosts_root)?;
                    return path_to_config_value(&path);
                }
                return path_to_config_value(resource.path());
            }
        }

        if option_requires_resource(option) || looks_like_path(value) {
            return Err(MaterializationError::UndeclaredConfigPath(value.into()));
        }
        Ok(value.to_owned())
    }

    fn materialize_auto_hostlist(
        &self,
        category: DpiCategory,
        seed: &VerifiedResource,
        autohosts_root: &Path,
    ) -> Result<PathBuf, MaterializationError> {
        let category_root = autohosts_root.join(category_name(category));
        ensure_existing_or_create_directory(&category_root)?;
        self.verify_service_owned_acl(&category_root)?;
        let mut name_digest = Sha256::new();
        name_digest.update(seed.relative_path().to_string_lossy().as_bytes());
        let file_name = format!("{}.txt", &hex_digest(name_digest.finalize())[..24]);
        let destination = category_root.join(file_name);

        if destination.exists() {
            reject_reparse_point(&destination)?;
            self.verify_service_owned_acl(&destination)?;
            let bytes = read_regular_file_bounded(&destination, MAX_AUTOHOST_BYTES)?;
            validate_auto_hostlist(&bytes)?;
            return Ok(destination);
        }

        let seed_bytes = read_bounded(seed, MAX_AUTOHOST_BYTES)?;
        validate_auto_hostlist(&seed_bytes)?;
        match write_new_synced(&destination, &seed_bytes) {
            Ok(()) => {
                self.verify_service_owned_acl(&destination)?;
                Ok(destination)
            }
            Err(MaterializationError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::AlreadyExists =>
            {
                reject_reparse_point(&destination)?;
                self.verify_service_owned_acl(&destination)?;
                let bytes = read_regular_file_bounded(&destination, MAX_AUTOHOST_BYTES)?;
                validate_auto_hostlist(&bytes)?;
                Ok(destination)
            }
            Err(error) => {
                let _ = fs::remove_file(&destination);
                Err(error)
            }
        }
    }

    /// Fixed, service-owned journal location; never accepts a client path.
    pub(crate) fn tcp_timestamp_journal(&self) -> Result<PathBuf, MaterializationError> {
        let directory = self.ensure_directory(Path::new("tcp-settings"))?;
        let path = directory.join("timestamps.restore");
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                reject_reparse_point(&path)?;
                self.verify_service_owned_acl(&path)?;
                if !fs::metadata(&path).map_err(|source| MaterializationError::Io {
                    operation: "inspect TCP timestamps journal", path: path.clone(), source,
                })?.is_file() {
                    return Err(MaterializationError::InvalidStatePath);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(MaterializationError::Io {
                operation: "inspect TCP timestamps journal", path: path.clone(), source,
            }),
        }
        Ok(path)
    }

    fn ensure_directory(&self, relative: &Path) -> Result<PathBuf, MaterializationError> {
        self.verify_root()?;
        let mut current = self.root.clone();
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(MaterializationError::InvalidStatePath);
            };
            current.push(component);
            ensure_existing_or_create_directory(&current)?;
            self.verify_service_owned_acl(&current)?;
        }
        Ok(current)
    }

    fn verify_service_owned_acl(&self, path: &Path) -> Result<(), MaterializationError> {
        if self.enforce_acl {
            verify_protected_object_acl(path, false)?;
        }
        Ok(())
    }

    fn verify_root(&self) -> Result<(), MaterializationError> {
        let canonical = canonical_directory(&self.root)?;
        if path_key(&canonical) != path_key(&self.root)
            || path_key(&canonical) != path_key(&self.program_data.join(RUNTIME_STATE_RELATIVE))
        {
            return Err(MaterializationError::UnexpectedStateRoot {
                expected: self.program_data.join(RUNTIME_STATE_RELATIVE),
                actual: canonical,
            });
        }
        reject_reparse_chain(&self.program_data, &self.root)?;
        if self.enforce_acl {
            let product_root = self
                .root
                .parent()
                .ok_or(MaterializationError::InvalidStatePath)?;
            verify_protected_object_acl(product_root, true)?;
            verify_protected_object_acl(&self.root, true)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializedLaunch {
    executable: PathBuf,
    executable_sha256: String,
    working_directory: PathBuf,
    arguments: Vec<String>,
    response_file: Option<PathBuf>,
    engine: DpiEngine,
    category: DpiCategory,
    strategy_id: String,
    config_sha256: String,
}

impl MaterializedLaunch {
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn executable_sha256(&self) -> &str {
        &self.executable_sha256
    }

    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub fn response_file(&self) -> Option<&Path> {
        self.response_file.as_deref()
    }

    pub fn engine(&self) -> DpiEngine {
        self.engine
    }

    pub fn category(&self) -> DpiCategory {
        self.category
    }

    pub fn strategy_id(&self) -> &str {
        &self.strategy_id
    }

    pub fn config_sha256(&self) -> &str {
        &self.config_sha256
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializedGeneration {
    generation: u64,
    directory: PathBuf,
    fingerprint: String,
    launches: Vec<MaterializedLaunch>,
}

impl MaterializedGeneration {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn launches(&self) -> &[MaterializedLaunch] {
        &self.launches
    }

    pub fn reverify(&self) -> Result<(), MaterializationError> {
        reject_reparse_point(&self.directory)?;
        for launch in &self.launches {
            validate_launch_arguments(&launch.arguments)?;
            if let Some(response_file) = &launch.response_file {
                let parent = response_file
                    .parent()
                    .ok_or(MaterializationError::InvalidStatePath)?;
                reject_reparse_point(parent)?;
                reject_reparse_point(response_file)?;
                let bytes = read_regular_file_bounded(response_file, MAX_CONFIG_BYTES)?;
                if sha256_bytes(&bytes) != launch.config_sha256 {
                    return Err(MaterializationError::InvalidConfig(
                        "materialized config integrity mismatch",
                    ));
                }
            } else if launch_arguments_sha256(&launch.arguments) != launch.config_sha256 {
                return Err(MaterializationError::InvalidConfig(
                    "materialized arguments integrity mismatch",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum MaterializationError {
    ProgramDataUnavailable,
    SecurityVerificationUnavailable,
    MissingOrNotDirectory(PathBuf),
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
    UnexpectedStateRoot {
        expected: PathBuf,
        actual: PathBuf,
    },
    ReparsePoint(PathBuf),
    AclQueryFailed {
        path: PathBuf,
        code: u32,
    },
    InsecureAcl {
        path: PathBuf,
        reason: &'static str,
    },
    InvalidStatePath,
    InvalidGeneration,
    InvalidConfig(&'static str),
    UndeclaredConfigPath(String),
    InvalidAutoHostlist,
    Layout(LayoutError),
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for MaterializationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProgramDataUnavailable => formatter.write_str("ProgramData is unavailable"),
            Self::SecurityVerificationUnavailable => {
                formatter.write_str("protected ACL verification is unavailable on this platform")
            }
            Self::MissingOrNotDirectory(path) => {
                write!(
                    formatter,
                    "protected state directory is missing: {}",
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
            Self::UnexpectedStateRoot { expected, actual } => write!(
                formatter,
                "protected state root must be {}, not {}",
                expected.display(),
                actual.display()
            ),
            Self::ReparsePoint(path) => {
                write!(
                    formatter,
                    "protected state contains a reparse point: {}",
                    path.display()
                )
            }
            Self::AclQueryFailed { path, code } => write!(
                formatter,
                "could not inspect protected ACL for {}: Windows error {code}",
                path.display()
            ),
            Self::InsecureAcl { path, reason } => write!(
                formatter,
                "protected state ACL is unsafe for {}: {reason}",
                path.display()
            ),
            Self::InvalidStatePath => formatter.write_str("invalid protected state path"),
            Self::InvalidGeneration => formatter.write_str("generation must be non-zero"),
            Self::InvalidConfig(reason) => write!(formatter, "invalid protected config: {reason}"),
            Self::UndeclaredConfigPath(path) => {
                write!(formatter, "config references an undeclared path: {path}")
            }
            Self::InvalidAutoHostlist => formatter.write_str("invalid service-owned auto-hostlist"),
            Self::Layout(error) => {
                write!(formatter, "protected resource verification failed: {error}")
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
        }
    }
}

impl std::error::Error for MaterializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Canonicalize { source, .. } | Self::Io { source, .. } => Some(source),
            Self::Layout(source) => Some(source),
            _ => None,
        }
    }
}

impl From<LayoutError> for MaterializationError {
    fn from(value: LayoutError) -> Self {
        Self::Layout(value)
    }
}

fn tokenize_config(source: &str) -> Result<Vec<String>, MaterializationError> {
    if source.len() as u64 > MAX_CONFIG_BYTES {
        return Err(MaterializationError::InvalidConfig("config is oversized"));
    }
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut comment = false;
    for character in source.chars() {
        if character == '\0' || (character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
        {
            return Err(MaterializationError::InvalidConfig(
                "config contains control characters",
            ));
        }
        if comment {
            if character == '\n' {
                comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                token.push(character);
            }
        } else {
            match character {
                '#' if token.is_empty() => comment = true,
                '#' => token.push('#'),
                '"' | '\'' => quote = Some(character),
                value if value.is_whitespace() => {
                    if !token.is_empty() {
                        push_token(&mut tokens, &mut token)?;
                    }
                }
                value => token.push(value),
            }
        }
        if token.len() > MAX_CONFIG_TOKEN_BYTES {
            return Err(MaterializationError::InvalidConfig(
                "config token is oversized",
            ));
        }
    }
    if quote.is_some() {
        return Err(MaterializationError::InvalidConfig(
            "config contains an unterminated quote",
        ));
    }
    if !token.is_empty() {
        push_token(&mut tokens, &mut token)?;
    }
    if tokens.is_empty() {
        return Err(MaterializationError::InvalidConfig("config has no options"));
    }
    Ok(tokens)
}

fn push_token(tokens: &mut Vec<String>, token: &mut String) -> Result<(), MaterializationError> {
    if tokens.len() >= MAX_CONFIG_TOKENS {
        return Err(MaterializationError::InvalidConfig(
            "config has too many tokens",
        ));
    }
    tokens.push(std::mem::take(token));
    Ok(())
}

fn validate_option_name(option: &str) -> Result<(), MaterializationError> {
    let suffix = option
        .strip_prefix("--")
        .ok_or(MaterializationError::InvalidConfig(
            "response-file option has an invalid name",
        ))?;
    if suffix.is_empty()
        || suffix.len() > 96
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Err(MaterializationError::InvalidConfig(
            "response-file option has an invalid name",
        ))
    } else {
        Ok(())
    }
}

fn option_requires_resource(option: &str) -> bool {
    matches!(
        option,
        "--hostlist"
            | "--hostlist-auto"
            | "--hostlist-exclude"
            | "--ipset"
            | "--ipset-exclude"
            | "--dpi-desync-fake-tls"
            | "--dpi-desync-fake-http"
            | "--dpi-desync-fake-quic"
            | "--dpi-desync-fake-discord"
            | "--dpi-desync-fake-stun"
            | "--dpi-desync-fake-unknown-tcp"
            | "--dpi-desync-fake-unknown-udp"
            | "--dpi-desync-split-seqovl-pattern"
            | "--dpi-desync-multisplit-seqovl-pattern"
    )
}

fn validate_scalar(value: &str) -> Result<(), MaterializationError> {
    if value.is_empty()
        || value.len() > MAX_CONFIG_TOKEN_BYTES
        || value.contains(['"', '\r', '\n', '\0'])
    {
        Err(MaterializationError::InvalidConfig("invalid option value"))
    } else {
        Ok(())
    }
}

fn normalize_reference(value: &str) -> Option<String> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.contains(['\0', ':'])
        || !normalized.is_ascii()
        || Path::new(&normalized).is_absolute()
        || !Path::new(&normalized)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(normalized.to_ascii_lowercase())
}

fn relative_reference_key(path: &Path) -> Option<String> {
    normalize_reference(&path.to_string_lossy())
}

fn looks_like_path(value: &str) -> bool {
    value.contains(['\\', '/'])
        || value.starts_with('.')
        || matches!(
            value.as_bytes(),
            [drive, b':', ..] if drive.is_ascii_alphabetic()
        )
}

fn path_to_config_value(path: &Path) -> Result<String, MaterializationError> {
    let value = engine_file_path(path);
    validate_scalar(&value)?;
    if !path.is_absolute() {
        return Err(MaterializationError::InvalidConfig(
            "materialized resource path is not absolute",
        ));
    }
    Ok(value)
}

/// Rust's canonical Windows paths use a verbatim prefix that the engines' C
/// file APIs cannot read. Convert only at the CLI boundary, after verification.
pub(crate) fn engine_file_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    if let Some(disk) = value.strip_prefix(r"\\?\") {
        if matches!(disk.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic()) {
            return disk.to_owned();
        }
    }
    value.into_owned()
}

fn validate_auto_hostlist(bytes: &[u8]) -> Result<(), MaterializationError> {
    if bytes.len() as u64 > MAX_AUTOHOST_BYTES {
        return Err(MaterializationError::InvalidAutoHostlist);
    }
    let source =
        std::str::from_utf8(bytes).map_err(|_| MaterializationError::InvalidAutoHostlist)?;
    let mut domains = BTreeSet::new();
    for line in source.lines() {
        let value = line.trim();
        if value.is_empty() || value.starts_with('#') {
            continue;
        }
        let value = value.strip_suffix('.').unwrap_or(value);
        if !valid_domain(value) || !domains.insert(value.to_ascii_lowercase()) {
            return Err(MaterializationError::InvalidAutoHostlist);
        }
        if domains.len() > MAX_AUTOHOST_DOMAINS {
            return Err(MaterializationError::InvalidAutoHostlist);
        }
    }
    Ok(())
}

fn valid_domain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.is_ascii()
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

fn read_bounded(resource: &VerifiedResource, limit: u64) -> Result<Vec<u8>, MaterializationError> {
    resource.verify()?;
    read_regular_file_bounded(resource.path(), limit)
}

fn read_regular_file_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, MaterializationError> {
    reject_reparse_point(path)?;
    let metadata = fs::metadata(path).map_err(|source| MaterializationError::Io {
        operation: "read metadata",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(MaterializationError::InvalidConfig(
            "resource is not a bounded file",
        ));
    }
    let file = File::open(path).map_err(|source| MaterializationError::Io {
        operation: "open resource",
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| MaterializationError::Io {
            operation: "read resource",
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > limit {
        return Err(MaterializationError::InvalidConfig(
            "resource grew beyond its bound",
        ));
    }
    Ok(bytes)
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), MaterializationError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| MaterializationError::Io {
            operation: "create service-owned file",
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| MaterializationError::Io {
            operation: "write service-owned file",
            path: path.to_path_buf(),
            source,
        })
}

fn ensure_existing_or_create_directory(path: &Path) -> Result<(), MaterializationError> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(MaterializationError::Io {
                operation: "create protected state directory",
                path: path.to_path_buf(),
                source,
            })
        }
    }
    reject_reparse_point(path)?;
    if !path.is_dir() {
        return Err(MaterializationError::MissingOrNotDirectory(
            path.to_path_buf(),
        ));
    }
    Ok(())
}

fn remove_exact_generation(
    state_root: &Path,
    generations_root: &Path,
    generation_dir: &Path,
) -> Result<(), MaterializationError> {
    if generation_dir.parent().map(path_key) != Some(path_key(generations_root))
        || !generation_dir
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| {
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
            })
    {
        return Err(MaterializationError::InvalidStatePath);
    }
    if !generation_dir.exists() {
        return Ok(());
    }
    reject_reparse_chain(state_root, generations_root)?;
    reject_reparse_chain(generations_root, generation_dir)?;
    let mut visited = 0;
    reject_generation_tree(generation_dir, &mut visited)?;
    fs::remove_dir_all(generation_dir).map_err(|source| MaterializationError::Io {
        operation: "remove exact materialized generation",
        path: generation_dir.to_path_buf(),
        source,
    })
}

fn reject_generation_tree(path: &Path, visited: &mut usize) -> Result<(), MaterializationError> {
    *visited += 1;
    if *visited > 64 {
        return Err(MaterializationError::InvalidStatePath);
    }
    reject_reparse_point(path)?;
    if path.is_dir() {
        let entries = fs::read_dir(path).map_err(|source| MaterializationError::Io {
            operation: "inspect materialized generation",
            path: path.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| MaterializationError::Io {
                operation: "inspect materialized generation entry",
                path: path.to_path_buf(),
                source,
            })?;
            reject_generation_tree(&entry.path(), visited)?;
        }
    }
    Ok(())
}

fn canonical_directory(path: &Path) -> Result<PathBuf, MaterializationError> {
    if !path.is_dir() {
        return Err(MaterializationError::MissingOrNotDirectory(
            path.to_path_buf(),
        ));
    }
    fs::canonicalize(path).map_err(|source| MaterializationError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn reject_reparse_chain(base: &Path, target: &Path) -> Result<(), MaterializationError> {
    let mut current = target;
    loop {
        reject_reparse_point(current)?;
        if path_key(current) == path_key(base) {
            return Ok(());
        }
        current = current
            .parent()
            .ok_or(MaterializationError::InvalidStatePath)?;
    }
}

fn reject_reparse_point(path: &Path) -> Result<(), MaterializationError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| MaterializationError::Io {
        operation: "inspect protected state path",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata) {
        Err(MaterializationError::ReparsePoint(path.to_path_buf()))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn access_mask_grants_write_like_access(mask: u32) -> bool {
    use windows::Win32::Foundation::{GENERIC_ALL, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_DELETE_CHILD, FILE_WRITE_ATTRIBUTES,
        FILE_WRITE_EA, WRITE_DAC, WRITE_OWNER,
    };

    // Do not use FILE_GENERIC_WRITE as a mask here. It contains READ_CONTROL
    // and SYNCHRONIZE, which are also present in ordinary read/execute ACEs.
    // Intersecting against the aggregate would therefore reject the installer's
    // intentional BUILTIN\Users read-only access as writable.
    let write_like = FILE_ADD_FILE.0
        | FILE_ADD_SUBDIRECTORY.0
        | FILE_WRITE_EA.0
        | FILE_WRITE_ATTRIBUTES.0
        | FILE_DELETE_CHILD.0
        | WRITE_DAC.0
        | WRITE_OWNER.0
        | DELETE.0
        | GENERIC_WRITE.0
        | GENERIC_ALL.0;
    mask & write_like != 0
}

#[cfg(windows)]
fn verify_protected_object_acl(
    path: &Path,
    require_protected_dacl: bool,
) -> Result<(), MaterializationError> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GetLastError, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        AclSizeInformation, CreateWellKnownSid, EqualSid, GetAce, GetAclInformation, GetLengthSid,
        GetSecurityDescriptorControl, IsValidSid, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        ACCESS_ALLOWED_ACE, ACCESS_DENIED_ACE, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_MAX_SID_SIZE,
        SE_DACL_PROTECTED,
    };
    use windows::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};

    struct OwnedSecurityDescriptor(PSECURITY_DESCRIPTOR);

    impl Drop for OwnedSecurityDescriptor {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = windows::Win32::Foundation::LocalFree(HLOCAL(self.0 .0));
                }
            }
        }
    }

    const SID_WORDS: usize = (SECURITY_MAX_SID_SIZE as usize).div_ceil(4);

    struct WellKnownSid {
        storage: [u32; SID_WORDS],
    }

    impl WellKnownSid {
        fn create(
            sid_type: windows::Win32::Security::WELL_KNOWN_SID_TYPE,
            path: &Path,
        ) -> Result<Self, MaterializationError> {
            let mut sid = Self {
                storage: [0; SID_WORDS],
            };
            let mut length = SECURITY_MAX_SID_SIZE;
            let result = unsafe {
                CreateWellKnownSid(sid_type, PSID::default(), sid.as_psid(), &mut length)
            };
            if result.is_err() {
                return Err(MaterializationError::AclQueryFailed {
                    path: path.to_path_buf(),
                    code: unsafe { GetLastError().0 },
                });
            }
            Ok(sid)
        }

        fn as_psid(&mut self) -> PSID {
            PSID(self.storage.as_mut_ptr().cast::<c_void>())
        }

        fn psid(&self) -> PSID {
            PSID(self.storage.as_ptr().cast_mut().cast::<c_void>())
        }
    }

    fn query_failed(path: &Path) -> MaterializationError {
        MaterializationError::AclQueryFailed {
            path: path.to_path_buf(),
            code: unsafe { GetLastError().0 },
        }
    }

    fn insecure(path: &Path, reason: &'static str) -> MaterializationError {
        MaterializationError::InsecureAcl {
            path: path.to_path_buf(),
            reason,
        }
    }

    fn is_privileged_sid(sid: PSID, system: &WellKnownSid, admins: &WellKnownSid) -> bool {
        unsafe { EqualSid(sid, system.psid()).is_ok() || EqualSid(sid, admins.psid()).is_ok() }
    }

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(insecure(path, "path contains an embedded NUL"));
    }
    wide.push(0);

    let mut owner = PSID::default();
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(MaterializationError::AclQueryFailed {
            path: path.to_path_buf(),
            code: status.0,
        });
    }
    let _descriptor = OwnedSecurityDescriptor(descriptor);
    if descriptor.is_invalid() || owner.is_invalid() {
        return Err(insecure(path, "security descriptor has no owner"));
    }
    if dacl.is_null() {
        return Err(insecure(
            path,
            "security descriptor has a null or missing DACL",
        ));
    }

    let system = WellKnownSid::create(WinLocalSystemSid, path)?;
    let admins = WellKnownSid::create(WinBuiltinAdministratorsSid, path)?;
    if !is_privileged_sid(owner, &system, &admins) {
        return Err(insecure(
            path,
            "owner is neither LocalSystem nor Administrators",
        ));
    }

    let mut control = 0u16;
    let mut revision = 0u32;
    unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
        .map_err(|_| query_failed(path))?;
    if require_protected_dacl && control & SE_DACL_PROTECTED.0 == 0 {
        return Err(insecure(path, "DACL inheritance is not protected"));
    }

    let mut acl_info = ACL_SIZE_INFORMATION::default();
    unsafe {
        GetAclInformation(
            dacl,
            (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast::<c_void>(),
            size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    }
    .map_err(|_| query_failed(path))?;

    const SID_OFFSET: usize = size_of::<ACCESS_ALLOWED_ACE>() - size_of::<u32>();
    for index in 0..acl_info.AceCount {
        let mut ace_pointer = std::ptr::null_mut();
        unsafe { GetAce(dacl, index, &mut ace_pointer) }.map_err(|_| query_failed(path))?;
        if ace_pointer.is_null() {
            return Err(insecure(path, "DACL contains a null ACE"));
        }
        let header = unsafe { &*ace_pointer.cast::<windows::Win32::Security::ACE_HEADER>() };
        match header.AceType as u32 {
            ACCESS_DENIED_ACE_TYPE => {
                if usize::from(header.AceSize) < size_of::<ACCESS_DENIED_ACE>() {
                    return Err(insecure(path, "DACL contains a truncated deny ACE"));
                }
            }
            ACCESS_ALLOWED_ACE_TYPE => {
                if usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>() {
                    return Err(insecure(path, "DACL contains a truncated allow ACE"));
                }
                let ace = unsafe { &*ace_pointer.cast::<ACCESS_ALLOWED_ACE>() };
                let sid = PSID(std::ptr::addr_of!(ace.SidStart).cast_mut().cast::<c_void>());
                if !unsafe { IsValidSid(sid).as_bool() } {
                    return Err(insecure(path, "DACL contains an invalid SID"));
                }
                let sid_length = unsafe { GetLengthSid(sid) } as usize;
                if SID_OFFSET + sid_length > usize::from(header.AceSize) {
                    return Err(insecure(path, "DACL contains an out-of-bounds SID"));
                }
                if access_mask_grants_write_like_access(ace.Mask)
                    && !is_privileged_sid(sid, &system, &admins)
                {
                    return Err(insecure(path, "a non-privileged SID has write-like access"));
                }
            }
            _ => return Err(insecure(path, "DACL contains an unsupported ACE type")),
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn verify_protected_object_acl(
    _path: &Path,
    _require_protected_dacl: bool,
) -> Result<(), MaterializationError> {
    Err(MaterializationError::SecurityVerificationUnavailable)
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_windows_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn category_name(category: DpiCategory) -> &'static str {
    match category {
        DpiCategory::Discord => "discord",
        DpiCategory::YoutubeTwitch => "youtube-twitch",
        DpiCategory::Gaming => "gaming",
        DpiCategory::AtRisk => "at-risk",
        DpiCategory::Universal => "universal",
    }
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

fn sha256_bytes(bytes: &[u8]) -> String {
    hex_digest(Sha256::digest(bytes))
}

fn launch_arguments_sha256(arguments: &[String]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"obsession/typed-launch-arguments/v1\0");
    for argument in arguments {
        digest.update((argument.len() as u64).to_le_bytes());
        digest.update(argument.as_bytes());
    }
    hex_digest(digest.finalize())
}

fn validate_launch_arguments(arguments: &[String]) -> Result<(), MaterializationError> {
    if arguments.is_empty() || arguments.len() > MAX_LAUNCH_ARGUMENTS {
        return Err(MaterializationError::InvalidConfig(
            "invalid launch argument count",
        ));
    }
    if arguments.iter().any(|argument| {
        argument.is_empty()
            || argument.len() > MAX_LAUNCH_ARGUMENT_BYTES
            || argument
                .chars()
                .any(|character| character == '\0' || character.is_control())
    }) {
        return Err(MaterializationError::InvalidConfig(
            "invalid launch argument",
        ));
    }
    Ok(())
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_paths_remove_only_windows_verbatim_filesystem_prefixes() {
        assert_eq!(
            engine_file_path(Path::new(r"\\?\C:\Program Files\Obsession\blob.bin")),
            r"C:\Program Files\Obsession\blob.bin"
        );
        assert_eq!(
            engine_file_path(Path::new(r"\\?\UNC\server\share\blob.bin")),
            r"\\server\share\blob.bin"
        );
        for unchanged in [
            r"C:\data\blob.bin",
            r"\\server\share\blob.bin",
            "/opt/obsession/blob.bin",
            r"\\?\Volume{example}\blob.bin",
        ] {
            assert_eq!(engine_file_path(Path::new(unchanged)), unchanged);
        }
    }
    use crate::protected_layout::{ProtectedLayout, RESOURCE_MANIFEST};
    use obsession_runtime_protocol::{DpiRuntimeOptions, DpiSelection, DpiStartRequest};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("obsession-materializer-test-{nonce}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_resource(root: &Path, relative: &str, bytes: &[u8]) -> serde_json::Value {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        serde_json::json!({
            "path": relative,
            "size": bytes.len(),
            "sha256": sha256_bytes(bytes),
        })
    }

    fn verified_legacy_plan(test: &TestRoot) -> (VerifiedDpiPlan, ProtectedDataLayout) {
        let program_files = test.0.join("Program Files");
        let install_root = program_files.join("Obsession");
        let program_data = test.0.join("ProgramData");
        let state_root = program_data.join(RUNTIME_STATE_RELATIVE);
        fs::create_dir_all(&program_files).unwrap();
        fs::create_dir_all(&state_root).unwrap();

        let config = b"--wf-tcp=80,443\n--hostlist=\"runtime\\lists\\discord.txt\" --hostlist-auto=runtime/autohosts/discord.txt --dpi-desync-fake-tls=runtime/bin/fake.bin --new\n";
        let files = vec![
            write_resource(&install_root, "runtime/legacy/winws.exe", b"engine"),
            write_resource(&install_root, "runtime/configs/discord.conf", config),
            write_resource(&install_root, "runtime/lists/discord.txt", b"discord.com\n"),
            write_resource(&install_root, "runtime/autohosts/discord.txt", b""),
            write_resource(&install_root, "runtime/bin/fake.bin", b"fake-packet"),
        ];
        let manifest_path = install_root.join(RESOURCE_MANIFEST);
        fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        fs::write(
            manifest_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "engines": [{
                    "engine": "legacy",
                    "executable": "runtime/legacy/winws.exe",
                    "files": files,
                    "strategies": [{
                        "id": "discord_1.conf",
                        "category": "discord",
                        "artifact": "runtime/configs/discord.conf",
                        "dependencies": [
                            "runtime/lists/discord.txt",
                            "runtime/autohosts/discord.txt",
                            "runtime/bin/fake.bin"
                        ]
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let install = ProtectedLayout::inspect(&program_files, &install_root).unwrap();
        let catalog = install.load_verified_catalog().unwrap();
        let plan = catalog
            .resolve_dpi_plan(&DpiStartRequest {
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
            })
            .unwrap();
        let state = ProtectedDataLayout::inspect(&program_data, &state_root).unwrap();
        (plan, state)
    }

    #[cfg(windows)]
    #[test]
    fn production_state_discovery_rejects_a_user_owned_directory() {
        let test = TestRoot::new();
        let program_data = test.0.join("ProgramData");
        fs::create_dir_all(program_data.join(RUNTIME_STATE_RELATIVE)).unwrap();

        assert!(matches!(
            ProtectedDataLayout::from_program_data(&program_data),
            Err(MaterializationError::InsecureAcl { .. })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn acl_write_mask_accepts_installer_read_execute_but_rejects_real_write_rights() {
        use windows::Win32::Foundation::{GENERIC_ALL, GENERIC_WRITE};
        use windows::Win32::Storage::FileSystem::{
            DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_DELETE_CHILD, FILE_WRITE_ATTRIBUTES,
            FILE_WRITE_EA, WRITE_DAC, WRITE_OWNER,
        };

        // Exact expanded mask observed for the installer's
        // BUILTIN\Users:(ReadAndExecute,Synchronize) ACE.
        const USERS_READ_EXECUTE: u32 = 0x0012_00A9;
        assert!(!access_mask_grants_write_like_access(USERS_READ_EXECUTE));

        for (name, mask) in [
            ("add file/write data", FILE_ADD_FILE.0),
            ("add subdirectory/append data", FILE_ADD_SUBDIRECTORY.0),
            ("write extended attributes", FILE_WRITE_EA.0),
            ("write attributes", FILE_WRITE_ATTRIBUTES.0),
            ("delete child", FILE_DELETE_CHILD.0),
            ("delete", DELETE.0),
            ("write DACL", WRITE_DAC.0),
            ("write owner", WRITE_OWNER.0),
            ("generic write", GENERIC_WRITE.0),
            ("generic all", GENERIC_ALL.0),
        ] {
            assert!(
                access_mask_grants_write_like_access(mask),
                "{name} must be treated as write-like"
            );
        }
    }

    #[test]
    fn tokenizer_removes_comments_but_never_creates_free_arguments() {
        let tokens = tokenize_config(
            "# header\n--wf-tcp=80,443 --hostlist=\"lists\\discord.txt\" # tail\n--new",
        )
        .unwrap();
        assert_eq!(
            tokens,
            ["--wf-tcp=80,443", "--hostlist=lists\\discord.txt", "--new"]
        );
        assert!(tokenize_config("--hostlist=\"unterminated").is_err());
        assert_eq!(
            tokenize_config("--dpi-desync-hostfakesplit-mod=host##www.google.com # comment")
                .unwrap(),
            ["--dpi-desync-hostfakesplit-mod=host##www.google.com"]
        );
    }

    #[test]
    fn scalar_ranges_are_not_confused_with_windows_drive_paths() {
        assert!(!looks_like_path("1:6"));
        assert!(!looks_like_path("::1"));
        assert!(looks_like_path(r"C:payload"));
        assert!(looks_like_path(r"C:\payload"));
        assert!(looks_like_path(r"..\payload"));
    }

    #[test]
    fn path_detection_requires_manifest_dependencies() {
        assert_eq!(
            normalize_reference(r"lists\discord.txt").as_deref(),
            Some("lists/discord.txt")
        );
        assert!(looks_like_path(r"..\payload.exe"));
        assert!(looks_like_path(r"C:\payload.exe"));
        assert!(option_requires_resource("--hostlist-auto"));
        assert!(!option_requires_resource("--dpi-desync-fake-tls-mod"));
    }

    #[test]
    fn dynamic_hostlist_validation_is_bounded_and_strict() {
        assert!(validate_auto_hostlist(b"discord.com\ncdn.discord.com\n").is_ok());
        assert!(validate_auto_hostlist(b"../payload\n").is_err());
        assert!(validate_auto_hostlist(b"duplicate.example\nDUPLICATE.example\n").is_err());
        assert!(validate_auto_hostlist(&[]).is_ok());
    }

    #[test]
    fn every_bundled_legacy_config_is_accepted_by_the_bounded_lexer() {
        fn visit(directory: &Path, configs: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    visit(&path, configs);
                } else if path.extension().and_then(|value| value.to_str()) == Some("conf") {
                    configs.push(path);
                }
            }
        }

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src-tauri/resources/configs");
        let mut configs = Vec::new();
        visit(&root, &mut configs);
        assert!(!configs.is_empty());
        for path in configs {
            let source = fs::read_to_string(&path).unwrap();
            tokenize_config(&source).unwrap_or_else(|error| {
                panic!("{} failed bounded parsing: {error}", path.display())
            });
        }
    }

    #[test]
    fn materialization_rewrites_every_resource_and_keeps_mutable_state_in_program_data() {
        let test = TestRoot::new();
        let (plan, state) = verified_legacy_plan(&test);
        let materialized = state.materialize(&plan, 7).unwrap();
        assert_eq!(materialized.generation(), 7);
        assert_eq!(materialized.fingerprint().len(), 64);
        assert_eq!(materialized.launches().len(), 1);

        let launch = &materialized.launches()[0];
        let config = fs::read_to_string(launch.response_file().unwrap()).unwrap();
        assert!(config.contains(&engine_file_path(
            &plan.root().join("runtime/lists/discord.txt")
        )));
        assert!(config.contains(&engine_file_path(&plan.root().join("runtime/bin/fake.bin"))));
        assert!(config.contains(&engine_file_path(
            &state.root().join("dpi/autohosts/discord")
        )));
        assert!(!config.contains(r"\\?\"));
        assert!(!launch.arguments()[0].contains(r"\\?\"));
        assert!(!config.contains("=\"runtime\\"));
        materialized.reverify().unwrap();

        fs::write(
            launch.response_file().unwrap(),
            "--hostlist=C:\\payload.txt",
        )
        .unwrap();
        assert!(matches!(
            materialized.reverify(),
            Err(MaterializationError::InvalidConfig(
                "materialized config integrity mismatch"
            ))
        ));
        state.cleanup_generation(&materialized).unwrap();
        assert!(!materialized.directory().exists());
    }
}
