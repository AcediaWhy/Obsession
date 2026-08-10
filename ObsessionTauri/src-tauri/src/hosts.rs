//! Medium-integrity projection of the protected hosts controller.
//!
//! The UI process never receives a system path, payload, backup path or hash.
//! It can select only one protocol provider and receives sanitized status.

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::util;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Malw,
    Geohide,
}

impl Provider {
    pub fn parse(value: &str) -> Self {
        match value {
            "geohide" => Self::Geohide,
            _ => Self::Malw,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Malw => "malw",
            Self::Geohide => "geohide",
        }
    }

    fn protocol(self) -> obsession_runtime_protocol::HostsProvider {
        match self {
            Self::Malw => obsession_runtime_protocol::HostsProvider::Malw,
            Self::Geohide => obsession_runtime_protocol::HostsProvider::Geohide,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct HostsStatus {
    pub provider: String,
    /// "installed" | "not_installed" | "offline" | "external"
    pub status: String,
    pub local_version: String,
    pub remote_version: String,
    pub rollback_available: bool,
}

impl HostsStatus {
    fn unavailable(provider: Provider) -> Self {
        Self {
            provider: provider.name().to_owned(),
            status: "not_installed".to_owned(),
            local_version: String::new(),
            remote_version: String::new(),
            rollback_available: false,
        }
    }

    fn from_runtime(
        selected: Provider,
        runtime: obsession_runtime_protocol::HostsRuntimeSnapshot,
    ) -> Self {
        if runtime.provider != selected.protocol() {
            return Self::unavailable(selected);
        }
        let status = if runtime.externally_modified {
            "external"
        } else if runtime.installed {
            "installed"
        } else {
            "not_installed"
        };
        Self {
            provider: selected.name().to_owned(),
            status: status.to_owned(),
            local_version: runtime.local_version.unwrap_or_default(),
            // Remote payload checks are deliberately performed only during a
            // bounded service-owned install, never by the UI process.
            remote_version: String::new(),
            rollback_available: runtime.rollback_available,
        }
    }
}

pub async fn check_status(_app: &AppHandle, provider: Provider) -> HostsStatus {
    snapshot_status(_app, provider)
}

pub async fn install(app: &AppHandle, provider: Provider) -> Result<(), String> {
    util::emit_log(
        app,
        "info",
        "hosts",
        "Защищённая служба устанавливает hosts…",
    );
    crate::protected_runtime::hosts_install(provider.protocol()).await?;
    app.state::<AppState>().hosts_revision.bump();
    util::emit_log(
        app,
        "success",
        "hosts",
        "Hosts применён и проверен защищённой службой.",
    );
    Ok(())
}

pub async fn uninstall(app: &AppHandle) -> Result<(), String> {
    crate::protected_runtime::hosts_uninstall().await?;
    app.state::<AppState>().hosts_revision.bump();
    util::emit_log(
        app,
        "success",
        "hosts",
        "Исходный hosts восстановлен защищённой службой.",
    );
    Ok(())
}

pub async fn restore_last_known_good(app: &AppHandle, provider: Provider) -> Result<(), String> {
    crate::protected_runtime::hosts_restore(provider.protocol()).await?;
    app.state::<AppState>().hosts_revision.bump();
    util::emit_log(
        app,
        "success",
        "hosts",
        "Последняя защищённая рабочая версия hosts восстановлена.",
    );
    Ok(())
}

/// Fast service-owned snapshot for startup/resume. No network request occurs.
pub fn snapshot_status(_app: &AppHandle, provider: Provider) -> HostsStatus {
    crate::protected_runtime::hosts_runtime_snapshot()
        .ok()
        .flatten()
        .map(|snapshot| HostsStatus::from_runtime(provider, snapshot))
        .unwrap_or_else(|| HostsStatus::unavailable(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_projection_is_provider_scoped_and_surfaces_external_changes() {
        let runtime = obsession_runtime_protocol::HostsRuntimeSnapshot {
            provider: obsession_runtime_protocol::HostsProvider::Malw,
            installed: true,
            externally_modified: true,
            rollback_available: true,
            local_version: Some("2026-08-04".into()),
        };
        let status = HostsStatus::from_runtime(Provider::Malw, runtime.clone());
        assert_eq!(status.status, "external");
        assert!(status.rollback_available);
        assert_eq!(status.local_version, "2026-08-04");

        let other = HostsStatus::from_runtime(Provider::Geohide, runtime);
        assert_eq!(other.status, "not_installed");
        assert!(!other.rollback_available);
    }
}
