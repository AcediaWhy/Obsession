//! One-time recovery of named fixed choices from the retired cache.
//!
//! Cache data is never compiled or executed. Only an exact native semantic
//! match can resolve to a profile ID already present in the verified pack.
//! Historical confirmation is not a claim that the current network works.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::dpi_engine::{manifest::StrategyDef, profile_transport, LoadedPack, Zapret2ProfileSelection};

const MAX_CACHE_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct SavedRecord {
    engine_version: String,
    strategy_schema_version: u32,
    trust: String,
    confirmed_at: u64,
    #[serde(default)]
    disabled_reason: Option<String>,
    candidate: SavedCandidate,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedCandidate {
    schema_version: u32,
    category: String,
    transport: String,
    steps: Vec<SavedStep>,
    payload: Vec<String>,
    out_range: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedStep {
    function: String,
    args: BTreeMap<String, Value>,
}

#[derive(PartialEq, Eq)]
struct NativeStep {
    function: String,
    args: BTreeMap<String, String>,
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::Bool(true) => Some(String::new()),
        Value::Number(value) => value.as_i64().map(|value| value.to_string()),
        Value::String(value) if value.len() <= 128 && value.bytes().all(|byte| (32..=126).contains(&byte) && byte != b':') => Some(value.clone()),
        _ => None,
    }
}

fn native_step(function: &str, args: BTreeMap<String, String>) -> Option<NativeStep> {
    if !matches!(function, "fake" | "multisplit" | "multidisorder" | "multidisorder_legacy" | "fakedsplit" | "fakeddisorder")
        || args.len() > 16
        || args.keys().any(|key| key.is_empty() || key.len() > 32 || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')) {
        return None;
    }
    // Native numeric marker 1 is byte offset 1. The Lua resolver adds one:
    // delete_pos_1 removes offset 0, not the literal native marker 1.
    // Preserve all positions, order and fake parameters exactly.
    Some(NativeStep { function: function.to_owned(), args })
}

fn saved_steps(candidate: &SavedCandidate) -> Option<Vec<NativeStep>> {
    if candidate.steps.is_empty() || candidate.steps.len() > 4 { return None; }
    candidate.steps.iter().map(|step| {
        let args = step.args.iter().map(|(key, value)| scalar(value).map(|value| (key.clone(), value)))
            .collect::<Option<BTreeMap<_, _>>>()?;
        native_step(&step.function, args)
    }).collect()
}

fn pack_steps(profile: &StrategyDef) -> Option<Vec<NativeStep>> {
    profile.desync.iter().map(|step| {
        let mut pieces = step.split(':');
        let function = pieces.next()?;
        let mut args = BTreeMap::new();
        for piece in pieces {
            let (key, value) = piece.split_once('=').unwrap_or((piece, ""));
            if args.insert(key.to_owned(), value.to_owned()).is_some() { return None; }
        }
        native_step(function, args)
    }).collect()
}

fn import_bytes(bytes: &[u8], pack: &LoadedPack) -> Vec<Zapret2ProfileSelection> {
    let Ok(cache) = serde_json::from_slice::<Value>(bytes) else { return Vec::new(); };
    if cache.get("schema_version").and_then(Value::as_u64) != Some(1) { return Vec::new(); }
    let Some(networks) = cache.get("networks").and_then(Value::as_object) else { return Vec::new(); };
    if networks.len() > 64 { return Vec::new(); }
    let (a, b, c, d) = crate::dpi_engine::zapret2::WINWS2_VERSION;
    let version = format!("{a}.{b}.{c}.{d}");
    let mut newest: BTreeMap<(String, String), (u64, String)> = BTreeMap::new();
    for network in networks.values() {
        let Some(categories) = network.get("categories").and_then(Value::as_object) else { continue; };
        if categories.len() > 16 { continue; }
        for (key, value) in categories {
            let Ok(record) = serde_json::from_value::<SavedRecord>(value.clone()) else { continue; };
            let candidate = &record.candidate;
            if record.engine_version != version || record.strategy_schema_version != 1 || candidate.schema_version != 1
                || record.trust != "confirmed" || record.confirmed_at == 0 || record.disabled_reason.is_some()
                || !matches!(candidate.category.as_str(), "discord" | "youtube_twitch" | "gaming")
                || !matches!(candidate.transport.as_str(), "tls" | "quic")
                || *key != format!("{}:{}", candidate.category, candidate.transport) {
                continue;
            }
            let expected_payload = if candidate.transport == "tls" { "tls_client_hello" } else { "quic_initial" };
            if candidate.payload != [expected_payload] { continue; }
            let out_range = match candidate.out_range.as_deref() {
                None | Some("always") => None,
                Some("-d10") => Some("-d10"),
                _ => continue,
            };
            let Some(steps) = saved_steps(candidate) else { continue; };
            let matched = pack.manifest.strategies.iter().chain(pack.manifest.presets.iter()).find(|profile| {
                profile.category == candidate.category && profile.lua == "lua/zapret-antidpi.lua"
                    && profile_transport(profile) == Some(candidate.transport.as_str())
                    && profile.payload == candidate.payload && profile.out_range.as_deref() == out_range
                    && pack_steps(profile).as_ref() == Some(&steps)
            });
            let Some(profile) = matched else { continue; };
            let slot = (candidate.category.clone(), candidate.transport.clone());
            let selected = (record.confirmed_at, profile.id.clone());
            if newest.get(&slot).is_none_or(|previous| selected > *previous) {
                newest.insert(slot, selected);
            }
        }
    }
    newest.into_iter().map(|((category, transport), (_, profile_id))| Zapret2ProfileSelection {
        category, transport, profile_id,
    }).collect()
}

/// Reads only the fixed historical cache filename; writes no user data.
/// Caller supplies an integrity-verified pack and decides whether to persist
/// the returned fixed IDs as manual choices during its one-time migration.
pub fn import_saved(base_dir: &Path, pack: &LoadedPack) -> Vec<Zapret2ProfileSelection> {
    let Ok(file) = std::fs::File::open(base_dir.join("adaptive-strategies.json")) else { return Vec::new(); };
    if file.metadata().map_or(true, |metadata| metadata.len() > MAX_CACHE_BYTES) { return Vec::new(); }
    let mut bytes = Vec::new();
    if file.take(MAX_CACHE_BYTES + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > MAX_CACHE_BYTES {
        return Vec::new();
    }
    import_bytes(&bytes, pack)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pack() -> LoadedPack {
        LoadedPack { manifest: serde_json::from_slice(include_bytes!("../resources/strategy-packs/builtin/manifest.json")).unwrap(),
            dir: Path::new("test-only-verified-pack").to_path_buf() }
    }
    fn record(category: &str, transport: &str, function: &str, args: Value, out_range: Value, confirmed_at: u64) -> Value {
        json!({"engine_version":"1.0.5.2", "strategy_schema_version":1, "trust":"confirmed", "confirmed_at":confirmed_at,
            "disabled_reason":null, "candidate":{"schema_version":1,"category":category,"transport":transport,
                "steps":[{"function":function,"args":args}],"payload":[if transport=="tls" {"tls_client_hello"} else {"quic_initial"}],"out_range":out_range}})
    }
    fn cache(records: Value) -> Vec<u8> { serde_json::to_vec(&json!({"schema_version":1,"networks":{"fixture":{"categories":records}}})).unwrap() }

    #[test]
    fn imports_exact_old_fixed_choices_without_executing_them() {
        let input = cache(json!({
            "discord:tls":record("discord","tls","multidisorder",json!({"pos":"1,2,midsld,sniext"}),json!("-d10"),10),
            "youtube_twitch:tls":record("youtube_twitch","tls","multidisorder",json!({"pos":"1,midsld"}),json!("-d10"),11),
            "youtube_twitch:quic":record("youtube_twitch","quic","fake",json!({"blob":"fake_default_quic","repeats":1}),Value::Null,12)
        }));
        let actual = import_bytes(&input, &pack());
        assert_eq!(actual.len(), 3);
        assert_eq!(actual[0].profile_id, "discord_tls_multidisorder_alt");
        assert_eq!(actual[1].profile_id, "classic_youtube_twitch_quic_fake_default_once");
        assert_eq!(actual[2].profile_id, "classic_youtube_twitch_tls_saved_disorder");
    }

    #[test]
    fn unsupported_untrusted_and_modified_commands_are_ignored() {
        let valid = record("youtube_twitch","tls","multidisorder",json!({"pos":"1,midsld"}),json!("-d10"),10);
        let mut wrong_payload = valid.clone(); wrong_payload["candidate"]["payload"] = json!(["all"]);
        let mut unknown = valid.clone(); unknown["candidate"]["steps"][0]["function"] = json!("luaexec");
        let mut disabled = valid.clone(); disabled["disabled_reason"] = json!("failed");
        let mut old_engine = valid.clone(); old_engine["engine_version"] = json!("1.0.4");
        let mut unconfirmed = valid.clone(); unconfirmed["trust"] = json!("suggested");
        let mut injected = valid.clone(); injected["candidate"]["steps"][0]["args"]["pos"] = json!("midsld:tcp_seq=1000");
        let mut no_schema = valid.clone(); no_schema["candidate"]["schema_version"] = json!(2);
        let mut extra_arg = valid.clone(); extra_arg["candidate"]["steps"][0]["args"]["blob"] = json!("tls_google");
        let mut arbitrary_range = valid.clone(); arbitrary_range["candidate"]["out_range"] = json!("-d100");
        for invalid in [wrong_payload, unknown, disabled, old_engine, unconfirmed, injected, no_schema, extra_arg, arbitrary_range] {
            assert!(import_bytes(&cache(json!({"youtube_twitch:tls":invalid})), &pack()).is_empty());
        }
        assert!(import_bytes(&cache(json!({"discord:tls":valid})), &pack()).is_empty());
        assert!(import_bytes(b"not json", &pack()).is_empty());
    }

    #[test]
    fn flags_and_step_order_must_match_verified_named_profile() {
        let mut saved = record("gaming","tls","fake",json!({"tcp_md5":true,"tcp_seq":-10000,"repeats":2,"blob":"fake_default_tls"}),json!("-d10"),10);
        saved["candidate"]["steps"].as_array_mut().unwrap().push(json!({"function":"multidisorder","args":{"pos":"midsld"}}));
        assert_eq!(import_bytes(&cache(json!({"gaming:tls":saved.clone()})), &pack())[0].profile_id,"classic_gaming_tls_fake_md5_disorder");
        saved["candidate"]["steps"][0]["args"]["tcp_md5"] = json!(false);
        assert!(import_bytes(&cache(json!({"gaming:tls":saved})), &pack()).is_empty());
        let data = record("gaming","tls","fakeddisorder",json!({"blob":"tls_google","pos":"midsld"}),json!("-d10"),10);
        assert!(import_bytes(&cache(json!({"gaming:tls":data})), &pack()).is_empty());
    }

    #[test]
    fn newest_confirmed_known_choice_per_slot_wins() {
        let old = record("youtube_twitch","tls","multidisorder",json!({"pos":"midsld"}),json!("-d10"),10);
        let new = record("youtube_twitch","tls","multisplit",json!({"pos":"sniext"}),json!("-d10"),20);
        let bytes = serde_json::to_vec(&json!({"schema_version":1,"networks":{
            "old":{"categories":{"youtube_twitch:tls":old}},"new":{"categories":{"youtube_twitch:tls":new}}
        }})).unwrap();
        let imported = import_bytes(&bytes,&pack());
        assert_eq!(imported.len(),1);
        assert_eq!(imported[0].profile_id,"classic_youtube_twitch_tls_split_extension");
    }

    #[test]
    fn numeric_first_byte_marker_is_never_discarded_as_lua_index_one() {
        let without = record("youtube_twitch","tls","multidisorder",json!({"pos":"midsld"}),json!("-d10"),10);
        let with = record("youtube_twitch","tls","multidisorder",json!({"pos":"1,midsld"}),json!("-d10"),10);
        assert_eq!(import_bytes(&cache(json!({"youtube_twitch:tls":without})),&pack())[0].profile_id,"classic_youtube_twitch_tls_disorder_sni");
        assert_eq!(import_bytes(&cache(json!({"youtube_twitch:tls":with})),&pack())[0].profile_id,"classic_youtube_twitch_tls_saved_disorder");
        let unknown = record("youtube_twitch","tls","multidisorder",json!({"pos":"1,1,midsld"}),json!("-d10"),10);
        assert!(import_bytes(&cache(json!({"youtube_twitch:tls":unknown})),&pack()).is_empty());
    }

    #[test]
    fn reading_fixed_cache_is_bounded_and_does_not_change_its_bytes() {
        let root = std::env::temp_dir().join(format!("obsession-saved-profile-test-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert_eq!(root.parent(),Some(std::env::temp_dir().as_path()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("adaptive-strategies.json");
        let bytes = cache(json!({"youtube_twitch:tls":record("youtube_twitch","tls","multidisorder",json!({"pos":"1,midsld"}),json!("-d10"),10)}));
        std::fs::write(&source,&bytes).unwrap();
        assert_eq!(import_saved(&root,&pack()).len(),1);
        assert_eq!(std::fs::read(&source).unwrap(),bytes);
        std::fs::write(&source,vec![b' ';MAX_CACHE_BYTES as usize+1]).unwrap();
        assert!(import_saved(&root,&pack()).is_empty());
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
