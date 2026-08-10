//! Read-only machine network identity for generation-fenced Environment Gate.
//!
//! The privileged service never shells out to `route`, `arp`, PowerShell or a
//! caller-provided executable. It reads connected Windows Network List Manager
//! profile GUIDs through COM, hashes them in memory and exposes only the hash
//! to the reliability state machine.

#![cfg(windows)]

use obsession_runtime_reliability::legacy_reliability::contracts::NetworkFingerprint;
use obsession_runtime_reliability::legacy_reliability::environment_gate::LocalNetworkSnapshot;
use sha2::{Digest, Sha256};
use windows::core::GUID;
use windows::Win32::Networking::NetworkListManager::{
    INetworkListManager, NetworkListManager, NLM_ENUM_NETWORK_CONNECTED,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

const MAX_CONNECTED_NETWORKS: usize = 32;

pub fn snapshot() -> LocalNetworkSnapshot {
    read_snapshot().unwrap_or_else(|_| unavailable_snapshot())
}

fn read_snapshot() -> windows::core::Result<LocalNetworkSnapshot> {
    let initialization = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialization.is_err() {
        // RPC_E_CHANGED_MODE means the thread already owns a different COM
        // apartment. COM calls are still legal there; other failures are not.
        const RPC_E_CHANGED_MODE: windows::core::HRESULT =
            windows::core::HRESULT(0x80010106u32 as i32);
        if initialization != RPC_E_CHANGED_MODE {
            initialization.ok()?;
        }
    }
    let must_uninitialize = initialization.is_ok();
    let result = read_initialized_snapshot();
    if must_uninitialize {
        unsafe { CoUninitialize() };
    }
    result
}

fn read_initialized_snapshot() -> windows::core::Result<LocalNetworkSnapshot> {
    let manager: INetworkListManager =
        unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL) }?;
    let connected = unsafe { manager.IsConnected() }?.0 != 0;
    let internet = unsafe { manager.IsConnectedToInternet() }?.0 != 0;
    let networks = unsafe { manager.GetNetworks(NLM_ENUM_NETWORK_CONNECTED) }?;
    let mut ids = Vec::new();

    loop {
        let mut slot = [None];
        let mut fetched = 0u32;
        unsafe { networks.Next(&mut slot, Some(&mut fetched)) }?;
        if fetched == 0 {
            break;
        }
        let Some(network) = slot[0].take() else {
            return Ok(unavailable_snapshot());
        };
        ids.push(unsafe { network.GetNetworkId() }?);
        if ids.len() > MAX_CONNECTED_NETWORKS {
            return Ok(LocalNetworkSnapshot {
                online: internet,
                interface_up: connected,
                default_route_available: connected,
                gateway_reachable: connected,
                network_fingerprint: NetworkFingerprint::Unstable {
                    reason: "connected_network_set_exceeded_bound".into(),
                },
            });
        }
    }

    Ok(LocalNetworkSnapshot {
        online: internet,
        interface_up: connected,
        default_route_available: connected,
        gateway_reachable: connected,
        network_fingerprint: fingerprint_for_ids(ids),
    })
}

fn fingerprint_for_ids<I>(ids: I) -> NetworkFingerprint
where
    I: IntoIterator<Item = GUID>,
{
    let mut ids = ids.into_iter().map(|id| id.to_u128()).collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return NetworkFingerprint::Unstable {
            reason: "connected_network_identity_unavailable".into(),
        };
    }

    let mut digest = Sha256::new();
    digest.update(b"obsession/service-network-list/v1\0");
    digest.update((ids.len() as u64).to_be_bytes());
    for id in ids {
        digest.update(id.to_be_bytes());
    }
    let key = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    NetworkFingerprint::Stable { key }
}

fn unavailable_snapshot() -> LocalNetworkSnapshot {
    LocalNetworkSnapshot {
        online: false,
        interface_up: false,
        default_route_available: false,
        gateway_reachable: false,
        network_fingerprint: NetworkFingerprint::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_fingerprint_is_order_independent_and_private() {
        let first = GUID::from_u128(1);
        let second = GUID::from_u128(2);
        let left = fingerprint_for_ids([first, second]);
        let right = fingerprint_for_ids([second, first, first]);
        assert_eq!(left, right);
        let NetworkFingerprint::Stable { key } = left else {
            panic!("connected GUIDs must produce a stable fingerprint");
        };
        assert_eq!(key.len(), 64);
        assert!(!key.contains("00000000"));
    }

    #[test]
    fn missing_network_identity_stays_non_actionable() {
        assert!(matches!(
            fingerprint_for_ids([]),
            NetworkFingerprint::Unstable { .. }
        ));
    }
}
