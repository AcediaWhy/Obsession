//! Bounded service-owned correlation between supported target applications and
//! their exact outbound TCP sockets.
//!
//! Process identifiers and executable paths stay inside LocalSystem. The wire
//! projection contains only running target categories, while Eyes receives an
//! exact FlowKey suitable for first-SYN attribution without guessing from a
//! shared CDN IP address.

#![cfg(windows)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::{Duration, Instant};

use obsession_runtime_protocol::{DpiCategory, LegacyAccessActivitySnapshot};
use obsession_runtime_reliability::eyes::parse::FlowKey;
use windows::core::HRESULT;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, HANDLE,
};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_TCP_STATE_SYN_SENT,
    TCP_TABLE_OWNER_PID_ALL,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

const MAX_PROCESSES: usize = 16_384;
const MAX_TCP_TABLE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TCP_ROWS: usize = 65_536;
const MAX_SOCKET_ATTRIBUTIONS: usize = 256;
const ACCESS_POLL_INTERVAL: Duration = Duration::from_secs(2);
const DISCORD_EXECUTABLES: [&str; 4] = [
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "discorddevelopment.exe",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessSocketAttribution {
    pub(crate) category: DpiCategory,
    pub(crate) key: FlowKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LegacyAccessPoll {
    pub(crate) public: LegacyAccessActivitySnapshot,
    pub(crate) sockets: Vec<ProcessSocketAttribution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MachineObservation {
    running_categories: BTreeSet<DpiCategory>,
    sockets: Vec<ProcessSocketAttribution>,
}

pub(crate) struct LegacyAccessTracker {
    revision: u64,
    sensor_available: bool,
    running_categories: BTreeSet<DpiCategory>,
    last_poll: Option<Instant>,
}

impl LegacyAccessTracker {
    pub(crate) fn new() -> Self {
        Self {
            revision: 1,
            sensor_available: false,
            running_categories: BTreeSet::new(),
            last_poll: None,
        }
    }

    pub(crate) fn poll(&mut self) -> Option<LegacyAccessPoll> {
        self.poll_at(Instant::now(), poll_machine)
    }

    pub(crate) fn public_snapshot(&self) -> LegacyAccessActivitySnapshot {
        LegacyAccessActivitySnapshot {
            revision: self.revision,
            sensor_available: self.sensor_available,
            running_categories: self.running_categories.iter().copied().collect(),
        }
    }

    fn poll_at(
        &mut self,
        now: Instant,
        observe: impl FnOnce() -> Result<MachineObservation, ()>,
    ) -> Option<LegacyAccessPoll> {
        if self
            .last_poll
            .is_some_and(|last| now.saturating_duration_since(last) < ACCESS_POLL_INTERVAL)
        {
            return None;
        }
        self.last_poll = Some(now);
        Some(self.apply(observe()))
    }

    fn apply(&mut self, result: Result<MachineObservation, ()>) -> LegacyAccessPoll {
        let (sensor_available, running_categories, sockets) = match result {
            Ok(observation) => (true, observation.running_categories, observation.sockets),
            Err(()) => (false, BTreeSet::new(), Vec::new()),
        };
        if self.sensor_available != sensor_available
            || self.running_categories != running_categories
        {
            self.sensor_available = sensor_available;
            self.running_categories = running_categories;
            self.revision = next_nonzero(self.revision);
        }
        LegacyAccessPoll {
            public: self.public_snapshot(),
            sockets,
        }
    }
}

fn poll_machine() -> Result<MachineObservation, ()> {
    let processes = target_processes()?;
    let running_categories = processes.values().copied().collect::<BTreeSet<_>>();
    let mut sockets = if processes.is_empty() {
        Vec::new()
    } else {
        let mut sockets = tcp4_sockets(&processes)?;
        sockets.extend(tcp6_sockets(&processes)?);
        sockets
    };
    sockets.sort_by(|left, right| {
        (
            left.category,
            left.key.local_port,
            left.key.remote_ip,
            left.key.remote_port,
        )
            .cmp(&(
                right.category,
                right.key.local_port,
                right.key.remote_ip,
                right.key.remote_port,
            ))
    });
    sockets.dedup();
    sockets.truncate(MAX_SOCKET_ATTRIBUTIONS);
    Ok(MachineObservation {
        running_categories,
        sockets,
    })
}

fn target_processes() -> Result<BTreeMap<u32, DpiCategory>, ()> {
    let snapshot =
        OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|_| ())?);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if let Err(error) = unsafe { Process32FirstW(snapshot.0, &mut entry) } {
        return if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
            Ok(BTreeMap::new())
        } else {
            Err(())
        };
    }

    let mut result = BTreeMap::new();
    for scanned in 0..MAX_PROCESSES {
        if entry.th32ProcessID != 0 {
            if let Some(category) = category_for_executable(&executable_name(&entry)) {
                result.insert(entry.th32ProcessID, category);
            }
        }
        match unsafe { Process32NextW(snapshot.0, &mut entry) } {
            Ok(()) => {}
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => {
                return Ok(result)
            }
            Err(_) => return Err(()),
        }
        if scanned + 1 == MAX_PROCESSES {
            return Err(());
        }
    }
    Err(())
}

fn category_for_executable(name: &str) -> Option<DpiCategory> {
    DISCORD_EXECUTABLES
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
        .then_some(DpiCategory::Discord)
}

fn executable_name(entry: &PROCESSENTRY32W) -> String {
    let end = entry
        .szExeFile
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(entry.szExeFile.len());
    String::from_utf16_lossy(&entry.szExeFile[..end])
}

fn tcp4_sockets(
    processes: &BTreeMap<u32, DpiCategory>,
) -> Result<Vec<ProcessSocketAttribution>, ()> {
    let table = tcp_table(AF_INET.0.into())?;
    parse_rows::<MIB_TCPROW_OWNER_PID>(&table, |row| {
        if row.dwState != MIB_TCP_STATE_SYN_SENT.0 as u32 {
            return None;
        }
        let category = *processes.get(&row.dwOwningPid)?;
        let remote_ip = Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes());
        let local_port = decode_port(row.dwLocalPort)?;
        let remote_port = decode_port(row.dwRemotePort)?;
        (!remote_ip.is_unspecified()).then_some(ProcessSocketAttribution {
            category,
            key: FlowKey {
                local_port,
                remote_ip: IpAddr::V4(remote_ip),
                remote_port,
            },
        })
    })
}

fn tcp6_sockets(
    processes: &BTreeMap<u32, DpiCategory>,
) -> Result<Vec<ProcessSocketAttribution>, ()> {
    let table = tcp_table(AF_INET6.0.into())?;
    parse_rows::<MIB_TCP6ROW_OWNER_PID>(&table, |row| {
        if row.dwState != MIB_TCP_STATE_SYN_SENT.0 as u32 {
            return None;
        }
        let category = *processes.get(&row.dwOwningPid)?;
        let remote_ip = Ipv6Addr::from(row.ucRemoteAddr);
        let local_port = decode_port(row.dwLocalPort)?;
        let remote_port = decode_port(row.dwRemotePort)?;
        (!remote_ip.is_unspecified()).then_some(ProcessSocketAttribution {
            category,
            key: FlowKey {
                local_port,
                remote_ip: IpAddr::V6(remote_ip),
                remote_port,
            },
        })
    })
}

fn tcp_table(address_family: u32) -> Result<Vec<u8>, ()> {
    let mut size = 0u32;
    let first = unsafe {
        GetExtendedTcpTable(
            None,
            &mut size,
            false,
            address_family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if first != 0 && first != ERROR_INSUFFICIENT_BUFFER.0 {
        return Err(());
    }
    let size = usize::try_from(size).map_err(|_| ())?;
    if size < size_of::<u32>() || size > MAX_TCP_TABLE_BYTES {
        return Err(());
    }
    let mut table = vec![0u8; size];
    let mut actual = u32::try_from(table.len()).map_err(|_| ())?;
    let status = unsafe {
        GetExtendedTcpTable(
            Some(table.as_mut_ptr().cast::<c_void>()),
            &mut actual,
            false,
            address_family,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if status != 0 {
        return Err(());
    }
    let actual = usize::try_from(actual).map_err(|_| ())?;
    if actual < size_of::<u32>() || actual > table.len() {
        return Err(());
    }
    table.truncate(actual);
    Ok(table)
}

fn parse_rows<Row: Copy>(
    table: &[u8],
    mut convert: impl FnMut(Row) -> Option<ProcessSocketAttribution>,
) -> Result<Vec<ProcessSocketAttribution>, ()> {
    let count = u32::from_ne_bytes(table.get(..4).ok_or(())?.try_into().map_err(|_| ())?) as usize;
    if count > MAX_TCP_ROWS {
        return Err(());
    }
    let bytes = count.checked_mul(size_of::<Row>()).ok_or(())?;
    let end = size_of::<u32>().checked_add(bytes).ok_or(())?;
    if end > table.len() {
        return Err(());
    }
    let mut result = Vec::new();
    for index in 0..count {
        let offset = size_of::<u32>() + index * size_of::<Row>();
        let row = unsafe { std::ptr::read_unaligned(table.as_ptr().add(offset).cast::<Row>()) };
        if let Some(attribution) = convert(row) {
            result.push(attribution);
            if result.len() >= MAX_SOCKET_ATTRIBUTIONS {
                break;
            }
        }
    }
    Ok(result)
}

fn decode_port(value: u32) -> Option<u16> {
    let port = u16::from_be(value as u16);
    (port != 0).then_some(port)
}

const fn next_nonzero(current: u64) -> u64 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(running: bool) -> MachineObservation {
        MachineObservation {
            running_categories: if running {
                BTreeSet::from([DpiCategory::Discord])
            } else {
                BTreeSet::new()
            },
            sockets: Vec::new(),
        }
    }

    #[test]
    fn duplicate_process_polls_do_not_advance_public_revision() {
        let mut tracker = LegacyAccessTracker::new();
        let first = tracker.apply(Ok(observation(true)));
        let duplicate = tracker.apply(Ok(observation(true)));
        assert_eq!(first.public.revision, duplicate.public.revision);
        assert_eq!(
            duplicate.public.running_categories,
            vec![DpiCategory::Discord]
        );
    }

    #[test]
    fn machine_poll_is_cached_for_two_seconds() {
        let mut tracker = LegacyAccessTracker::new();
        let started = Instant::now();
        let first = tracker.poll_at(started, || Ok(observation(true)));
        let cached = tracker.poll_at(started + Duration::from_millis(1_999), || {
            panic!("cached poll must not touch the machine")
        });
        let refreshed = tracker.poll_at(started + ACCESS_POLL_INTERVAL, || Ok(observation(false)));

        assert!(first.is_some());
        assert!(cached.is_none());
        assert!(refreshed.is_some());
        assert!(tracker.public_snapshot().running_categories.is_empty());
    }

    #[test]
    fn official_discord_channels_are_recognized_without_widening_the_match() {
        for executable in [
            "Discord.exe",
            "DISCORDPTB.EXE",
            "discordcanary.exe",
            "DiscordDevelopment.exe",
        ] {
            assert_eq!(
                category_for_executable(executable),
                Some(DpiCategory::Discord)
            );
        }
        assert_eq!(category_for_executable("discord-helper.exe"), None);
        assert_eq!(category_for_executable("update.exe"), None);
    }

    #[test]
    fn sensor_failure_fails_closed_without_stale_running_category() {
        let mut tracker = LegacyAccessTracker::new();
        let running = tracker.apply(Ok(observation(true)));
        let failed = tracker.apply(Err(()));
        assert!(failed.public.revision > running.public.revision);
        assert!(!failed.public.sensor_available);
        assert!(failed.public.running_categories.is_empty());
        assert!(failed.sockets.is_empty());
    }

    #[test]
    fn network_order_port_decode_is_exact() {
        assert_eq!(decode_port(u32::from(443u16.to_be())), Some(443));
        assert_eq!(decode_port(0), None);
    }
}
