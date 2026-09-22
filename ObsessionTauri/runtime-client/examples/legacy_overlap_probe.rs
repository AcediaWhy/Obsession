//! Opt-in, generation-fenced A/B check. Never installs/updates a service.
use obsession_runtime_client::RuntimeClient;
use obsession_runtime_protocol::*;
use std::os::windows::process::CommandExt;
use std::{error::Error, process::Command, time::Duration};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn snapshot(client: &RuntimeClient) -> Result<RuntimeSnapshot> {
    match client
        .call("overlap-snapshot", Request::GetRuntimeSnapshot)?
        .response
    {
        Response::RuntimeSnapshot(value) => Ok(value),
        other => Err(format!("snapshot failed: {other:?}").into()),
    }
}

fn stop(client: &RuntimeClient, generation: u64) -> Result<()> {
    match client
        .call(
            "overlap-stop",
            Request::DpiStop(DpiStopRequest { generation }),
        )?
        .response
    {
        Response::Stopped => Ok(()),
        other => Err(format!("stop failed: {other:?}").into()),
    }
}

fn start(client: &RuntimeClient, selections: Vec<DpiSelection>, reliability: bool) -> Result<u64> {
    let request = DpiStartRequest {
        engine: DpiEngine::Legacy,
        selections,
        options: DpiRuntimeOptions {
            zapret2_level: 0,
            legacy_reliability: reliability,
            zapret2_overrides: vec![],
        },
    };
    match client
        .call("overlap-start", Request::DpiStart(request))?
        .response
    {
        Response::Started(value) => Ok(value.generation),
        other => Err(format!("start failed: {other:?}").into()),
    }
}

fn probe(label: &str) -> Result<()> {
    for url in [
        "https://discord.com/api/v10/gateway",
        "https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64",
    ] {
        let output = Command::new("curl.exe").creation_flags(0x08000000).args([
            "--noproxy", "*", "-sS", "-o", "NUL", "--connect-timeout", "4", "--max-time", "8",
            "-w", "HTTP=%{http_code} TLS=%{time_appconnect} total=%{time_total}", url,
        ]).output()?;
        println!("{label} {url}: {} exit={} {}", String::from_utf8_lossy(&output.stdout), output.status,
            String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

fn main() -> Result<()> {
    let client = RuntimeClient::new(Duration::from_secs(10))?;
    let mut before = snapshot(&client)?;
    println!("original: {:?}", before.dpi);
    if !std::env::args().any(|arg| arg == "--run-ab-test") {
        return Ok(());
    }
    let controls = before
        .legacy_reliability
        .as_ref()
        .map(|r| r.recovery.controls.clone());
    let _restore_controls = if let Some(controls) = controls {
        let response = client.call(
            "overlap-pause",
            Request::SetLegacyRecoveryControls(LegacyRecoveryControlsRequest {
                mode: controls.mode,
                automatic_paused: true,
                frozen_categories: controls.frozen_categories.clone(),
            }),
        )?;
        if !matches!(response.response, Response::Accepted(_)) {
            return Err("Could not pause automatic recovery".into());
        }
        let guard = RestoreControls {
            client: &client,
            controls,
        };
        before = snapshot(&client)?;
        Some(guard)
    } else {
        None
    };
    let original = before.dpi.ok_or("No active runtime")?;
    let categories: std::collections::BTreeSet<_> =
        original.selections.iter().map(|s| s.category).collect();
    if original.engine != DpiEngine::Legacy
        || categories
            != [DpiCategory::Discord, DpiCategory::YoutubeTwitch]
                .into_iter()
                .collect()
    {
        return Err("Expected exactly Discord + YouTube on Legacy".into());
    }
    let reliability = before.legacy_reliability.is_some();
    let discord = original
        .selections
        .iter()
        .filter(|s| s.category == DpiCategory::Discord)
        .cloned()
        .collect();
    probe("both-before")?;
    stop(&client, original.generation)?;
    let mut test_generation = None;
    let test_result: Result<()> = (|| {
        let generation = start(&client, discord, reliability)?;
        test_generation = Some(generation);
        std::thread::sleep(Duration::from_millis(900));
        probe("discord-only")
    })();
    // Always restore after a probe/start error. Never stop a generation that
    // the user or another operation started during the test.
    if let Some(current) = snapshot(&client)?.dpi {
        if Some(current.generation) != test_generation {
            return Err("Runtime changed externally; refusing to stop another generation".into());
        }
        stop(&client, current.generation)?;
    }
    let restored = start(&client, original.selections.clone(), reliability)?;
    let after = snapshot(&client)?.dpi.ok_or("Restore missing")?;
    if after.generation != restored || after.selections != original.selections {
        return Err("Restore mismatch".into());
    }
    println!("RESTORED original Discord + YouTube selections; generation {restored}");
    test_result?;
    probe("both-restored")
}

struct RestoreControls<'a> {
    client: &'a RuntimeClient,
    controls: LegacyRecoveryControlsSnapshot,
}
impl Drop for RestoreControls<'_> {
    fn drop(&mut self) {
        let result = self.client.call(
            "overlap-restore-controls",
            Request::SetLegacyRecoveryControls(LegacyRecoveryControlsRequest {
                mode: self.controls.mode,
                automatic_paused: self.controls.automatic_paused,
                frozen_categories: self.controls.frozen_categories.clone(),
            }),
        );
        match result {
            Ok(response) if matches!(response.response, Response::Accepted(_)) => {
                println!("RESTORED recovery controls")
            }
            other => eprintln!("RECOVERY CONTROL RESTORE FAILED: {other:?}"),
        }
    }
}
