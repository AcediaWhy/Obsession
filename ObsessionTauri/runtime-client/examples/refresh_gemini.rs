//! Opt-in targeted repair through the protected service; never edits hosts directly.
use obsession_runtime_client::RuntimeClient;
use obsession_runtime_protocol::{GeminiRoutePreference, Request, Response};
use std::{error::Error, time::Duration};

fn main() -> Result<(), Box<dyn Error>> {
    let client = RuntimeClient::new(Duration::from_secs(240))?;
    if std::env::args().any(|arg| arg == "--apply") {
        match client.call("gemini-scoped-refresh", Request::HostsRefreshGemini(GeminiRoutePreference::Auto))?.response {
            Response::Accepted(result) => println!("Repair accepted: {result:?}"),
            other => return Err(format!("Repair failed: {other:?}").into()),
        }
    }
    match client.call("gemini-scoped-status", Request::GetRuntimeSnapshot)?.response {
        Response::RuntimeSnapshot(snapshot) => println!("Hosts: {:?}", snapshot.hosts),
        other => return Err(format!("Status failed: {other:?}").into()),
    }
    Ok(())
}
