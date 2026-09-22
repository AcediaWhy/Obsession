//! Типизированная сборка argv для winws2.
//!
//! В Zapret2 `--new` завершает текущий профиль и начинает следующий. Поэтому
//! глобальные параметры идут один раз, первый профиль начинается без `--new`,
//! а bare `--new` вставляется только МЕЖДУ полностью описанными профилями.

use super::manifest::StrategyDef;

pub const WINWS2_VERSION: (u32, u32, u32, u32) = (1, 0, 5, 2);
/// Patched `winws2` v1.0.5.2-h2 keeps upstream `lua_compat_ver 6`.
pub const LUA_API: u32 = 6;
pub const LUA_LIB: &str = "lua/zapret-lib.lua";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobArg {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Zapret2Invocation {
    pub wf_tcp_out: Option<String>,
    pub wf_udp_out: Option<String>,
    pub lua_init: Vec<String>,
    pub blobs: Vec<BlobArg>,
    pub profiles: Vec<Zapret2Profile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zapret2Profile {
    pub name: String,
    pub filter_tcp: Option<String>,
    pub filter_udp: Option<String>,
    pub filter_l7: Vec<String>,
    pub hostlist: Option<String>,
    pub ipset: Option<String>,
    pub payload: Vec<String>,
    pub out_range: Option<String>,
    pub in_range: Option<String>,
    pub lua_desync: Vec<String>,
}

pub fn build_winws2_args(invocation: &Zapret2Invocation) -> Vec<String> {
    let mut args = Vec::new();

    if let Some(tcp) = &invocation.wf_tcp_out {
        args.push(format!("--wf-tcp-out={tcp}"));
    }
    if let Some(udp) = &invocation.wf_udp_out {
        args.push(format!("--wf-udp-out={udp}"));
    }
    for lua in &invocation.lua_init {
        if !lua.is_empty() {
            args.push(format!("--lua-init=@{lua}"));
        }
    }
    for blob in &invocation.blobs {
        args.push(format!("--blob={}:@{}", blob.name, blob.path));
    }

    for (index, profile) in invocation.profiles.iter().enumerate() {
        if !profile.name.is_empty() {
            args.push(format!("--name={}", profile.name));
        }
        if let Some(tcp) = &profile.filter_tcp {
            args.push(format!("--filter-tcp={tcp}"));
        }
        if let Some(udp) = &profile.filter_udp {
            args.push(format!("--filter-udp={udp}"));
        }
        if !profile.filter_l7.is_empty() {
            args.push(format!("--filter-l7={}", profile.filter_l7.join(",")));
        }
        if let Some(hostlist) = &profile.hostlist {
            args.push(format!("--hostlist={hostlist}"));
        }
        if let Some(ipset) = &profile.ipset {
            args.push(format!("--ipset={ipset}"));
        }
        if let Some(range) = &profile.out_range {
            args.push(format!("--out-range={range}"));
        }
        if let Some(range) = &profile.in_range {
            args.push(format!("--in-range={range}"));
        }
        if !profile.payload.is_empty() {
            args.push(format!("--payload={}", profile.payload.join(",")));
        }
        for desync in &profile.lua_desync {
            args.push(format!("--lua-desync={desync}"));
        }

        if index + 1 < invocation.profiles.len() {
            args.push("--new".to_string());
        }
    }

    args
}

pub fn profile_from_strategy_def(
    strategy: &StrategyDef,
    hostlist: Option<String>,
    ipset: Option<String>,
) -> Zapret2Profile {
    let has = |transport: &str| strategy.transports.iter().any(|x| x == transport);
    let tcp = strategy.transports.is_empty() || has("tcp") || has("tls") || has("http");
    let udp = has("udp") || has("quic");

    Zapret2Profile {
        name: strategy.id.clone(),
        filter_tcp: strategy
            .filter_tcp
            .clone()
            .or_else(|| tcp.then(|| "443".to_string())),
        filter_udp: strategy
            .filter_udp
            .clone()
            .or_else(|| udp.then(|| "443".to_string())),
        filter_l7: strategy.filter_l7.clone(),
        hostlist,
        ipset,
        payload: strategy.payload.clone(),
        out_range: strategy.out_range.clone(),
        in_range: strategy.in_range.clone(),
        lua_desync: strategy.desync.clone(),
    }
}

fn filter_has_high_port(filter: &str) -> bool {
    filter.split(',').any(|part| {
        let end = part.split_once('-').map(|(_, end)| end).unwrap_or(part);
        end.parse::<u16>().is_ok_and(|port| port > 1023)
    })
}

/// Предел суммарного числа портов у high-port профиля без ipset. Совпадает с
/// порогом в `dpi_engine::manifest` и в service-компиляторе.
const MAX_SCOPED_HIGH_PORTS: u32 = 256;

const RECOGNIZED_UDP_L7: &[&str] = &["discord", "stun"];
const RECOGNIZED_UDP_PAYLOAD: &[&str] = &["discord_ip_discovery", "stun"];

fn filter_port_count(filter: &str) -> Option<u32> {
    filter
        .split(',')
        .map(|part| {
            let (start, end) = match part.split_once('-') {
                Some((start, end)) => (start.parse::<u16>().ok()?, end.parse::<u16>().ok()?),
                None => {
                    let port = part.parse::<u16>().ok()?;
                    (port, port)
                }
            };
            (start > 0 && start <= end).then(|| u32::from(end - start) + 1)
        })
        .sum()
}

fn bounded_high_ports(filter: &str) -> bool {
    filter_port_count(filter).is_some_and(|count| count <= MAX_SCOPED_HIGH_PORTS)
}

fn recognized_udp_scope(filter_l7: &[String], payload: &[String]) -> bool {
    !filter_l7.is_empty()
        && filter_l7
            .iter()
            .all(|value| RECOGNIZED_UDP_L7.contains(&value.as_str()))
        && payload
            .iter()
            .all(|value| RECOGNIZED_UDP_PAYLOAD.contains(&value.as_str()))
}

pub fn validate_profile_scope(profile: &Zapret2Profile) -> Result<(), String> {
    // Сужение high-port захвата: TCP — хостлистом, UDP — распознаванием
    // протокола движком. Плюс ограничение размера захвата в обоих случаях.
    if profile.ipset.is_none() {
        if profile
            .filter_tcp
            .as_deref()
            .is_some_and(|value| filter_has_high_port(value) && !bounded_high_ports(value))
        {
            return Err(format!(
                "Zapret2 profile {} использует широкий high-port TCP filter без ipset",
                profile.name
            ));
        }
        if let Some(udp) = profile
            .filter_udp
            .as_deref()
            .filter(|value| filter_has_high_port(value))
        {
            if !bounded_high_ports(udp)
                || !recognized_udp_scope(&profile.filter_l7, &profile.payload)
            {
                return Err(format!(
                    "Zapret2 profile {} использует high-port UDP filter без ipset и без распознаваемого L7",
                    profile.name
                ));
            }
        }
    }
    Ok(())
}

/// Protocol-scoped discovery packets have no SNI/Host; do not bind a default hostlist.
pub fn is_voice_profile(strategy: &StrategyDef) -> bool {
    let has = |transport: &str| strategy.transports.iter().any(|value| value == transport);
    let tcp = strategy.transports.is_empty() || has("tcp") || has("tls") || has("http");
    !tcp && (has("udp") || has("quic"))
        && recognized_udp_scope(&strategy.filter_l7, &strategy.payload)
}

pub fn capture_ports(profiles: &[Zapret2Profile], tcp: bool) -> Option<String> {
    let mut ports = Vec::<String>::new();
    for profile in profiles {
        let filter = if tcp {
            profile.filter_tcp.as_deref()
        } else {
            profile.filter_udp.as_deref()
        };
        for part in filter.into_iter().flat_map(|value| value.split(',')) {
            if !ports.iter().any(|existing| existing == part) {
                ports.push(part.to_string());
            }
        }
    }
    (!ports.is_empty()).then(|| ports.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str) -> Zapret2Profile {
        Zapret2Profile {
            name: name.into(),
            filter_tcp: Some("443".into()),
            filter_udp: None,
            filter_l7: vec!["tls".into()],
            hostlist: Some(format!("lists/{name}.txt")),
            ipset: None,
            payload: vec!["tls_client_hello".into()],
            out_range: Some("-d10".into()),
            in_range: None,
            lua_desync: vec!["multisplit:pos=1,midsld".into()],
        }
    }

    fn invocation(profiles: Vec<Zapret2Profile>) -> Zapret2Invocation {
        Zapret2Invocation {
            wf_tcp_out: Some("443".into()),
            wf_udp_out: None,
            lua_init: vec![
                "C:/pack/lua/zapret-lib.lua".into(),
                "C:/pack/lua/zapret-antidpi.lua".into(),
            ],
            blobs: vec![],
            profiles,
        }
    }

    #[test]
    fn two_profiles_use_one_separator_without_empty_first_profile() {
        let args = build_winws2_args(&invocation(vec![profile("discord"), profile("youtube")]));
        assert_eq!(args.iter().filter(|a| a.as_str() == "--new").count(), 1);
        let first_name = args.iter().position(|a| a == "--name=discord").unwrap();
        let separator = args.iter().position(|a| a == "--new").unwrap();
        let second_name = args.iter().position(|a| a == "--name=youtube").unwrap();
        assert!(first_name < separator && separator < second_name);
        assert_ne!(args.last().map(String::as_str), Some("--new"));
    }

    #[test]
    fn global_arguments_precede_first_profile() {
        let mut inv = invocation(vec![profile("discord")]);
        inv.wf_udp_out = Some("443".into());
        inv.blobs.push(BlobArg {
            name: "quic_google".into(),
            path: "C:/pack/blobs/quic.bin".into(),
        });
        let args = build_winws2_args(&inv);
        let name = args.iter().position(|a| a == "--name=discord").unwrap();
        assert!(args[..name].iter().any(|a| a == "--wf-tcp-out=443"));
        assert!(args[..name].iter().any(|a| a == "--wf-udp-out=443"));
        assert!(args[..name]
            .iter()
            .any(|a| a == "--blob=quic_google:@C:/pack/blobs/quic.bin"));
        assert!(!args[..name].iter().any(|a| a == "--new"));
    }

    #[test]
    fn profile_filters_and_desync_keep_order() {
        let args = build_winws2_args(&invocation(vec![profile("discord")]));
        assert!(args.contains(&"--filter-l7=tls".to_string()));
        assert!(args.contains(&"--payload=tls_client_hello".to_string()));
        assert!(args.contains(&"--out-range=-d10".to_string()));
        assert!(args.contains(&"--lua-desync=multisplit:pos=1,midsld".to_string()));
    }

    #[test]
    fn rejects_unscoped_high_ports_and_merges_capture_ranges() {
        let mut control = profile("control");
        control.filter_udp = Some("443".into());
        let mut data = profile("data");
        data.filter_tcp = None;
        data.filter_udp = Some("443,1024-65535".into());
        data.ipset = Some("C:/lists/ipset-gaming.txt".into());

        assert!(validate_profile_scope(&control).is_ok());
        assert!(validate_profile_scope(&data).is_ok());
        assert_eq!(
            capture_ports(&[control.clone(), data.clone()], false).as_deref(),
            Some("443,1024-65535")
        );

        data.ipset = None;
        assert!(validate_profile_scope(&data).is_err());
    }

    #[test]
    fn empty_invocation_has_no_profile_separator() {
        let args = build_winws2_args(&Zapret2Invocation::default());
        assert!(args.is_empty());
    }

    #[test]
    fn profile_from_strategy_maps_tls_and_quic_fields() {
        let strategy = StrategyDef {
            id: "youtube_quic".into(),
            category: "youtube_twitch".into(),
            aggressiveness: 1,
            lua: "lua/zapret-antidpi.lua".into(),
            desync: vec!["fake:blob=quic_google:repeats=11".into()],
            transports: vec!["udp".into(), "quic".into()],
            hostlist: None,
            ipset: None,
            filter_tcp: None,
            filter_udp: None,
            filter_l7: vec!["quic".into()],
            payload: vec!["quic_initial".into()],
            out_range: None,
            in_range: None,
        };
        let profile = profile_from_strategy_def(
            &strategy,
            Some("lists/youtube.txt".into()),
            Some("lists/ipset-youtube.txt".into()),
        );
        assert_eq!(profile.filter_tcp, None);
        assert_eq!(profile.filter_udp.as_deref(), Some("443"));
        assert_eq!(profile.filter_l7, vec!["quic"]);
        assert_eq!(profile.payload, vec!["quic_initial"]);
        assert_eq!(profile.ipset.as_deref(), Some("lists/ipset-youtube.txt"));
    }

    #[test]
    fn explicit_filters_and_ipset_are_serialized() {
        let mut value = profile("gaming");
        value.filter_tcp = Some("80,443".into());
        value.filter_udp = Some("443,1024-65535".into());
        value.ipset = Some("C:/lists/ipset-gaming.txt".into());
        let args = build_winws2_args(&invocation(vec![value]));
        let hostlist = args
            .iter()
            .position(|arg| arg.starts_with("--hostlist="))
            .unwrap();
        let ipset = args
            .iter()
            .position(|arg| arg.starts_with("--ipset="))
            .unwrap();
        assert!(hostlist < ipset);
        assert!(args.contains(&"--filter-tcp=80,443".to_string()));
        assert!(args.contains(&"--filter-udp=443,1024-65535".to_string()));
    }
}
