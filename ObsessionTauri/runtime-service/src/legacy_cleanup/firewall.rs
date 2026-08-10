//! Native removal of historically owned Obsession proxy firewall rules.
//!
//! The rule name is only a candidate selector. Every security-relevant field
//! is read through the Firewall COM API and must match a historical Obsession
//! shape before deletion by exact name. A lookalike fails the whole cleanup.

use windows::core::{Interface, BSTR, VARIANT};
use windows::Win32::Foundation::{S_FALSE, S_OK};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule, INetFwRules, NetFwPolicy2, NET_FW_ACTION_ALLOW,
    NET_FW_IP_PROTOCOL_TCP, NET_FW_PROFILE2_PRIVATE, NET_FW_RULE_DIR_IN,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Ole::IEnumVARIANT;

use super::LegacyCleanupError;

const LEGACY_EXACT_NAME: &str = "Obsession TgWsProxy";
const GENERATION_PREFIX: &str = "Obsession TgWsProxy ";
const GENERATION_GROUP: &str = "Obsession";
const MAX_FIREWALL_RULES: usize = 1_024;
const MAX_OWNED_RULES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
enum OwnedRuleKind {
    LegacyExact,
    Generation { port: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FirewallRuleProperties {
    name: String,
    direction_inbound: bool,
    action_allow: bool,
    protocol_tcp: bool,
    local_ports: String,
    grouping: String,
    profiles: i32,
    application_name: String,
    service_name: String,
    remote_ports: String,
}

pub(super) fn remove_owned_legacy_rules() -> Result<(), LegacyCleanupError> {
    let _com = ComApartment::initialize()?;
    let policy: INetFwPolicy2 =
        unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }.map_err(
            |source| LegacyCleanupError::Windows {
                operation: "firewall policy creation",
                source,
            },
        )?;
    let rules = unsafe { policy.Rules() }.map_err(|source| LegacyCleanupError::Windows {
        operation: "firewall rules lookup",
        source,
    })?;

    let mut removals = 0usize;
    loop {
        let mut owned_names = collect_owned_rule_names(&rules)?;
        if owned_names.is_empty() {
            return Ok(());
        }
        owned_names.sort_unstable();
        owned_names.dedup();
        for name in owned_names {
            removals += 1;
            if removals > MAX_OWNED_RULES {
                return Err(LegacyCleanupError::FirewallRuleBoundExceeded);
            }
            let name = BSTR::from(name.as_str());
            unsafe { rules.Remove(&name) }.map_err(|source| LegacyCleanupError::Windows {
                operation: "legacy firewall rule removal",
                source,
            })?;
        }
    }
}

fn collect_owned_rule_names(rules: &INetFwRules) -> Result<Vec<String>, LegacyCleanupError> {
    let count = unsafe { rules.Count() }.map_err(|source| LegacyCleanupError::Windows {
        operation: "firewall rule count",
        source,
    })?;
    if count < 0 || count as usize > MAX_FIREWALL_RULES {
        return Err(LegacyCleanupError::FirewallRuleBoundExceeded);
    }
    let unknown = unsafe { rules._NewEnum() }.map_err(|source| LegacyCleanupError::Windows {
        operation: "firewall rule enumerator lookup",
        source,
    })?;
    let enumerator: IEnumVARIANT =
        unknown
            .cast()
            .map_err(|source| LegacyCleanupError::Windows {
                operation: "firewall rule enumerator cast",
                source,
            })?;
    let mut names = Vec::new();
    let mut scanned = 0usize;
    loop {
        let mut values = [VARIANT::default()];
        let mut fetched = 0u32;
        let status = unsafe { enumerator.Next(&mut values, &mut fetched) };
        if status == S_FALSE && fetched == 0 {
            break;
        }
        if status != S_OK || fetched != 1 {
            return Err(LegacyCleanupError::Windows {
                operation: "firewall rule enumeration",
                source: windows::core::Error::from_hresult(status),
            });
        }
        scanned += 1;
        if scanned > MAX_FIREWALL_RULES {
            return Err(LegacyCleanupError::FirewallRuleBoundExceeded);
        }
        let dispatch =
            IDispatch::try_from(&values[0]).map_err(|source| LegacyCleanupError::Windows {
                operation: "firewall rule dispatch conversion",
                source,
            })?;
        let rule: INetFwRule = dispatch
            .cast()
            .map_err(|source| LegacyCleanupError::Windows {
                operation: "firewall rule interface cast",
                source,
            })?;
        let name = unsafe { rule.Name() }
            .map_err(|source| LegacyCleanupError::Windows {
                operation: "firewall rule name",
                source,
            })?
            .to_string();
        let Some(kind) = parse_owned_rule_name(&name) else {
            continue;
        };
        let properties = read_properties(&rule, name)?;
        validate_owned_rule(&properties, &kind)?;
        if names.len() >= MAX_OWNED_RULES {
            return Err(LegacyCleanupError::FirewallRuleBoundExceeded);
        }
        names.push(properties.name);
    }
    Ok(names)
}

fn read_properties(
    rule: &INetFwRule,
    name: String,
) -> Result<FirewallRuleProperties, LegacyCleanupError> {
    let windows = |operation: &'static str, error| LegacyCleanupError::Windows {
        operation,
        source: error,
    };
    Ok(FirewallRuleProperties {
        name,
        direction_inbound: unsafe { rule.Direction() }
            .map_err(|error| windows("firewall rule direction", error))?
            == NET_FW_RULE_DIR_IN,
        action_allow: unsafe { rule.Action() }
            .map_err(|error| windows("firewall rule action", error))?
            == NET_FW_ACTION_ALLOW,
        protocol_tcp: unsafe { rule.Protocol() }
            .map_err(|error| windows("firewall rule protocol", error))?
            == NET_FW_IP_PROTOCOL_TCP.0,
        local_ports: unsafe { rule.LocalPorts() }
            .map_err(|error| windows("firewall rule local ports", error))?
            .to_string(),
        grouping: unsafe { rule.Grouping() }
            .map_err(|error| windows("firewall rule grouping", error))?
            .to_string(),
        profiles: unsafe { rule.Profiles() }
            .map_err(|error| windows("firewall rule profiles", error))?,
        application_name: unsafe { rule.ApplicationName() }
            .map_err(|error| windows("firewall rule application", error))?
            .to_string(),
        service_name: unsafe { rule.ServiceName() }
            .map_err(|error| windows("firewall rule service", error))?
            .to_string(),
        remote_ports: unsafe { rule.RemotePorts() }
            .map_err(|error| windows("firewall rule remote ports", error))?
            .to_string(),
    })
}

fn parse_owned_rule_name(name: &str) -> Option<OwnedRuleKind> {
    if name == LEGACY_EXACT_NAME {
        return Some(OwnedRuleKind::LegacyExact);
    }
    let remainder = name.strip_prefix(GENERATION_PREFIX)?;
    let (port_text, generation_text) = remainder.split_once(" gen")?;
    if port_text.is_empty()
        || generation_text.is_empty()
        || port_text.starts_with('0')
        || generation_text.starts_with('0')
        || !port_text.bytes().all(|byte| byte.is_ascii_digit())
        || !generation_text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let port = port_text.parse::<u16>().ok().filter(|port| *port != 0)?;
    generation_text
        .parse::<u64>()
        .ok()
        .filter(|value| *value != 0)?;
    Some(OwnedRuleKind::Generation { port })
}

fn validate_owned_rule(
    rule: &FirewallRuleProperties,
    kind: &OwnedRuleKind,
) -> Result<(), LegacyCleanupError> {
    let port = match kind {
        OwnedRuleKind::LegacyExact => rule.local_ports.parse::<u16>().ok().filter(|port| {
            *port != 0
                && rule.local_ports == port.to_string()
                && rule.grouping.is_empty()
                && rule.application_name.is_empty()
                && rule.service_name.is_empty()
                && matches!(rule.remote_ports.as_str(), "" | "*")
        }),
        OwnedRuleKind::Generation { port } => Some(*port).filter(|expected| {
            rule.local_ports == expected.to_string()
                && rule.grouping == GENERATION_GROUP
                && rule.profiles == NET_FW_PROFILE2_PRIVATE.0
                && rule.application_name.is_empty()
                && rule.service_name.is_empty()
                && matches!(rule.remote_ports.as_str(), "" | "*")
        }),
    };
    if rule.direction_inbound && rule.action_allow && rule.protocol_tcp && port.is_some() {
        Ok(())
    } else {
        Err(LegacyCleanupError::FirewallRuleInvalid(rule.name.clone()))
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, LegacyCleanupError> {
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if status == S_OK || status == S_FALSE {
            Ok(Self)
        } else {
            Err(LegacyCleanupError::Windows {
                operation: "COM initialization",
                source: windows::core::Error::from_hresult(status),
            })
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation_rule(name: &str, local_ports: &str) -> FirewallRuleProperties {
        FirewallRuleProperties {
            name: name.into(),
            direction_inbound: true,
            action_allow: true,
            protocol_tcp: true,
            local_ports: local_ports.into(),
            grouping: GENERATION_GROUP.into(),
            profiles: NET_FW_PROFILE2_PRIVATE.0,
            application_name: String::new(),
            service_name: String::new(),
            remote_ports: String::new(),
        }
    }

    #[test]
    fn names_are_parsed_as_exact_historical_grammars() {
        assert_eq!(
            parse_owned_rule_name(LEGACY_EXACT_NAME),
            Some(OwnedRuleKind::LegacyExact)
        );
        assert_eq!(
            parse_owned_rule_name("Obsession TgWsProxy 1080 gen7"),
            Some(OwnedRuleKind::Generation { port: 1080 })
        );
        for name in [
            "Obsession TgWsProxy 0 gen7",
            "Obsession TgWsProxy 01080 gen7",
            "Obsession TgWsProxy 1080 gen0",
            "Obsession TgWsProxy 1080 gen07",
            "Obsession TgWsProxy 65536 gen7",
            "Obsession TgWsProxy 1080 gen7 extra",
            "Obsession TgWsProxy Evil",
        ] {
            assert_eq!(parse_owned_rule_name(name), None, "must reject {name}");
        }
    }

    #[test]
    fn generation_rule_requires_port_group_profile_and_empty_program_scope() {
        let name = "Obsession TgWsProxy 1080 gen7";
        let kind = parse_owned_rule_name(name).unwrap();
        let valid = generation_rule(name, "1080");
        assert!(validate_owned_rule(&valid, &kind).is_ok());

        let mut invalid = valid.clone();
        invalid.local_ports = "1081".into();
        assert!(validate_owned_rule(&invalid, &kind).is_err());
        let mut invalid = valid.clone();
        invalid.grouping.clear();
        assert!(validate_owned_rule(&invalid, &kind).is_err());
        let mut invalid = valid.clone();
        invalid.profiles = i32::MAX;
        assert!(validate_owned_rule(&invalid, &kind).is_err());
        let mut invalid = valid.clone();
        invalid.application_name = r"C:\malware.exe".into();
        assert!(validate_owned_rule(&invalid, &kind).is_err());
    }

    #[test]
    fn legacy_exact_rule_still_requires_tcp_inbound_allow_and_one_port() {
        let mut rule = generation_rule(LEGACY_EXACT_NAME, "1080");
        rule.grouping.clear();
        rule.profiles = i32::MAX;
        assert!(validate_owned_rule(&rule, &OwnedRuleKind::LegacyExact).is_ok());

        rule.direction_inbound = false;
        assert!(validate_owned_rule(&rule, &OwnedRuleKind::LegacyExact).is_err());
        rule.direction_inbound = true;
        rule.local_ports = "1080,1081".into();
        assert!(validate_owned_rule(&rule, &OwnedRuleKind::LegacyExact).is_err());
    }
}
