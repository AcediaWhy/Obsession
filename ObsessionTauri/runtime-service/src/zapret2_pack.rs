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
    DpiCategory, DpiEngine, Zapret2AdaptiveFunction, Zapret2AdaptiveOverride,
    Zapret2AdaptivePayload, Zapret2AdaptiveRange, Zapret2AdaptiveTransport, Zapret2AdaptiveValue,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::protected_layout::{VerifiedDpiPlan, VerifiedResource};

const PACK_SCHEMA_VERSION: u32 = 1;
const WINWS2_VERSION: (u32, u32, u32, u32) = (1, 0, 5, 2);
const LUA_API: u32 = 6;
const LUA_LIB: &str = "lua/zapret-lib.lua";
const MAX_PACK_BYTES: u64 = 1024 * 1024;
const MAX_PACK_FILES: usize = 256;
const MAX_PACK_BLOBS: usize = 64;
const MAX_PACK_STRATEGIES: usize = 512;
const MAX_PROFILE_VALUES: usize = 64;
/// Глубина лестницы агрессивности. Раньше здесь стояло жёсткое `3`, хотя
/// `profiles_for` в приложении вычисляет максимум динамически и уровень 0 берёт
/// самую агрессивную ступень. Из-за этого пак с четвёртой ступенью отвергался
/// службой, а не приложением. Предел оставлен, но перестал диктовать длину
/// лестницы: сколько ступеней в паке — решает пак.
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
    id: String,
    category: String,
    aggressiveness: u8,
    lua: String,
    #[serde(default)]
    desync: Vec<String>,
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
        let available = manifest
            .strategies
            .iter()
            .filter(|strategy| strategy.category == category)
            .collect::<Vec<_>>();
        let maximum = available
            .iter()
            .map(|strategy| strategy.aggressiveness)
            .max()
            .ok_or(Zapret2CompileError("selected category has no profiles"))?;
        let level = if plan.zapret2_level() > 0
            && available
                .iter()
                .any(|strategy| strategy.aggressiveness == plan.zapret2_level())
        {
            plan.zapret2_level()
        } else {
            maximum
        };
        let selected_profiles = available
            .into_iter()
            .filter(|strategy| strategy.aggressiveness == level)
            .collect::<Vec<_>>();
        for strategy in &selected_profiles {
            if !lua_files.contains(&strategy.lua) {
                lua_files.push(strategy.lua.clone());
            }
        }

        let overrides = plan
            .zapret2_overrides()
            .iter()
            .filter(|candidate| candidate.category == selected.category())
            .collect::<Vec<_>>();
        for candidate in &overrides {
            let control = selected_profiles
                .iter()
                .copied()
                .find(|strategy| {
                    strategy.ipset.is_none()
                        && strategy_supports_transport(strategy, candidate.transport)
                })
                .ok_or(Zapret2CompileError(
                    "adaptive override has no protected control profile",
                ))?;
            let control_profile = profile_from_strategy(control, selected.category(), &resources)?;
            profiles.push(adaptive_profile(candidate, &control_profile)?);
        }

        for strategy in selected_profiles {
            let replaced = strategy.ipset.is_none()
                && overrides
                    .iter()
                    .any(|candidate| strategy_supports_transport(strategy, candidate.transport));
            if !replaced {
                profiles.push(profile_from_strategy(
                    strategy,
                    selected.category(),
                    &resources,
                )?);
            }
        }
    }
    if profiles.is_empty() {
        return Err(Zapret2CompileError("Strategy Pack produced no profiles"));
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
        push_option(&mut arguments, "--ipset", profile.ipset.as_deref())?;
        push_option(&mut arguments, "--out-range", profile.out_range.as_deref())?;
        push_option(&mut arguments, "--in-range", profile.in_range.as_deref())?;
        push_joined(&mut arguments, "--payload", &profile.payload)?;
        for desync in &profile.desync {
            push_argument(&mut arguments, format!("--lua-desync={desync}"))?;
        }
        if index + 1 < profiles.len() {
            push_argument(&mut arguments, "--new".into())?;
        }
    }
    Ok(arguments)
}

fn strategy_supports_transport(
    strategy: &StrategyDef,
    transport: Zapret2AdaptiveTransport,
) -> bool {
    match transport {
        Zapret2AdaptiveTransport::Tls => {
            strategy.transports.is_empty() || strategy.transports.iter().any(|value| value == "tcp")
        }
        Zapret2AdaptiveTransport::Quic => strategy.transports.iter().any(|value| value == "quic"),
    }
}

fn adaptive_profile(
    candidate: &Zapret2AdaptiveOverride,
    control: &Profile,
) -> Result<Profile, Zapret2CompileError> {
    let bytes = serde_json::to_vec(candidate)
        .map_err(|_| Zapret2CompileError("could not fingerprint adaptive override"))?;
    let digest = Sha256::digest(bytes);
    let short = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let desync = candidate
        .steps
        .iter()
        .map(|step| {
            let mut rendered = match step.function {
                Zapret2AdaptiveFunction::Fake => "fake",
                Zapret2AdaptiveFunction::MultiSplit => "multisplit",
                Zapret2AdaptiveFunction::MultiDisorder => "multidisorder",
                Zapret2AdaptiveFunction::MultiDisorderLegacy => "multidisorder_legacy",
                Zapret2AdaptiveFunction::FakeDSplit => "fakedsplit",
                Zapret2AdaptiveFunction::FakeDDisorder => "fakeddisorder",
                Zapret2AdaptiveFunction::SendIpFrag => "send:ipfrag",
                Zapret2AdaptiveFunction::Drop => "drop",
            }
            .to_owned();
            for (key, value) in &step.args {
                match value {
                    Zapret2AdaptiveValue::Bool(true) => {
                        rendered.push(':');
                        rendered.push_str(key);
                    }
                    Zapret2AdaptiveValue::Bool(false) => {
                        return Err(Zapret2CompileError("invalid adaptive false flag"));
                    }
                    Zapret2AdaptiveValue::Integer(value) => {
                        rendered.push(':');
                        rendered.push_str(key);
                        rendered.push('=');
                        rendered.push_str(&value.to_string());
                    }
                    Zapret2AdaptiveValue::Text(value) => {
                        rendered.push(':');
                        rendered.push_str(key);
                        rendered.push('=');
                        rendered.push_str(value);
                    }
                }
            }
            if !valid_cli_value(&rendered) {
                return Err(Zapret2CompileError("invalid compiled adaptive step"));
            }
            Ok(rendered)
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Порты наследуются у контрольного профиля пака. Схема кандидата их не
    // описывает, а зашитый «443» отбирал у профиля покрытие альтернативных
    // портов Cloudflare: подтверждённый кандидат молча сужал обход до 443.
    // Контрольный профиль high-port правило уже прошёл, поэтому унаследованные
    // порты допустимы по построению.
    let (filter_tcp, filter_udp, filter_l7) = match candidate.transport {
        Zapret2AdaptiveTransport::Tls => (
            control.filter_tcp.clone().or_else(|| Some("443".into())),
            None,
            vec!["tls".into()],
        ),
        Zapret2AdaptiveTransport::Quic => (
            None,
            control.filter_udp.clone().or_else(|| Some("443".into())),
            vec!["quic".into()],
        ),
    };
    let hostlist = control.hostlist.clone().ok_or(Zapret2CompileError(
        "adaptive override has no protected hostlist",
    ))?;
    let expected_payload = match candidate.transport {
        Zapret2AdaptiveTransport::Tls => Zapret2AdaptivePayload::TlsClientHello,
        Zapret2AdaptiveTransport::Quic => Zapret2AdaptivePayload::QuicInitial,
    };
    if candidate.payload != expected_payload {
        return Err(Zapret2CompileError(
            "adaptive payload does not match transport",
        ));
    }
    Ok(Profile {
        name: format!("adaptive-{short}"),
        filter_tcp,
        filter_udp,
        filter_l7,
        hostlist: Some(hostlist),
        ipset: None,
        payload: vec![match candidate.payload {
            Zapret2AdaptivePayload::TlsClientHello => "tls_client_hello".into(),
            Zapret2AdaptivePayload::QuicInitial => "quic_initial".into(),
        }],
        out_range: match candidate.out_range {
            Some(Zapret2AdaptiveRange::FirstTenDataPackets) => Some("-d10".into()),
            Some(Zapret2AdaptiveRange::Always) | None => None,
        },
        in_range: None,
        desync,
    })
}

fn validate_manifest(
    manifest: &StrategyPackManifest,
    pack_root: &Path,
    resources: &BTreeMap<String, &VerifiedResource>,
) -> Result<(), Zapret2CompileError> {
    if manifest.schema_version != PACK_SCHEMA_VERSION
        || !valid_pack_id(&manifest.pack_id)
        || !valid_text(&manifest.pack_version, 64)
        || !minimum_version_satisfied(&manifest.engine.winws2_min)
        || manifest.engine.lua_api > LUA_API
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_PACK_FILES
        || manifest.blobs.len() > MAX_PACK_BLOBS
        || manifest.strategies.is_empty()
        || manifest.strategies.len() > MAX_PACK_STRATEGIES
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
    for strategy in &manifest.strategies {
        validate_strategy(strategy, &categories, &protocols, &files)?;
        if !strategy_ids.insert(strategy.id.as_str()) {
            return Err(Zapret2CompileError("duplicate Strategy Pack profile"));
        }
    }
    if let Some(fallback) = &manifest.fallback {
        if !strategy_ids.contains(fallback.as_str()) {
            return Err(Zapret2CompileError("invalid Strategy Pack fallback"));
        }
    }
    if manifest.probes.iter().any(|probe| !valid_domain(probe)) {
        return Err(Zapret2CompileError("invalid Strategy Pack probe"));
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
        || strategy.desync.is_empty()
        || strategy.desync.len() > MAX_PROFILE_VALUES
        || strategy.transports.len() > 8
        || strategy.filter_l7.len() > MAX_PROFILE_VALUES
        || strategy.payload.len() > MAX_PROFILE_VALUES
    {
        return Err(Zapret2CompileError("invalid Strategy Pack profile"));
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
        .map(|name| list_resource(&name, resources).map(|resource| argv_path(resource.path())))
        .transpose()?;
    let ipset = strategy
        .ipset
        .as_deref()
        .map(|name| list_resource(name, resources).map(|resource| argv_path(resource.path())))
        .transpose()?;
    Ok(Profile {
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
mod adaptive_profile_tests {
    use super::*;

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
    use obsession_runtime_protocol::{Zapret2AdaptiveStep, ZAPRET2_ADAPTIVE_SCHEMA_VERSION};

    #[test]
    fn builtin_voice_profiles_are_hostname_free_and_survive_quic_overrides() {
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
            assert!(!strategy_supports_transport(
                voice,
                Zapret2AdaptiveTransport::Quic
            ));
            assert!(!strategy_supports_transport(
                voice,
                Zapret2AdaptiveTransport::Tls
            ));
            assert!(profiles
                .iter()
                .any(|s| strategy_supports_transport(s, Zapret2AdaptiveTransport::Quic)));
            assert!(profiles
                .iter()
                .any(|s| strategy_supports_transport(s, Zapret2AdaptiveTransport::Tls)));
        }
    }

    fn control(filter_tcp: Option<&str>, filter_udp: Option<&str>) -> Profile {
        Profile {
            name: "control".into(),
            filter_tcp: filter_tcp.map(str::to_owned),
            filter_udp: filter_udp.map(str::to_owned),
            filter_l7: vec!["tls".into()],
            hostlist: Some("C:\\lists\\discord.txt".into()),
            ipset: None,
            payload: vec!["tls_client_hello".into()],
            out_range: Some("-d10".into()),
            in_range: None,
            desync: vec!["multidisorder:pos=1,2,midsld,sniext".into()],
        }
    }

    fn candidate(transport: Zapret2AdaptiveTransport) -> Zapret2AdaptiveOverride {
        Zapret2AdaptiveOverride {
            schema_version: ZAPRET2_ADAPTIVE_SCHEMA_VERSION,
            category: DpiCategory::Discord,
            transport,
            steps: vec![Zapret2AdaptiveStep {
                function: Zapret2AdaptiveFunction::MultiDisorder,
                args: BTreeMap::from([(
                    "pos".to_owned(),
                    Zapret2AdaptiveValue::Text("1,midsld".into()),
                )]),
            }],
            payload: match transport {
                Zapret2AdaptiveTransport::Tls => Zapret2AdaptivePayload::TlsClientHello,
                Zapret2AdaptiveTransport::Quic => Zapret2AdaptivePayload::QuicInitial,
            },
            out_range: Some(Zapret2AdaptiveRange::FirstTenDataPackets),
        }
    }

    #[test]
    fn inherits_alt_ports_from_the_control_profile() {
        let control = control(Some("443,2053,2083,2087,2096,8443"), None);
        let built = adaptive_profile(&candidate(Zapret2AdaptiveTransport::Tls), &control).unwrap();
        assert_eq!(
            built.filter_tcp.as_deref(),
            Some("443,2053,2083,2087,2096,8443")
        );
        assert_eq!(built.filter_udp, None);
        assert_eq!(built.hostlist, control.hostlist);
    }

    #[test]
    fn falls_back_to_443_when_the_control_profile_has_no_filter() {
        let built = adaptive_profile(
            &candidate(Zapret2AdaptiveTransport::Tls),
            &control(None, None),
        )
        .unwrap();
        assert_eq!(built.filter_tcp.as_deref(), Some("443"));
    }

    #[test]
    fn quic_candidate_inherits_the_udp_filter() {
        let control = control(None, Some("443,19294-19344"));
        let built = adaptive_profile(&candidate(Zapret2AdaptiveTransport::Quic), &control).unwrap();
        assert_eq!(built.filter_udp.as_deref(), Some("443,19294-19344"));
        assert_eq!(built.filter_tcp, None);
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
