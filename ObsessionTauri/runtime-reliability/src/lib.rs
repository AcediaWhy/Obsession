//! Shared, UI-independent reliability core for the protected runtime.
//!
//! The source modules below are the existing pure Eyes/Manager/recovery state
//! machines. Keeping one source of truth lets the LocalSystem service own the
//! runtime without linking the Tauri application or accepting UI-owned paths,
//! process identities, or command-line fragments.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod util {
    pub fn std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = std::process::Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
}

#[path = "../../src-tauri/src/dpi_supervisor.rs"]
mod dpi_supervisor;

/// Minimal engine identity required by the reliability contracts. The service
/// maps this enum to its own protected protocol enum at the boundary.
pub mod dpi_engine {
    use super::{Deserialize, Serialize};

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum EngineKind {
        #[default]
        Legacy,
        Zapret2,
    }
}

#[path = "../../src-tauri/src/eyes/mod.rs"]
pub mod eyes;

pub mod legacy_reliability;

#[path = "../../src-tauri/src/service_health.rs"]
pub mod service_health;

pub fn acknowledge_manager_publication(
    manager: &legacy_reliability::manager::ObserveOnlyManager,
    scope: &legacy_reliability::ingress::PendingIngressScope,
) {
    manager.acknowledge_published_ingress_event(scope);
}

#[cfg(windows)]
pub struct EyesTeardownTicket {
    inner: dpi_supervisor::WorkerTeardown,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EyesTeardownStatus {
    pub resolved: bool,
    pub clean: bool,
}

#[cfg(windows)]
impl EyesTeardownTicket {
    pub fn begin(handle: eyes::EyesHandle) -> Self {
        Self {
            inner: handle.begin_stop(),
        }
    }

    pub fn wait_bounded(&mut self, timeout: std::time::Duration) -> EyesTeardownStatus {
        let outcomes = self.inner.wait_bounded(timeout);
        EyesTeardownStatus {
            resolved: self.inner.is_resolved(),
            clean: outcomes.into_iter().all(|outcome| outcome.is_clean()),
        }
    }
}
