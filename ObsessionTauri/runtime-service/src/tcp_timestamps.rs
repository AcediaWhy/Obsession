//! A durable, exclusive lease for Legacy's TCP timestamp fooling prerequisite.
//! Only this single global TCP option is touched. Recovery runs at service startup.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use obsession_runtime_protocol::DpiEngine;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;

use crate::dpi_materializer::{MaterializedLaunch, ProtectedDataLayout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setting {
    Allowed,
    Enabled,
    Disabled,
}

impl Setting {
    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "allowed" => Ok(Self::Allowed),
            "enabled" => Ok(Self::Enabled),
            "disabled" => Ok(Self::Disabled),
            _ => Err(io::Error::other("unknown TCP timestamps value")),
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

fn parse_dump(text: &str) -> io::Result<Setting> {
    let mut values = text
        .lines()
        .filter(|line| line.trim_start().starts_with("set global "))
        .flat_map(str::split_whitespace)
        .filter_map(|part| part.strip_prefix("timestamps="));
    let value = values
        .next()
        .ok_or_else(|| io::Error::other("netsh dump omitted timestamps"))?;
    if values.next().is_some() {
        return Err(io::Error::other("ambiguous TCP timestamps dump"));
    }
    Setting::parse(value)
}

fn config_requires_timestamps(text: &str) -> bool {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| {
            line.split_whitespace().any(|token| {
                let token = token.trim_matches('"');
                ["--dpi-desync-fooling=", "--dup-fooling="]
                    .iter()
                    .any(|prefix| {
                        token.strip_prefix(prefix).is_some_and(|value| {
                            value.trim_matches('"').split(',').any(|item| item == "ts")
                        })
                    })
            })
        })
}

trait Machine: Send {
    fn read(&mut self) -> io::Result<Setting>;
    fn set(&mut self, value: Setting) -> io::Result<()>;
    fn saved(&mut self) -> io::Result<Option<Setting>>;
    fn save(&mut self, value: Setting) -> io::Result<()>;
    fn clear(&mut self) -> io::Result<()>;
}

struct Session<M: Machine> {
    machine: M,
    active: bool,
}

impl<M: Machine> Session<M> {
    fn recover(&mut self) -> io::Result<()> {
        if self.active {
            return Err(io::Error::other("TCP timestamp lease is still active"));
        }
        if let Some(previous) = self.machine.saved()? {
            // Preserve a distinguishable external administrator change.
            if self.machine.read()? == Setting::Enabled && previous != Setting::Enabled {
                self.machine.set(previous)?;
                if self.machine.read()? != previous {
                    return Err(io::Error::other(
                        "TCP timestamp restoration did not take effect",
                    ));
                }
            }
            self.machine.clear()?;
        }
        Ok(())
    }

    fn acquire(&mut self, required: bool) -> io::Result<()> {
        self.recover()?;
        if required {
            let previous = self.machine.read()?;
            if previous != Setting::Enabled {
                // Persist and sync BEFORE mutation, including commands that fail
                // after changing the setting or are interrupted by a crash.
                self.machine.save(previous)?;
                let enabled = self.machine.set(Setting::Enabled).and_then(|()| {
                    if self.machine.read()? == Setting::Enabled {
                        Ok(())
                    } else {
                        Err(io::Error::other("TCP timestamps could not be enabled"))
                    }
                });
                if let Err(error) = enabled {
                    return match self.recover() {
                        Ok(()) => Err(error),
                        Err(restore) => Err(io::Error::other(format!(
                            "{error}; recovery pending: {restore}"
                        ))),
                    };
                }
            }
        }
        self.active = true;
        Ok(())
    }

    fn release(&mut self) -> io::Result<()> {
        self.active = false;
        self.recover()
    }
}

pub(crate) struct TimestampController(Arc<Mutex<Session<WindowsMachine>>>);
pub(crate) struct TimestampLease {
    state: Arc<Mutex<Session<WindowsMachine>>>,
    released: bool,
}

impl TimestampController {
    pub(crate) fn new(layout: ProtectedDataLayout) -> io::Result<Self> {
        let mut state = Session {
            machine: WindowsMachine { layout },
            active: false,
        };
        state.recover()?;
        Ok(Self(Arc::new(Mutex::new(state))))
    }

    pub(crate) fn acquire(&self, launches: &[MaterializedLaunch]) -> io::Result<TimestampLease> {
        let mut required = false;
        for launch in launches
            .iter()
            .filter(|launch| launch.engine() == DpiEngine::Legacy)
        {
            if let Some(path) = launch.response_file() {
                let mut bytes = Vec::new();
                File::open(path)?
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 1024 * 1024 {
                    return Err(io::Error::other("Legacy response file too large"));
                }
                let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
                required |= config_requires_timestamps(text);
            }
            required |= config_requires_timestamps(&launch.arguments().join("\n"));
        }
        self.0
            .lock()
            .map_err(|_| io::Error::other("TCP timestamp lock poisoned"))?
            .acquire(required)?;
        Ok(TimestampLease {
            state: self.0.clone(),
            released: false,
        })
    }
}

impl TimestampLease {
    pub(crate) fn release(&mut self) -> io::Result<()> {
        if !self.released {
            self.state
                .lock()
                .map_err(|_| io::Error::other("TCP timestamp lock poisoned"))?
                .release()?;
            self.released = true;
        }
        Ok(())
    }
}

impl Drop for TimestampLease {
    fn drop(&mut self) {
        if let Err(error) = self.release() {
            eprintln!("TCP timestamp restoration pending until next start: {error}");
        }
    }
}

struct WindowsMachine {
    layout: ProtectedDataLayout,
}

impl WindowsMachine {
    fn journal(&self) -> io::Result<PathBuf> {
        self.layout
            .tcp_timestamp_journal()
            .map_err(io::Error::other)
    }
}

impl Machine for WindowsMachine {
    fn read(&mut self) -> io::Result<Setting> {
        parse_dump(&netsh(&["interface", "tcp", "dump"])?)
    }
    fn set(&mut self, value: Setting) -> io::Result<()> {
        netsh(&[
            "interface",
            "tcp",
            "set",
            "global",
            &format!("timestamps={}", value.as_str()),
        ])
        .map(|_| ())
    }
    fn saved(&mut self) -> io::Result<Option<Setting>> {
        let file = match File::open(self.journal()?) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut value = String::new();
        file.take(32).read_to_string(&mut value)?;
        // Exact serialization also rejects truncated or unexpected journals.
        Setting::parse(&value).map(Some)
    }
    fn save(&mut self, value: Setting) -> io::Result<()> {
        let path = self.journal()?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(value.as_str().as_bytes())?;
        file.sync_all()?;
        self.journal()?;
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        fs::remove_file(self.journal()?)
    }
}

fn netsh(args: &[&str]) -> io::Result<String> {
    let mut buffer = [0u16; 32768];
    let count = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if count == 0 || count >= buffer.len() {
        return Err(io::Error::other("Windows system directory unavailable"));
    }
    let executable = PathBuf::from(String::from_utf16(&buffer[..count]).map_err(io::Error::other)?)
        .join("netsh.exe");
    let mut child = Command::new(executable)
        .args(args)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("netsh output unavailable"))?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(65537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(match result {
                    Err(error) => error,
                    _ => io::Error::new(io::ErrorKind::TimedOut, "netsh timed out"),
                });
            }
        }
    };
    let output = reader
        .join()
        .map_err(|_| io::Error::other("netsh output reader failed"))??;
    if !status?.success() {
        return Err(io::Error::other("netsh TCP timestamps command failed"));
    }
    if output.len() > 65536 {
        return Err(io::Error::other("netsh output exceeded limit"));
    }
    // Dump command/enum tokens are ASCII even on a localized Windows installation.
    Ok(String::from_utf8_lossy(&output).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_journal_roundtrip_rejects_corruption_and_overwrite() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "obsession-timestamps-test-{}-{nonce}",
            std::process::id()
        ));
        let root = base.join(crate::dpi_materializer::RUNTIME_STATE_RELATIVE);
        fs::create_dir_all(&root).unwrap();
        let layout = ProtectedDataLayout::inspect(&base, &root).unwrap();
        let mut machine = WindowsMachine {
            layout: layout.clone(),
        };
        assert_eq!(machine.saved().unwrap(), None);
        machine.save(Setting::Allowed).unwrap();
        assert!(machine.save(Setting::Disabled).is_err());
        let mut restarted = WindowsMachine { layout };
        assert_eq!(restarted.saved().unwrap(), Some(Setting::Allowed));
        restarted.clear().unwrap();
        let path = restarted.journal().unwrap();
        fs::write(&path, b"allo").unwrap();
        assert!(restarted.saved().is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(restarted.journal().is_err());
        // Only the uniquely-created test fixture is removed.
        fs::remove_dir_all(&base).unwrap();
    }

    struct Fake {
        value: Setting,
        journal: Option<Setting>,
        writes: Vec<Setting>,
        fail_save: bool,
        fail_set: bool,
        fail_after_set: bool,
    }
    impl Machine for Fake {
        fn read(&mut self) -> io::Result<Setting> {
            Ok(self.value)
        }
        fn set(&mut self, value: Setting) -> io::Result<()> {
            if self.fail_set {
                return Err(io::Error::other("denied"));
            }
            assert!(self.journal.is_some(), "mutation must have durable journal");
            self.writes.push(value);
            self.value = value;
            if std::mem::take(&mut self.fail_after_set) {
                return Err(io::Error::other("interrupted"));
            }
            Ok(())
        }
        fn saved(&mut self) -> io::Result<Option<Setting>> {
            Ok(self.journal)
        }
        fn save(&mut self, value: Setting) -> io::Result<()> {
            if self.fail_save {
                return Err(io::Error::other("disk full"));
            }
            assert!(self.journal.is_none());
            self.journal = Some(value);
            Ok(())
        }
        fn clear(&mut self) -> io::Result<()> {
            self.journal = None;
            Ok(())
        }
    }
    fn session(value: Setting) -> Session<Fake> {
        Session {
            active: false,
            machine: Fake {
                value,
                journal: None,
                writes: vec![],
                fail_save: false,
                fail_set: false,
                fail_after_set: false,
            },
        }
    }
    #[test]
    fn dump_parser_is_strict_and_ignores_comments() {
        assert_eq!(
            parse_dump("# timestamps=disabled\nset global rss=enabled timestamps=allowed\n")
                .unwrap(),
            Setting::Allowed
        );
        for input in [
            "",
            "set global timestamps=other",
            "set global timestamps=enabled timestamps=allowed",
        ] {
            assert!(parse_dump(input).is_err());
        }
    }
    #[test]
    fn detects_only_exact_ts_fooling() {
        for input in [
            "--dpi-desync-fooling=ts",
            "--dpi-desync-fooling=md5sig,ts",
            "--dup-fooling=ts",
        ] {
            assert!(config_requires_timestamps(input));
        }
        for input in [
            "--dpi-desync-ts-increment=-30000",
            "--dpi-desync-fooling=badseq",
            "# --dpi-desync-fooling=ts",
            "--dpi-desync-fooling=notts",
        ] {
            assert!(!config_requires_timestamps(input));
        }
        assert!(config_requires_timestamps(include_str!(
            "../../src-tauri/resources/configs/discord/discord_12.conf"
        )));
    }
    #[test]
    fn restores_original_allowed_and_disabled() {
        for previous in [Setting::Allowed, Setting::Disabled] {
            let mut s = session(previous);
            s.acquire(true).unwrap();
            assert_eq!(s.machine.value, Setting::Enabled);
            assert_eq!(s.machine.journal, Some(previous));
            s.release().unwrap();
            assert_eq!(s.machine.value, previous);
            assert_eq!(s.machine.journal, None);
            s.release().unwrap();
            assert_eq!(s.machine.writes.len(), 2);
        }
    }
    #[test]
    fn non_ts_and_already_enabled_do_not_mutate() {
        for (previous, required) in [(Setting::Allowed, false), (Setting::Enabled, true)] {
            let mut s = session(previous);
            s.acquire(required).unwrap();
            s.release().unwrap();
            assert_eq!(s.machine.value, previous);
            assert!(s.machine.writes.is_empty());
            assert!(s.machine.journal.is_none());
        }
    }
    #[test]
    fn startup_recovers_crash_journal() {
        let mut s = session(Setting::Enabled);
        s.machine.journal = Some(Setting::Allowed);
        s.recover().unwrap();
        assert_eq!(s.machine.value, Setting::Allowed);
        assert!(s.machine.journal.is_none());
    }
    #[test]
    fn journal_failure_prevents_mutation() {
        let mut s = session(Setting::Allowed);
        s.machine.fail_save = true;
        assert!(s.acquire(true).is_err());
        assert_eq!(s.machine.value, Setting::Allowed);
        assert!(s.machine.writes.is_empty());
    }
    #[test]
    fn failure_after_mutation_rolls_back() {
        let mut s = session(Setting::Allowed);
        s.machine.fail_after_set = true;
        assert!(s.acquire(true).is_err());
        assert_eq!(s.machine.value, Setting::Allowed);
        assert!(s.machine.journal.is_none());
        assert!(!s.active);
    }
    #[test]
    fn restore_failure_preserves_journal_for_retry() {
        let mut s = session(Setting::Disabled);
        s.acquire(true).unwrap();
        s.machine.fail_set = true;
        assert!(s.release().is_err());
        assert_eq!(s.machine.journal, Some(Setting::Disabled));
        assert!(s.acquire(false).is_err());
        s.machine.fail_set = false;
        s.recover().unwrap();
        assert_eq!(s.machine.value, Setting::Disabled);
    }
    #[test]
    fn external_change_is_preserved() {
        let mut s = session(Setting::Allowed);
        s.acquire(true).unwrap();
        s.machine.value = Setting::Disabled;
        s.release().unwrap();
        assert_eq!(s.machine.value, Setting::Disabled);
        assert!(s.machine.journal.is_none());
    }
    #[test]
    fn concurrent_acquisition_is_rejected_and_switch_restores() {
        let mut s = session(Setting::Allowed);
        s.acquire(true).unwrap();
        assert!(s.acquire(true).is_err());
        s.release().unwrap();
        s.acquire(false).unwrap();
        assert_eq!(s.machine.value, Setting::Allowed);
        s.release().unwrap();
    }
}
