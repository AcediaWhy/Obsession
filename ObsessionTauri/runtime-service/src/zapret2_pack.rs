//! Strict service-owned compiler for the bundled Zapret2 Strategy Pack.
//!
//! The IPC request selects only an allowlisted category group and aggression
//! level. Lua, blobs, lists, profile fields and the final argv are reconstructed
//! exclusively from hash-verified resources below Program Files.

#![cfg(windows)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

use obsession_runtime_protocol::{
    DpiCategory, DpiEngine, Zapret2ProfileSelection,
};
use serde::Deserialize;

use crate::protected_layout::{VerifiedDpiPlan, VerifiedResource};

const PACK_SCHEMA_VERSION: u32 = obsession_runtime_protocol::zapret2_recipe::PACK_SCHEMA_VERSION;
const WINWS2_VERSION: (u32, u32, u32, u32) = (1, 0, 5, 2);
const LUA_API: u32 = 6;
const LUA_LIB: &str = "lua/zapret-lib.lua";
const LUA_DIAGNOSTIC: &str = "lua/obsession-diagnostic.lua";
const CLIENTHELLO_EXPERIMENT_ID: &str = "discord_tls_clienthello_clone";
const MAX_PACK_BYTES: u64 = 1024 * 1024;
const MAX_PACK_FILES: usize = 256;
const MAX_PACK_BLOBS: usize = 64;
const MAX_PACK_STRATEGIES: usize = 512;
const MAX_PROFILE_VALUES: usize = 64;
/// Верхняя граница допустимого поля `aggressiveness` в стратегии пака.
const MAX_AGGRESSIVENESS: u8 = 8;
const MAX_VALUE_BYTES: usize = 512;
const MAX_ARGUMENTS: usize = 2048;
const MAX_ARGUMENT_BYTES: usize = 8 * 1024;

#[derive(Debug)]
pub(crate) struct Zapret2CompileError(&'static str);

impl std::fmt::Display for Zapret2CompileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for Zapret2CompileError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyPackManifest {
    schema_version: u32,
    pack_id: String,
    pack_version: String,
    engine: EngineCompat,
    categories: Vec<String>,
    protocols: Vec<String>,
    files: Vec<PackFile>,
    #[serde(default)]
    blobs: Vec<BlobDef>,
    strategies: Vec<StrategyDef>,
    #[serde(default)]
    presets: Vec<StrategyDef>,
    #[serde(default)]
    experiments: Vec<StrategyDef>,
    #[serde(default)]
    probes: Vec<String>,
    #[serde(default)]
    fallback: Option<String>,
    #[serde(default)]
    notes: Option<PackNotes>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineCompat {
    winws2_min: String,
    lua_api: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackFile {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlobDef {
    name: String,
    path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrategyDef {
    #[serde(skip)]
    composition_scope: Option<obsession_runtime_protocol::zapret2_composition::DiscordScope>,
    id: String,
    category: String,
    aggressiveness: u8,
    lua: String,
    #[serde(default)]
    desync: Vec<String>,
    #[serde(default)]
    steps: Vec<obsession_runtime_protocol::zapret2_recipe::DesyncStep>,
    #[serde(default)]
    candidate: Option<obsession_runtime_protocol::zapret2_recipe::CandidateSource>,
    #[serde(default)]
    transports: Vec<String>,
    #[serde(default)]
    hostlist: Option<String>,
    #[serde(default)]
    ipset: Option<String>,
    #[serde(default)]
    filter_tcp: Option<String>,
    #[serde(default)]
    filter_udp: Option<String>,
    #[serde(default)]
    filter_l7: Vec<String>,
    #[serde(default)]
    payload: Vec<String>,
    #[serde(default)]
    out_range: Option<String>,
    #[serde(default)]
    in_range: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackNotes {
    scope: String,
    profile_grammar: String,
}

#[derive(Clone, Debug)]
struct Profile {
    composition_scope: Option<obsession_runtime_protocol::zapret2_composition::DiscordScope>,
    name: String,
    filter_tcp: Option<String>,
    filter_udp: Option<String>,
    filter_l7: Vec<String>,
    hostlist: Option<String>,
    ipset: Option<String>,
    payload: Vec<String>,
    out_range: Option<String>,
    in_range: Option<String>,
    desync: Vec<String>,
    steps: Vec<obsession_runtime_protocol::zapret2_recipe::DesyncStep>,
}

pub(crate) fn compile(plan: &VerifiedDpiPlan) -> Result<Vec<String>, Zapret2CompileError> {
    if plan.engine() != DpiEngine::Zapret2 || plan.strategies().is_empty() {
        return Err(Zapret2CompileError("invalid Zapret2 plan"));
    }
    let artifact = plan
        .strategies()
        .first()
        .ok_or(Zapret2CompileError("missing Strategy Pack"))?
        .artifact_resource();
    if plan
        .strategies()
        .iter()
        .any(|strategy| strategy.artifact_resource().relative_path() != artifact.relative_path())
    {
        return Err(Zapret2CompileError("mixed Strategy Packs are forbidden"));
    }

    let metadata = fs::metadata(artifact.path())
        .map_err(|_| Zapret2CompileError("could not inspect Strategy Pack"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_PACK_BYTES {
        return Err(Zapret2CompileError("invalid Strategy Pack size"));
    }
    let bytes = fs::read(artifact.path())
        .map_err(|_| Zapret2CompileError("could not read Strategy Pack"))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(Zapret2CompileError("Strategy Pack changed while reading"));
    }
    let manifest: StrategyPackManifest = serde_json::from_slice(&bytes)
        .map_err(|_| Zapret2CompileError("invalid Strategy Pack schema"))?;
    let resources = resource_index(plan)?;
    let pack_root = artifact
        .relative_path()
        .parent()
        .ok_or(Zapret2CompileError("invalid Strategy Pack path"))?;
    validate_manifest(&manifest, pack_root, &resources)?;

    let mut profiles = Vec::new();
    let mut lua_files = vec![LUA_LIB.to_owned()];
    for selected in plan.strategies() {
        let category = category_name(selected.category());
        let selected_profiles = select_profiles_with_selections(&manifest, category, plan.zapret2_level(), plan.zapret2_clienthello(), plan.zapret2_profiles())?;
        for strategy in &selected_profiles {
            if !lua_files.contains(&strategy.lua) {
                lua_files.push(strategy.lua.clone());
            }
        }

        for strategy in selected_profiles {
            profiles.push(profile_from_strategy(&strategy, selected.category(), &resources)?);
        }
    }
    if profiles.is_empty() {
        return Err(Zapret2CompileError("Strategy Pack produced no profiles"));
    }

    if plan.zapret2_diagnostics() {
        // The observer is a fixed, hash-verified resource, loaded after all
        // native strategy functions. A final PASS profile exposes captured
        // traffic that did not match any selected host/protocol profile.
        lua_files.push(LUA_DIAGNOSTIC.to_owned());
    }

    let mut arguments = Vec::new();
    push_optional_capture(
        &mut arguments,
        "--wf-tcp-out",
        capture_ports(&profiles, true),
    )?;
    push_optional_capture(
        &mut arguments,
        "--wf-udp-out",
        capture_ports(&profiles, false),
    )?;
    for lua in lua_files {
        let path = pack_resource(pack_root, &lua, &resources)?;
        push_argument(
            &mut arguments,
            format!("--lua-init=@{}", argv_path(path.path())),
        )?;
    }
    for blob in &manifest.blobs {
        let path = pack_resource(pack_root, &blob.path, &resources)?;
        push_argument(
            &mut arguments,
            format!("--blob={}:@{}", blob.name, argv_path(path.path())),
        )?;
    }
    for (index, profile) in profiles.iter().enumerate() {
        push_argument(&mut arguments, format!("--name={}", profile.name))?;
        push_option(
            &mut arguments,
            "--filter-tcp",
            profile.filter_tcp.as_deref(),
        )?;
        push_option(
            &mut arguments,
            "--filter-udp",
            profile.filter_udp.as_deref(),
        )?;
        push_joined(&mut arguments, "--filter-l7", &profile.filter_l7)?;
        push_option(&mut arguments, "--hostlist", profile.hostlist.as_deref())?;
        if let Some(scope) = profile.composition_scope {
            for argument in scope.arguments() { push_argument(&mut arguments, argument)?; }
        }
        push_option(&mut arguments, "--ipset", profile.ipset.as_deref())?;
        push_option(&mut arguments, "--out-range", profile.out_range.as_deref())?;
        push_option(&mut arguments, "--in-range", profile.in_range.as_deref())?;
        if plan.zapret2_diagnostics() {
            push_joined(&mut arguments, "--payload", &profile.payload)?;
            push_argument(&mut arguments, format!("--lua-desync=obsession_trace:profile={}:routing", profile.name))?;
        }
        for argument in obsession_runtime_protocol::zapret2_recipe::arguments(
            &profile.payload, profile.out_range.as_deref(), profile.in_range.as_deref(),
            &profile.desync, &profile.steps).into_iter().skip(usize::from(
                plan.zapret2_diagnostics() && profile.steps.is_empty() && !profile.payload.is_empty())) {
            push_argument(&mut arguments, argument)?;
        }
        if index + 1 < profiles.len() {
            push_argument(&mut arguments, "--new".into())?;
        }
    }
    if plan.zapret2_diagnostics() {
        push_argument(&mut arguments, "--new".into())?;
        push_argument(&mut arguments, "--name=obsession_unmatched".into())?;
        push_argument(&mut arguments, "--lua-desync=obsession_trace:profile=unmatched:routing".into())?;
    }
    Ok(arguments)
}



fn validate_manifest(
    manifest: &StrategyPackManifest,
    pack_root: &Path,
    resources: &BTreeMap<String, &VerifiedResource>,
) -> Result<(), Zapret2CompileError> {
    if !matches!(manifest.schema_version, 1 | PACK_SCHEMA_VERSION)
        || !valid_pack_id(&manifest.pack_id)
        || !valid_text(&manifest.pack_version, 64)
        || !minimum_version_satisfied(&manifest.engine.winws2_min)
        || manifest.engine.lua_api > LUA_API
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_PACK_FILES
        || manifest.blobs.len() > MAX_PACK_BLOBS
        || manifest.strategies.is_empty()
        || manifest.strategies.len() > MAX_PACK_STRATEGIES
        || manifest.presets.len() > 96
        || manifest.experiments.len() > 8
        || manifest.categories.is_empty()
        || manifest.categories.len() > 16
        || manifest.protocols.len() > 16
        || manifest.probes.len() > 64
    {
        return Err(Zapret2CompileError("invalid Strategy Pack bounds"));
    }
    if let Some(notes) = &manifest.notes {
        if !valid_text(&notes.scope, 2048) || !valid_text(&notes.profile_grammar, 2048) {
            return Err(Zapret2CompileError("invalid Strategy Pack notes"));
        }
    }

    let mut categories = BTreeSet::new();
    for category in &manifest.categories {
        if protocol_category(category).is_none() || !categories.insert(category.as_str()) {
            return Err(Zapret2CompileError("invalid Strategy Pack category"));
        }
    }
    let allowed_protocols = ["tcp", "http", "tls", "udp", "quic"];
    let mut protocols = BTreeSet::new();
    for protocol in &manifest.protocols {
        if !allowed_protocols.contains(&protocol.as_str()) || !protocols.insert(protocol.as_str()) {
            return Err(Zapret2CompileError("invalid Strategy Pack protocol"));
        }
    }

    let mut files = BTreeSet::new();
    for file in &manifest.files {
        if !safe_relative(&file.path)
            || !is_sha256(&file.sha256)
            || !files.insert(file.path.as_str())
        {
            return Err(Zapret2CompileError("invalid Strategy Pack file"));
        }
        let resource = pack_resource(pack_root, &file.path, resources)?;
        if !resource.sha256().eq_ignore_ascii_case(&file.sha256) {
            return Err(Zapret2CompileError("Strategy Pack file hash mismatch"));
        }
    }
    if !files.contains(LUA_LIB) {
        return Err(Zapret2CompileError("Strategy Pack Lua runtime is missing"));
    }

    let mut blob_names = BTreeSet::new();
    for blob in &manifest.blobs {
        if !valid_identifier(&blob.name)
            || !blob_names.insert(blob.name.as_str())
            || !files.contains(blob.path.as_str())
        {
            return Err(Zapret2CompileError("invalid Strategy Pack blob"));
        }
    }

    let mut strategy_ids = BTreeSet::new();
    if manifest.schema_version == 1 && manifest.strategies.iter().chain(&manifest.presets).chain(&manifest.experiments)
        .any(|strategy| !strategy.steps.is_empty() || strategy.candidate.is_some()) {
        return Err(Zapret2CompileError("scoped recipes require pack schema 2"));
    }
    for strategy in manifest.strategies.iter().chain(&manifest.presets) {
        validate_strategy(strategy, &categories, &protocols, &files)?;
        validate_action_blobs(strategy, &blob_names)?;
        if !strategy_ids.insert(strategy.id.as_str()) {
            return Err(Zapret2CompileError("duplicate Strategy Pack profile"));
        }
    }
    if manifest.presets.iter().any(|strategy| profile_transport(strategy).is_none()) {
        return Err(Zapret2CompileError("fixed preset has no supported transport slot"));
    }
    for experiment in &manifest.experiments {
        validate_strategy(experiment, &categories, &protocols, &files)?;
        validate_action_blobs(experiment, &blob_names)?;
        if experiment.id != CLIENTHELLO_EXPERIMENT_ID
            || !is_discord_tls_profile(experiment)
            || !strategy_ids.insert(experiment.id.as_str()) {
            return Err(Zapret2CompileError("invalid Strategy Pack experiment"));
        }
    }
    if let Some(fallback) = &manifest.fallback {
        if !manifest.strategies.iter().any(|strategy| &strategy.id == fallback) {
            return Err(Zapret2CompileError("invalid Strategy Pack fallback"));
        }
    }
    if manifest.probes.iter().any(|probe| !valid_domain(probe)) {
        return Err(Zapret2CompileError("invalid Strategy Pack probe"));
    }
    Ok(())
}

fn is_discord_tls_profile(strategy: &StrategyDef) -> bool {
    strategy.category == "discord"
        && strategy.filter_l7 == ["tls"]
        && strategy.payload == ["tls_client_hello"]
        && strategy.filter_udp.is_none()
        && strategy.transports.iter().all(|value| matches!(value.as_str(), "tcp" | "tls"))
}

#[cfg(test)]
fn select_profiles(
    manifest: &StrategyPackManifest,
    category: &str,
    requested_level: u8,
    clienthello: bool,
) -> Result<Vec<StrategyDef>, Zapret2CompileError> {
    select_profiles_with_selections(manifest, category, requested_level, clienthello, &[])
}

/// Control TLS/QUIC are separate from Gaming's ipset-scoped data plane.
fn profile_transport(strategy: &StrategyDef) -> Option<&'static str> {
    let has_l7 = |value: &str| strategy.filter_l7.iter().any(|entry| entry == value);
    if strategy.ipset.is_none() && has_l7("tls") && strategy.filter_udp.is_none() { Some("tls") }
    else if strategy.ipset.is_none() && has_l7("quic") && strategy.filter_tcp.is_none() { Some("quic") }
    else if strategy.category == "discord" && strategy.filter_udp.is_some()
        && strategy.filter_tcp.is_none() && (has_l7("discord") || has_l7("stun")) { Some("voice") }
    else if has_l7("http") && strategy.filter_udp.is_none() { Some("http") }
    else if strategy.filter_udp.is_some() && strategy.filter_tcp.is_none() && strategy.filter_l7.is_empty() { Some("udp") }
    else { None }
}

fn select_profiles_with_selections(
    manifest: &StrategyPackManifest,
    category: &str,
    requested_level: u8,
    clienthello: bool,
    selections: &[Zapret2ProfileSelection],
) -> Result<Vec<StrategyDef>, Zapret2CompileError> {
    let available = manifest.strategies.iter().filter(|strategy| strategy.category == category).collect::<Vec<_>>();
    let maximum = available.iter().map(|strategy| strategy.aggressiveness).max()
        .ok_or(Zapret2CompileError("selected category has no profiles"))?;
    let level = if requested_level > 0 && available.iter().any(|strategy| strategy.aggressiveness == requested_level) {
        requested_level
    } else { maximum };
    let mut profiles = available.into_iter().filter(|strategy| strategy.aggressiveness == level).cloned().collect::<Vec<_>>();
    let explicit_tls = selections.iter().any(|selection| selection.category == category && selection.transport == "tls");
    if clienthello && category == "discord" && !explicit_tls {
        let experiment = manifest.experiments.iter()
            .find(|strategy| strategy.id == CLIENTHELLO_EXPERIMENT_ID && is_discord_tls_profile(strategy))
            .ok_or(Zapret2CompileError("ClientHello experiment is missing from the verified pack; update Obsession"))?;
        let index = profiles.iter().position(|strategy| is_discord_tls_profile(strategy))
            .ok_or(Zapret2CompileError("selected Discord set has no TLS profile"))?;
        profiles.retain(|strategy| !is_discord_tls_profile(strategy));
        profiles.insert(index.min(profiles.len()), experiment.clone());
    }
    let mut selected_slots = BTreeSet::new();
    for selection in selections.iter().filter(|selection| selection.category == category) {
        if selection.validate().is_err() || !selected_slots.insert(selection.transport.as_str()) {
            return Err(Zapret2CompileError("invalid or duplicate fixed profile selection"));
        }
        use obsession_runtime_protocol::zapret2_composition::{DiscordComposition, DiscordScope, compatible};
        if let Some(composition) = DiscordComposition::parse(&selection.profile_id).map_err(Zapret2CompileError)? {
            if category != "discord" || selection.transport != "tls" {
                return Err(Zapret2CompileError("composition requires Discord TLS"));
            }
            let mut parts = Vec::new();
            for (id, scope) in [(&composition.api_profile_id, DiscordScope::Api), (&composition.cdn_profile_id, DiscordScope::Cdn)] {
                let parent = manifest.strategies.iter().chain(&manifest.presets).find(|entry| &entry.id == id)
                    .ok_or(Zapret2CompileError("composition parent is absent from the verified pack"))?;
                if !compatible(&parent.category, parent.filter_tcp.as_deref(), parent.filter_udp.as_deref(),
                    &parent.filter_l7, parent.hostlist.as_deref(), parent.ipset.as_deref(), &parent.transports) {
                    return Err(Zapret2CompileError("incompatible composition parent"));
                }
                let mut part = parent.clone();
                part.composition_scope = Some(scope);
                parts.push(part);
            }
            let position = profiles.iter().position(|entry| profile_transport(entry) == Some("tls"))
                .ok_or(Zapret2CompileError("selected transport has no native profile slot"))?;
            profiles.retain(|entry| profile_transport(entry) != Some("tls"));
            let position = position.min(profiles.len());
            profiles.splice(position..position, parts);
            continue;
        }
        let replacement = manifest.strategies.iter().chain(&manifest.presets).chain(&manifest.experiments)
            .find(|strategy| strategy.id == selection.profile_id)
            .ok_or(Zapret2CompileError("selected profile is absent from the verified pack"))?;
        if replacement.category != category || profile_transport(replacement) != Some(selection.transport.as_str()) {
            return Err(Zapret2CompileError("selected profile does not match category and transport"));
        }
        let position = profiles.iter().position(|strategy| profile_transport(strategy) == Some(selection.transport.as_str()))
            .ok_or(Zapret2CompileError("selected transport has no native profile slot"))?;
        profiles.retain(|strategy| profile_transport(strategy) != Some(selection.transport.as_str()));
        profiles.insert(position.min(profiles.len()), replacement.clone());
    }
    Ok(profiles)
}

fn validate_action_blobs(strategy: &StrategyDef, blobs: &BTreeSet<&str>) -> Result<(), Zapret2CompileError> {
    for action in strategy.desync.iter().chain(strategy.steps.iter().map(|step| &step.action)) {
        for field in action.split(':') {
            if let Some(name) = field.strip_prefix("blob=").or_else(|| field.strip_prefix("seqovl_pattern=")) {
                if !name.starts_with("fake_default_") && !name.starts_with("0x") && !blobs.contains(name) {
                    return Err(Zapret2CompileError("undeclared Strategy Pack action blob"));
                }
            }
        }
    }
    Ok(())
}

fn validate_strategy<'a>(
    strategy: &'a StrategyDef,
    categories: &BTreeSet<&str>,
    protocols: &BTreeSet<&str>,
    files: &BTreeSet<&str>,
) -> Result<(), Zapret2CompileError> {
    if !valid_identifier(&strategy.id)
        || !categories.contains(strategy.category.as_str())
        || strategy.aggressiveness == 0
        || strategy.aggressiveness > MAX_AGGRESSIVENESS
        || !files.contains(strategy.lua.as_str())
        || (strategy.desync.is_empty() && strategy.steps.is_empty())
        || strategy.desync.len() > MAX_PROFILE_VALUES
        || strategy.transports.len() > 8
        || strategy.filter_l7.len() > MAX_PROFILE_VALUES
        || strategy.payload.len() > MAX_PROFILE_VALUES
    {
        return Err(Zapret2CompileError("invalid Strategy Pack profile"));
    }
    obsession_runtime_protocol::zapret2_recipe::validate_steps(&strategy.desync, &strategy.steps)
        .map_err(Zapret2CompileError)?;
    if strategy.candidate.as_ref().is_some_and(|candidate| !candidate.valid()) {
        return Err(Zapret2CompileError("invalid candidate provenance"));
    }
    if strategy
        .transports
        .iter()
        .any(|transport| !protocols.contains(transport.as_str()))
        || strategy.desync.iter().any(|value| !valid_cli_value(value))
        || strategy
            .filter_l7
            .iter()
            .any(|value| !valid_cli_value(value))
        || strategy.payload.iter().any(|value| !valid_cli_value(value))
        || strategy
            .out_range
            .as_deref()
            .is_some_and(|value| !valid_cli_value(value))
        || strategy
            .in_range
            .as_deref()
            .is_some_and(|value| !valid_cli_value(value))
        || strategy
            .hostlist
            .as_deref()
            .is_some_and(|value| !safe_list_basename(value))
        || strategy
            .ipset
            .as_deref()
            .is_some_and(|value| !safe_list_basename(value))
        || strategy
            .filter_tcp
            .as_deref()
            .is_some_and(|value| parse_ports(value).is_none())
        || strategy
            .filter_udp
            .as_deref()
            .is_some_and(|value| parse_ports(value).is_none())
    {
        return Err(Zapret2CompileError("unsafe Strategy Pack profile value"));
    }
    // High-port профиль без ipset допустим только когда он чем-то сужен. Для TCP
    // сужение даёт хостлист: при отсутствии ipset `profile_from_strategy`
    // подставляет `{category}.txt`, поэтому действие идёт только по известным
    // доменам. Для UDP имени хоста нет, поэтому единственное допустимое сужение —
    // распознавание протокола движком через filter_l7/payload. В обоих случаях
    // дополнительно ограничен размер захвата.
    if strategy.ipset.is_none() {
        if strategy
            .filter_tcp
            .as_deref()
            .is_some_and(|value| has_high_ports(value) && !bounded_high_ports(value))
        {
            return Err(Zapret2CompileError(
                "high-port TCP profile requires ipset or a bounded port set",
            ));
        }
        if let Some(udp) = strategy
            .filter_udp
            .as_deref()
            .filter(|value| has_high_ports(value))
        {
            if !bounded_high_ports(udp)
                || !recognized_udp_scope(&strategy.filter_l7, &strategy.payload)
            {
                return Err(Zapret2CompileError(
                    "high-port UDP profile requires ipset or a recognized L7 scope",
                ));
            }
        }
    }
    Ok(())
}

fn profile_from_strategy(
    strategy: &StrategyDef,
    category: DpiCategory,
    resources: &BTreeMap<String, &VerifiedResource>,
) -> Result<Profile, Zapret2CompileError> {
    let has = |value: &str| strategy.transports.iter().any(|entry| entry == value);
    let tcp = strategy.transports.is_empty() || has("tcp") || has("tls") || has("http");
    let udp = has("udp") || has("quic");
    let hostlist = strategy
        .hostlist
        .clone()
        .or_else(|| {
            // Voice discovery has no SNI; a default hostlist would exclude it.
            let voice = !tcp && udp && recognized_udp_scope(&strategy.filter_l7, &strategy.payload);
            (strategy.ipset.is_none() && !voice).then(|| format!("{}.txt", category_name(category)))
        })
        .filter(|_| strategy.composition_scope != Some(obsession_runtime_protocol::zapret2_composition::DiscordScope::Cdn))
        .map(|name| list_resource(&name, resources).map(|resource| argv_path(resource.path())))
        .transpose()?;
    let ipset = strategy
        .ipset
        .as_deref()
        .map(|name| list_resource(name, resources).map(|resource| argv_path(resource.path())))
        .transpose()?;
    Ok(Profile {
        composition_scope: strategy.composition_scope,
        name: strategy.id.clone(),
        filter_tcp: strategy
            .filter_tcp
            .clone()
            .or_else(|| tcp.then(|| "443".into())),
        filter_udp: strategy
            .filter_udp
            .clone()
            .or_else(|| udp.then(|| "443".into())),
        filter_l7: strategy.filter_l7.clone(),
        hostlist,
        ipset,
        payload: strategy.payload.clone(),
        out_range: strategy.out_range.clone(),
        in_range: strategy.in_range.clone(),
        desync: strategy.desync.clone(),
        steps: strategy.steps.clone(),
    })
}

fn resource_index(
    plan: &VerifiedDpiPlan,
) -> Result<BTreeMap<String, &VerifiedResource>, Zapret2CompileError> {
    let mut resources = BTreeMap::new();
    for strategy in plan.strategies() {
        for resource in std::iter::once(strategy.artifact_resource())
            .chain(strategy.dependency_resources().iter())
        {
            let key = relative_key(resource.relative_path())?;
            if let Some(existing) = resources.insert(key, resource) {
                if existing.path() != resource.path() || existing.sha256() != resource.sha256() {
                    return Err(Zapret2CompileError("conflicting protected resources"));
                }
            }
        }
    }
    Ok(resources)
}

fn pack_resource<'a>(
    pack_root: &Path,
    relative: &str,
    resources: &'a BTreeMap<String, &VerifiedResource>,
) -> Result<&'a VerifiedResource, Zapret2CompileError> {
    if !safe_relative(relative) {
        return Err(Zapret2CompileError("unsafe Strategy Pack path"));
    }
    let key = relative_key(&pack_root.join(relative))?;
    resources
        .get(&key)
        .copied()
        .ok_or(Zapret2CompileError("undeclared Strategy Pack resource"))
}

fn list_resource<'a>(
    basename: &str,
    resources: &'a BTreeMap<String, &VerifiedResource>,
) -> Result<&'a VerifiedResource, Zapret2CompileError> {
    if !safe_list_basename(basename) {
        return Err(Zapret2CompileError("unsafe list basename"));
    }
    resources
        .get(&format!("lists/{}", basename.to_ascii_lowercase()))
        .copied()
        .ok_or(Zapret2CompileError("undeclared Zapret2 list"))
}

fn push_optional_capture(
    arguments: &mut Vec<String>,
    option: &str,
    value: Option<String>,
) -> Result<(), Zapret2CompileError> {
    push_option(arguments, option, value.as_deref())
}

fn push_option(
    arguments: &mut Vec<String>,
    option: &str,
    value: Option<&str>,
) -> Result<(), Zapret2CompileError> {
    if let Some(value) = value {
        push_argument(arguments, format!("{option}={value}"))?;
    }
    Ok(())
}

fn push_joined(
    arguments: &mut Vec<String>,
    option: &str,
    values: &[String],
) -> Result<(), Zapret2CompileError> {
    if !values.is_empty() {
        push_argument(arguments, format!("{option}={}", values.join(",")))?;
    }
    Ok(())
}

fn push_argument(arguments: &mut Vec<String>, argument: String) -> Result<(), Zapret2CompileError> {
    if arguments.len() >= MAX_ARGUMENTS
        || argument.is_empty()
        || argument.len() > MAX_ARGUMENT_BYTES
        || argument
            .chars()
            .any(|character| character == '\0' || character.is_control())
    {
        return Err(Zapret2CompileError("invalid compiled Zapret2 argument"));
    }
    arguments.push(argument);
    Ok(())
}

fn capture_ports(profiles: &[Profile], tcp: bool) -> Option<String> {
    let mut ports = Vec::new();
    for profile in profiles {
        let filter = if tcp {
            profile.filter_tcp.as_deref()
        } else {
            profile.filter_udp.as_deref()
        };
        for part in filter.into_iter().flat_map(|value| value.split(',')) {
            if !ports.iter().any(|existing| existing == part) {
                ports.push(part.to_owned());
            }
        }
    }
    (!ports.is_empty()).then(|| ports.join(","))
}

fn minimum_version_satisfied(value: &str) -> bool {
    let Some(version) = value.trim().strip_prefix(">=") else {
        return false;
    };
    let mut parts = version.trim().split('.');
    let number = |part: &str| -> Option<u32> {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let Some(major) = parts.next().and_then(number) else {
        return false;
    };
    let Some(minor) = parts.next().and_then(number) else {
        return false;
    };
    let Some(patch) = number(parts.next().unwrap_or("0")) else {
        return false;
    };
    let Some(revision) = number(parts.next().unwrap_or("0")) else {
        return false;
    };
    parts.next().is_none() && WINWS2_VERSION >= (major, minor, patch, revision)
}

fn parse_ports(value: &str) -> Option<Vec<(u16, u16)>> {
    if value.is_empty() || value.len() > 128 || value.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return None;
    }
    value
        .split(',')
        .map(|part| {
            let (start, end) = match part.split_once('-') {
                Some((start, end)) => (start.parse().ok()?, end.parse().ok()?),
                None => {
                    let port = part.parse().ok()?;
                    (port, port)
                }
            };
            (start > 0 && start <= end).then_some((start, end))
        })
        .collect()
}

fn has_high_ports(value: &str) -> bool {
    parse_ports(value).is_some_and(|ranges| ranges.iter().any(|(_, end)| *end > 1023))
}

/// Предел суммарного числа портов у high-port профиля без ipset. Discord media
/// (443 плюс пять альтернативных портов Cloudflare) и voice (`19294-19344` +
/// `50000-50100`, 152 порта) проходят; широкие диапазоны вида `1024-65535` — нет.
const MAX_SCOPED_HIGH_PORTS: u32 = 256;

const RECOGNIZED_UDP_L7: &[&str] = &["discord", "stun"];
const RECOGNIZED_UDP_PAYLOAD: &[&str] = &["discord_ip_discovery", "stun"];

fn bounded_high_ports(value: &str) -> bool {
    parse_ports(value).is_some_and(|ranges| {
        ranges
            .iter()
            .map(|(start, end)| u32::from(*end - *start) + 1)
            .sum::<u32>()
            <= MAX_SCOPED_HIGH_PORTS
    })
}

/// UDP-профиль нельзя сузить хостлистом: в STUN и Discord IP discovery нет имени
/// хоста. Поэтому допускаем только то, что движок распознаёт сам.
fn recognized_udp_scope(filter_l7: &[String], payload: &[String]) -> bool {
    !filter_l7.is_empty()
        && filter_l7
            .iter()
            .all(|value| RECOGNIZED_UDP_L7.contains(&value.as_str()))
        && payload
            .iter()
            .all(|value| RECOGNIZED_UDP_PAYLOAD.contains(&value.as_str()))
}

fn relative_key(path: &Path) -> Result<String, Zapret2CompileError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(Zapret2CompileError("invalid protected resource path"));
    }
    Ok(path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase())
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 240
        && value.is_ascii()
        && !value.contains(['\\', '\0', ':'])
        && !Path::new(value).is_absolute()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn safe_list_basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.ends_with(".txt")
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_pack_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_cli_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VALUE_BYTES
        && !value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || byte.is_ascii_whitespace())
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value
            .chars()
            .any(|character| character == '\0' || character.is_control())
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
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn argv_path(path: &Path) -> String {
    crate::dpi_materializer::engine_file_path(path).replace('\\', "/")
}

fn category_name(category: DpiCategory) -> &'static str {
    match category {
        DpiCategory::Discord => "discord",
        DpiCategory::YoutubeTwitch => "youtube_twitch",
        DpiCategory::Gaming => "gaming",
        DpiCategory::AtRisk => "atrisk",
        DpiCategory::Universal => "universal",
    }
}

fn protocol_category(value: &str) -> Option<DpiCategory> {
    match value {
        "discord" => Some(DpiCategory::Discord),
        "youtube_twitch" => Some(DpiCategory::YoutubeTwitch),
        "gaming" => Some(DpiCategory::Gaming),
        "atrisk" => Some(DpiCategory::AtRisk),
        "universal" => Some(DpiCategory::Universal),
        _ => None,
    }
}

#[cfg(test)]
mod builtin_profile_tests {
    use super::*;

    fn selected(category: &str, transport: &str, profile_id: &str) -> Zapret2ProfileSelection {
        Zapret2ProfileSelection { category: category.into(), transport: transport.into(), profile_id: profile_id.into() }
    }

    #[test]
    fn composition_preserves_parent_actions_and_other_transports_with_disjoint_scopes() {
        use obsession_runtime_protocol::zapret2_composition::{DiscordComposition, DiscordScope};
        let manifest: StrategyPackManifest = serde_json::from_str(include_str!("../../src-tauri/resources/strategy-packs/builtin/manifest.json")).unwrap();
        let pair = DiscordComposition::new("research_discord_tls_hostfake_md5", "classic_discord_tls_split_sni").unwrap();
        let profiles=select_profiles_with_selections(&manifest,"discord",4,false,&[selected("discord","tls",&pair.id())]).unwrap();
        let scoped: Vec<_>=profiles.iter().filter(|profile| profile.composition_scope.is_some()).collect();
        assert_eq!(scoped.len(),2);
        for (profile,scope,id) in [(scoped[0],DiscordScope::Api,&pair.api_profile_id),(scoped[1],DiscordScope::Cdn,&pair.cdn_profile_id)] {
            let parent=manifest.presets.iter().find(|parent| &parent.id==id).unwrap();
            assert_eq!(profile.composition_scope,Some(scope)); assert_eq!(profile.steps,parent.steps); assert_eq!(profile.desync,parent.desync);
        }
        assert_eq!(profiles.iter().filter(|profile|profile_transport(profile)!=Some("tls")).map(|p|&p.id).collect::<Vec<_>>(),
            select_profiles(&manifest,"discord",4,false).unwrap().iter().filter(|profile|profile_transport(profile)!=Some("tls")).map(|p|&p.id).collect::<Vec<_>>());
        for (category,transport) in [("discord","quic"),("youtube_twitch","tls")] {
            assert!(select_profiles_with_selections(&manifest,category,4,false,&[selected(category,transport,&pair.id())]).is_err());
        }
        for parent in ["discord_quic_level_1",CLIENTHELLO_EXPERIMENT_ID,"missing"] {
            let wrong=DiscordComposition::new(&pair.api_profile_id,parent).unwrap();
            assert!(select_profiles_with_selections(&manifest,"discord",4,false,&[selected("discord","tls",&wrong.id())]).is_err());
        }
        let mut value=serde_json::to_value(serde_json::json!({"id":"a","category":"discord","aggressiveness":1,"lua":LUA_LIB})).unwrap();
        value["composition_scope"]=serde_json::json!("Cdn");
        assert!(serde_json::from_value::<StrategyDef>(value).is_err());
    }

    #[test]
    fn named_profiles_overlay_only_matching_slots_and_preserve_gaming_data_plane() {
        let manifest: StrategyPackManifest = serde_json::from_str(include_str!("../../src-tauri/resources/strategy-packs/builtin/manifest.json")).unwrap();
        let selections = vec![selected("discord", "tls", "discord_tls_text"),
            selected("youtube_twitch", "tls", "classic_youtube_twitch_tls_split_sni"),
            selected("gaming", "tls", "classic_gaming_tls_disorder_sni")];
        for category in ["discord", "youtube_twitch", "gaming"] {
            let original = select_profiles(&manifest, category, 4, false).unwrap();
            let result = select_profiles_with_selections(&manifest, category, 4, true, &selections).unwrap();
            let expected = selections.iter().find(|selection| selection.category == category).unwrap();
            assert!(result.iter().any(|profile| profile.id == expected.profile_id));
            assert_eq!(result.iter().filter(|profile| profile_transport(profile) != Some("tls")).map(|profile| &profile.id).collect::<Vec<_>>(),
                original.iter().filter(|profile| profile_transport(profile) != Some("tls")).map(|profile| &profile.id).collect::<Vec<_>>());
            if category == "gaming" {
                assert!(result.iter().any(|profile| profile.id == "gaming_ipset_tls"));
                assert_eq!(result.len(), original.len());
            }
        }
        let clone = select_profiles_with_selections(&manifest, "discord", 4, false, &[selected("discord", "tls", CLIENTHELLO_EXPERIMENT_ID)]).unwrap();
        assert_eq!(clone.iter().filter(|profile| profile.id == CLIENTHELLO_EXPERIMENT_ID).count(), 1);
        assert!(clone.iter().any(|profile| profile.id == "discord_voice_level_4"));
    }

    #[test]
    fn named_profile_mismatch_and_unknown_ids_never_fall_back() {
        let manifest: StrategyPackManifest = serde_json::from_str(include_str!("../../src-tauri/resources/strategy-packs/builtin/manifest.json")).unwrap();
        for profile in [selected("discord", "tls", "youtube_tls"), selected("discord", "tls", "discord_quic_level_1"),
            selected("discord", "tls", "absent_profile"), selected("gaming", "tls", "gaming_ipset_tls")] {
            assert!(select_profiles_with_selections(&manifest, &profile.category, 0, false, &[profile.clone()]).is_err());
        }
        let profile = selected("discord", "tls", "discord_tls_text");
        assert!(select_profiles_with_selections(&manifest, "discord", 0, false, &[profile.clone(), profile]).is_err());
    }

    #[test]
    fn research_recipes_validate_and_overlay_only_the_selected_slot() {
        let manifest: StrategyPackManifest = serde_json::from_str(include_str!("../../src-tauri/resources/strategy-packs/builtin/manifest.json")).unwrap();
        let categories = manifest.categories.iter().map(String::as_str).collect();
        let protocols = manifest.protocols.iter().map(String::as_str).collect();
        let files = manifest.files.iter().map(|file| file.path.as_str()).collect();
        let blobs = manifest.blobs.iter().map(|blob| blob.name.as_str()).collect();
        let research: Vec<_> = manifest.presets.iter().filter(|profile| profile.candidate.is_some()).collect();
        assert_eq!(research.len(), 11);
        for profile in research {
            validate_strategy(profile, &categories, &protocols, &files).unwrap();
            validate_action_blobs(profile, &blobs).unwrap();
            let transport = profile_transport(profile).unwrap();
            let original = select_profiles(&manifest, &profile.category, 4, false).unwrap();
            assert!(original.iter().all(|entry| entry.candidate.is_none()));
            let result = select_profiles_with_selections(&manifest, &profile.category, 4, false,
                &[selected(&profile.category, transport, &profile.id)]).unwrap();
            assert!(result.iter().any(|entry| entry.id == profile.id));
            assert_eq!(original.iter().filter(|entry| profile_transport(entry) != Some(transport)).map(|entry| &entry.id).collect::<Vec<_>>(),
                result.iter().filter(|entry| profile_transport(entry) != Some(transport)).map(|entry| &entry.id).collect::<Vec<_>>());
        }
        let mut profile = manifest.presets.iter().find(|profile| !profile.steps.is_empty()).unwrap().clone();
        profile.steps[0].action = "fake:blob=missing".into();
        assert!(validate_action_blobs(&profile, &blobs).is_err());
        profile.desync.push("multisplit:pos=2".into());
        assert!(validate_strategy(&profile, &categories, &protocols, &files).is_err());
    }

    #[test]
    fn experiment_overlays_tls_only_and_is_excluded_from_default_ladder() {
        let mut manifest: StrategyPackManifest = serde_json::from_str(include_str!(
            "../../src-tauri/resources/strategy-packs/builtin/manifest.json"
        )).unwrap();
        assert_eq!(manifest.strategies.len(), 19);
        let ids = |profiles: Vec<StrategyDef>| profiles.into_iter().map(|strategy| strategy.id).collect::<Vec<_>>();
        assert!(ids(select_profiles(&manifest, "discord", 0, false).unwrap()).contains(&"discord_tls_multidisorder_alt".to_string()));
        for level in 0..=4 {
            let stable = select_profiles(&manifest, "discord", level, false).unwrap();
            let experimental = select_profiles(&manifest, "discord", level, true).unwrap();
            assert_eq!(experimental.iter().filter(|strategy| is_discord_tls_profile(strategy)).count(), 1);
            assert!(experimental.iter().any(|strategy| strategy.id == CLIENTHELLO_EXPERIMENT_ID));
            assert_eq!(ids(experimental.into_iter().filter(|strategy| !is_discord_tls_profile(strategy)).collect()),
                ids(stable.into_iter().filter(|strategy| !is_discord_tls_profile(strategy)).collect()));
            for category in ["youtube_twitch", "gaming"] {
                assert_eq!(ids(select_profiles(&manifest, category, level, true).unwrap()), ids(select_profiles(&manifest, category, level, false).unwrap()));
            }
        }
        manifest.experiments.clear();
        assert!(select_profiles(&manifest, "discord", 0, true).is_err());
        assert!(select_profiles(&manifest, "discord", 0, false).is_ok());
    }

    #[test]
    fn compares_all_four_engine_version_components() {
        for requirement in [">=1.0", ">=1.0.4", ">=1.0.5", ">=1.0.5.1", ">=1.0.5.2"] {
            assert!(minimum_version_satisfied(requirement), "{requirement}");
        }
        for requirement in [
            ">=1.0.5.3",
            ">=1.0.6",
            ">=2.0",
            ">=1.0.bad",
            ">=1.0.5.",
            ">=1.0.5.2.0",
            ">=1.0.5.+2",
            ">=1.0.5.4294967296",
        ] {
            assert!(!minimum_version_satisfied(requirement), "{requirement}");
        }
    }

    #[test]
    fn builtin_voice_profiles_are_hostname_free() {
        let manifest: StrategyPackManifest = serde_json::from_str(include_str!(
            "../../src-tauri/resources/strategy-packs/builtin/manifest.json"
        ))
        .unwrap();
        for level in 1..=4 {
            let profiles = manifest
                .strategies
                .iter()
                .filter(|s| s.category == "discord" && s.aggressiveness == level)
                .collect::<Vec<_>>();
            let voice = profiles
                .iter()
                .find(|s| s.filter_l7.iter().any(|l7| l7 == "discord"))
                .unwrap();
            // No resources: this would fail if the default discord.txt were bound.
            let built =
                profile_from_strategy(voice, DpiCategory::Discord, &BTreeMap::new()).unwrap();
            assert!(built.hostlist.is_none());
        }
    }







}

#[cfg(test)]
mod high_port_scope_tests {
    use super::*;

    fn validate(extra: &str) -> Result<(), Zapret2CompileError> {
        let json = format!(
            r#"{{"id":"discord_probe","category":"discord","aggressiveness":1,
                 "lua":"lua/zapret-antidpi.lua","desync":["multisplit:pos=1"]{extra}}}"#
        );
        let strategy: StrategyDef = serde_json::from_str(&json).expect("valid StrategyDef");
        let categories = BTreeSet::from(["discord"]);
        let protocols = BTreeSet::from(["tcp", "tls", "udp", "quic"]);
        let files = BTreeSet::from(["lua/zapret-antidpi.lua"]);
        validate_strategy(&strategy, &categories, &protocols, &files)
    }

    #[test]
    fn accepts_bounded_high_tcp_ports_scoped_by_hostlist() {
        // Альтернативные порты Cloudflare, на которых сидят CDN и часть клиента
        // Discord. Сужение — хостлист категории, подставляемый компилятором.
        assert!(validate(r#","filter_tcp":"443,2053,2083,2087,2096,8443""#).is_ok());
    }

    #[test]
    fn rejects_wide_high_tcp_ports_even_with_hostlist() {
        assert!(validate(r#","filter_tcp":"1024-65535""#).is_err());
    }

    #[test]
    fn accepts_discord_voice_udp_with_recognized_l7_and_payload() {
        assert!(validate(
            r#","filter_udp":"3478-3480,19294-19344,50000-50100","filter_l7":["discord","stun"],"payload":["discord_ip_discovery","stun"]"#
        )
        .is_ok());
    }

    #[test]
    fn rejects_high_udp_ports_without_l7_scope() {
        assert!(validate(r#","filter_udp":"19294-19344""#).is_err());
    }

    #[test]
    fn rejects_high_udp_ports_with_foreign_l7_or_payload() {
        assert!(validate(
            r#","filter_udp":"19294-19344","filter_l7":["tls"],"payload":["discord_ip_discovery"]"#
        )
        .is_err());
        assert!(validate(
            r#","filter_udp":"19294-19344","filter_l7":["discord"],"payload":["tls_client_hello"]"#
        )
        .is_err());
    }

    #[test]
    fn rejects_wide_udp_range_even_with_recognized_l7() {
        assert!(validate(
            r#","filter_udp":"1024-65535","filter_l7":["discord","stun"],"payload":["discord_ip_discovery"]"#
        )
        .is_err());
    }

    #[test]
    fn ipset_keeps_permitting_wide_ranges() {
        assert!(validate(r#","filter_udp":"1024-65535","ipset":"ipset-global.txt""#).is_ok());
    }
}
