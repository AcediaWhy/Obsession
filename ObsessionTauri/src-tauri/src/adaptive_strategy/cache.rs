//! Versioned cache подтверждённых адаптивных стратегий по отпечатку сети.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::dsl::{
    override_key, AdaptiveCategory, StrategyCandidate, StrategyTransport, STRATEGY_SCHEMA_VERSION,
};
use super::validator;
use crate::paths::Paths;

pub const CACHE_SCHEMA_VERSION: u32 = 3;
const DISABLE_AFTER_FAILURES: u64 = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyTrust {
    Prepared,
    Recommended,
    #[default]
    Confirmed,
}

impl StrategyTrust {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Recommended => "recommended",
            Self::Confirmed => "confirmed",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeSummary {
    pub passed: u32,
    pub failed: u32,
    pub measured_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveCacheEntry {
    pub engine_version: String,
    pub strategy_schema_version: u32,
    pub candidate_id: String,
    pub candidate: StrategyCandidate,
    #[serde(default)]
    pub trust: StrategyTrust,
    pub confirmed_at: u64,
    pub success_count: u64,
    pub failure_count: u64,
    #[serde(default)]
    pub last_probe: Option<ProbeSummary>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
    #[serde(default)]
    pub scope_fingerprint: Option<String>,
    #[serde(default)]
    pub evidence_source: Option<String>,
    #[serde(default)]
    pub source_candidate_id: Option<String>,
    #[serde(default)]
    pub source_category: Option<AdaptiveCategory>,
    #[serde(default)]
    pub source_transport: Option<StrategyTransport>,
    #[serde(default)]
    pub source_scope_fingerprint: Option<String>,
    #[serde(default)]
    pub recommendation_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkEntries {
    #[serde(default)]
    asn_region: Option<String>,
    #[serde(default)]
    categories: BTreeMap<String, AdaptiveCacheEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveStrategyCache {
    schema_version: u32,
    #[serde(default)]
    networks: BTreeMap<String, NetworkEntries>,
}

impl Default for AdaptiveStrategyCache {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            networks: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheError {
    EmptyNetworkKey,
    InvalidCandidate(Vec<String>),
    InvalidRecommendation(String),
    ConfirmedEntryExists,
    Io(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyNetworkKey => write!(f, "adaptive cache requires a network key"),
            Self::InvalidCandidate(errors) => {
                write!(f, "invalid adaptive candidate: {}", errors.join("; "))
            }
            Self::InvalidRecommendation(error) => {
                write!(f, "invalid adaptive recommendation: {error}")
            }
            Self::ConfirmedEntryExists => {
                write!(
                    f,
                    "confirmed adaptive entry cannot be replaced by a recommendation"
                )
            }
            Self::Io(error) => write!(f, "adaptive cache I/O error: {error}"),
        }
    }
}

impl AdaptiveStrategyCache {
    pub fn load(paths: &Paths) -> Self {
        let Ok(text) = std::fs::read_to_string(paths.adaptive_strategy_cache_path()) else {
            return Self::default();
        };
        let Ok(mut cache) = serde_json::from_str::<Self>(&text) else {
            return Self::default();
        };
        if cache.schema_version > CACHE_SCHEMA_VERSION {
            return Self::default();
        }
        if cache.schema_version < 2 {
            cache.migrate_v1_transport_keys();
        }
        if cache.schema_version < 3 {
            cache.migrate_v2_trust();
        }
        cache.schema_version = CACHE_SCHEMA_VERSION;
        cache.sanitize();
        cache
    }

    pub fn save(&self, paths: &Paths) -> Result<(), CacheError> {
        let path = paths.adaptive_strategy_cache_path();
        let tmp = path.with_extension("json.tmp");
        let json =
            serde_json::to_vec_pretty(self).map_err(|error| CacheError::Io(error.to_string()))?;
        let result = (|| {
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(&json)?;
            file.sync_all()?;
            drop(file);
            atomic_replace(&tmp, &path)
        })();
        result.map_err(|error| {
            let _ = std::fs::remove_file(&tmp);
            CacheError::Io(error.to_string())
        })
    }

    #[allow(dead_code)]
    pub fn confirmed_candidate(
        &self,
        network_key: &str,
        category: AdaptiveCategory,
        engine_version: &str,
    ) -> Option<StrategyCandidate> {
        self.confirmed_candidate_for_transport(
            network_key,
            category,
            StrategyTransport::Tls,
            engine_version,
        )
        .or_else(|| {
            self.confirmed_candidate_for_transport(
                network_key,
                category,
                StrategyTransport::Quic,
                engine_version,
            )
        })
    }

    pub fn confirmed_candidate_for_transport(
        &self,
        network_key: &str,
        category: AdaptiveCategory,
        transport: StrategyTransport,
        engine_version: &str,
    ) -> Option<StrategyCandidate> {
        self.confirmed_candidate_for_transport_scoped(
            network_key,
            category,
            transport,
            engine_version,
            None,
        )
    }

    pub fn confirmed_candidate_for_transport_scoped(
        &self,
        network_key: &str,
        category: AdaptiveCategory,
        transport: StrategyTransport,
        engine_version: &str,
        expected_scope_fingerprint: Option<&str>,
    ) -> Option<StrategyCandidate> {
        let entry = self.entry_for_transport_scoped(
            network_key,
            category,
            transport,
            engine_version,
            expected_scope_fingerprint,
        )?;
        (entry.trust == StrategyTrust::Confirmed).then_some(entry.candidate)
    }

    pub fn entry_for_transport_scoped(
        &self,
        network_key: &str,
        category: AdaptiveCategory,
        transport: StrategyTransport,
        engine_version: &str,
        expected_scope_fingerprint: Option<&str>,
    ) -> Option<AdaptiveCacheEntry> {
        let entry = self
            .networks
            .get(network_key)?
            .categories
            .get(&override_key(category, transport))?;
        if entry.disabled_reason.is_some()
            || entry.engine_version != engine_version
            || entry.strategy_schema_version != STRATEGY_SCHEMA_VERSION
            || expected_scope_fingerprint
                .is_some_and(|expected| entry.scope_fingerprint.as_deref() != Some(expected))
        {
            return None;
        }
        Some(entry.clone())
    }

    #[allow(dead_code)]
    pub fn put_confirmed(
        &mut self,
        network_key: &str,
        asn_region: Option<&str>,
        engine_version: &str,
        candidate: StrategyCandidate,
        confirmed_at: u64,
        probe: ProbeSummary,
    ) -> Result<(), CacheError> {
        self.put_confirmed_scoped(
            network_key,
            asn_region,
            engine_version,
            candidate,
            None,
            confirmed_at,
            probe,
        )
    }

    // Cache identity, scope and evidence stay explicit at this boundary. Grouping
    // them would only hide the persisted fields without simplifying call sites.
    #[allow(clippy::too_many_arguments)]
    pub fn put_confirmed_scoped(
        &mut self,
        network_key: &str,
        asn_region: Option<&str>,
        engine_version: &str,
        candidate: StrategyCandidate,
        scope_fingerprint: Option<&str>,
        confirmed_at: u64,
        probe: ProbeSummary,
    ) -> Result<(), CacheError> {
        if network_key.trim().is_empty() {
            return Err(CacheError::EmptyNetworkKey);
        }
        let report = validator::validate(&candidate);
        if !report.is_valid() {
            return Err(CacheError::InvalidCandidate(
                report
                    .errors
                    .into_iter()
                    .map(|error| error.message)
                    .collect(),
            ));
        }

        let category = override_key(candidate.category, candidate.transport);
        let candidate_id = candidate.candidate_id();
        let network = self.networks.entry(network_key.to_string()).or_default();
        if asn_region.is_some() {
            network.asn_region = asn_region.map(str::to_string);
        }
        let entry = network
            .categories
            .entry(category)
            .or_insert_with(|| AdaptiveCacheEntry {
                engine_version: engine_version.to_string(),
                strategy_schema_version: candidate.schema_version,
                candidate_id: candidate_id.clone(),
                candidate: candidate.clone(),
                trust: StrategyTrust::Confirmed,
                confirmed_at,
                success_count: 0,
                failure_count: 0,
                last_probe: None,
                disabled_reason: None,
                scope_fingerprint: scope_fingerprint.map(str::to_string),
                evidence_source: None,
                source_candidate_id: None,
                source_category: None,
                source_transport: None,
                source_scope_fingerprint: None,
                recommendation_reason: None,
            });

        if entry.candidate_id != candidate_id
            || entry.engine_version != engine_version
            || entry.scope_fingerprint.as_deref() != scope_fingerprint
        {
            *entry = AdaptiveCacheEntry {
                engine_version: engine_version.to_string(),
                strategy_schema_version: candidate.schema_version,
                candidate_id,
                candidate,
                trust: StrategyTrust::Confirmed,
                confirmed_at,
                success_count: 0,
                failure_count: 0,
                last_probe: None,
                disabled_reason: None,
                scope_fingerprint: scope_fingerprint.map(str::to_string),
                evidence_source: None,
                source_candidate_id: None,
                source_category: None,
                source_transport: None,
                source_scope_fingerprint: None,
                recommendation_reason: None,
            };
        }
        entry.confirmed_at = confirmed_at;
        entry.trust = StrategyTrust::Confirmed;
        entry.success_count = entry.success_count.saturating_add(1);
        entry.failure_count = 0;
        entry.last_probe = Some(probe);
        entry.disabled_reason = None;
        entry.scope_fingerprint = scope_fingerprint.map(str::to_string);
        entry.evidence_source = None;
        entry.source_candidate_id = None;
        entry.source_category = None;
        entry.source_transport = None;
        entry.source_scope_fingerprint = None;
        entry.recommendation_reason = None;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn put_recommended_scoped(
        &mut self,
        network_key: &str,
        asn_region: Option<&str>,
        engine_version: &str,
        candidate: StrategyCandidate,
        scope_fingerprint: Option<&str>,
        source_category: AdaptiveCategory,
        source_candidate_id: &str,
        source_scope_fingerprint: Option<&str>,
        recommended_at: u64,
        recommendation_reason: &str,
    ) -> Result<(), CacheError> {
        if network_key.trim().is_empty() {
            return Err(CacheError::EmptyNetworkKey);
        }
        let report = validator::validate(&candidate);
        if !report.is_valid() {
            return Err(CacheError::InvalidCandidate(
                report
                    .errors
                    .into_iter()
                    .map(|error| error.message)
                    .collect(),
            ));
        }
        if source_category == candidate.category {
            return Err(CacheError::InvalidRecommendation(
                "source category must differ from target category".into(),
            ));
        }

        let source_transport = candidate.transport;
        let source = self
            .entry_for_transport_scoped(
                network_key,
                source_category,
                source_transport,
                engine_version,
                source_scope_fingerprint,
            )
            .filter(|entry| {
                entry.trust == StrategyTrust::Confirmed && entry.candidate_id == source_candidate_id
            })
            .ok_or_else(|| {
                CacheError::InvalidRecommendation(
                    "confirmed same-network source is unavailable".into(),
                )
            })?;
        if source.candidate.transport != candidate.transport {
            return Err(CacheError::InvalidRecommendation(
                "source and target transports differ".into(),
            ));
        }

        let key = override_key(candidate.category, candidate.transport);
        if self
            .networks
            .get(network_key)
            .and_then(|network| network.categories.get(&key))
            .is_some_and(|entry| entry.trust == StrategyTrust::Confirmed)
        {
            return Err(CacheError::ConfirmedEntryExists);
        }

        let candidate_id = candidate.candidate_id();
        let network = self.networks.entry(network_key.to_string()).or_default();
        if asn_region.is_some() {
            network.asn_region = asn_region.map(str::to_string);
        }
        network.categories.insert(
            key,
            AdaptiveCacheEntry {
                engine_version: engine_version.to_string(),
                strategy_schema_version: candidate.schema_version,
                candidate_id,
                candidate,
                trust: StrategyTrust::Recommended,
                confirmed_at: recommended_at,
                success_count: 0,
                failure_count: 0,
                last_probe: None,
                disabled_reason: None,
                scope_fingerprint: scope_fingerprint.map(str::to_string),
                evidence_source: Some(source_category.as_key().to_string()),
                source_candidate_id: Some(source_candidate_id.to_string()),
                source_category: Some(source_category),
                source_transport: Some(source_transport),
                source_scope_fingerprint: source_scope_fingerprint.map(str::to_string),
                recommendation_reason: Some(recommendation_reason.to_string()),
            },
        );
        Ok(())
    }

    #[allow(dead_code)]
    pub fn reset_recommended(
        &mut self,
        network_key: &str,
        category: AdaptiveCategory,
        transport: StrategyTransport,
    ) -> bool {
        let Some(network) = self.networks.get_mut(network_key) else {
            return false;
        };
        let key = override_key(category, transport);
        if !network
            .categories
            .get(&key)
            .is_some_and(|entry| entry.trust == StrategyTrust::Recommended)
        {
            return false;
        }
        network.categories.remove(&key);
        if network.categories.is_empty() {
            self.networks.remove(network_key);
        }
        true
    }

    #[allow(dead_code)]
    pub fn record_failure(
        &mut self,
        network_key: &str,
        category: AdaptiveCategory,
        probe: ProbeSummary,
    ) -> bool {
        let Some(transport) = self
            .networks
            .get(network_key)
            .and_then(|network| {
                network
                    .categories
                    .values()
                    .find(|entry| entry.candidate.category == category)
            })
            .map(|entry| entry.candidate.transport)
        else {
            return false;
        };
        self.record_failure_for_transport(network_key, category, transport, probe)
    }

    pub fn record_failure_for_transport(
        &mut self,
        network_key: &str,
        category: AdaptiveCategory,
        transport: StrategyTransport,
        probe: ProbeSummary,
    ) -> bool {
        let Some(entry) = self.networks.get_mut(network_key).and_then(|network| {
            network
                .categories
                .get_mut(&override_key(category, transport))
        }) else {
            return false;
        };
        entry.failure_count = entry.failure_count.saturating_add(1);
        entry.last_probe = Some(probe);
        if entry.failure_count >= DISABLE_AFTER_FAILURES {
            entry.disabled_reason = Some("repeated_probe_failure".to_string());
        }
        true
    }

    pub fn reset(&mut self, network_key: &str, category: AdaptiveCategory) -> bool {
        let Some(network) = self.networks.get_mut(network_key) else {
            return false;
        };
        let before = network.categories.len();
        network
            .categories
            .retain(|_, entry| entry.candidate.category != category);
        let removed = network.categories.len() != before;
        if network.categories.is_empty() {
            self.networks.remove(network_key);
        }
        removed
    }

    fn migrate_v1_transport_keys(&mut self) {
        for network in self.networks.values_mut() {
            let entries = std::mem::take(&mut network.categories);
            for (_, entry) in entries {
                network.categories.insert(
                    override_key(entry.candidate.category, entry.candidate.transport),
                    entry,
                );
            }
        }
    }

    fn migrate_v2_trust(&mut self) {
        for network in self.networks.values_mut() {
            // Старый Gaming search проверял доступные Roblox/GitHub/Epic endpoints,
            // поэтому его cache не доказывает recovery реальной блокировки.
            network
                .categories
                .retain(|_, entry| entry.candidate.category != AdaptiveCategory::Gaming);
            for entry in network.categories.values_mut() {
                entry.trust = StrategyTrust::Confirmed;
                entry.evidence_source = None;
                entry.source_candidate_id = None;
                entry.source_category = None;
                entry.source_transport = None;
                entry.source_scope_fingerprint = None;
                entry.recommendation_reason = None;
            }
        }
    }

    fn sanitize(&mut self) {
        for network in self.networks.values_mut() {
            network.categories.retain(|key, entry| {
                entry.trust != StrategyTrust::Prepared
                    && entry.strategy_schema_version == STRATEGY_SCHEMA_VERSION
                    && entry.candidate.schema_version == STRATEGY_SCHEMA_VERSION
                    && override_key(entry.candidate.category, entry.candidate.transport) == *key
                    && entry.candidate_id == entry.candidate.candidate_id()
                    && !entry.engine_version.trim().is_empty()
                    && validator::validate(&entry.candidate).is_valid()
            });

            let confirmed = network
                .categories
                .iter()
                .filter(|(_, entry)| entry.trust == StrategyTrust::Confirmed)
                .map(|(key, entry)| {
                    (
                        key.clone(),
                        (
                            entry.candidate_id.clone(),
                            entry.engine_version.clone(),
                            entry.scope_fingerprint.clone(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            network.categories.retain(|_, entry| {
                if entry.trust == StrategyTrust::Confirmed {
                    return true;
                }
                let (Some(source_category), Some(source_transport), Some(source_candidate_id)) = (
                    entry.source_category,
                    entry.source_transport,
                    entry.source_candidate_id.as_deref(),
                ) else {
                    return false;
                };
                if source_category == entry.candidate.category
                    || source_transport != entry.candidate.transport
                    || entry
                        .recommendation_reason
                        .as_deref()
                        .is_none_or(str::is_empty)
                {
                    return false;
                }
                confirmed
                    .get(&override_key(source_category, source_transport))
                    .is_some_and(|(candidate_id, engine_version, scope_fingerprint)| {
                        candidate_id == source_candidate_id
                            && engine_version == &entry.engine_version
                            && scope_fingerprint.as_deref()
                                == entry.source_scope_fingerprint.as_deref()
                    })
            });
        }
        self.networks
            .retain(|_, network| !network.categories.is_empty());
    }
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
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
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::adaptive_strategy::dsl::{
        AllowedPayload, AllowedRange, StrategyFunction, StrategyStep, StrategyTransport,
        StrategyValue,
    };

    fn temp_paths() -> Paths {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base_dir = std::env::temp_dir().join(format!("obsession-adaptive-cache-{nonce}"));
        std::fs::create_dir_all(&base_dir).unwrap();
        Paths { base_dir }
    }

    fn youtube_candidate() -> StrategyCandidate {
        StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Tls,
            vec![StrategyStep::new(StrategyFunction::MultiDisorderLegacy)
                .with_arg("pos", StrategyValue::Text("1,midsld".into()))],
            vec![AllowedPayload::TlsClientHello],
            Some(AllowedRange::FirstTenDataPackets),
        )
    }

    fn probe(at: u64) -> ProbeSummary {
        ProbeSummary {
            passed: 3,
            failed: 0,
            measured_at: at,
        }
    }

    #[test]
    fn confirmed_candidate_roundtrips_atomically() {
        let paths = temp_paths();
        let candidate = youtube_candidate();
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed(
                "aa:bb",
                Some("AS1_RU"),
                "1.0.2",
                candidate.clone(),
                100,
                probe(100),
            )
            .unwrap();
        cache.save(&paths).unwrap();
        cache
            .put_confirmed(
                "aa:bb",
                Some("AS1_RU"),
                "1.0.2",
                candidate.clone(),
                101,
                probe(101),
            )
            .unwrap();
        cache.save(&paths).unwrap();
        let loaded = AdaptiveStrategyCache::load(&paths);
        assert_eq!(
            loaded.confirmed_candidate("aa:bb", AdaptiveCategory::YoutubeTwitch, "1.0.2"),
            Some(candidate)
        );
        assert!(!paths
            .adaptive_strategy_cache_path()
            .with_extension("json.tmp")
            .exists());
        let _ = std::fs::remove_dir_all(paths.base_dir);
    }

    #[test]
    fn cache_isolated_by_network_category_and_engine() {
        let candidate = youtube_candidate();
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed("net-a", None, "1.0.2", candidate, 1, probe(1))
            .unwrap();
        assert!(cache
            .confirmed_candidate("net-a", AdaptiveCategory::YoutubeTwitch, "1.0.2")
            .is_some());
        assert!(cache
            .confirmed_candidate("net-b", AdaptiveCategory::YoutubeTwitch, "1.0.2")
            .is_none());
        assert!(cache
            .confirmed_candidate("net-a", AdaptiveCategory::Discord, "1.0.2")
            .is_none());
        assert!(cache
            .confirmed_candidate("net-a", AdaptiveCategory::YoutubeTwitch, "2.0.0")
            .is_none());
    }

    #[test]
    fn repeated_failures_disable_until_reconfirmed() {
        let candidate = youtube_candidate();
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed("net", None, "1.0.2", candidate.clone(), 1, probe(1))
            .unwrap();
        for at in 2..=4 {
            assert!(cache.record_failure(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                ProbeSummary {
                    passed: 0,
                    failed: 2,
                    measured_at: at,
                }
            ));
        }
        assert!(cache
            .confirmed_candidate("net", AdaptiveCategory::YoutubeTwitch, "1.0.2")
            .is_none());
        cache
            .put_confirmed("net", None, "1.0.2", candidate, 5, probe(5))
            .unwrap();
        assert!(cache
            .confirmed_candidate("net", AdaptiveCategory::YoutubeTwitch, "1.0.2")
            .is_some());
    }

    #[test]
    fn scoped_candidate_is_invalidated_when_lists_change() {
        let candidate = youtube_candidate();
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                candidate.clone(),
                Some("scope-a"),
                1,
                probe(1),
            )
            .unwrap();
        assert_eq!(
            cache.confirmed_candidate_for_transport_scoped(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Tls,
                "1.0.2",
                Some("scope-a"),
            ),
            Some(candidate)
        );
        assert!(cache
            .confirmed_candidate_for_transport_scoped(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Tls,
                "1.0.2",
                Some("scope-b"),
            )
            .is_none());
    }
    #[test]
    fn stores_tls_and_quic_independently() {
        let mut cache = AdaptiveStrategyCache::default();
        let tls = youtube_candidate();
        let quic = StrategyCandidate::new(
            AdaptiveCategory::YoutubeTwitch,
            StrategyTransport::Quic,
            vec![StrategyStep::new(StrategyFunction::Fake)
                .with_arg("blob", StrategyValue::Text("fake_default_quic".into()))
                .with_arg("repeats", StrategyValue::Integer(6))],
            vec![AllowedPayload::QuicInitial],
            None,
        );
        cache
            .put_confirmed("net", None, "1.0.2", tls.clone(), 1, probe(1))
            .unwrap();
        cache
            .put_confirmed("net", None, "1.0.2", quic.clone(), 2, probe(2))
            .unwrap();

        assert_eq!(
            cache.confirmed_candidate_for_transport(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Tls,
                "1.0.2",
            ),
            Some(tls)
        );
        assert_eq!(
            cache.confirmed_candidate_for_transport(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Quic,
                "1.0.2",
            ),
            Some(quic)
        );
        assert!(cache.reset("net", AdaptiveCategory::YoutubeTwitch));
        assert!(!cache.networks.contains_key("net"));
    }
    #[test]
    fn reset_removes_only_requested_category() {
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed("net", None, "1.0.2", youtube_candidate(), 1, probe(1))
            .unwrap();
        assert!(!cache.reset("net", AdaptiveCategory::Discord));
        assert!(cache.reset("net", AdaptiveCategory::YoutubeTwitch));
        assert!(!cache.networks.contains_key("net"));
    }

    #[test]
    fn invalid_candidate_and_empty_network_are_rejected() {
        let mut invalid = youtube_candidate();
        invalid.steps.clear();
        let mut cache = AdaptiveStrategyCache::default();
        assert!(matches!(
            cache.put_confirmed("net", None, "1.0.2", invalid, 1, probe(1)),
            Err(CacheError::InvalidCandidate(_))
        ));
        assert_eq!(
            cache.put_confirmed("  ", None, "1.0.2", youtube_candidate(), 1, probe(1)),
            Err(CacheError::EmptyNetworkKey)
        );
    }

    #[test]
    fn migrates_v1_category_keys_to_transport_keys() {
        let paths = temp_paths();
        let candidate = youtube_candidate();
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed("net", None, "1.0.2", candidate.clone(), 1, probe(1))
            .unwrap();
        let network = cache.networks.get_mut("net").unwrap();
        let entry = network.categories.remove("youtube_twitch:tls").unwrap();
        network.categories.insert("youtube_twitch".into(), entry);
        cache.schema_version = 1;
        cache.save(&paths).unwrap();

        let loaded = AdaptiveStrategyCache::load(&paths);
        assert_eq!(
            loaded.confirmed_candidate_for_transport(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Tls,
                "1.0.2",
            ),
            Some(candidate)
        );
        let _ = std::fs::remove_dir_all(paths.base_dir);
    }
    #[test]
    fn legacy_v2_entry_migrates_to_confirmed_trust() {
        let paths = temp_paths();
        let candidate = youtube_candidate();
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed("net", None, "1.0.2", candidate.clone(), 1, probe(1))
            .unwrap();
        let mut value = serde_json::to_value(cache).unwrap();
        value["schema_version"] = serde_json::json!(2);
        for network in value["networks"].as_object_mut().unwrap().values_mut() {
            for entry in network["categories"].as_object_mut().unwrap().values_mut() {
                let entry = entry.as_object_mut().unwrap();
                for field in [
                    "trust",
                    "evidence_source",
                    "source_candidate_id",
                    "source_category",
                    "source_transport",
                    "source_scope_fingerprint",
                    "recommendation_reason",
                ] {
                    entry.remove(field);
                }
            }
        }
        std::fs::write(
            paths.adaptive_strategy_cache_path(),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();

        let loaded = AdaptiveStrategyCache::load(&paths);
        let entry = loaded
            .entry_for_transport_scoped(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Tls,
                "1.0.2",
                None,
            )
            .unwrap();
        assert_eq!(entry.trust, StrategyTrust::Confirmed);
        assert_eq!(entry.candidate, candidate);
        let _ = std::fs::remove_dir_all(paths.base_dir);
    }

    #[test]
    fn legacy_v2_gaming_entry_is_not_migrated_as_confirmed() {
        let paths = temp_paths();
        let mut candidate = youtube_candidate();
        candidate.category = AdaptiveCategory::Gaming;
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                candidate,
                Some("gaming-scope"),
                1,
                probe(1),
            )
            .unwrap();
        cache.schema_version = 2;
        cache.save(&paths).unwrap();

        let loaded = AdaptiveStrategyCache::load(&paths);
        assert!(loaded
            .entry_for_transport_scoped(
                "net",
                AdaptiveCategory::Gaming,
                StrategyTransport::Tls,
                "1.0.2",
                Some("gaming-scope"),
            )
            .is_none());
        let _ = std::fs::remove_dir_all(paths.base_dir);
    }

    #[test]
    fn recommendation_is_not_confirmed_and_reset_preserves_source() {
        let source = youtube_candidate();
        let source_id = source.candidate_id();
        let mut gaming = source.clone();
        gaming.category = AdaptiveCategory::Gaming;
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                source.clone(),
                Some("youtube-scope"),
                10,
                probe(10),
            )
            .unwrap();
        cache
            .put_recommended_scoped(
                "net",
                None,
                "1.0.2",
                gaming,
                Some("gaming-scope"),
                AdaptiveCategory::YoutubeTwitch,
                &source_id,
                Some("youtube-scope"),
                11,
                "same-network TLS evidence",
            )
            .unwrap();

        let entry = cache
            .entry_for_transport_scoped(
                "net",
                AdaptiveCategory::Gaming,
                StrategyTransport::Tls,
                "1.0.2",
                Some("gaming-scope"),
            )
            .unwrap();
        assert_eq!(entry.trust, StrategyTrust::Recommended);
        assert!(cache
            .confirmed_candidate_for_transport_scoped(
                "net",
                AdaptiveCategory::Gaming,
                StrategyTransport::Tls,
                "1.0.2",
                Some("gaming-scope"),
            )
            .is_none());
        assert!(cache.reset_recommended("net", AdaptiveCategory::Gaming, StrategyTransport::Tls,));
        assert_eq!(
            cache.confirmed_candidate_for_transport_scoped(
                "net",
                AdaptiveCategory::YoutubeTwitch,
                StrategyTransport::Tls,
                "1.0.2",
                Some("youtube-scope"),
            ),
            Some(source)
        );
    }

    #[test]
    fn recommendation_cannot_replace_confirmed_target() {
        let source = youtube_candidate();
        let source_id = source.candidate_id();
        let mut gaming = source.clone();
        gaming.category = AdaptiveCategory::Gaming;
        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                source,
                Some("youtube-scope"),
                10,
                probe(10),
            )
            .unwrap();
        cache
            .put_confirmed_scoped(
                "net",
                None,
                "1.0.2",
                gaming.clone(),
                Some("gaming-scope"),
                11,
                probe(11),
            )
            .unwrap();
        assert_eq!(
            cache.put_recommended_scoped(
                "net",
                None,
                "1.0.2",
                gaming,
                Some("gaming-scope"),
                AdaptiveCategory::YoutubeTwitch,
                &source_id,
                Some("youtube-scope"),
                12,
                "same-network TLS evidence",
            ),
            Err(CacheError::ConfirmedEntryExists)
        );
    }

    #[test]
    fn corrupt_future_and_tampered_cache_do_not_apply() {
        let paths = temp_paths();
        std::fs::write(paths.adaptive_strategy_cache_path(), "not json").unwrap();
        assert!(AdaptiveStrategyCache::load(&paths).networks.is_empty());

        std::fs::write(
            paths.adaptive_strategy_cache_path(),
            r#"{"schema_version":99,"networks":{}}"#,
        )
        .unwrap();
        assert!(AdaptiveStrategyCache::load(&paths).networks.is_empty());

        let mut cache = AdaptiveStrategyCache::default();
        cache
            .put_confirmed("net", None, "1.0.2", youtube_candidate(), 1, probe(1))
            .unwrap();
        cache
            .networks
            .get_mut("net")
            .unwrap()
            .categories
            .get_mut("youtube_twitch:tls")
            .unwrap()
            .candidate_id = "tampered".into();
        cache.save(&paths).unwrap();
        assert!(AdaptiveStrategyCache::load(&paths).networks.is_empty());
        let _ = std::fs::remove_dir_all(paths.base_dir);
    }
}
