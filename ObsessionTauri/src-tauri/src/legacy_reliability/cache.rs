//! Durable, network-scoped trust memory for automatic Legacy recovery.
//!
//! This file deliberately does not reuse `netcache.json`: the compatibility
//! cache can be seeded by a manual frontend test and has no content fingerprint
//! or distinct-session proof. Only a successful fenced Legacy confirmation may
//! feed this store once the Phase 4 executor integration is connected.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths::Paths;

pub const CACHE_SCHEMA_VERSION: u32 = 1;
pub const PROVISIONAL_TTL_SECS: u64 = 7 * 24 * 60 * 60;
pub const TRUSTED_TTL_SECS: u64 = 30 * 24 * 60 * 60;
pub const NEGATIVE_COOLDOWN_SECS: u64 = 5 * 60;
pub const MAX_NETWORKS: usize = 64;
pub const MAX_CATEGORIES_PER_NETWORK: usize = 32;
pub const MAX_CANDIDATES_PER_CATEGORY: usize = 32;

const MAX_CACHE_BYTES: u64 = 1024 * 1024;
const MAX_NETWORK_KEY_BYTES: usize = 128;
const MAX_CATEGORY_BYTES: usize = 128;
const MAX_CONFIG_ID_BYTES: usize = 260;
const MAX_FINGERPRINT_BYTES: usize = 256;
const MAX_SESSION_KEY_BYTES: usize = 160;
const MAX_REASON_BYTES: usize = 512;
const TEMP_CREATE_ATTEMPTS: usize = 128;
const REPLACE_ATTEMPTS: usize = 50;

static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    #[default]
    Provisional,
    Trusted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateOutcome {
    ConfirmedSuccess,
    StrategyFailure,
    ReadinessFailure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Strategy,
    Readiness,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateEvidence {
    pub config_id: String,
    pub config_fingerprint: String,
    pub trust: TrustLevel,
    /// Distinct confirmation sessions in the current trust epoch, capped at 2.
    pub confirmation_sessions: u8,
    pub last_confirmation_session: Option<String>,
    pub first_success_at: Option<u64>,
    pub last_success_at: Option<u64>,
    pub success_count: u64,
    pub strategy_failure_count: u64,
    pub readiness_failure_count: u64,
    pub last_failure_at: Option<u64>,
    pub cooldown_until: Option<u64>,
    pub last_outcome: Option<CandidateOutcome>,
    pub last_reason: Option<String>,
    pub updated_at: u64,
}

impl CandidateEvidence {
    fn new(config_id: String, config_fingerprint: String, now: u64) -> Self {
        Self {
            config_id,
            config_fingerprint,
            trust: TrustLevel::Provisional,
            confirmation_sessions: 0,
            last_confirmation_session: None,
            first_success_at: None,
            last_success_at: None,
            success_count: 0,
            strategy_failure_count: 0,
            readiness_failure_count: 0,
            last_failure_at: None,
            cooldown_until: None,
            last_outcome: None,
            last_reason: None,
            updated_at: now,
        }
    }

    fn structurally_valid(&self, map_fingerprint: &str) -> bool {
        if self.config_fingerprint != map_fingerprint
            || !valid_component(&self.config_id, MAX_CONFIG_ID_BYTES)
            || !valid_component(&self.config_fingerprint, MAX_FINGERPRINT_BYTES)
            || self.confirmation_sessions > 2
            || self
                .last_reason
                .as_deref()
                .is_some_and(|reason| !valid_component(reason, MAX_REASON_BYTES))
        {
            return false;
        }

        match self.confirmation_sessions {
            0 => {
                if self.last_confirmation_session.is_some()
                    || self.first_success_at.is_some()
                    || self.last_success_at.is_some()
                    || self.success_count != 0
                    || self.trust == TrustLevel::Trusted
                    || self.last_outcome == Some(CandidateOutcome::ConfirmedSuccess)
                {
                    return false;
                }
            }
            sessions => {
                let session_is_valid = self
                    .last_confirmation_session
                    .as_deref()
                    .is_some_and(|session| valid_component(session, MAX_SESSION_KEY_BYTES));
                let timestamps_are_valid = self
                    .first_success_at
                    .zip(self.last_success_at)
                    .is_some_and(|(first, last)| first <= last);
                if !session_is_valid
                    || !timestamps_are_valid
                    || self.success_count < u64::from(sessions)
                    || (self.trust == TrustLevel::Trusted && sessions < 2)
                {
                    return false;
                }
            }
        }

        if self.last_failure_at.is_none()
            && (self.cooldown_until.is_some()
                || matches!(
                    self.last_outcome,
                    Some(CandidateOutcome::StrategyFailure | CandidateOutcome::ReadinessFailure)
                ))
        {
            return false;
        }
        true
    }

    fn trust_evidence_expired(&self, now: u64) -> bool {
        let ttl = match self.trust {
            TrustLevel::Provisional => PROVISIONAL_TTL_SECS,
            TrustLevel::Trusted => TRUSTED_TTL_SECS,
        };
        self.last_success_at
            .is_none_or(|confirmed_at| !timestamp_is_fresh(confirmed_at, now, ttl))
    }

    fn is_fresh_trusted(&self, now: u64) -> bool {
        self.trust == TrustLevel::Trusted
            && self.confirmation_sessions >= 2
            && self.last_outcome == Some(CandidateOutcome::ConfirmedSuccess)
            && self
                .last_success_at
                .is_some_and(|at| timestamp_is_fresh(at, now, TRUSTED_TTL_SECS))
            && self
                .cooldown_until
                .is_none_or(|cooldown_until| now >= cooldown_until)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CategoryEntry {
    #[serde(default)]
    candidates: BTreeMap<String, CandidateEvidence>,
}

impl CategoryEntry {
    fn last_activity_at(&self) -> u64 {
        self.candidates
            .values()
            .map(|candidate| candidate.updated_at)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkEntry {
    last_seen_at: u64,
    #[serde(default)]
    categories: BTreeMap<String, CategoryEntry>,
}

impl NetworkEntry {
    fn last_activity_at(&self) -> u64 {
        self.categories
            .values()
            .map(CategoryEntry::last_activity_at)
            .max()
            .unwrap_or(0)
            .max(self.last_seen_at)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyTrustCache {
    schema_version: u32,
    #[serde(default)]
    networks: BTreeMap<String, NetworkEntry>,
}

impl Default for LegacyTrustCache {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            networks: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CandidateIdentity<'a> {
    pub stable_network_key: &'a str,
    pub category: &'a str,
    pub config_id: &'a str,
    pub config_fingerprint: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub struct ConfirmationRecord<'a> {
    pub candidate: CandidateIdentity<'a>,
    /// Caller-owned unique session key. Production must combine a random boot
    /// nonce with the process-local Legacy SessionId.
    pub session_key: &'a str,
    pub confirmed_at: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct FailureRecord<'a> {
    pub candidate: CandidateIdentity<'a>,
    pub kind: FailureKind,
    pub reason: &'a str,
    pub failed_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfirmationResult {
    pub trust: TrustLevel,
    pub counted_new_session: bool,
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustedCandidate {
    pub config_id: String,
    pub config_fingerprint: String,
    pub last_success_at: u64,
    pub success_count: u64,
}

impl LegacyTrustCache {
    pub fn record_confirmation(
        &mut self,
        record: ConfirmationRecord<'_>,
    ) -> Result<ConfirmationResult, CacheError> {
        let identity = ValidatedIdentity::new(record.candidate)?;
        let session_key = checked_value("session_key", record.session_key, MAX_SESSION_KEY_BYTES)?;
        let now = record.confirmed_at;

        if let Some(existing) = self.candidate_evidence_parts(
            &identity.network_key,
            &identity.category,
            &identity.config_fingerprint,
        ) {
            let same_session =
                existing.last_confirmation_session.as_deref() == Some(session_key.as_str());
            let already_current = same_session
                && !existing.trust_evidence_expired(now)
                && existing.last_outcome == Some(CandidateOutcome::ConfirmedSuccess)
                && existing.cooldown_until.is_none()
                && existing.config_id == identity.config_id;
            if already_current {
                return Ok(ConfirmationResult {
                    trust: existing.trust,
                    counted_new_session: false,
                    changed: false,
                });
            }
        }

        let network = self
            .networks
            .entry(identity.network_key)
            .or_insert_with(|| NetworkEntry {
                last_seen_at: now,
                categories: BTreeMap::new(),
            });
        network.last_seen_at = network.last_seen_at.max(now);
        let entry = network
            .categories
            .entry(identity.category)
            .or_default()
            .candidates
            .entry(identity.config_fingerprint.clone())
            .or_insert_with(|| {
                CandidateEvidence::new(identity.config_id.clone(), identity.config_fingerprint, now)
            });

        let same_session = entry.last_confirmation_session.as_deref() == Some(session_key.as_str());
        let counted_new_session = !same_session;
        let expired = entry.trust_evidence_expired(now);
        if expired {
            entry.confirmation_sessions = 0;
            entry.last_confirmation_session = None;
            entry.trust = TrustLevel::Provisional;
        }
        if counted_new_session {
            entry.confirmation_sessions = entry.confirmation_sessions.saturating_add(1).min(2);
            entry.last_confirmation_session = Some(session_key);
            entry.success_count = entry.success_count.saturating_add(1);
            entry.trust = if entry.confirmation_sessions >= 2 {
                TrustLevel::Trusted
            } else {
                TrustLevel::Provisional
            };
        } else if expired {
            // The same Legacy session may refresh expired evidence, but it can
            // contribute only one proof to the new trust epoch.
            entry.confirmation_sessions = 1;
            entry.last_confirmation_session = Some(session_key);
        }
        entry.config_id = identity.config_id;
        entry.first_success_at.get_or_insert(now);
        entry.last_success_at = Some(now);
        entry.cooldown_until = None;
        entry.last_outcome = Some(CandidateOutcome::ConfirmedSuccess);
        entry.last_reason = None;
        entry.updated_at = now;
        let trust = entry.trust;

        self.enforce_bounds();
        Ok(ConfirmationResult {
            trust,
            counted_new_session,
            changed: true,
        })
    }

    pub fn record_failure(&mut self, record: FailureRecord<'_>) -> Result<(), CacheError> {
        let identity = ValidatedIdentity::new(record.candidate)?;
        let reason = checked_value("reason", record.reason, MAX_REASON_BYTES)?;
        let now = record.failed_at;
        let network = self
            .networks
            .entry(identity.network_key)
            .or_insert_with(|| NetworkEntry {
                last_seen_at: now,
                categories: BTreeMap::new(),
            });
        network.last_seen_at = network.last_seen_at.max(now);
        let entry = network
            .categories
            .entry(identity.category)
            .or_default()
            .candidates
            .entry(identity.config_fingerprint.clone())
            .or_insert_with(|| {
                CandidateEvidence::new(identity.config_id.clone(), identity.config_fingerprint, now)
            });
        entry.config_id = identity.config_id;
        match record.kind {
            FailureKind::Strategy => {
                entry.strategy_failure_count = entry.strategy_failure_count.saturating_add(1);
                entry.last_outcome = Some(CandidateOutcome::StrategyFailure);
            }
            FailureKind::Readiness => {
                entry.readiness_failure_count = entry.readiness_failure_count.saturating_add(1);
                entry.last_outcome = Some(CandidateOutcome::ReadinessFailure);
            }
        }
        entry.last_failure_at = Some(now);
        entry.cooldown_until = Some(now.saturating_add(NEGATIVE_COOLDOWN_SECS));
        entry.last_reason = Some(reason);
        entry.updated_at = now;
        self.enforce_bounds();
        Ok(())
    }

    pub fn candidate_evidence(
        &self,
        stable_network_key: &str,
        category: &str,
        config_fingerprint: &str,
    ) -> Option<&CandidateEvidence> {
        let network_key = normalized_lookup(stable_network_key, MAX_NETWORK_KEY_BYTES)?;
        let category = normalized_category(category)?;
        let fingerprint = normalized_lookup(config_fingerprint, MAX_FINGERPRINT_BYTES)?;
        self.candidate_evidence_parts(&network_key, &category, &fingerprint)
    }

    pub fn cooldown_until(
        &self,
        stable_network_key: &str,
        category: &str,
        config_fingerprint: &str,
        now: u64,
    ) -> Option<u64> {
        self.candidate_evidence(stable_network_key, category, config_fingerprint)
            .and_then(|entry| entry.cooldown_until)
            .filter(|until| now < *until)
    }

    /// Fresh trusted candidates for the exact network/category, ordered by
    /// newest successful evidence, then success count, then fingerprint.
    pub fn fresh_trusted_candidates(
        &self,
        stable_network_key: &str,
        category: &str,
        now: u64,
    ) -> Vec<TrustedCandidate> {
        let Some(network_key) = normalized_lookup(stable_network_key, MAX_NETWORK_KEY_BYTES) else {
            return Vec::new();
        };
        let Some(category) = normalized_category(category) else {
            return Vec::new();
        };
        let mut candidates = self
            .networks
            .get(&network_key)
            .and_then(|network| network.categories.get(&category))
            .into_iter()
            .flat_map(|category| &category.candidates)
            .filter(|(fingerprint, entry)| {
                entry.config_fingerprint == **fingerprint && entry.is_fresh_trusted(now)
            })
            .map(|(fingerprint, entry)| TrustedCandidate {
                config_id: entry.config_id.clone(),
                config_fingerprint: fingerprint.clone(),
                last_success_at: entry.last_success_at.unwrap_or(0),
                success_count: entry.success_count,
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .last_success_at
                .cmp(&left.last_success_at)
                .then_with(|| right.success_count.cmp(&left.success_count))
                .then_with(|| left.config_fingerprint.cmp(&right.config_fingerprint))
        });
        candidates
    }

    pub fn network_count(&self) -> usize {
        self.networks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.networks.is_empty()
    }

    fn candidate_evidence_parts(
        &self,
        network_key: &str,
        category: &str,
        fingerprint: &str,
    ) -> Option<&CandidateEvidence> {
        self.networks
            .get(network_key)?
            .categories
            .get(category)?
            .candidates
            .get(fingerprint)
    }

    fn sanitize(&mut self) {
        self.schema_version = CACHE_SCHEMA_VERSION;
        self.networks.retain(|network_key, network| {
            if !valid_component(network_key, MAX_NETWORK_KEY_BYTES) {
                return false;
            }
            network.categories.retain(|category, entries| {
                if !valid_component(category, MAX_CATEGORY_BYTES)
                    || category != &category.trim().to_ascii_lowercase()
                {
                    return false;
                }
                entries
                    .candidates
                    .retain(|fingerprint, candidate| candidate.structurally_valid(fingerprint));
                !entries.candidates.is_empty()
            });
            !network.categories.is_empty()
        });
        self.enforce_bounds();
    }

    fn enforce_bounds(&mut self) {
        for network in self.networks.values_mut() {
            for category in network.categories.values_mut() {
                while category.candidates.len() > MAX_CANDIDATES_PER_CATEGORY {
                    let Some(oldest) = category
                        .candidates
                        .iter()
                        .min_by_key(|(fingerprint, candidate)| {
                            (candidate.updated_at, (*fingerprint).clone())
                        })
                        .map(|(fingerprint, _)| fingerprint.clone())
                    else {
                        break;
                    };
                    category.candidates.remove(&oldest);
                }
            }
            while network.categories.len() > MAX_CATEGORIES_PER_NETWORK {
                let Some(oldest) = network
                    .categories
                    .iter()
                    .min_by_key(|(category, entries)| {
                        (entries.last_activity_at(), (*category).clone())
                    })
                    .map(|(category, _)| category.clone())
                else {
                    break;
                };
                network.categories.remove(&oldest);
            }
        }
        while self.networks.len() > MAX_NETWORKS {
            let Some(oldest) = self
                .networks
                .iter()
                .min_by_key(|(network_key, network)| {
                    (network.last_activity_at(), (*network_key).clone())
                })
                .map(|(network_key, _)| network_key.clone())
            else {
                break;
            };
            self.networks.remove(&oldest);
        }
    }

    fn encoded(&self) -> Result<Vec<u8>, CacheError> {
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| CacheError::Serialization(error.to_string()))?;
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return Err(CacheError::TooLarge);
        }
        Ok(bytes)
    }
}

struct ValidatedIdentity {
    network_key: String,
    category: String,
    config_id: String,
    config_fingerprint: String,
}

impl ValidatedIdentity {
    fn new(value: CandidateIdentity<'_>) -> Result<Self, CacheError> {
        Ok(Self {
            network_key: checked_value(
                "stable_network_key",
                value.stable_network_key,
                MAX_NETWORK_KEY_BYTES,
            )?,
            category: checked_value("category", value.category, MAX_CATEGORY_BYTES)?
                .to_ascii_lowercase(),
            config_id: checked_value("config_id", value.config_id, MAX_CONFIG_ID_BYTES)?,
            config_fingerprint: checked_value(
                "config_fingerprint",
                value.config_fingerprint,
                MAX_FINGERPRINT_BYTES,
            )?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheLoadState {
    Missing,
    Loaded,
    CorruptQuarantined { path: PathBuf },
    FutureSchemaReadOnly { found: u32 },
    UnreadableReadOnly { error: String },
    CorruptReadOnly { error: String },
}

impl CacheLoadState {
    pub const fn is_writable(&self) -> bool {
        matches!(
            self,
            Self::Missing | Self::Loaded | Self::CorruptQuarantined { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub struct LegacyTrustCacheStore {
    cache: LegacyTrustCache,
    load_state: CacheLoadState,
}

impl LegacyTrustCacheStore {
    pub fn load(paths: &Paths) -> Self {
        Self::load_at(paths, unix_now())
    }

    pub fn load_at(paths: &Paths, now: u64) -> Self {
        Self::load_at_with_quarantine(paths, now, |source, destination| {
            fs::rename(source, destination)
        })
    }

    pub fn cache(&self) -> &LegacyTrustCache {
        &self.cache
    }

    pub fn load_state(&self) -> &CacheLoadState {
        &self.load_state
    }

    pub fn is_writable(&self) -> bool {
        self.load_state.is_writable()
    }

    /// Copy-on-write persistence boundary. Failed serialization/replacement
    /// leaves both the prior in-memory snapshot and destination file intact.
    pub fn update<T, F>(&mut self, paths: &Paths, mutate: F) -> Result<T, CacheError>
    where
        F: FnOnce(&mut LegacyTrustCache) -> Result<CacheMutation<T>, CacheError>,
    {
        if !self.is_writable() {
            return Err(self.read_only_error());
        }
        let mut next = self.cache.clone();
        let mutation = mutate(&mut next)?;
        if mutation.changed {
            next.schema_version = CACHE_SCHEMA_VERSION;
            next.enforce_bounds();
            save_atomic(paths, &next.encoded()?)?;
            self.cache = next;
            self.load_state = CacheLoadState::Loaded;
        }
        Ok(mutation.value)
    }

    pub fn record_confirmation(
        &mut self,
        paths: &Paths,
        record: ConfirmationRecord<'_>,
    ) -> Result<ConfirmationResult, CacheError> {
        self.update(paths, |cache| {
            let result = cache.record_confirmation(record)?;
            Ok(CacheMutation {
                changed: result.changed,
                value: result,
            })
        })
    }

    pub fn record_failure(
        &mut self,
        paths: &Paths,
        record: FailureRecord<'_>,
    ) -> Result<(), CacheError> {
        self.update(paths, |cache| {
            cache.record_failure(record)?;
            Ok(CacheMutation::changed(()))
        })
    }

    pub fn fresh_trusted_candidates(
        &self,
        stable_network_key: &str,
        category: &str,
        now: u64,
    ) -> Vec<TrustedCandidate> {
        self.cache
            .fresh_trusted_candidates(stable_network_key, category, now)
    }

    pub fn cooldown_until(
        &self,
        stable_network_key: &str,
        category: &str,
        config_fingerprint: &str,
        now: u64,
    ) -> Option<u64> {
        self.cache
            .cooldown_until(stable_network_key, category, config_fingerprint, now)
    }

    fn read_only_error(&self) -> CacheError {
        match &self.load_state {
            CacheLoadState::FutureSchemaReadOnly { found } => {
                CacheError::FutureSchemaReadOnly { found: *found }
            }
            CacheLoadState::UnreadableReadOnly { error }
            | CacheLoadState::CorruptReadOnly { error } => CacheError::ReadOnly(error.clone()),
            CacheLoadState::Missing
            | CacheLoadState::Loaded
            | CacheLoadState::CorruptQuarantined { .. } => {
                CacheError::ReadOnly("cache unexpectedly became read-only".into())
            }
        }
    }

    fn load_at_with_quarantine<F>(paths: &Paths, now: u64, quarantine: F) -> Self
    where
        F: FnOnce(&Path, &Path) -> io::Result<()>,
    {
        let path = paths.legacy_reliability_cache_path();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Self {
                    cache: LegacyTrustCache::default(),
                    load_state: CacheLoadState::Missing,
                };
            }
            Err(error) => {
                return Self {
                    cache: LegacyTrustCache::default(),
                    load_state: CacheLoadState::UnreadableReadOnly {
                        error: error.to_string(),
                    },
                };
            }
        };
        if metadata.len() > MAX_CACHE_BYTES {
            return Self::quarantine_corrupt(paths, now, quarantine);
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Self {
                    cache: LegacyTrustCache::default(),
                    load_state: CacheLoadState::UnreadableReadOnly {
                        error: error.to_string(),
                    },
                };
            }
        };
        let header = match serde_json::from_slice::<SchemaHeader>(&bytes) {
            Ok(header) => header,
            Err(_) => return Self::quarantine_corrupt(paths, now, quarantine),
        };
        if header.schema_version > CACHE_SCHEMA_VERSION {
            return Self {
                cache: LegacyTrustCache::default(),
                load_state: CacheLoadState::FutureSchemaReadOnly {
                    found: header.schema_version,
                },
            };
        }
        if header.schema_version != CACHE_SCHEMA_VERSION {
            return Self::quarantine_corrupt(paths, now, quarantine);
        }
        let mut cache = match serde_json::from_slice::<LegacyTrustCache>(&bytes) {
            Ok(cache) => cache,
            Err(_) => return Self::quarantine_corrupt(paths, now, quarantine),
        };
        cache.sanitize();
        Self {
            cache,
            load_state: CacheLoadState::Loaded,
        }
    }

    fn quarantine_corrupt<F>(paths: &Paths, now: u64, quarantine: F) -> Self
    where
        F: FnOnce(&Path, &Path) -> io::Result<()>,
    {
        let source = paths.legacy_reliability_cache_path();
        let destination = unique_quarantine_path(paths, now);
        match quarantine(&source, &destination) {
            Ok(()) => Self {
                cache: LegacyTrustCache::default(),
                load_state: CacheLoadState::CorruptQuarantined { path: destination },
            },
            Err(error) => Self {
                cache: LegacyTrustCache::default(),
                load_state: CacheLoadState::CorruptReadOnly {
                    error: error.to_string(),
                },
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheMutation<T> {
    value: T,
    changed: bool,
}

impl<T> CacheMutation<T> {
    pub const fn changed(value: T) -> Self {
        Self {
            value,
            changed: true,
        }
    }

    pub const fn unchanged(value: T) -> Self {
        Self {
            value,
            changed: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheError {
    InvalidField(&'static str),
    Serialization(String),
    Io(String),
    TooLarge,
    FutureSchemaReadOnly { found: u32 },
    ReadOnly(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField(field) => write!(formatter, "invalid Legacy cache field: {field}"),
            Self::Serialization(error) => {
                write!(formatter, "Legacy cache serialization failed: {error}")
            }
            Self::Io(error) => write!(formatter, "Legacy cache I/O failed: {error}"),
            Self::TooLarge => formatter.write_str("Legacy cache exceeds its size limit"),
            Self::FutureSchemaReadOnly { found } => write!(
                formatter,
                "Legacy cache schema {found} is newer than supported schema {CACHE_SCHEMA_VERSION}"
            ),
            Self::ReadOnly(error) => write!(formatter, "Legacy cache is read-only: {error}"),
        }
    }
}

impl std::error::Error for CacheError {}

#[derive(Deserialize)]
struct SchemaHeader {
    schema_version: u32,
}

fn checked_value(field: &'static str, value: &str, max_bytes: usize) -> Result<String, CacheError> {
    let value = value.trim();
    if !valid_component(value, max_bytes) {
        return Err(CacheError::InvalidField(field));
    }
    Ok(value.to_owned())
}

fn valid_component(value: &str, max_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn normalized_lookup(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    valid_component(value, max_bytes).then(|| value.to_owned())
}

fn normalized_category(value: &str) -> Option<String> {
    normalized_lookup(value, MAX_CATEGORY_BYTES).map(|value| value.to_ascii_lowercase())
}

fn timestamp_is_fresh(timestamp: u64, now: u64, ttl: u64) -> bool {
    timestamp <= now && now - timestamp < ttl
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unique_quarantine_path(paths: &Paths, now: u64) -> PathBuf {
    let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    paths.base_dir.join(format!(
        "legacy-reliability-cache.corrupt.{now}.{}.{}.json",
        std::process::id(),
        sequence
    ))
}

fn save_atomic(paths: &Paths, bytes: &[u8]) -> Result<(), CacheError> {
    fs::create_dir_all(&paths.base_dir).map_err(io_error)?;
    let destination = paths.legacy_reliability_cache_path();
    let (temporary, mut file) = create_unique_temporary(paths)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        atomic_replace_with_retry(&temporary, &destination)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(error));
    }
    Ok(())
}

fn create_unique_temporary(paths: &Paths) -> Result<(PathBuf, fs::File), CacheError> {
    for _ in 0..TEMP_CREATE_ATTEMPTS {
        let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = paths.base_dir.join(format!(
            ".legacy-reliability-cache.json.tmp.{}.{}",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(error)),
        }
    }
    Err(CacheError::Io(
        "could not allocate a unique cache temporary file".into(),
    ))
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
                std::thread::sleep(std::time::Duration::from_millis(2));
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
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

fn io_error(error: io::Error) -> CacheError {
    CacheError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(name: &str) -> Paths {
        let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let base_dir = std::env::temp_dir().join(format!(
            "obsession-legacy-trust-cache-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&base_dir).unwrap();
        Paths { base_dir }
    }

    fn identity<'a>(
        network: &'a str,
        category: &'a str,
        config_id: &'a str,
        fingerprint: &'a str,
    ) -> CandidateIdentity<'a> {
        CandidateIdentity {
            stable_network_key: network,
            category,
            config_id,
            config_fingerprint: fingerprint,
        }
    }

    fn confirm<'a>(
        candidate: CandidateIdentity<'a>,
        session: &'a str,
        at: u64,
    ) -> ConfirmationRecord<'a> {
        ConfirmationRecord {
            candidate,
            session_key: session,
            confirmed_at: at,
        }
    }

    fn failure<'a>(
        candidate: CandidateIdentity<'a>,
        kind: FailureKind,
        at: u64,
    ) -> FailureRecord<'a> {
        FailureRecord {
            candidate,
            kind,
            reason: "acceptance_failure",
            failed_at: at,
        }
    }

    fn remove(paths: &Paths) {
        let _ = fs::remove_dir_all(&paths.base_dir);
    }

    #[test]
    fn two_distinct_sessions_promote_but_same_session_is_idempotent() {
        let mut cache = LegacyTrustCache::default();
        let candidate = identity("net-a", "Discord", "discord_2.conf", "fp-a");
        let first = cache
            .record_confirmation(confirm(candidate, "boot-a:1", 10))
            .unwrap();
        assert_eq!(first.trust, TrustLevel::Provisional);
        assert!(first.counted_new_session);

        let repeated = cache
            .record_confirmation(confirm(candidate, "boot-a:1", 20))
            .unwrap();
        assert_eq!(
            repeated,
            ConfirmationResult {
                trust: TrustLevel::Provisional,
                counted_new_session: false,
                changed: false,
            }
        );
        let evidence = cache
            .candidate_evidence("net-a", "discord", "fp-a")
            .unwrap();
        assert_eq!(evidence.success_count, 1);
        assert_eq!(evidence.last_success_at, Some(10));

        let promoted = cache
            .record_confirmation(confirm(candidate, "boot-b:1", 30))
            .unwrap();
        assert_eq!(promoted.trust, TrustLevel::Trusted);
        assert!(promoted.counted_new_session);
        assert_eq!(
            cache
                .candidate_evidence("net-a", "DISCORD", "fp-a")
                .unwrap()
                .success_count,
            2
        );
    }

    #[test]
    fn provisional_and_trusted_ttls_fail_closed_at_exact_boundary() {
        let candidate = identity("net", "video", "video.conf", "fp");
        let mut before_boundary = LegacyTrustCache::default();
        before_boundary
            .record_confirmation(confirm(candidate, "boot:1", 100))
            .unwrap();
        let promoted = before_boundary
            .record_confirmation(confirm(candidate, "boot:2", 100 + PROVISIONAL_TTL_SECS - 1))
            .unwrap();
        assert_eq!(promoted.trust, TrustLevel::Trusted);
        let trusted_at = 100 + PROVISIONAL_TTL_SECS - 1;
        assert_eq!(
            before_boundary
                .fresh_trusted_candidates("net", "video", trusted_at + TRUSTED_TTL_SECS - 1)
                .len(),
            1
        );
        assert!(before_boundary
            .fresh_trusted_candidates("net", "video", trusted_at + TRUSTED_TTL_SECS)
            .is_empty());

        let mut exact_boundary = LegacyTrustCache::default();
        exact_boundary
            .record_confirmation(confirm(candidate, "boot:1", 100))
            .unwrap();
        let reset = exact_boundary
            .record_confirmation(confirm(candidate, "boot:2", 100 + PROVISIONAL_TTL_SECS))
            .unwrap();
        assert_eq!(reset.trust, TrustLevel::Provisional);
        assert!(exact_boundary
            .fresh_trusted_candidates("net", "video", u64::MAX)
            .is_empty());
    }

    #[test]
    fn future_dated_success_is_never_fresh() {
        let mut cache = LegacyTrustCache::default();
        let candidate = identity("net", "video", "video.conf", "fp");
        cache
            .record_confirmation(confirm(candidate, "boot:1", 200))
            .unwrap();
        cache
            .record_confirmation(confirm(candidate, "boot:2", 201))
            .unwrap();
        assert!(cache
            .fresh_trusted_candidates("net", "video", 199)
            .is_empty());
    }

    #[test]
    fn same_session_refresh_after_hard_ttl_starts_a_provisional_epoch() {
        let mut cache = LegacyTrustCache::default();
        let candidate = identity("net", "video", "video.conf", "fp");
        cache
            .record_confirmation(confirm(candidate, "boot:1", 1))
            .unwrap();
        cache
            .record_confirmation(confirm(candidate, "boot:2", 2))
            .unwrap();
        let refresh_at = 2 + TRUSTED_TTL_SECS;
        cache
            .record_failure(failure(
                candidate,
                FailureKind::Strategy,
                refresh_at.saturating_sub(1),
            ))
            .unwrap();
        let refreshed = cache
            .record_confirmation(confirm(candidate, "boot:2", refresh_at))
            .unwrap();
        assert_eq!(refreshed.trust, TrustLevel::Provisional);
        assert!(!refreshed.counted_new_session);
        let evidence = cache.candidate_evidence("net", "video", "fp").unwrap();
        assert_eq!(evidence.confirmation_sessions, 1);
        assert_eq!(evidence.success_count, 2);
        assert!(cache
            .fresh_trusted_candidates("net", "video", refresh_at)
            .is_empty());
    }

    #[test]
    fn network_category_and_fingerprint_are_exactly_isolated() {
        let mut cache = LegacyTrustCache::default();
        let candidate = identity("net-a", "video", "video.conf", "fp-a");
        cache
            .record_confirmation(confirm(candidate, "boot:1", 1))
            .unwrap();
        cache
            .record_confirmation(confirm(candidate, "boot:2", 2))
            .unwrap();
        assert_eq!(cache.fresh_trusted_candidates("net-a", "video", 3).len(), 1);
        assert!(cache
            .fresh_trusted_candidates("net-b", "video", 3)
            .is_empty());
        assert!(cache
            .fresh_trusted_candidates("net-a", "discord", 3)
            .is_empty());
        assert!(cache.candidate_evidence("net-a", "video", "fp-b").is_none());
    }

    #[test]
    fn failure_cools_only_exact_candidate_and_success_clears_it_without_recounting_session() {
        let mut cache = LegacyTrustCache::default();
        let first = identity("net", "video", "a.conf", "fp-a");
        let second = identity("net", "video", "b.conf", "fp-b");
        cache
            .record_confirmation(confirm(first, "boot:1", 1))
            .unwrap();
        cache
            .record_confirmation(confirm(first, "boot:2", 2))
            .unwrap();
        cache
            .record_confirmation(confirm(second, "boot:1", 1))
            .unwrap();
        cache
            .record_confirmation(confirm(second, "boot:2", 2))
            .unwrap();
        cache
            .record_failure(failure(first, FailureKind::Strategy, 10))
            .unwrap();

        assert_eq!(
            cache.cooldown_until("net", "video", "fp-a", 10),
            Some(10 + NEGATIVE_COOLDOWN_SECS)
        );
        assert_eq!(cache.cooldown_until("net", "video", "fp-b", 10), None);
        assert_eq!(
            cache.fresh_trusted_candidates("net", "video", 10),
            vec![TrustedCandidate {
                config_id: "b.conf".into(),
                config_fingerprint: "fp-b".into(),
                last_success_at: 2,
                success_count: 2,
            }]
        );
        assert_eq!(
            cache.cooldown_until("net", "video", "fp-a", 10 + NEGATIVE_COOLDOWN_SECS),
            None
        );
        assert!(cache
            .fresh_trusted_candidates("net", "video", 10 + NEGATIVE_COOLDOWN_SECS)
            .iter()
            .all(|candidate| candidate.config_fingerprint != "fp-a"));

        let restored = cache
            .record_confirmation(confirm(first, "boot:2", 20))
            .unwrap();
        assert!(restored.changed);
        assert!(!restored.counted_new_session);
        assert_eq!(restored.trust, TrustLevel::Trusted);
        let evidence = cache.candidate_evidence("net", "video", "fp-a").unwrap();
        assert_eq!(evidence.success_count, 2);
        assert_eq!(evidence.cooldown_until, None);
        assert_eq!(
            evidence.last_outcome,
            Some(CandidateOutcome::ConfirmedSuccess)
        );
    }

    #[test]
    fn readiness_and_strategy_failures_have_separate_persisted_counters() {
        let mut cache = LegacyTrustCache::default();
        let candidate = identity("net", "video", "a.conf", "fp-a");
        cache
            .record_failure(failure(candidate, FailureKind::Readiness, 10))
            .unwrap();
        cache
            .record_failure(failure(candidate, FailureKind::Strategy, 20))
            .unwrap();
        let evidence = cache.candidate_evidence("net", "video", "fp-a").unwrap();
        assert_eq!(evidence.readiness_failure_count, 1);
        assert_eq!(evidence.strategy_failure_count, 1);
        assert_eq!(
            evidence.last_outcome,
            Some(CandidateOutcome::StrategyFailure)
        );
        assert_eq!(evidence.cooldown_until, Some(20 + NEGATIVE_COOLDOWN_SECS));
    }

    #[test]
    fn trusted_ranking_is_recent_then_success_count_then_fingerprint() {
        let mut cache = LegacyTrustCache::default();
        for (fingerprint, at) in [("fp-b", 20), ("fp-a", 20), ("fp-c", 30)] {
            let candidate = identity("net", "video", fingerprint, fingerprint);
            cache
                .record_confirmation(confirm(candidate, "boot:1", at - 1))
                .unwrap();
            cache
                .record_confirmation(confirm(candidate, "boot:2", at))
                .unwrap();
        }
        let ranked = cache.fresh_trusted_candidates("net", "video", 31);
        assert_eq!(
            ranked
                .iter()
                .map(|candidate| candidate.config_fingerprint.as_str())
                .collect::<Vec<_>>(),
            vec!["fp-c", "fp-a", "fp-b"]
        );
    }

    #[test]
    fn store_roundtrip_is_atomic_and_cooldown_survives_reload() {
        let paths = temp_paths("roundtrip");
        let mut store = LegacyTrustCacheStore::load_at(&paths, 1);
        assert_eq!(store.load_state(), &CacheLoadState::Missing);
        let candidate = identity("net", "video", "video.conf", "fp");
        store
            .record_confirmation(&paths, confirm(candidate, "boot:1", 10))
            .unwrap();
        store
            .record_failure(&paths, failure(candidate, FailureKind::Strategy, 20))
            .unwrap();
        let loaded = LegacyTrustCacheStore::load_at(&paths, 30);
        assert_eq!(loaded.load_state(), &CacheLoadState::Loaded);
        assert_eq!(
            loaded.cooldown_until("net", "video", "fp", 30),
            Some(20 + NEGATIVE_COOLDOWN_SECS)
        );
        let raw = fs::read(paths.legacy_reliability_cache_path()).unwrap();
        serde_json::from_slice::<LegacyTrustCache>(&raw).unwrap();
        assert!(!fs::read_dir(&paths.base_dir)
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains(".tmp.")));
        remove(&paths);
    }

    #[test]
    fn same_session_store_update_does_not_rewrite_or_increment() {
        let paths = temp_paths("idempotent");
        let mut store = LegacyTrustCacheStore::load_at(&paths, 1);
        let candidate = identity("net", "video", "video.conf", "fp");
        store
            .record_confirmation(&paths, confirm(candidate, "boot:1", 10))
            .unwrap();
        let before = fs::read(paths.legacy_reliability_cache_path()).unwrap();
        let repeated = store
            .record_confirmation(&paths, confirm(candidate, "boot:1", 20))
            .unwrap();
        let after = fs::read(paths.legacy_reliability_cache_path()).unwrap();
        assert!(!repeated.changed);
        assert_eq!(before, after);
        remove(&paths);
    }

    #[test]
    fn future_schema_is_read_only_and_never_overwritten() {
        let paths = temp_paths("future");
        let future = br#"{"schema_version":99,"networks":{},"future":true}"#;
        fs::write(paths.legacy_reliability_cache_path(), future).unwrap();
        let mut store = LegacyTrustCacheStore::load_at(&paths, 1);
        assert_eq!(
            store.load_state(),
            &CacheLoadState::FutureSchemaReadOnly { found: 99 }
        );
        let candidate = identity("net", "video", "video.conf", "fp");
        assert_eq!(
            store.record_confirmation(&paths, confirm(candidate, "boot:1", 10)),
            Err(CacheError::FutureSchemaReadOnly { found: 99 })
        );
        assert_eq!(
            fs::read(paths.legacy_reliability_cache_path()).unwrap(),
            future
        );
        remove(&paths);
    }

    #[test]
    fn corrupt_cache_is_quarantined_before_new_writes() {
        let paths = temp_paths("quarantine");
        fs::write(paths.legacy_reliability_cache_path(), b"not json").unwrap();
        let mut store = LegacyTrustCacheStore::load_at(&paths, 123);
        let quarantine = match store.load_state() {
            CacheLoadState::CorruptQuarantined { path } => path.clone(),
            state => panic!("unexpected load state: {state:?}"),
        };
        assert_eq!(fs::read(&quarantine).unwrap(), b"not json");
        assert!(!paths.legacy_reliability_cache_path().exists());
        let candidate = identity("net", "video", "video.conf", "fp");
        store
            .record_confirmation(&paths, confirm(candidate, "boot:1", 10))
            .unwrap();
        assert!(paths.legacy_reliability_cache_path().is_file());
        remove(&paths);
    }

    #[test]
    fn quarantine_failure_keeps_corrupt_source_and_fails_closed() {
        let paths = temp_paths("quarantine-failure");
        fs::write(paths.legacy_reliability_cache_path(), b"not json").unwrap();
        let mut store = LegacyTrustCacheStore::load_at_with_quarantine(&paths, 123, |_, _| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        });
        assert!(matches!(
            store.load_state(),
            CacheLoadState::CorruptReadOnly { .. }
        ));
        let candidate = identity("net", "video", "video.conf", "fp");
        assert!(matches!(
            store.record_confirmation(&paths, confirm(candidate, "boot:1", 10)),
            Err(CacheError::ReadOnly(_))
        ));
        assert_eq!(
            fs::read(paths.legacy_reliability_cache_path()).unwrap(),
            b"not json"
        );
        remove(&paths);
    }

    #[test]
    fn maps_are_deterministic_and_bounded() {
        let mut forward = LegacyTrustCache::default();
        let mut reverse = LegacyTrustCache::default();
        let records = [("net-b", "cat-b", "fp-b"), ("net-a", "cat-a", "fp-a")];
        for (network, category, fingerprint) in records {
            forward
                .record_failure(failure(
                    identity(network, category, fingerprint, fingerprint),
                    FailureKind::Strategy,
                    1,
                ))
                .unwrap();
        }
        for (network, category, fingerprint) in records.into_iter().rev() {
            reverse
                .record_failure(failure(
                    identity(network, category, fingerprint, fingerprint),
                    FailureKind::Strategy,
                    1,
                ))
                .unwrap();
        }
        assert_eq!(forward.encoded().unwrap(), reverse.encoded().unwrap());

        let mut bounded = LegacyTrustCache::default();
        for index in 0..=MAX_NETWORKS {
            let network = format!("net-{index:03}");
            bounded
                .record_failure(failure(
                    identity(&network, "category", "config", "fingerprint"),
                    FailureKind::Readiness,
                    index as u64,
                ))
                .unwrap();
        }
        assert_eq!(bounded.network_count(), MAX_NETWORKS);
        assert!(bounded.networks.contains_key("net-064"));
        assert!(!bounded.networks.contains_key("net-000"));

        let mut candidates = LegacyTrustCache::default();
        for index in 0..=MAX_CANDIDATES_PER_CATEGORY {
            let config = format!("config-{index:03}");
            let fingerprint = format!("fp-{index:03}");
            candidates
                .record_failure(failure(
                    identity("net", "category", &config, &fingerprint),
                    FailureKind::Strategy,
                    index as u64,
                ))
                .unwrap();
        }
        let category = &candidates.networks["net"].categories["category"];
        assert_eq!(category.candidates.len(), MAX_CANDIDATES_PER_CATEGORY);
        assert!(category.candidates.contains_key("fp-032"));
        assert!(!category.candidates.contains_key("fp-000"));
    }

    #[test]
    fn tampered_entry_is_removed_without_lending_trust() {
        let paths = temp_paths("tampered");
        let mut cache = LegacyTrustCache::default();
        let candidate = identity("net", "video", "video.conf", "fp");
        cache
            .record_confirmation(confirm(candidate, "boot:1", 1))
            .unwrap();
        cache
            .record_confirmation(confirm(candidate, "boot:2", 2))
            .unwrap();
        cache
            .networks
            .get_mut("net")
            .unwrap()
            .categories
            .get_mut("video")
            .unwrap()
            .candidates
            .get_mut("fp")
            .unwrap()
            .config_fingerprint = "other".into();
        save_atomic(&paths, &cache.encoded().unwrap()).unwrap();

        let loaded = LegacyTrustCacheStore::load_at(&paths, 3);
        assert!(loaded
            .fresh_trusted_candidates("net", "video", 3)
            .is_empty());
        assert!(loaded.cache().is_empty());
        remove(&paths);
    }

    #[test]
    fn invalid_identity_is_rejected_without_mutation() {
        let mut cache = LegacyTrustCache::default();
        let error = cache
            .record_confirmation(confirm(
                identity(" ", "video", "video.conf", "fp"),
                "boot:1",
                1,
            ))
            .unwrap_err();
        assert_eq!(error, CacheError::InvalidField("stable_network_key"));
        assert!(cache.is_empty());
    }
}
