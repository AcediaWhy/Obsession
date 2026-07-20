//! Pure Legacy target attribution and TCP capture-plan construction.
//!
//! The registry consumes already-loaded config and hostlist contents. It never
//! resolves paths, rewrites configs, or combines independent winws processes.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use sha2::{Digest, Sha256};

use super::contracts::RegistryVersion;

const CONTENT_HASH_DOMAIN: &[u8] = b"obsession/legacy-target-registry-content/v1";
const VERSION_HASH_DOMAIN: &[u8] = b"obsession/legacy-target-registry-version/v1";
const MAX_PORT_TERMS_PER_CONFIG: usize = 4_096;

/// One independent Legacy `.conf` and the contents of lists it references.
///
/// Keys in `referenced_hostlists` are the literal paths used by winws (for
/// example `lists\\discord.txt`). Path separators and ASCII case are normalized
/// by the registry. A missing `--hostlist-auto` is accepted because the file may
/// not have been learned yet; regular and exclusion hostlists are required.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyConfigRecord {
    pub category: String,
    pub config_name: String,
    pub config_content: String,
    pub referenced_hostlists: BTreeMap<String, String>,
}

impl LegacyConfigRecord {
    pub fn new(
        category: impl Into<String>,
        config_name: impl Into<String>,
        config_content: impl Into<String>,
    ) -> Self {
        Self {
            category: category.into(),
            config_name: config_name.into(),
            config_content: config_content.into(),
            referenced_hostlists: BTreeMap::new(),
        }
    }

    pub fn with_hostlist(
        mut self,
        reference: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        self.referenced_hostlists
            .insert(reference.into(), content.into());
        self
    }
}

/// Short alias for call sites that are already scoped to Legacy reliability.
pub type ConfigRecord = LegacyConfigRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub const fn new(start: u16, end: u16) -> Option<Self> {
        if start == 0 || start > end {
            None
        } else {
            Some(Self { start, end })
        }
    }

    pub const fn contains(self, port: u16) -> bool {
        self.start <= port && port <= self.end
    }
}

/// Compact, sorted union of TCP ports observed by Legacy Eyes.
///
/// Ranges are kept compact instead of being expanded into up to 65,535 ports.
/// UDP options are deliberately absent from this type.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PortPlan {
    tcp_ranges: Vec<PortRange>,
}

impl PortPlan {
    pub fn parse(config_content: &str) -> Result<Self, ConfigParseError> {
        Ok(parse_legacy_config(config_content)?.tcp_ports)
    }

    pub fn tcp_ranges(&self) -> &[PortRange] {
        &self.tcp_ranges
    }

    pub fn contains(&self, port: u16) -> bool {
        self.tcp_ranges.iter().any(|range| range.contains(port))
    }

    pub fn is_empty(&self) -> bool {
        self.tcp_ranges.is_empty()
    }

    /// Renders a bounded WinDivert filter without expanding large ranges.
    /// Empty plans return None and must not silently become a watch-all filter.
    pub fn to_windivert_filter(&self) -> Option<String> {
        if self.tcp_ranges.is_empty() {
            return None;
        }
        let outbound = render_port_terms(&self.tcp_ranges, "tcp.DstPort");
        let inbound = render_port_terms(&self.tcp_ranges, "tcp.SrcPort");
        Some(format!(
            "tcp and ((outbound and ({outbound})) or (inbound and ({inbound})))"
        ))
    }

    fn add_ranges(&mut self, ranges: impl IntoIterator<Item = PortRange>) {
        self.tcp_ranges.extend(ranges);
        self.tcp_ranges.sort_unstable();

        let mut compacted: Vec<PortRange> = Vec::with_capacity(self.tcp_ranges.len());
        for range in self.tcp_ranges.drain(..) {
            if let Some(previous) = compacted.last_mut() {
                if range.start <= previous.end.saturating_add(1) {
                    previous.end = previous.end.max(range.end);
                    continue;
                }
            }
            compacted.push(range);
        }
        self.tcp_ranges = compacted;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HostlistKind {
    Include,
    AutoInclude,
    Exclude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostlistReferenceError {
    Empty,
    AbsoluteOrDriveQualified,
    Traversal,
    ControlCharacter,
}

impl fmt::Display for HostlistReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "hostlist reference is empty"),
            Self::AbsoluteOrDriveQualified => {
                write!(f, "hostlist reference must be relative and drive-free")
            }
            Self::Traversal => write!(f, "hostlist reference contains parent traversal"),
            Self::ControlCharacter => {
                write!(f, "hostlist reference contains a control character")
            }
        }
    }
}

impl Error for HostlistReferenceError {}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostlistReference {
    pub kind: HostlistKind,
    pub reference: String,
}

/// Parsed metadata needed by the registry. No filesystem paths are resolved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedLegacyConfig {
    pub tcp_ports: PortPlan,
    pub hostlists: Vec<HostlistReference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigParseError {
    UnterminatedQuote {
        line: usize,
    },
    MissingOptionValue {
        option: String,
    },
    InvalidPortTerm {
        option: String,
        term: String,
    },
    InvalidHostlistReference {
        option: String,
        reference: String,
        source: HostlistReferenceError,
    },
    TooManyPortTerms {
        limit: usize,
    },
}

impl fmt::Display for ConfigParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnterminatedQuote { line } => {
                write!(f, "unterminated quote on config line {line}")
            }
            Self::MissingOptionValue { option } => {
                write!(f, "missing value for {option}")
            }
            Self::InvalidPortTerm { option, term } => {
                write!(f, "invalid TCP port term {term:?} in {option}")
            }
            Self::InvalidHostlistReference {
                option,
                reference,
                source,
            } => write!(
                f,
                "invalid hostlist reference {reference:?} in {option}: {source}"
            ),
            Self::TooManyPortTerms { limit } => {
                write!(f, "TCP port plan exceeds the limit of {limit} terms")
            }
        }
    }
}

impl Error for ConfigParseError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostlistParseError {
    InvalidDomain { line: usize, value: String },
}

impl fmt::Display for HostlistParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDomain { line, value } => {
                write!(f, "invalid domain {value:?} on hostlist line {line}")
            }
        }
    }
}

impl Error for HostlistParseError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryBuildError {
    EmptyCategory {
        config_name: String,
    },
    EmptyConfigName {
        category: String,
    },
    DuplicateConfig {
        category: String,
        config_name: String,
    },
    ConflictingHostlistContent {
        category: String,
        config_name: String,
        reference: String,
    },
    InvalidProvidedHostlistReference {
        category: String,
        config_name: String,
        reference: String,
        source: HostlistReferenceError,
    },
    InvalidConfig {
        category: String,
        config_name: String,
        source: ConfigParseError,
    },
    MissingHostlist {
        category: String,
        config_name: String,
        reference: String,
        kind: HostlistKind,
    },
    EmptyRequiredHostlist {
        category: String,
        config_name: String,
        reference: String,
    },
    InvalidHostlist {
        category: String,
        config_name: String,
        reference: String,
        source: HostlistParseError,
    },
    ConflictingActiveSelection {
        category: String,
        first_config: String,
        second_config: String,
    },
    UnknownActiveConfig {
        category: String,
        config_name: String,
    },
    NoTargets,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapturePlanError {
    EmptySelection,
    ConflictingSelection {
        category: String,
        first_config: String,
        second_config: String,
    },
    UnknownConfig {
        category: String,
        config_name: String,
    },
}

impl fmt::Display for CapturePlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySelection => f.write_str("capture plan requires at least one selection"),
            Self::ConflictingSelection {
                category,
                first_config,
                second_config,
            } => write!(
                f,
                "conflicting capture selections for {category:?}: {first_config:?} and {second_config:?}"
            ),
            Self::UnknownConfig {
                category,
                config_name,
            } => write!(
                f,
                "capture selection {config_name:?} is not present in category {category:?}"
            ),
        }
    }
}

impl Error for CapturePlanError {}

impl fmt::Display for RegistryBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCategory { config_name } => {
                write!(f, "empty category for config {config_name:?}")
            }
            Self::EmptyConfigName { category } => {
                write!(f, "empty config name in category {category:?}")
            }
            Self::DuplicateConfig {
                category,
                config_name,
            } => write!(f, "duplicate Legacy config {category}/{config_name}"),
            Self::ConflictingHostlistContent {
                category,
                config_name,
                reference,
            } => write!(
                f,
                "conflicting contents for hostlist {reference:?} in {category}/{config_name}"
            ),
            Self::InvalidProvidedHostlistReference {
                category,
                config_name,
                reference,
                source,
            } => write!(
                f,
                "invalid supplied hostlist reference {reference:?} for {category}/{config_name}: {source}"
            ),
            Self::InvalidConfig {
                category,
                config_name,
                source,
            } => write!(f, "invalid config {category}/{config_name}: {source}"),
            Self::MissingHostlist {
                category,
                config_name,
                reference,
                kind,
            } => write!(
                f,
                "missing {kind:?} hostlist {reference:?} for {category}/{config_name}"
            ),
            Self::EmptyRequiredHostlist {
                category,
                config_name,
                reference,
            } => write!(
                f,
                "required hostlist {reference:?} is empty for {category}/{config_name}"
            ),
            Self::InvalidHostlist {
                category,
                config_name,
                reference,
                source,
            } => write!(
                f,
                "invalid hostlist {reference:?} for {category}/{config_name}: {source}"
            ),
            Self::ConflictingActiveSelection {
                category,
                first_config,
                second_config,
            } => write!(
                f,
                "conflicting active Legacy selections for {category:?}: {first_config:?} and {second_config:?}"
            ),
            Self::UnknownActiveConfig {
                category,
                config_name,
            } => write!(
                f,
                "active Legacy config {config_name:?} is not present in category {category:?}"
            ),
            Self::NoTargets => write!(f, "Legacy target registry has no eligible domains"),
        }
    }
}

impl Error for RegistryBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfig { source, .. } => Some(source),
            Self::InvalidProvidedHostlistReference { source, .. } => Some(source),
            Self::InvalidHostlist { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetOwner {
    pub category: String,
    pub config_names: Vec<String>,
    pub active_config: Option<String>,
}

/// Result of label-boundary suffix attribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attribution {
    Matched {
        target: String,
        owner: TargetOwner,
    },
    Ambiguous {
        target: String,
        owners: Vec<TargetOwner>,
    },
    Excluded {
        target: String,
        owners: Vec<TargetOwner>,
    },
    Unmatched,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct OwnerKey {
    category: String,
    config_name: String,
}

#[derive(Debug)]
struct PreparedRecord {
    category: String,
    config_name: String,
    config_content: String,
    hostlists: BTreeMap<String, String>,
    parsed: ParsedLegacyConfig,
}

/// Immutable attribution snapshot shared by Eyes and the reliability manager.
#[derive(Clone, Debug)]
pub struct TargetRegistry {
    version: RegistryVersion,
    content_hash: [u8; 32],
    content_hash_hex: String,
    targets: BTreeMap<String, BTreeSet<OwnerKey>>,
    exclusions: BTreeMap<String, BTreeSet<OwnerKey>>,
    config_port_plans: BTreeMap<OwnerKey, PortPlan>,
    active_selections: BTreeMap<String, String>,
    port_plan: PortPlan,
}

impl TargetRegistry {
    pub fn from_records<I, R>(records: I) -> Result<Self, RegistryBuildError>
    where
        I: IntoIterator<Item = R>,
        R: Borrow<LegacyConfigRecord>,
    {
        Self::build(records, BTreeMap::new())
    }

    /// Builds an immutable registry snapshot and binds the active config for
    /// every category into its identity. Candidate records remain independent;
    /// the active map only affects ownership metadata and the registry version.
    pub fn from_records_with_active_selections<I, R, S, C, F>(
        records: I,
        active_selections: S,
    ) -> Result<Self, RegistryBuildError>
    where
        I: IntoIterator<Item = R>,
        R: Borrow<LegacyConfigRecord>,
        S: IntoIterator<Item = (C, F)>,
        C: AsRef<str>,
        F: AsRef<str>,
    {
        let active_selections = normalize_active_selections(active_selections)?;
        Self::build(records, active_selections)
    }

    fn build<I, R>(
        records: I,
        active_selections: BTreeMap<String, String>,
    ) -> Result<Self, RegistryBuildError>
    where
        I: IntoIterator<Item = R>,
        R: Borrow<LegacyConfigRecord>,
    {
        let mut prepared = prepare_records(records)?;
        prepared.sort_by(|left, right| {
            (&left.category, &left.config_name).cmp(&(&right.category, &right.config_name))
        });

        let mut targets: BTreeMap<String, BTreeSet<OwnerKey>> = BTreeMap::new();
        let mut exclusions: BTreeMap<String, BTreeSet<OwnerKey>> = BTreeMap::new();
        let mut config_port_plans: BTreeMap<OwnerKey, PortPlan> = BTreeMap::new();
        let mut port_plan = PortPlan::default();
        let mut hasher = Sha256::new();
        hash_field(&mut hasher, CONTENT_HASH_DOMAIN);
        hash_field(&mut hasher, &(prepared.len() as u64).to_be_bytes());

        for record in &prepared {
            hash_field(&mut hasher, record.category.as_bytes());
            hash_field(&mut hasher, record.config_name.as_bytes());
            hash_field(&mut hasher, record.config_content.as_bytes());
            hash_field(
                &mut hasher,
                &(record.parsed.hostlists.len() as u64).to_be_bytes(),
            );

            port_plan.add_ranges(record.parsed.tcp_ports.tcp_ranges.iter().copied());
            let owner = OwnerKey {
                category: record.category.clone(),
                config_name: record.config_name.clone(),
            };
            config_port_plans.insert(owner.clone(), record.parsed.tcp_ports.clone());

            for reference in &record.parsed.hostlists {
                hash_field(&mut hasher, &[hostlist_kind_tag(reference.kind)]);
                hash_field(&mut hasher, reference.reference.as_bytes());
                let content = record.hostlists.get(&reference.reference);
                hash_field(&mut hasher, &[u8::from(content.is_some())]);
                if let Some(content) = content {
                    hash_field(&mut hasher, content.as_bytes());
                    let domains = parse_hostlist(content).map_err(|source| {
                        RegistryBuildError::InvalidHostlist {
                            category: record.category.clone(),
                            config_name: record.config_name.clone(),
                            reference: reference.reference.clone(),
                            source,
                        }
                    })?;
                    if reference.kind == HostlistKind::Include && domains.is_empty() {
                        return Err(RegistryBuildError::EmptyRequiredHostlist {
                            category: record.category.clone(),
                            config_name: record.config_name.clone(),
                            reference: reference.reference.clone(),
                        });
                    }
                    let destination = match reference.kind {
                        HostlistKind::Include | HostlistKind::AutoInclude => &mut targets,
                        HostlistKind::Exclude => &mut exclusions,
                    };
                    for domain in domains {
                        destination.entry(domain).or_default().insert(owner.clone());
                    }
                } else if reference.kind != HostlistKind::AutoInclude {
                    return Err(RegistryBuildError::MissingHostlist {
                        category: record.category.clone(),
                        config_name: record.config_name.clone(),
                        reference: reference.reference.clone(),
                        kind: reference.kind,
                    });
                }
            }
        }

        if targets.is_empty() {
            return Err(RegistryBuildError::NoTargets);
        }

        for (category, config_name) in &active_selections {
            let key = OwnerKey {
                category: category.clone(),
                config_name: config_name.clone(),
            };
            if !config_port_plans.contains_key(&key) {
                return Err(RegistryBuildError::UnknownActiveConfig {
                    category: category.clone(),
                    config_name: config_name.clone(),
                });
            }
        }

        let content_hash: [u8; 32] = hasher.finalize().into();
        let mut version_hasher = Sha256::new();
        hash_field(&mut version_hasher, VERSION_HASH_DOMAIN);
        hash_field(&mut version_hasher, &content_hash);
        hash_field(
            &mut version_hasher,
            &(active_selections.len() as u64).to_be_bytes(),
        );
        for (category, config_name) in &active_selections {
            hash_field(&mut version_hasher, category.as_bytes());
            hash_field(&mut version_hasher, config_name.as_bytes());
        }
        let version_hash: [u8; 32] = version_hasher.finalize().into();
        let version_value = u64::from_be_bytes(
            version_hash[..8]
                .try_into()
                .expect("SHA-256 prefix always contains eight bytes"),
        )
        .max(1);
        let content_hash_hex = hex_digest(&content_hash);

        Ok(Self {
            version: version_value.into(),
            content_hash,
            content_hash_hex,
            targets,
            exclusions,
            config_port_plans,
            active_selections,
            port_plan,
        })
    }

    pub fn version(&self) -> RegistryVersion {
        self.version
    }

    pub fn content_hash(&self) -> &[u8; 32] {
        &self.content_hash
    }

    pub fn content_hash_hex(&self) -> &str {
        &self.content_hash_hex
    }

    /// Returns the config bound as active for a category in this snapshot.
    pub fn active_config(&self, category: &str) -> Option<&str> {
        self.active_selections
            .get(&category.trim().to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn active_selections(&self) -> impl Iterator<Item = (&str, &str)> {
        self.active_selections
            .iter()
            .map(|(category, config_name)| (category.as_str(), config_name.as_str()))
    }

    pub fn active_capture_plan(&self) -> Result<PortPlan, CapturePlanError> {
        self.capture_plan_for(self.active_selections())
    }

    /// Union across the full candidate snapshot. Runtime capture must use
    /// `active_capture_plan` (or an explicitly fenced `capture_plan_for`).
    pub fn port_plan(&self) -> &PortPlan {
        &self.port_plan
    }

    /// Builds the runtime capture plan from configs that are active now. The
    /// registry may contain broad future candidates, but they must not expand
    /// WinDivert capture until a fenced attempt actually selects them.
    pub fn capture_plan_for<I, C, F>(&self, selections: I) -> Result<PortPlan, CapturePlanError>
    where
        I: IntoIterator<Item = (C, F)>,
        C: AsRef<str>,
        F: AsRef<str>,
    {
        let mut selected_any = false;
        let mut plan = PortPlan::default();
        let mut normalized = BTreeMap::new();
        for (category, config_name) in selections {
            selected_any = true;
            let category = category.as_ref().trim().to_ascii_lowercase();
            let config_name = config_name.as_ref().trim().to_string();
            if let Some(previous) = normalized.insert(category.clone(), config_name.clone()) {
                if previous != config_name {
                    return Err(CapturePlanError::ConflictingSelection {
                        category,
                        first_config: previous,
                        second_config: config_name,
                    });
                }
            }
        }
        for (category, config_name) in normalized {
            let key = OwnerKey {
                category: category.clone(),
                config_name: config_name.clone(),
            };
            let Some(config_plan) = self.config_port_plans.get(&key) else {
                return Err(CapturePlanError::UnknownConfig {
                    category,
                    config_name,
                });
            };
            plan.add_ranges(config_plan.tcp_ranges.iter().copied());
        }
        if !selected_any {
            return Err(CapturePlanError::EmptySelection);
        }
        Ok(plan)
    }

    pub fn is_usable(&self) -> bool {
        !self.targets.is_empty() && !self.port_plan.is_empty()
    }

    pub fn target_count(&self) -> usize {
        self.targets.len()
    }

    pub fn target_suffixes(&self) -> impl Iterator<Item = &str> {
        self.targets.keys().map(String::as_str)
    }

    /// Returns only domains owned by each category's bound active config.
    /// Future candidates remain in the registry for attribution and scoped
    /// attempts, but must not broaden the active Eyes hostlist.
    pub fn active_target_suffixes(&self) -> impl Iterator<Item = &str> {
        self.targets.iter().filter_map(|(target, owners)| {
            owners
                .iter()
                .any(|owner| {
                    self.active_selections
                        .get(&owner.category)
                        .is_some_and(|active| active == &owner.config_name)
                })
                .then_some(target.as_str())
        })
    }

    pub fn attribute(&self, domain: &str) -> Attribution {
        self.attribute_scoped(domain, false)
    }

    /// Attributes runtime traffic only to configs bound active in this
    /// snapshot. Inactive candidate suffixes remain available through
    /// [`Self::attribute`] for diagnostics and fenced candidate preflight, but
    /// cannot steal a flow from an active lane through a longer suffix.
    pub fn attribute_active(&self, domain: &str) -> Attribution {
        self.attribute_scoped(domain, true)
    }

    fn attribute_scoped(&self, domain: &str, active_only: bool) -> Attribution {
        let Some(domain) = normalize_domain(domain) else {
            return Attribution::Unmatched;
        };

        let suffixes = suffixes(&domain);
        let mut excluded_owners = BTreeSet::new();
        for suffix in &suffixes {
            if let Some(owners) = self.exclusions.get(*suffix) {
                excluded_owners.extend(
                    owners
                        .iter()
                        .filter(|owner| self.owner_in_scope(owner, active_only))
                        .cloned(),
                );
            }
        }

        let mut deepest_excluded: Option<(&str, Vec<TargetOwner>)> = None;
        for suffix in suffixes {
            let Some(owners) = self.targets.get(suffix) else {
                continue;
            };

            let scoped = owners
                .iter()
                .filter(|owner| self.owner_in_scope(owner, active_only))
                .collect::<Vec<_>>();
            if scoped.is_empty() {
                continue;
            }

            let eligible: Vec<&OwnerKey> = scoped
                .iter()
                .copied()
                .filter(|owner| !excluded_owners.contains(*owner))
                .collect();
            if eligible.is_empty() {
                if deepest_excluded.is_none() {
                    deepest_excluded = Some((
                        suffix,
                        group_owners(scoped.iter().copied(), &self.active_selections),
                    ));
                }
                continue;
            }

            let grouped = group_owners(eligible, &self.active_selections);
            return if grouped.len() == 1 {
                Attribution::Matched {
                    target: suffix.to_string(),
                    owner: grouped.into_iter().next().expect("one owner group"),
                }
            } else {
                Attribution::Ambiguous {
                    target: suffix.to_string(),
                    owners: grouped,
                }
            };
        }

        match deepest_excluded {
            Some((target, owners)) => Attribution::Excluded {
                target: target.to_string(),
                owners,
            },
            None => Attribution::Unmatched,
        }
    }

    fn owner_in_scope(&self, owner: &OwnerKey, active_only: bool) -> bool {
        !active_only
            || self
                .active_selections
                .get(&owner.category)
                .is_some_and(|active| active == &owner.config_name)
    }
}

/// Parses TCP capture options and hostlist references from raw `.conf` text.
/// `--wf-udp` and `--filter-udp` are intentionally ignored.
pub fn parse_legacy_config(content: &str) -> Result<ParsedLegacyConfig, ConfigParseError> {
    let tokens = tokenize_config(content)?;
    let mut wf_tcp_ranges = Vec::new();
    let mut filter_tcp_ranges = Vec::new();
    let mut has_wf_tcp = false;
    let mut port_terms = 0usize;
    let mut hostlists = BTreeSet::new();

    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        let (option, inline_value) = match token.value.split_once('=') {
            Some((option, value)) => (option, Some(value)),
            None => (token.value.as_str(), None),
        };

        let hostlist_kind = match option {
            "--hostlist" => Some(HostlistKind::Include),
            "--hostlist-auto" => Some(HostlistKind::AutoInclude),
            "--hostlist-exclude" => Some(HostlistKind::Exclude),
            _ => None,
        };
        let is_tcp_port_option = matches!(option, "--wf-tcp" | "--filter-tcp");

        if hostlist_kind.is_some() || is_tcp_port_option {
            let (value, consumed_next) = option_value(&tokens, index, option, inline_value)?;
            if let Some(kind) = hostlist_kind {
                let reference = normalize_hostlist_reference(value).map_err(|source| {
                    ConfigParseError::InvalidHostlistReference {
                        option: option.to_string(),
                        reference: value.to_string(),
                        source,
                    }
                })?;
                hostlists.insert(HostlistReference { kind, reference });
            } else {
                let ranges = if option == "--wf-tcp" {
                    has_wf_tcp = true;
                    &mut wf_tcp_ranges
                } else {
                    &mut filter_tcp_ranges
                };
                parse_port_spec(option, value, ranges, &mut port_terms)?;
            }
            if consumed_next {
                index += 1;
            }
        }
        index += 1;
    }

    let mut tcp_ports = PortPlan::default();
    tcp_ports.add_ranges(if has_wf_tcp {
        wf_tcp_ranges
    } else {
        filter_tcp_ranges
    });
    Ok(ParsedLegacyConfig {
        tcp_ports,
        hostlists: hostlists.into_iter().collect(),
    })
}

/// Parses and normalizes a hostlist without consulting the filesystem.
pub fn parse_hostlist(content: &str) -> Result<BTreeSet<String>, HostlistParseError> {
    let mut domains = BTreeSet::new();
    for (index, raw_line) in content.lines().enumerate() {
        let value = raw_line
            .trim_start_matches('\u{feff}')
            .split_once('#')
            .map_or(raw_line, |(value, _)| value)
            .trim();
        if value.is_empty() {
            continue;
        }
        let Some(domain) = normalize_domain(value) else {
            return Err(HostlistParseError::InvalidDomain {
                line: index + 1,
                value: value.to_string(),
            });
        };
        domains.insert(domain);
    }
    Ok(domains)
}

/// Normalizes SNI/hostlist domains while preserving label boundaries.
pub fn normalize_domain(value: &str) -> Option<String> {
    let mut domain = value.trim().trim_start_matches('\u{feff}');
    if let Some(stripped) = domain.strip_prefix("*.") {
        domain = stripped;
    }
    domain = domain.strip_prefix('.').unwrap_or(domain);
    domain = domain.trim_end_matches('.');
    if domain.is_empty() || domain.len() > 253 || !domain.contains('.') {
        return None;
    }

    if domain.split('.').any(|label| {
        label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }) {
        return None;
    }
    Some(domain.to_ascii_lowercase())
}

fn prepare_records<I, R>(records: I) -> Result<Vec<PreparedRecord>, RegistryBuildError>
where
    I: IntoIterator<Item = R>,
    R: Borrow<LegacyConfigRecord>,
{
    let mut prepared = Vec::new();
    let mut identities = BTreeSet::new();
    for record in records {
        let record = record.borrow();
        let category = record.category.trim().to_ascii_lowercase();
        let config_name = record.config_name.trim().to_string();
        if category.is_empty() {
            return Err(RegistryBuildError::EmptyCategory { config_name });
        }
        if config_name.is_empty() {
            return Err(RegistryBuildError::EmptyConfigName { category });
        }
        if !identities.insert((category.clone(), config_name.to_ascii_lowercase())) {
            return Err(RegistryBuildError::DuplicateConfig {
                category,
                config_name,
            });
        }

        let mut hostlists = BTreeMap::new();
        for (reference, content) in &record.referenced_hostlists {
            let reference = normalize_hostlist_reference(reference).map_err(|source| {
                RegistryBuildError::InvalidProvidedHostlistReference {
                    category: category.clone(),
                    config_name: config_name.clone(),
                    reference: reference.clone(),
                    source,
                }
            })?;
            if let Some(previous) = hostlists.insert(reference.clone(), content.clone()) {
                if previous != *content {
                    return Err(RegistryBuildError::ConflictingHostlistContent {
                        category,
                        config_name,
                        reference,
                    });
                }
            }
        }

        let parsed = parse_legacy_config(&record.config_content).map_err(|source| {
            RegistryBuildError::InvalidConfig {
                category: category.clone(),
                config_name: config_name.clone(),
                source,
            }
        })?;
        prepared.push(PreparedRecord {
            category,
            config_name,
            config_content: record.config_content.clone(),
            hostlists,
            parsed,
        });
    }
    Ok(prepared)
}

fn normalize_active_selections<I, C, F>(
    active_selections: I,
) -> Result<BTreeMap<String, String>, RegistryBuildError>
where
    I: IntoIterator<Item = (C, F)>,
    C: AsRef<str>,
    F: AsRef<str>,
{
    let mut normalized = BTreeMap::new();
    for (category, config_name) in active_selections {
        let category = category.as_ref().trim().to_ascii_lowercase();
        let config_name = config_name.as_ref().trim().to_string();
        if category.is_empty() {
            return Err(RegistryBuildError::EmptyCategory { config_name });
        }
        if config_name.is_empty() {
            return Err(RegistryBuildError::EmptyConfigName { category });
        }
        if let Some(previous) = normalized.insert(category.clone(), config_name.clone()) {
            if previous != config_name {
                return Err(RegistryBuildError::ConflictingActiveSelection {
                    category,
                    first_config: previous,
                    second_config: config_name,
                });
            }
        }
    }
    Ok(normalized)
}

#[derive(Debug)]
struct ConfigToken {
    line: usize,
    value: String,
}

fn tokenize_config(content: &str) -> Result<Vec<ConfigToken>, ConfigParseError> {
    let mut tokens = Vec::new();
    for (line_index, raw_line) in content.lines().enumerate() {
        let mut current = String::new();
        let mut quote = None;
        for ch in raw_line.trim_start_matches('\u{feff}').chars() {
            if let Some(delimiter) = quote {
                if ch == delimiter {
                    quote = None;
                } else {
                    current.push(ch);
                }
                continue;
            }
            match ch {
                '"' | '\'' => quote = Some(ch),
                '#' => break,
                ch if ch.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(ConfigToken {
                            line: line_index + 1,
                            value: std::mem::take(&mut current),
                        });
                    }
                }
                _ => current.push(ch),
            }
        }
        if quote.is_some() {
            return Err(ConfigParseError::UnterminatedQuote {
                line: line_index + 1,
            });
        }
        if !current.is_empty() {
            tokens.push(ConfigToken {
                line: line_index + 1,
                value: current,
            });
        }
    }
    Ok(tokens)
}

fn option_value<'a>(
    tokens: &'a [ConfigToken],
    index: usize,
    option: &str,
    inline_value: Option<&'a str>,
) -> Result<(&'a str, bool), ConfigParseError> {
    if let Some(value) = inline_value {
        if !value.is_empty() {
            return Ok((value, false));
        }
    } else if let Some(next) = tokens.get(index + 1) {
        if next.line == tokens[index].line && !next.value.starts_with("--") {
            return Ok((&next.value, true));
        }
    }
    Err(ConfigParseError::MissingOptionValue {
        option: option.to_string(),
    })
}

fn parse_port_spec(
    option: &str,
    value: &str,
    output: &mut Vec<PortRange>,
    term_count: &mut usize,
) -> Result<(), ConfigParseError> {
    for raw_term in value.split(',') {
        *term_count += 1;
        if *term_count > MAX_PORT_TERMS_PER_CONFIG {
            return Err(ConfigParseError::TooManyPortTerms {
                limit: MAX_PORT_TERMS_PER_CONFIG,
            });
        }
        let term = raw_term.trim();
        let parsed = if let Some((start, end)) = term.split_once('-') {
            let start = start.parse::<u16>().ok();
            let end = end.parse::<u16>().ok();
            start
                .zip(end)
                .and_then(|(start, end)| PortRange::new(start, end))
        } else {
            term.parse::<u16>()
                .ok()
                .and_then(|port| PortRange::new(port, port))
        };
        let Some(range) = parsed else {
            return Err(ConfigParseError::InvalidPortTerm {
                option: option.to_string(),
                term: term.to_string(),
            });
        };
        output.push(range);
    }
    Ok(())
}

fn normalize_hostlist_reference(value: &str) -> Result<String, HostlistReferenceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(HostlistReferenceError::Empty);
    }
    if value.chars().any(char::is_control) {
        return Err(HostlistReferenceError::ControlCharacter);
    }

    let replaced = value.replace('\\', "/");
    if replaced.starts_with('/') || replaced.contains(':') {
        return Err(HostlistReferenceError::AbsoluteOrDriveQualified);
    }
    let mut parts = Vec::new();
    for part in replaced.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(HostlistReferenceError::Traversal),
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        Err(HostlistReferenceError::Empty)
    } else {
        Ok(parts.join("/").to_ascii_lowercase())
    }
}

fn suffixes(domain: &str) -> Vec<&str> {
    let mut suffixes = Vec::new();
    let mut suffix = domain;
    loop {
        suffixes.push(suffix);
        let Some(dot) = suffix.find('.') else {
            break;
        };
        suffix = &suffix[dot + 1..];
    }
    suffixes
}

fn group_owners<'a>(
    owners: impl IntoIterator<Item = &'a OwnerKey>,
    active_selections: &BTreeMap<String, String>,
) -> Vec<TargetOwner> {
    let mut grouped: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for owner in owners {
        grouped
            .entry(&owner.category)
            .or_default()
            .insert(&owner.config_name);
    }
    grouped
        .into_iter()
        .map(|(category, config_names)| TargetOwner {
            category: category.to_string(),
            config_names: config_names.into_iter().map(str::to_string).collect(),
            active_config: active_selections.get(category).cloned(),
        })
        .collect()
}

fn render_port_terms(ranges: &[PortRange], field: &str) -> String {
    ranges
        .iter()
        .map(|range| {
            if range.start == range.end {
                format!("{field} == {}", range.start)
            } else {
                format!("({field} >= {} and {field} <= {})", range.start, range.end)
            }
        })
        .collect::<Vec<_>>()
        .join(" or ")
}

fn hostlist_kind_tag(kind: HostlistKind) -> u8 {
    match kind {
        HostlistKind::Include => 1,
        HostlistKind::AutoInclude => 2,
        HostlistKind::Exclude => 3,
    }
}

fn hash_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn hex_digest(digest: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        category: &str,
        config_name: &str,
        config_content: &str,
        hostlists: &[(&str, &str)],
    ) -> LegacyConfigRecord {
        hostlists.iter().fold(
            LegacyConfigRecord::new(category, config_name, config_content),
            |record, (reference, content)| record.with_hostlist(*reference, *content),
        )
    }

    fn assert_match(result: Attribution, category: &str, target: &str) -> TargetOwner {
        match result {
            Attribution::Matched {
                target: actual_target,
                owner,
            } => {
                assert_eq!(actual_target, target);
                assert_eq!(owner.category, category);
                owner
            }
            other => panic!("expected match, got {other:?}"),
        }
    }

    #[test]
    fn suffix_matching_respects_label_boundaries() {
        let registry = TargetRegistry::from_records([record(
            "youtube_twitch",
            "youtube_1.conf",
            "--wf-tcp=443 --hostlist=\"lists\\youtube.txt\"",
            &[("lists/youtube.txt", "youtube.com\n")],
        )])
        .unwrap();

        assert_match(
            registry.attribute("WWW.YouTube.com."),
            "youtube_twitch",
            "youtube.com",
        );
        assert_eq!(registry.attribute("notyoutube.com"), Attribution::Unmatched);
        assert_eq!(
            registry.attribute("youtube.com.evil.example"),
            Attribution::Unmatched
        );
    }

    #[test]
    fn longest_suffix_wins_before_generic_category() {
        let generic = record(
            "universal",
            "universal.conf",
            "--filter-tcp=443 --hostlist=lists/generic.txt",
            &[("lists/generic.txt", "example.com\n")],
        );
        let specific = record(
            "video",
            "video.conf",
            "--filter-tcp=443 --hostlist=lists/video.txt",
            &[("lists/video.txt", "video.example.com\n")],
        );
        let registry = TargetRegistry::from_records([generic, specific]).unwrap();

        assert_match(
            registry.attribute("cdn.video.example.com"),
            "video",
            "video.example.com",
        );
        assert_match(
            registry.attribute("api.example.com"),
            "universal",
            "example.com",
        );
    }

    #[test]
    fn same_target_in_different_categories_is_ambiguous() {
        let records = [
            record(
                "first",
                "first.conf",
                "--wf-tcp=443 --hostlist=lists/first.txt",
                &[("lists/first.txt", "shared.example\n")],
            ),
            record(
                "second",
                "second.conf",
                "--wf-tcp=443 --hostlist=lists/second.txt",
                &[("lists/second.txt", "shared.example\n")],
            ),
        ];
        let registry = TargetRegistry::from_records(records).unwrap();

        match registry.attribute("cdn.shared.example") {
            Attribution::Ambiguous { target, owners } => {
                assert_eq!(target, "shared.example");
                assert_eq!(
                    owners
                        .iter()
                        .map(|owner| owner.category.as_str())
                        .collect::<Vec<_>>(),
                    ["first", "second"]
                );
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_target_within_one_category_is_not_ambiguous() {
        let records = [
            record(
                "discord",
                "discord_1.conf",
                "--wf-tcp=443 --hostlist=lists/discord.txt",
                &[("lists/discord.txt", "discord.com\n")],
            ),
            record(
                "discord",
                "discord_2.conf",
                "--wf-tcp=443 --hostlist=lists/discord.txt",
                &[("lists/discord.txt", "discord.com\n")],
            ),
        ];
        let registry = TargetRegistry::from_records(records).unwrap();

        let owner = assert_match(
            registry.attribute("api.discord.com"),
            "discord",
            "discord.com",
        );
        assert_eq!(owner.config_names, ["discord_1.conf", "discord_2.conf"]);
    }

    #[test]
    fn comments_auto_hostlists_and_exclusions_are_applied() {
        let config = "# ignored --wf-tcp=9999\n\
            --wf-tcp=80,443 \
            --hostlist=\"lists\\gaming.txt\" \
            --hostlist-auto=\"autohosts\\gaming.txt\" \
            --hostlist-exclude=\"lists\\white-list.txt\"";
        let registry = TargetRegistry::from_records([record(
            "gaming",
            "gaming.conf",
            config,
            &[
                (
                    "lists/gaming.txt",
                    "# games\nepicgames.com # Epic\nsteamcommunity.com\n",
                ),
                ("autohosts/gaming.txt", "learned.game.example\n"),
                ("lists/white-list.txt", "# excluded\nsteamcommunity.com\n"),
            ],
        )])
        .unwrap();

        assert_match(
            registry.attribute("store.epicgames.com"),
            "gaming",
            "epicgames.com",
        );
        assert_match(
            registry.attribute("cdn.learned.game.example"),
            "gaming",
            "learned.game.example",
        );
        assert!(matches!(
            registry.attribute("cdn.steamcommunity.com"),
            Attribution::Excluded { .. }
        ));
        assert!(!registry.port_plan().contains(9999));
    }

    #[test]
    fn a_missing_auto_hostlist_is_not_a_build_error() {
        let registry = TargetRegistry::from_records([record(
            "discord",
            "discord.conf",
            "--wf-tcp=443 --hostlist=lists/discord.txt \
             --hostlist-auto=autohosts/discord.txt",
            &[("lists/discord.txt", "discord.com\n")],
        )])
        .unwrap();

        assert_match(registry.attribute("discord.com"), "discord", "discord.com");
    }

    #[test]
    fn parses_and_compacts_tcp_port_ranges() {
        let parsed = parse_legacy_config(
            "--wf-tcp=80,443,2053,1024-65535 --wf-udp=443,3478-3480\n\
             --filter-tcp=53 --filter-udp=1024-65535",
        )
        .unwrap();

        assert_eq!(
            parsed.tcp_ports.tcp_ranges(),
            &[
                PortRange { start: 80, end: 80 },
                PortRange {
                    start: 443,
                    end: 443
                },
                PortRange {
                    start: 1024,
                    end: 65535
                },
            ]
        );
        for port in [80, 443, 2053, 1024, 65535] {
            assert!(parsed.tcp_ports.contains(port), "missing TCP port {port}");
        }
        assert!(!parsed.tcp_ports.contains(81));
        assert!(!parsed.tcp_ports.contains(53));
    }

    #[test]
    fn filter_tcp_is_used_only_when_wf_tcp_is_absent() {
        let authoritative = PortPlan::parse("--wf-tcp=443 --filter-tcp=80,2053").unwrap();
        assert_eq!(
            authoritative.tcp_ranges(),
            &[PortRange {
                start: 443,
                end: 443
            }]
        );

        let fallback = PortPlan::parse("--filter-tcp=80,443,2053,1024-65535").unwrap();
        for port in [80, 443, 2053, 1024, 65535] {
            assert!(fallback.contains(port), "missing fallback TCP port {port}");
        }
    }

    #[test]
    fn udp_options_never_enter_the_tcp_plan() {
        let udp_only =
            PortPlan::parse("--wf-udp=80,443,2053,1024-65535 --filter-udp=443,3478-3480").unwrap();
        assert!(udp_only.is_empty());

        let mixed = PortPlan::parse("--wf-tcp=443 --wf-udp=2053").unwrap();
        assert!(mixed.contains(443));
        assert!(!mixed.contains(2053));
    }

    #[test]
    fn windivert_filter_keeps_ranges_compact_and_is_never_watch_all() {
        let empty = PortPlan::default();
        assert_eq!(empty.to_windivert_filter(), None);

        let plan = PortPlan::parse("--wf-tcp=80,443,1024-65535").unwrap();
        let filter = plan.to_windivert_filter().unwrap();
        assert_eq!(
            filter,
            "tcp and ((outbound and (tcp.DstPort == 80 or tcp.DstPort == 443 or \
             (tcp.DstPort >= 1024 and tcp.DstPort <= 65535))) or \
             (inbound and (tcp.SrcPort == 80 or tcp.SrcPort == 443 or \
             (tcp.SrcPort >= 1024 and tcp.SrcPort <= 65535))))"
        );
        let (outbound, inbound) = filter.split_once(" or (inbound").unwrap();
        assert!(!outbound.contains("tcp.SrcPort"));
        assert!(!inbound.contains("tcp.DstPort"));
        assert!(!filter.contains("udp"));
        assert!(filter.len() < 512, "range must not be expanded: {filter}");
    }

    #[test]
    fn runtime_capture_plan_uses_active_configs_not_broad_future_candidates() {
        let active = record(
            "gaming",
            "gaming_1.conf",
            "--wf-tcp=80,443 --hostlist=lists/gaming.txt",
            &[("lists/gaming.txt", "game.example\n")],
        );
        let future = record(
            "gaming",
            "gaming_ultimate.conf",
            "--wf-tcp=80,443,1024-65535 --hostlist=lists/gaming.txt",
            &[("lists/gaming.txt", "game.example\n")],
        );
        let registry = TargetRegistry::from_records_with_active_selections(
            [active, future],
            [("gaming", "gaming_1.conf")],
        )
        .unwrap();

        assert!(registry.port_plan().contains(50_000));
        let active_plan = registry.active_capture_plan().unwrap();
        assert!(active_plan.contains(80));
        assert!(active_plan.contains(443));
        assert!(!active_plan.contains(50_000));
        assert_eq!(registry.active_config("GAMING"), Some("gaming_1.conf"));
        assert!(matches!(
            registry.capture_plan_for([("gaming", "missing.conf")]),
            Err(CapturePlanError::UnknownConfig { .. })
        ));
        assert!(matches!(
            registry.capture_plan_for([
                ("gaming", "gaming_1.conf"),
                ("gaming", "gaming_ultimate.conf")
            ]),
            Err(CapturePlanError::ConflictingSelection { .. })
        ));
    }

    #[test]
    fn active_selection_changes_version_but_not_candidate_content_hash() {
        let first = record(
            "video",
            "video_1.conf",
            "--wf-tcp=443 --hostlist=lists/first.txt",
            &[("lists/first.txt", "first.example\nshared.example\n")],
        );
        let second = record(
            "video",
            "video_2.conf",
            "--wf-tcp=8443 --hostlist=lists/second.txt",
            &[("lists/second.txt", "second.example\nshared.example\n")],
        );
        let first_active = TargetRegistry::from_records_with_active_selections(
            [first.clone(), second.clone()],
            [("video", "video_1.conf")],
        )
        .unwrap();
        let second_active = TargetRegistry::from_records_with_active_selections(
            [first, second],
            [("video", "video_2.conf")],
        )
        .unwrap();

        assert_eq!(first_active.content_hash(), second_active.content_hash());
        assert_ne!(first_active.version(), second_active.version());
        assert_eq!(first_active.active_config("video"), Some("video_1.conf"));
        assert_eq!(second_active.active_config("video"), Some("video_2.conf"));
        let first_targets = first_active.active_target_suffixes().collect::<Vec<_>>();
        assert_eq!(first_targets, ["first.example", "shared.example"]);
        let owner = assert_match(
            first_active.attribute("cdn.shared.example"),
            "video",
            "shared.example",
        );
        assert_eq!(owner.config_names, ["video_1.conf", "video_2.conf"]);
        assert_eq!(owner.active_config.as_deref(), Some("video_1.conf"));
    }

    #[test]
    fn invalid_tcp_ranges_are_reported_but_invalid_udp_is_ignored() {
        assert!(matches!(
            PortPlan::parse("--wf-tcp=443-80"),
            Err(ConfigParseError::InvalidPortTerm { .. })
        ));
        assert!(PortPlan::parse("--wf-udp=not-a-port").is_ok());
    }

    #[test]
    fn fingerprint_is_deterministic_and_changes_with_content() {
        let first = record(
            "a",
            "a.conf",
            "--wf-tcp=443 --hostlist=lists/a.txt",
            &[("lists/a.txt", "a.example\n")],
        );
        let second = record(
            "b",
            "b.conf",
            "--wf-tcp=80 --hostlist=lists/b.txt",
            &[("lists/b.txt", "b.example\n")],
        );
        let forward = TargetRegistry::from_records([first.clone(), second.clone()]).unwrap();
        let reverse = TargetRegistry::from_records([second.clone(), first.clone()]).unwrap();

        assert_eq!(forward.version(), reverse.version());
        assert_eq!(forward.content_hash(), reverse.content_hash());
        assert_eq!(forward.content_hash_hex().len(), 64);

        let selected_forward = TargetRegistry::from_records_with_active_selections(
            [first.clone(), second.clone()],
            [("a", "a.conf"), ("b", "b.conf")],
        )
        .unwrap();
        let selected_reverse = TargetRegistry::from_records_with_active_selections(
            [second.clone(), first.clone()],
            [("b", "b.conf"), ("a", "a.conf")],
        )
        .unwrap();
        assert_eq!(selected_forward.version(), selected_reverse.version());

        let changed = second.with_hostlist("lists/b.txt", "b.example\nnew.example\n");
        let changed = TargetRegistry::from_records([first, changed]).unwrap();
        assert_ne!(forward.version(), changed.version());
        assert_ne!(forward.content_hash(), changed.content_hash());
    }

    #[test]
    fn config_hash_changes_even_when_parsed_plan_is_equivalent() {
        let compact = record(
            "video",
            "video.conf",
            "--wf-tcp=443 --hostlist=lists/video.txt",
            &[("lists/video.txt", "video.example\n")],
        );
        let spaced = record(
            "video",
            "video.conf",
            "--wf-tcp=443  --hostlist=lists/video.txt",
            &[("lists/video.txt", "video.example\n")],
        );
        let compact = TargetRegistry::from_records([compact]).unwrap();
        let spaced = TargetRegistry::from_records([spaced]).unwrap();

        assert_eq!(compact.port_plan(), spaced.port_plan());
        assert_ne!(compact.content_hash(), spaced.content_hash());
    }

    #[test]
    fn required_hostlist_must_be_supplied() {
        let result = TargetRegistry::from_records([LegacyConfigRecord::new(
            "discord",
            "discord.conf",
            "--wf-tcp=443 --hostlist=lists/discord.txt",
        )]);
        assert!(matches!(
            result,
            Err(RegistryBuildError::MissingHostlist {
                kind: HostlistKind::Include,
                ..
            })
        ));
    }

    #[test]
    fn unsafe_hostlist_references_are_rejected_before_path_resolution() {
        assert!(matches!(
            parse_legacy_config("--hostlist=../outside.txt"),
            Err(ConfigParseError::InvalidHostlistReference {
                source: HostlistReferenceError::Traversal,
                ..
            })
        ));
        assert!(matches!(
            parse_legacy_config(r#"--hostlist="C:\\lists\\outside.txt""#),
            Err(ConfigParseError::InvalidHostlistReference {
                source: HostlistReferenceError::AbsoluteOrDriveQualified,
                ..
            })
        ));

        let result = TargetRegistry::from_records([record(
            "discord",
            "discord.conf",
            "--wf-tcp=443 --hostlist=lists/discord.txt",
            &[
                ("lists/discord.txt", "discord.com\n"),
                ("../outside.txt", "outside.example\n"),
            ],
        )]);
        assert!(matches!(
            result,
            Err(RegistryBuildError::InvalidProvidedHostlistReference {
                source: HostlistReferenceError::Traversal,
                ..
            })
        ));
    }
}
