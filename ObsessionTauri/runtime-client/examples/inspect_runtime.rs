//! Read-only installed runtime inspection. No start/replace/stop request exists.
use obsession_runtime_client::RuntimeClient;
use obsession_runtime_protocol::{Request, Response};
use std::{error::Error, time::Duration};

fn main() -> Result<(), Box<dyn Error>> {
    let client = RuntimeClient::new(Duration::from_secs(3))?;
    match client.call("zapret2-read-only-inspection", Request::GetRuntimeSnapshot)?.response {
        Response::RuntimeSnapshot(snapshot) => println!("DPI: {:?}", snapshot.dpi),
        other => return Err(format!("Runtime inspection failed: {other:?}").into()),
    }
    Ok(())
}
