//! Explicit undo of DNS changes made by a previous release. The production
//! service cannot install a DNS provider or flush DNS during adaptive search.
#![cfg(windows)]

use crate::{dpi_materializer::ProtectedDataLayout, BackendError};
use obsession_runtime_protocol::{SystemDnsProvider as Provider, SystemDnsStatus};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    net::IpAddr,
    path::PathBuf,
};
#[cfg(test)]
use std::{net::SocketAddr, time::Duration};
use windows::{
    core::{GUID, PWSTR},
    Win32::NetworkManagement::IpHelper::*,
};

const MAX_JOURNAL: u64 = 16 * 1024;
#[cfg(test)]
const MAX_DOH_BODY: u64 = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DohProperty {
    index: u32,
    template: Option<String>,
    flags: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DnsConfig {
    servers: Option<String>,
    properties: Vec<DohProperty>,
}

impl DnsConfig {
    fn validate(&self, ipv6: bool) -> Result<(), BackendError> {
        let servers = self.servers.as_deref().unwrap_or("");
        if servers.len() > 512 || self.properties.len() > 8 {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        let addresses = servers
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|s| !s.is_empty())
            .map(str::parse::<IpAddr>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        if addresses.len() > 8 || addresses.iter().any(|ip| ip.is_ipv6() != ipv6) {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        let mut indices = std::collections::HashSet::new();
        for property in &self.properties {
            if property.index as usize >= addresses.len()
                || !indices.insert(property.index)
                || property.flags & !31 != 0
                || property.template.as_ref().is_some_and(|s| {
                    s.len() > 2048 || !s.starts_with("https://") || s.chars().any(char::is_control)
                })
            {
                return Err(BackendError::ProtectedResourceInvalid);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    interface: u128,
    name: String,
    provider: Provider,
    original: [DnsConfig; 2],
    desired: [DnsConfig; 2],
    #[serde(default)]
    restoring: bool,
}

impl Journal {
    fn validate(&self) -> Result<(), BackendError> {
        if self.version != 1
            || self.interface == 0
            || self.name.is_empty()
            || self.name.len() > 1024
            || self.name.chars().any(char::is_control)
            || self.desired != provider_configs(self.provider)
        {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        for (index, config) in self.original.iter().enumerate() {
            config.validate(index == 1)?;
        }
        Ok(())
    }
}

trait DnsApi: Send {
    #[cfg(test)]
    fn select(&self) -> Result<(u128, String), BackendError>;
    fn read(&self, interface: u128, ipv6: bool) -> Result<DnsConfig, BackendError>;
    fn write(&self, interface: u128, ipv6: bool, config: &DnsConfig) -> Result<(), BackendError>;
    fn flush(&self) -> Result<(), BackendError>;
    #[cfg(test)]
    fn provider_works(&self, provider: Provider) -> bool;
}

pub(crate) struct DnsRecovery {
    layout: ProtectedDataLayout,
    api: Box<dyn DnsApi>,
}

impl DnsRecovery {
    pub(crate) fn discover(layout: &ProtectedDataLayout) -> Result<Self, BackendError> {
        layout
            .dns_recovery_journal()
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        // Dynamic imports keep older Windows versions loadable.
        let library = system_library("iphlpapi.dll")?;
        unsafe {
            library.get::<unsafe extern "system" fn(GUID, *const DNS_INTERFACE_SETTINGS) -> u32>(
                b"SetInterfaceDnsSettings\0",
            )
        }
        .map_err(|_| BackendError::ServiceUnavailable)?;
        Ok(Self {
            layout: layout.clone(),
            api: Box::new(NativeDnsApi),
        })
    }

    pub(crate) fn status(&self) -> Result<SystemDnsStatus, BackendError> {
        let Some(journal) = self.load()? else {
            return Ok(SystemDnsStatus::default());
        };
        let active = !journal.restoring
            && (0..2).all(|index| {
                self.api.read(journal.interface, index == 1).ok().as_ref()
                    == Some(&journal.desired[index])
            });
        Ok(SystemDnsStatus {
            active,
            restore_available: true,
            provider: Some(journal.provider),
            interface_name: Some(journal.name),
        })
    }

    // Test-only creation of legacy journals exercises explicit undo, including
    // interrupted writes. No repair entry point is compiled into the service.
    #[cfg(test)]
    pub(crate) fn repair(&mut self) -> Result<SystemDnsStatus, BackendError> {
        let (interface, name) = self.api.select()?;
        if let Some(journal) = self.load()? {
            if journal.interface != interface {
                return Err(BackendError::Conflict);
            }
            // An interrupted two-family write is undone before a new repair.
            if !journal.restoring
                && self.api.provider_works(journal.provider)
                && (0..2).all(|i| {
                    self.api.read(interface, i == 1).ok().as_ref() == Some(&journal.desired[i])
                })
            {
                self.api.flush()?;
                return self.status();
            }
            self.restore()?;
        }
        let original = [
            self.api.read(interface, false)?,
            self.api.read(interface, true)?,
        ];
        let provider = [Provider::Cloudflare, Provider::Google]
            .into_iter()
            .find(|provider| self.api.provider_works(*provider))
            .ok_or(BackendError::RuntimeFailed)?;
        // Provider checks may take time; fence both the selected adapter and its settings again.
        if self.api.select()?.0 != interface {
            return Err(BackendError::Conflict);
        }
        for i in 0..2 {
            if self.api.read(interface, i == 1)? != original[i] {
                return Err(BackendError::Conflict);
            }
        }
        let journal = Journal {
            version: 1,
            interface,
            name,
            provider,
            original,
            desired: provider_configs(provider),
            restoring: false,
        };
        self.save(&journal)?;
        let result = (|| {
            for i in 0..2 {
                if self.api.select()?.0 != interface {
                    return Err(BackendError::Conflict);
                }
                if self.api.read(interface, i == 1)? != journal.original[i] {
                    return Err(BackendError::Conflict);
                }
                self.api.write(interface, i == 1, &journal.desired[i])?;
                if self.api.read(interface, i == 1)? != journal.desired[i] {
                    return Err(BackendError::RuntimeFailed);
                }
            }
            self.api.flush()?;
            self.status()
        })();
        if result.is_err() {
            self.restore()?;
        }
        result
    }

    pub(crate) fn restore(&mut self) -> Result<SystemDnsStatus, BackendError> {
        let Some(mut journal) = self.load()? else {
            return Ok(SystemDnsStatus::default());
        };
        for i in 0..2 {
            let current = self.api.read(journal.interface, i == 1)?;
            if !owned_config(&current, &journal, i) {
                return Err(BackendError::Conflict);
            }
        }
        journal.restoring = true;
        self.save(&journal)?;
        for i in 0..2 {
            let current = self.api.read(journal.interface, i == 1)?;
            if !owned_config(&current, &journal, i) {
                return Err(BackendError::Conflict);
            }
            if current != journal.original[i] {
                self.api
                    .write(journal.interface, i == 1, &journal.original[i])?;
                if self.api.read(journal.interface, i == 1)? != journal.original[i] {
                    return Err(BackendError::RuntimeFailed);
                }
            }
        }
        self.api.flush()?;
        let path = self
            .layout
            .dns_recovery_journal()
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        fs::remove_file(path).map_err(|_| BackendError::RuntimeFailed)?;
        Ok(SystemDnsStatus::default())
    }

    fn load(&self) -> Result<Option<Journal>, BackendError> {
        let path = self
            .layout
            .dns_recovery_journal()
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(BackendError::ProtectedResourceInvalid),
        };
        let mut bytes = Vec::new();
        file.take(MAX_JOURNAL + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        if bytes.len() as u64 > MAX_JOURNAL {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        let journal: Journal =
            serde_json::from_slice(&bytes).map_err(|_| BackendError::ProtectedResourceInvalid)?;
        journal.validate()?;
        Ok(Some(journal))
    }

    fn save(&self, journal: &Journal) -> Result<(), BackendError> {
        journal.validate()?;
        let path = self
            .layout
            .dns_recovery_journal()
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        let temporary = path.with_file_name("recovery.new");
        let bytes = serde_json::to_vec(journal).map_err(|_| BackendError::Internal)?;
        if bytes.len() as u64 > MAX_JOURNAL {
            return Err(BackendError::ProtectedResourceInvalid);
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|_| BackendError::RuntimeFailed)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| BackendError::RuntimeFailed)?;
        drop(file);
        self.layout
            .dns_recovery_journal()
            .map_err(|_| BackendError::ProtectedResourceInvalid)?;
        fs::rename(temporary, path).map_err(|_| BackendError::RuntimeFailed)
    }
}

fn provider_configs(provider: Provider) -> [DnsConfig; 2] {
    let (v4, v6, template) = match provider {
        Provider::Cloudflare => (
            "1.1.1.1,1.0.0.1",
            "2606:4700:4700::1111,2606:4700:4700::1001",
            "https://cloudflare-dns.com/dns-query",
        ),
        Provider::Google => (
            "8.8.8.8,8.8.4.4",
            "2001:4860:4860::8888,2001:4860:4860::8844",
            "https://dns.google/dns-query",
        ),
    };
    [v4, v6].map(|servers| DnsConfig {
        servers: Some(servers.into()),
        properties: (0..2)
            .map(|index| DohProperty {
                index,
                template: Some(template.into()),
                flags: u64::from(DNS_DOH_SERVER_SETTINGS_ENABLE),
            })
            .collect(),
    })
}

fn cleared_doh(config: &DnsConfig) -> DnsConfig {
    DnsConfig {
        servers: config.servers.clone(),
        properties: Vec::new(),
    }
}

fn owned_config(current: &DnsConfig, journal: &Journal, index: usize) -> bool {
    current == &journal.original[index]
        || current == &journal.desired[index]
        || (journal.restoring
            && journal.original[index].servers.is_none()
            && current == &cleared_doh(&journal.desired[index]))
}

struct NativeDnsApi;

fn system_library(name: &str) -> Result<libloading::Library, BackendError> {
    let mut buffer = [0u16; 32768];
    let len = unsafe {
        windows::Win32::System::SystemInformation::GetSystemDirectoryW(Some(&mut buffer))
    } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(BackendError::ServiceUnavailable);
    }
    let directory =
        PathBuf::from(String::from_utf16(&buffer[..len]).map_err(|_| BackendError::Internal)?);
    unsafe { libloading::Library::new(directory.join(name)) }
        .map_err(|_| BackendError::ServiceUnavailable)
}

fn wide(value: Option<&str>) -> Vec<u16> {
    value.unwrap_or("").encode_utf16().chain(Some(0)).collect()
}
fn string_from_wide(value: PWSTR) -> Result<Option<String>, BackendError> {
    if value.is_null() {
        return Ok(None);
    }
    for len in 0..=2048 {
        if unsafe { *value.0.add(len) } == 0 {
            let value = String::from_utf16(unsafe { std::slice::from_raw_parts(value.0, len) })
                .map_err(|_| BackendError::ProtectedResourceInvalid)?;
            return Ok((!value.is_empty()).then_some(value));
        }
    }
    Err(BackendError::ProtectedResourceInvalid)
}

fn normalize_servers(value: Option<String>) -> Option<String> {
    value
        .map(|s| {
            s.split(|c: char| c == ',' || c.is_ascii_whitespace())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|s| !s.is_empty())
}

impl DnsApi for NativeDnsApi {
    #[cfg(test)]
    fn select(&self) -> Result<(u128, String), BackendError> {
        let mut size = 16 * 1024u32;
        for _ in 0..3 {
            if size > 256 * 1024 {
                return Err(BackendError::Conflict);
            }
            let mut buffer = vec![0u64; size as usize / 8 + 1];
            let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
            let error = unsafe {
                GetAdaptersAddresses(0, GAA_FLAG_INCLUDE_GATEWAYS, None, Some(first), &mut size)
            };
            if error == 111 {
                continue;
            }
            if error != 0 {
                return Err(BackendError::ServiceUnavailable);
            }
            let mut current = first;
            let mut choices = Vec::new();
            for _ in 0..64 {
                if current.is_null() {
                    break;
                }
                let adapter = unsafe { &*current };
                let mut row = MIB_IF_ROW2 {
                    InterfaceIndex: unsafe { adapter.Anonymous1.Anonymous.IfIndex },
                    ..Default::default()
                };
                if unsafe { GetIfEntry2(&mut row) }.0 != 0 {
                    return Err(BackendError::ServiceUnavailable);
                }
                if row.OperStatus.0 == 1 {
                    let description =
                        String::from_utf16_lossy(&row.Description).to_ascii_lowercase();
                    let hardware = row.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
                    if !hardware
                        && (row.Type == 131
                            || row.Type == 23
                            || ["tun", "vpn", "tap", "wireguard", "warp"]
                                .iter()
                                .any(|word| description.contains(word)))
                    {
                        return Err(BackendError::Conflict);
                    }
                    if hardware
                        && matches!(row.Type, 6 | 71)
                        && !adapter.FirstGatewayAddress.is_null()
                    {
                        let name = string_from_wide(adapter.FriendlyName)?
                            .ok_or(BackendError::Conflict)?;
                        choices.push((adapter.Ipv4Metric, row.InterfaceGuid.to_u128(), name));
                    }
                }
                current = adapter.Next;
            }
            if !current.is_null() {
                return Err(BackendError::Conflict);
            }
            if choices.len() != 1 {
                return Err(BackendError::Conflict);
            }
            choices.sort_by_key(|row| row.0);
            return choices
                .into_iter()
                .next()
                .map(|(_, interface, name)| (interface, name))
                .ok_or(BackendError::Conflict);
        }
        Err(BackendError::ServiceUnavailable)
    }

    fn read(&self, interface: u128, ipv6: bool) -> Result<DnsConfig, BackendError> {
        let library = system_library("iphlpapi.dll")?;
        let get = unsafe {
            library.get::<unsafe extern "system" fn(GUID, *mut DNS_INTERFACE_SETTINGS) -> u32>(
                b"GetInterfaceDnsSettings\0",
            )
        }
        .map_err(|_| BackendError::ServiceUnavailable)?;
        let free = unsafe {
            library.get::<unsafe extern "system" fn(*mut DNS_INTERFACE_SETTINGS)>(
                b"FreeInterfaceDnsSettings\0",
            )
        }
        .map_err(|_| BackendError::ServiceUnavailable)?;
        let mut settings = DNS_INTERFACE_SETTINGS3 {
            Version: 3,
            Flags: if ipv6 { 1 } else { 0 },
            ..Default::default()
        };
        let base = (&mut settings as *mut DNS_INTERFACE_SETTINGS3).cast::<DNS_INTERFACE_SETTINGS>();
        if unsafe { get(GUID::from_u128(interface), base) } != 0 {
            return Err(BackendError::ServiceUnavailable);
        }
        let result = (|| {
            if string_from_wide(settings.ProfileNameServer)?.is_some()
                || settings.cServerProperties > 8
            {
                return Err(BackendError::Conflict);
            }
            let mut config = DnsConfig {
                servers: normalize_servers(string_from_wide(settings.NameServer)?),
                properties: Vec::new(),
            };
            if settings.cServerProperties > 0 && settings.ServerProperties.is_null() {
                return Err(BackendError::ProtectedResourceInvalid);
            }
            for i in 0..settings.cServerProperties as usize {
                let property = unsafe { &*settings.ServerProperties.add(i) };
                if property.Version != 1 || property.Type != DnsServerDohProperty {
                    return Err(BackendError::Conflict);
                }
                let doh = unsafe { property.Property.DohSettings.as_ref() }
                    .ok_or(BackendError::ProtectedResourceInvalid)?;
                config.properties.push(DohProperty {
                    index: property.ServerIndex,
                    template: string_from_wide(doh.Template)?,
                    flags: doh.Flags,
                });
            }
            config.properties.sort_by_key(|p| p.index);
            config.validate(ipv6)?;
            Ok(config)
        })();
        unsafe { free(base) };
        result
    }

    fn write(&self, interface: u128, ipv6: bool, config: &DnsConfig) -> Result<(), BackendError> {
        config.validate(ipv6)?;
        let library = system_library("iphlpapi.dll")?;
        let set = unsafe {
            library.get::<unsafe extern "system" fn(GUID, *const DNS_INTERFACE_SETTINGS) -> u32>(
                b"SetInterfaceDnsSettings\0",
            )
        }
        .map_err(|_| BackendError::ServiceUnavailable)?;
        let mut servers = wide(config.servers.as_deref());
        if config.servers.is_none() {
            // Clear the installed per-server DoH properties first, then reset
            // the static list to DHCP. No other adapter settings are replaced.
            let current = self.read(interface, ipv6)?;
            if !current.properties.is_empty() {
                self.write(
                    interface,
                    ipv6,
                    &DnsConfig {
                        servers: current.servers,
                        properties: Vec::new(),
                    },
                )?;
            }
            let settings = DNS_INTERFACE_SETTINGS {
                Version: 1,
                Flags: u64::from(DNS_SETTING_NAMESERVER) | if ipv6 { 1 } else { 0 },
                NameServer: PWSTR(servers.as_mut_ptr()),
                ..Default::default()
            };
            return if unsafe { set(GUID::from_u128(interface), &settings) } == 0 {
                Ok(())
            } else {
                Err(BackendError::RuntimeFailed)
            };
        }
        let mut templates = config
            .properties
            .iter()
            .map(|p| wide(p.template.as_deref()))
            .collect::<Vec<_>>();
        let mut values = config
            .properties
            .iter()
            .zip(&mut templates)
            .map(|(p, t)| DNS_DOH_SERVER_SETTINGS {
                Template: if p.template.is_some() {
                    PWSTR(t.as_mut_ptr())
                } else {
                    PWSTR::null()
                },
                Flags: p.flags,
            })
            .collect::<Vec<_>>();
        let mut properties = config
            .properties
            .iter()
            .enumerate()
            .map(|(i, p)| DNS_SERVER_PROPERTY {
                Version: 1,
                ServerIndex: p.index,
                Type: DnsServerDohProperty,
                Property: DNS_SERVER_PROPERTY_TYPES {
                    DohSettings: unsafe { values.as_mut_ptr().add(i) },
                },
            })
            .collect::<Vec<_>>();
        let settings = DNS_INTERFACE_SETTINGS3 {
            Version: 3,
            Flags: u64::from(DNS_SETTING_NAMESERVER | DNS_SETTING_DOH) | if ipv6 { 1 } else { 0 },
            NameServer: PWSTR(servers.as_mut_ptr()),
            cServerProperties: properties.len() as u32,
            ServerProperties: if properties.is_empty() {
                std::ptr::null_mut()
            } else {
                properties.as_mut_ptr()
            },
            ..Default::default()
        };
        if unsafe {
            set(
                GUID::from_u128(interface),
                (&settings as *const DNS_INTERFACE_SETTINGS3).cast(),
            )
        } == 0
        {
            Ok(())
        } else {
            Err(BackendError::RuntimeFailed)
        }
    }

    fn flush(&self) -> Result<(), BackendError> {
        let library = system_library("dnsapi.dll")?;
        let flush = unsafe {
            library.get::<unsafe extern "system" fn() -> i32>(b"DnsFlushResolverCache\0")
        }
        .map_err(|_| BackendError::ServiceUnavailable)?;
        if unsafe { flush() } != 0 {
            Ok(())
        } else {
            Err(BackendError::RuntimeFailed)
        }
    }

    #[cfg(test)]
    fn provider_works(&self, provider: Provider) -> bool {
        let (host, ip, endpoint) = match provider {
            Provider::Cloudflare => (
                "cloudflare-dns.com",
                [1, 1, 1, 1],
                "https://cloudflare-dns.com/dns-query",
            ),
            Provider::Google => ("dns.google", [8, 8, 8, 8], "https://dns.google/resolve"),
        };
        let Ok(client) = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_millis(1200))
            .connect_timeout(Duration::from_millis(1000))
            .resolve(host, SocketAddr::from((ip, 443)))
            .build()
        else {
            return false;
        };
        std::thread::scope(|scope| {
            let jobs = ["discord.com", "gateway.discord.gg"].map(|name| {
                let client = &client;
                scope.spawn(move || {
                    let response = client
                        .get(endpoint)
                        .header("accept", "application/dns-json")
                        .query(&[
                            ("name", name),
                            ("type", "A"),
                            ("cd", "0"),
                            ("edns_client_subnet", "0.0.0.0/0"),
                        ])
                        .send()
                        .ok()?;
                    if !response.status().is_success() {
                        return None;
                    }
                    let mut body = Vec::new();
                    response
                        .take(MAX_DOH_BODY + 1)
                        .read_to_end(&mut body)
                        .ok()?;
                    if body.len() as u64 > MAX_DOH_BODY {
                        return None;
                    }
                    valid_doh_answer(&body, name).then_some(())
                })
            });
            jobs.into_iter()
                .all(|job| job.join().ok().flatten().is_some())
        })
    }
}

#[cfg(test)]
fn valid_doh_answer(bytes: &[u8], expected: &str) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return false;
    };
    if value["Status"].as_u64() != Some(0)
        || value["TC"].as_bool() == Some(true)
        || value["CD"].as_bool() == Some(true)
    {
        return false;
    }
    let Some(questions) = value["Question"].as_array() else {
        return false;
    };
    if questions.len() != 1
        || questions[0]["type"].as_u64() != Some(1)
        || questions[0]["name"]
            .as_str()
            .map(|s| s.trim_end_matches('.').to_ascii_lowercase())
            != Some(expected.to_ascii_lowercase())
    {
        return false;
    }
    let Some(answers) = value["Answer"].as_array() else {
        return false;
    };
    if answers.len() > 64 {
        return false;
    }
    let mut owners = std::collections::HashSet::from([expected.to_ascii_lowercase()]);
    for _ in 0..8 {
        for answer in answers {
            let owner = answer["name"]
                .as_str()
                .unwrap_or("")
                .trim_end_matches('.')
                .to_ascii_lowercase();
            if answer["type"].as_u64() == Some(5) && owners.contains(&owner) {
                let target = answer["data"]
                    .as_str()
                    .unwrap_or("")
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                if !target.is_empty() && target.len() <= 253 {
                    owners.insert(target);
                }
            }
        }
    }
    answers.iter().any(|answer| {
        let owner = answer["name"]
            .as_str()
            .unwrap_or("")
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let ip = answer["data"]
            .as_str()
            .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok());
        answer["type"].as_u64() == Some(1)
            && owners.contains(&owner)
            && ip.is_some_and(|ip| {
                !ip.is_private()
                    && !ip.is_loopback()
                    && !ip.is_link_local()
                    && !ip.is_multicast()
                    && !ip.is_unspecified()
                    && !ip.is_broadcast()
                    && !ip.is_documentation()
                    && ip.octets()[0] > 0
                    && ip.octets()[0] < 224
                    && !(ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                    && !(ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        recovery: DnsRecovery,
        state: Arc<Mutex<MockState>>,
    }
    struct MockState {
        configs: [DnsConfig; 2],
        providers: [bool; 2],
        checked: Vec<Provider>,
        writes: usize,
        vpn: bool,
        fail_v6_once: bool,
        fail_dhcp_once: bool,
        change_during_health: bool,
    }
    struct MockApi {
        state: Arc<Mutex<MockState>>,
        journal: PathBuf,
    }
    impl DnsApi for MockApi {
        fn select(&self) -> Result<(u128, String), BackendError> {
            if self.state.lock().unwrap().vpn {
                Err(BackendError::Conflict)
            } else {
                Ok((1, "Wi-Fi".into()))
            }
        }
        fn read(&self, _: u128, ipv6: bool) -> Result<DnsConfig, BackendError> {
            Ok(self.state.lock().unwrap().configs[usize::from(ipv6)].clone())
        }
        fn write(&self, _: u128, ipv6: bool, config: &DnsConfig) -> Result<(), BackendError> {
            // Every write, including undo, must have a durable validated backup.
            let journal: Journal =
                serde_json::from_slice(&fs::read(&self.journal).unwrap()).unwrap();
            journal.validate().unwrap();
            let mut state = self.state.lock().unwrap();
            state.writes += 1;
            if ipv6 && state.fail_v6_once {
                state.fail_v6_once = false;
                return Err(BackendError::RuntimeFailed);
            }
            if config.servers.is_none() && state.fail_dhcp_once {
                state.fail_dhcp_once = false;
                state.configs[usize::from(ipv6)] = cleared_doh(&state.configs[usize::from(ipv6)]);
                return Err(BackendError::RuntimeFailed);
            }
            state.configs[usize::from(ipv6)] = config.clone();
            Ok(())
        }
        fn flush(&self) -> Result<(), BackendError> {
            Ok(())
        }
        fn provider_works(&self, provider: Provider) -> bool {
            let mut state = self.state.lock().unwrap();
            state.checked.push(provider);
            if state.change_during_health {
                state.configs[0] = DnsConfig {
                    servers: Some("9.9.9.9".into()),
                    properties: Vec::new(),
                };
            }
            state.providers[usize::from(provider == Provider::Google)]
        }
    }
    impl Fixture {
        fn new(configs: [DnsConfig; 2]) -> Self {
            let root = std::env::temp_dir().join(format!(
                "obsession-dns-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let data = root.join("ProgramData");
            let runtime = data.join("Obsession/Runtime");
            fs::create_dir_all(&runtime).unwrap();
            let layout = ProtectedDataLayout::inspect(&data, &runtime).unwrap();
            let journal = layout.dns_recovery_journal().unwrap();
            let state = Arc::new(Mutex::new(MockState {
                configs,
                providers: [true, true],
                checked: Vec::new(),
                writes: 0,
                vpn: false,
                fail_v6_once: false,
                fail_dhcp_once: false,
                change_during_health: false,
            }));
            let recovery = DnsRecovery {
                layout,
                api: Box::new(MockApi {
                    state: state.clone(),
                    journal,
                }),
            };
            Self {
                root,
                recovery,
                state,
            }
        }
        fn restart(&mut self) {
            self.recovery = DnsRecovery {
                layout: self.recovery.layout.clone(),
                api: Box::new(MockApi {
                    state: self.state.clone(),
                    journal: self.recovery.layout.dns_recovery_journal().unwrap(),
                }),
            };
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
            assert!(self
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("obsession-dns-test-"));
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn automatic() -> [DnsConfig; 2] {
        [DnsConfig::default(), DnsConfig::default()]
    }

    #[test]
    fn both_families_and_original_doh_survive_service_restart() {
        let mut original = provider_configs(Provider::Google);
        original[1] = DnsConfig::default();
        let mut fixture = Fixture::new(original.clone());
        let status = fixture.recovery.repair().unwrap();
        assert!(status.active && status.restore_available);
        assert_eq!(
            fixture.state.lock().unwrap().configs,
            provider_configs(Provider::Cloudflare)
        );
        fixture.restart();
        assert!(fixture.recovery.status().unwrap().active);
        assert_eq!(
            fixture.recovery.restore().unwrap(),
            SystemDnsStatus::default()
        );
        assert_eq!(fixture.state.lock().unwrap().configs, original);
        assert!(fixture.recovery.load().unwrap().is_none());
    }
    #[test]
    fn second_family_failure_rolls_back_first_family() {
        let mut fixture = Fixture::new(automatic());
        fixture.state.lock().unwrap().fail_v6_once = true;
        assert_eq!(fixture.recovery.repair(), Err(BackendError::RuntimeFailed));
        assert_eq!(fixture.state.lock().unwrap().configs, automatic());
        assert!(fixture.recovery.load().unwrap().is_none());
    }
    #[test]
    fn provider_fallback_is_measured_before_any_write() {
        let mut fixture = Fixture::new(automatic());
        fixture.state.lock().unwrap().providers = [false, true];
        assert_eq!(
            fixture.recovery.repair().unwrap().provider,
            Some(Provider::Google)
        );
        assert_eq!(
            fixture.state.lock().unwrap().checked,
            [Provider::Cloudflare, Provider::Google]
        );
    }
    #[test]
    fn no_provider_or_active_vpn_never_writes() {
        for vpn in [false, true] {
            let mut fixture = Fixture::new(automatic());
            {
                let mut state = fixture.state.lock().unwrap();
                state.providers = [false, false];
                state.vpn = vpn;
            }
            assert!(fixture.recovery.repair().is_err());
            assert_eq!(fixture.state.lock().unwrap().writes, 0);
            assert!(fixture.recovery.load().unwrap().is_none());
        }
    }
    #[test]
    fn changed_settings_during_provider_check_are_preserved() {
        let mut fixture = Fixture::new(automatic());
        fixture.state.lock().unwrap().change_during_health = true;
        assert_eq!(fixture.recovery.repair(), Err(BackendError::Conflict));
        assert_eq!(fixture.state.lock().unwrap().writes, 0);
        assert!(fixture.recovery.load().unwrap().is_none());
    }
    #[test]
    fn undo_never_overwrites_external_settings() {
        let mut fixture = Fixture::new(automatic());
        fixture.recovery.repair().unwrap();
        let writes = fixture.state.lock().unwrap().writes;
        fixture.state.lock().unwrap().configs[0].servers = Some("9.9.9.9".into());
        assert_eq!(fixture.recovery.restore(), Err(BackendError::Conflict));
        assert_eq!(fixture.state.lock().unwrap().writes, writes);
        assert!(fixture.recovery.status().unwrap().restore_available);
        assert!(!fixture.recovery.status().unwrap().active);
    }
    #[test]
    fn partial_install_and_interrupted_dhcp_undo_are_recoverable() {
        let mut fixture = Fixture::new(automatic());
        fixture.recovery.repair().unwrap();
        fixture.state.lock().unwrap().configs[1] = DnsConfig::default();
        let status = fixture.recovery.status().unwrap();
        assert!(!status.active && status.restore_available);
        fixture.state.lock().unwrap().fail_dhcp_once = true;
        assert_eq!(fixture.recovery.restore(), Err(BackendError::RuntimeFailed));
        assert!(fixture.recovery.load().unwrap().unwrap().restoring);
        fixture.restart();
        fixture.recovery.restore().unwrap();
        assert_eq!(fixture.state.lock().unwrap().configs, automatic());
    }
    #[test]
    fn externally_disabled_doh_is_not_mistaken_for_our_interrupted_undo() {
        let mut fixture = Fixture::new(automatic());
        fixture.recovery.repair().unwrap();
        fixture.state.lock().unwrap().configs[0] =
            cleared_doh(&provider_configs(Provider::Cloudflare)[0]);
        assert_eq!(fixture.recovery.restore(), Err(BackendError::Conflict));
    }
    #[test]
    fn corrupted_and_oversized_journals_fail_without_writes() {
        for data in [b"{}".to_vec(), vec![b'x'; MAX_JOURNAL as usize + 1]] {
            let mut fixture = Fixture::new(automatic());
            fs::write(
                fixture.recovery.layout.dns_recovery_journal().unwrap(),
                data,
            )
            .unwrap();
            assert_eq!(
                fixture.recovery.restore(),
                Err(BackendError::ProtectedResourceInvalid)
            );
            assert_eq!(fixture.state.lock().unwrap().writes, 0);
        }
    }

    #[test]
    fn full_width_adapter_guid_roundtrips_in_durable_journal() {
        let fixture = Fixture::new(automatic());
        let journal = Journal {
            version: 1, interface: u128::MAX, name: "Wi-Fi".into(), provider: Provider::Cloudflare,
            original: automatic(), desired: provider_configs(Provider::Cloudflare), restoring: false,
        };
        fixture.recovery.save(&journal).unwrap();
        assert_eq!(fixture.recovery.load().unwrap().unwrap().interface, u128::MAX);
    }
    #[test]
    fn bootstrap_requires_matching_question_public_address_and_complete_answer() {
        let good = serde_json::json!({"Status":0,"TC":false,"CD":false,"Question":[{"name":"discord.com.","type":1}],
            "Answer":[{"name":"discord.com.","type":5,"data":"edge.example."},{"name":"edge.example.","type":1,"data":"162.159.128.233"}]});
        let accepts = |value: &serde_json::Value| {
            valid_doh_answer(&serde_json::to_vec(value).unwrap(), "discord.com")
        };
        assert!(accepts(&good));
        for key in ["TC", "CD"] {
            let mut bad = good.clone();
            bad[key] = true.into();
            assert!(!accepts(&bad));
        }
        let mut bad = good.clone();
        bad["Status"] = 3.into();
        assert!(!accepts(&bad));
        let mut bad = good.clone();
        bad["Question"][0]["name"] = "other.example".into();
        assert!(!accepts(&bad));
        let mut bad = good.clone();
        bad["Answer"][1]["name"] = "unrelated.example".into();
        assert!(!accepts(&bad));
        for ip in [
            "127.0.0.1",
            "192.168.0.1",
            "0.1.2.3",
            "100.64.0.1",
            "198.18.0.1",
            "203.0.113.1",
        ] {
            let mut bad = good.clone();
            bad["Answer"][1]["data"] = ip.into();
            assert!(!accepts(&bad), "{ip}");
        }
    }
}
