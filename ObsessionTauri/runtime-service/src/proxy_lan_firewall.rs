//! Service-owned, bounded Windows Firewall lease for Telegram LAN publication.
//!
//! The authenticated client contributes only a typed TCP port and a bounded
//! lifetime. Rule identity and every security-relevant property are fixed by
//! the LocalSystem service. A same-name lookalike is never overwritten.

#![cfg(windows)]

use std::fs;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use obsession_runtime_protocol::{
    FirewallOpenProxyLanRequest, OperationAccepted, ProxyLanLease, MAX_FIREWALL_LEASE_SECONDS,
    MIN_FIREWALL_LEASE_SECONDS,
};
use windows::core::{BSTR, HRESULT};
use windows::Win32::Foundation::{S_FALSE, S_OK, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule, INetFwRules, NetFwPolicy2, NetFwRule, NET_FW_ACTION_ALLOW,
    NET_FW_IP_PROTOCOL_TCP, NET_FW_PROFILE2_PRIVATE, NET_FW_RULE_DIR_IN,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::dpi_materializer::{ProtectedDataLayout, RUNTIME_STATE_RELATIVE};
use crate::BackendError;

const RULE_NAME: &str = "Obsession Protected Proxy LAN";
const RULE_GROUP: &str = "Obsession Protected Runtime";
const RULE_DESCRIPTION: &str = "Temporary LocalSubnet access leased by Obsession Runtime";
const LOCAL_SUBNET: &str = "LocalSubnet";
const ALL_ADDRESSES: &str = "*";
const MAX_FIREWALL_RULES: i32 = 4_096;
const ERROR_FILE_NOT_FOUND_HRESULT: HRESULT = HRESULT::from_win32(2);

#[derive(Clone, Debug, PartialEq, Eq)]
struct FirewallRuleProperties {
    name: String,
    description: String,
    direction_inbound: bool,
    action_allow: bool,
    protocol_tcp: bool,
    local_port: Option<u16>,
    remote_ports: String,
    local_addresses: String,
    remote_addresses: String,
    interface_types: String,
    enabled: bool,
    private_profile_only: bool,
    edge_traversal: bool,
    grouping: String,
    application: String,
    service: String,
}

#[derive(Clone, Debug)]
struct ActiveLease {
    port: u16,
    deadline: Instant,
    expires_at_unix: u64,
}

trait FirewallApi: Send {
    fn preflight(&self) -> Result<(), BackendError>;
    fn replace(&self, port: u16) -> Result<(), BackendError>;
    fn verify(&self, port: u16) -> Result<(), BackendError>;
    fn remove(&self) -> Result<(), BackendError>;
}

struct NativeFirewallApi;

impl FirewallApi for NativeFirewallApi {
    fn preflight(&self) -> Result<(), BackendError> {
        with_rules(|rules| {
            if let Some(rule) = find_rule(rules)? {
                validate_rule(&rule, None)?;
                remove_rule(rules)?;
            }
            Ok(())
        })
    }

    fn replace(&self, port: u16) -> Result<(), BackendError> {
        with_rules(|rules| {
            if let Some(rule) = find_rule(rules)? {
                validate_rule(&rule, None)?;
                remove_rule(rules)?;
            }
            add_rule(rules, port)?;
            let result = find_rule(rules)?
                .ok_or(BackendError::RuntimeFailed)
                .and_then(|rule| validate_rule(&rule, Some(port)));
            if result.is_err() {
                let _ = find_rule(rules).and_then(|rule| match rule {
                    Some(rule) => {
                        validate_rule(&rule, Some(port))?;
                        remove_rule(rules)
                    }
                    None => Ok(()),
                });
            }
            result
        })
    }

    fn verify(&self, port: u16) -> Result<(), BackendError> {
        with_rules(|rules| {
            let rule = find_rule(rules)?.ok_or(BackendError::Conflict)?;
            validate_rule(&rule, Some(port))
        })
    }

    fn remove(&self) -> Result<(), BackendError> {
        with_rules(|rules| {
            let Some(rule) = find_rule(rules)? else {
                return Ok(());
            };
            validate_rule(&rule, None)?;
            remove_rule(rules)
        })
    }
}

pub(crate) struct ProxyLanFirewall {
    api: Box<dyn FirewallApi>,
    lease: Option<ActiveLease>,
    next_operation_id: u64,
}

impl ProxyLanFirewall {
    pub(crate) fn discover(layout: &ProtectedDataLayout) -> Result<Self, BackendError> {
        require_production_state_root(layout)?;
        let controller = Self::from_api(Box::new(NativeFirewallApi));
        controller.api.preflight()?;
        Ok(controller)
    }

    fn from_api(api: Box<dyn FirewallApi>) -> Self {
        Self {
            api,
            lease: None,
            next_operation_id: 0,
        }
    }

    pub(crate) fn open(
        &mut self,
        request: FirewallOpenProxyLanRequest,
    ) -> Result<OperationAccepted, BackendError> {
        self.open_at(request, Instant::now(), now_unix())
    }

    fn open_at(
        &mut self,
        request: FirewallOpenProxyLanRequest,
        now: Instant,
        unix_now: u64,
    ) -> Result<OperationAccepted, BackendError> {
        if request.port == 0
            || !(MIN_FIREWALL_LEASE_SECONDS..=MAX_FIREWALL_LEASE_SECONDS)
                .contains(&request.lease_seconds)
        {
            return Err(BackendError::InvalidRequest);
        }
        let duration = Duration::from_secs(u64::from(request.lease_seconds));
        let deadline = now
            .checked_add(duration)
            .ok_or(BackendError::InvalidRequest)?;
        let expires_at_unix = unix_now
            .checked_add(duration.as_secs())
            .ok_or(BackendError::InvalidRequest)?;

        if self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.port == request.port && lease.deadline > now)
        {
            self.api.verify(request.port)?;
        } else {
            self.api.replace(request.port)?;
        }

        self.lease = Some(ActiveLease {
            port: request.port,
            deadline,
            expires_at_unix,
        });
        Ok(OperationAccepted {
            operation_id: self.advance_operation_id(),
        })
    }

    pub(crate) fn close(&mut self) -> Result<(), BackendError> {
        let result = self.api.remove();
        self.lease = None;
        result
    }

    pub(crate) fn snapshot(&self) -> Option<ProxyLanLease> {
        self.snapshot_at(Instant::now())
    }

    fn snapshot_at(&self, now: Instant) -> Option<ProxyLanLease> {
        self.lease.as_ref().and_then(|lease| {
            (lease.deadline > now).then_some(ProxyLanLease {
                port: lease.port,
                expires_at_unix: lease.expires_at_unix,
            })
        })
    }

    pub(crate) fn poll_expired(&mut self) -> Result<(), BackendError> {
        self.poll_expired_at(Instant::now())
    }

    fn poll_expired_at(&mut self, now: Instant) -> Result<(), BackendError> {
        if !self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.deadline <= now)
        {
            return Ok(());
        }
        self.api.remove()?;
        self.lease = None;
        Ok(())
    }

    fn advance_operation_id(&mut self) -> u64 {
        self.next_operation_id = self.next_operation_id.wrapping_add(1);
        if self.next_operation_id == 0 {
            self.next_operation_id = 1;
        }
        self.next_operation_id
    }
}

impl Drop for ProxyLanFirewall {
    fn drop(&mut self) {
        let _ = self.api.remove();
    }
}

fn require_production_state_root(layout: &ProtectedDataLayout) -> Result<(), BackendError> {
    let program_data = std::env::var_os("ProgramData").ok_or(BackendError::ServiceUnavailable)?;
    let canonical_program_data =
        fs::canonicalize(program_data).map_err(|_| BackendError::ServiceUnavailable)?;
    let canonical_layout =
        fs::canonicalize(layout.root()).map_err(|_| BackendError::ServiceUnavailable)?;
    let expected = canonical_program_data.join(RUNTIME_STATE_RELATIVE);
    if path_key(&canonical_layout) != path_key(&expected) {
        return Err(BackendError::ServiceUnavailable);
    }
    Ok(())
}

fn path_key(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn with_rules<T>(
    operation: impl FnOnce(&INetFwRules) -> Result<T, BackendError>,
) -> Result<T, BackendError> {
    let _com = ComApartment::initialize()?;
    let policy: INetFwPolicy2 =
        unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| BackendError::ServiceUnavailable)?;
    let rules = unsafe { policy.Rules() }.map_err(|_| BackendError::ServiceUnavailable)?;
    let count = unsafe { rules.Count() }.map_err(|_| BackendError::ServiceUnavailable)?;
    if !(0..=MAX_FIREWALL_RULES).contains(&count) {
        return Err(BackendError::ServiceUnavailable);
    }
    operation(&rules)
}

fn find_rule(rules: &INetFwRules) -> Result<Option<INetFwRule>, BackendError> {
    let name = BSTR::from(RULE_NAME);
    match unsafe { rules.Item(&name) } {
        Ok(rule) => Ok(Some(rule)),
        Err(error) if error.code() == ERROR_FILE_NOT_FOUND_HRESULT => Ok(None),
        Err(_) => Err(BackendError::ServiceUnavailable),
    }
}

fn add_rule(rules: &INetFwRules, port: u16) -> Result<(), BackendError> {
    let rule: INetFwRule = unsafe { CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER) }
        .map_err(|_| BackendError::RuntimeFailed)?;
    let port = port.to_string();
    unsafe {
        rule.SetName(&BSTR::from(RULE_NAME))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetDescription(&BSTR::from(RULE_DESCRIPTION))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetProtocol(NET_FW_IP_PROTOCOL_TCP.0)
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetLocalPorts(&BSTR::from(port.as_str()))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetRemotePorts(&BSTR::from(ALL_ADDRESSES))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetLocalAddresses(&BSTR::from(ALL_ADDRESSES))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetRemoteAddresses(&BSTR::from(LOCAL_SUBNET))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetDirection(NET_FW_RULE_DIR_IN)
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetInterfaceTypes(&BSTR::from("All"))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetEnabled(VARIANT_TRUE)
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetGrouping(&BSTR::from(RULE_GROUP))
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetProfiles(NET_FW_PROFILE2_PRIVATE.0)
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetEdgeTraversal(VARIANT_FALSE)
            .map_err(|_| BackendError::RuntimeFailed)?;
        rule.SetAction(NET_FW_ACTION_ALLOW)
            .map_err(|_| BackendError::RuntimeFailed)?;
        rules.Add(&rule).map_err(|_| BackendError::RuntimeFailed)?;
    }
    Ok(())
}

fn remove_rule(rules: &INetFwRules) -> Result<(), BackendError> {
    let name = BSTR::from(RULE_NAME);
    unsafe { rules.Remove(&name) }.map_err(|_| BackendError::RuntimeFailed)
}

fn validate_rule(rule: &INetFwRule, expected_port: Option<u16>) -> Result<(), BackendError> {
    let local_ports = unsafe { rule.LocalPorts() }
        .map_err(|_| BackendError::Conflict)?
        .to_string();
    let local_port = local_ports
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0 && local_ports == port.to_string());
    let properties = FirewallRuleProperties {
        name: unsafe { rule.Name() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        description: unsafe { rule.Description() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        direction_inbound: unsafe { rule.Direction() }.map_err(|_| BackendError::Conflict)?
            == NET_FW_RULE_DIR_IN,
        action_allow: unsafe { rule.Action() }.map_err(|_| BackendError::Conflict)?
            == NET_FW_ACTION_ALLOW,
        protocol_tcp: unsafe { rule.Protocol() }.map_err(|_| BackendError::Conflict)?
            == NET_FW_IP_PROTOCOL_TCP.0,
        local_port,
        remote_ports: unsafe { rule.RemotePorts() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        local_addresses: unsafe { rule.LocalAddresses() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        remote_addresses: unsafe { rule.RemoteAddresses() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        interface_types: unsafe { rule.InterfaceTypes() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        enabled: unsafe { rule.Enabled() }.map_err(|_| BackendError::Conflict)? == VARIANT_TRUE,
        private_profile_only: unsafe { rule.Profiles() }.map_err(|_| BackendError::Conflict)?
            == NET_FW_PROFILE2_PRIVATE.0,
        edge_traversal: unsafe { rule.EdgeTraversal() }.map_err(|_| BackendError::Conflict)?
            != VARIANT_FALSE,
        grouping: unsafe { rule.Grouping() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        application: unsafe { rule.ApplicationName() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
        service: unsafe { rule.ServiceName() }
            .map_err(|_| BackendError::Conflict)?
            .to_string(),
    };
    validate_rule_properties(&properties, expected_port)
}

fn validate_rule_properties(
    rule: &FirewallRuleProperties,
    expected_port: Option<u16>,
) -> Result<(), BackendError> {
    if rule.name != RULE_NAME
        || rule.description != RULE_DESCRIPTION
        || !rule.direction_inbound
        || !rule.action_allow
        || !rule.protocol_tcp
        || !rule.private_profile_only
        || !rule.enabled
        || rule.edge_traversal
        || !rule.remote_addresses.eq_ignore_ascii_case(LOCAL_SUBNET)
        || rule.remote_ports != ALL_ADDRESSES
        || rule.local_addresses != ALL_ADDRESSES
        || !rule.interface_types.eq_ignore_ascii_case("All")
        || rule.grouping != RULE_GROUP
        || !rule.application.is_empty()
        || !rule.service.is_empty()
        || rule.local_port.is_none()
        || expected_port.is_some_and(|expected| rule.local_port != Some(expected))
    {
        return Err(BackendError::Conflict);
    }
    Ok(())
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, BackendError> {
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if status == S_OK || status == S_FALSE {
            Ok(Self)
        } else {
            Err(BackendError::ServiceUnavailable)
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
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Preflight,
        Replace(u16),
        Verify(u16),
        Remove,
    }

    struct FakeApi(Arc<Mutex<Vec<Call>>>);

    impl FirewallApi for FakeApi {
        fn preflight(&self) -> Result<(), BackendError> {
            self.0.lock().unwrap().push(Call::Preflight);
            Ok(())
        }

        fn replace(&self, port: u16) -> Result<(), BackendError> {
            self.0.lock().unwrap().push(Call::Replace(port));
            Ok(())
        }

        fn verify(&self, port: u16) -> Result<(), BackendError> {
            self.0.lock().unwrap().push(Call::Verify(port));
            Ok(())
        }

        fn remove(&self) -> Result<(), BackendError> {
            self.0.lock().unwrap().push(Call::Remove);
            Ok(())
        }
    }

    fn controller() -> (ProxyLanFirewall, Arc<Mutex<Vec<Call>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            ProxyLanFirewall::from_api(Box::new(FakeApi(calls.clone()))),
            calls,
        )
    }

    fn owned_rule(port: u16) -> FirewallRuleProperties {
        FirewallRuleProperties {
            name: RULE_NAME.into(),
            description: RULE_DESCRIPTION.into(),
            direction_inbound: true,
            action_allow: true,
            protocol_tcp: true,
            local_port: Some(port),
            remote_ports: ALL_ADDRESSES.into(),
            local_addresses: ALL_ADDRESSES.into(),
            remote_addresses: LOCAL_SUBNET.into(),
            interface_types: "All".into(),
            enabled: true,
            private_profile_only: true,
            edge_traversal: false,
            grouping: RULE_GROUP.into(),
            application: String::new(),
            service: String::new(),
        }
    }

    #[test]
    fn owned_rule_requires_private_localsubnet_and_exact_shape() {
        let valid = owned_rule(1443);
        assert!(validate_rule_properties(&valid, Some(1443)).is_ok());

        let mut public = valid.clone();
        public.private_profile_only = false;
        assert_eq!(
            validate_rule_properties(&public, Some(1443)),
            Err(BackendError::Conflict)
        );
        let mut internet = valid.clone();
        internet.remote_addresses = "*".into();
        assert_eq!(
            validate_rule_properties(&internet, Some(1443)),
            Err(BackendError::Conflict)
        );
        let mut wrong_port = valid;
        wrong_port.local_port = Some(1444);
        assert_eq!(
            validate_rule_properties(&wrong_port, Some(1443)),
            Err(BackendError::Conflict)
        );
    }

    #[test]
    fn open_renews_same_port_and_expiry_removes_the_rule() {
        let (mut controller, calls) = controller();
        let now = Instant::now();
        let request = FirewallOpenProxyLanRequest {
            port: 1443,
            lease_seconds: MIN_FIREWALL_LEASE_SECONDS,
        };
        assert_eq!(
            controller.open_at(request.clone(), now, 1_000).unwrap(),
            OperationAccepted { operation_id: 1 }
        );
        assert_eq!(controller.snapshot_at(now).unwrap().expires_at_unix, 1_030);
        assert_eq!(
            controller
                .open_at(request, now + Duration::from_secs(10), 1_010)
                .unwrap(),
            OperationAccepted { operation_id: 2 }
        );
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            [Call::Replace(1443), Call::Verify(1443)]
        );

        controller
            .poll_expired_at(now + Duration::from_secs(41))
            .unwrap();
        assert!(controller
            .snapshot_at(now + Duration::from_secs(41))
            .is_none());
        assert_eq!(calls.lock().unwrap().last(), Some(&Call::Remove));
    }

    #[test]
    fn invalid_typed_request_never_mutates_firewall() {
        let (mut controller, calls) = controller();
        assert_eq!(
            controller.open_at(
                FirewallOpenProxyLanRequest {
                    port: 0,
                    lease_seconds: MIN_FIREWALL_LEASE_SECONDS,
                },
                Instant::now(),
                1,
            ),
            Err(BackendError::InvalidRequest)
        );
        assert!(calls.lock().unwrap().is_empty());
    }
}
