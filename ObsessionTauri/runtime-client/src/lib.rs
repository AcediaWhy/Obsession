//! Medium-integrity client for the Obsession privileged runtime.
//!
//! Opening a well-known pipe name is not sufficient authentication: hostile
//! same-user code can create that name before the real service starts. This
//! client therefore verifies the server PID, session, LocalSystem token and
//! protected image path before sending any request.

#[cfg(windows)]
mod windows_client;

#[cfg(windows)]
pub use windows_client::{ClientError, RuntimeClient};

#[cfg(not(windows))]
compile_error!("obsession-runtime-client supports Windows only");
