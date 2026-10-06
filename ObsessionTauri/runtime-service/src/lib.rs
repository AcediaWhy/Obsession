//! Platform-neutral dispatch core for the privileged Obsession runtime.
//!
//! Windows named-pipe authentication and the SCM entry point wrap this crate.
//! The service exposes only preflighted protected DPI engines (Legacy and the
//! bundled Zapret2 Strategy Pack); any initialization failure selects a locked
//! backend with no privileged capabilities.

pub mod dpi_materializer;
#[cfg(windows)]
mod dpi_output;
mod legacy_pack;
#[cfg(windows)]
mod hosts_controller;
#[cfg(windows)]
mod legacy_confirmation;
#[cfg(windows)]
mod legacy_recovery_effects;
#[cfg(windows)]
mod legacy_recovery_preflight;
#[cfg(windows)]
mod legacy_recovery_transaction;
#[cfg(windows)]
mod legacy_reliability;
#[cfg(windows)]
mod packet_observer;
#[cfg(windows)]
mod network_identity;
pub mod protected_backend;
pub mod protected_layout;
#[cfg(windows)]
mod proxy_lan_firewall;
#[cfg(windows)]
mod zapret2_pack;

#[cfg(windows)]
mod legacy_access_activity;
#[cfg(windows)]
mod legacy_cleanup;

#[cfg(windows)]
pub mod dpi_executor;

#[cfg(windows)]
mod tcp_timestamps;
#[cfg(windows)]
mod dns_recovery;

#[cfg(windows)]
pub mod named_pipe;

#[cfg(windows)]
pub mod scm;

use obsession_runtime_protocol::{
    Capabilities, DpiDiagnostics, DpiReplaceRequest, DpiStartRequest, DpiStopRequest, FirewallOpenProxyLanRequest,
    HostsCheckRequest, HostsHealthSnapshot, HostsMutationRequest, LegacyRecoveryApprovalRequest,
    LegacyRecoveryControlsRequest, OperationAccepted, Request, RequestEnvelope, Response,
    ResponseEnvelope, RuntimeSnapshot, RuntimeStarted, ServiceError, ServiceErrorCode,
    SystemDnsRequest, SystemDnsStatus, PROTOCOL_VERSION,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientIntegrity {
    Medium,
    High,
    System,
    Service,
    Anonymous,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientIdentity {
    pub is_local: bool,
    pub session_id: u32,
    pub user_sid: String,
    pub integrity: ClientIntegrity,
}

impl ClientIdentity {
    fn is_allowed(&self) -> bool {
        self.is_local
            && self.session_id != 0
            && valid_user_sid_text(&self.user_sid)
            && matches!(
                self.integrity,
                ClientIntegrity::Medium | ClientIntegrity::High
            )
    }
}

fn valid_user_sid_text(value: &str) -> bool {
    if value.len() > 184 {
        return false;
    }
    let mut parts = value.split('-');
    if parts.next() != Some("S") || parts.next() != Some("1") {
        return false;
    }
    let numeric: Vec<_> = parts.collect();
    numeric.len() >= 2
        && numeric
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    Busy,
    Conflict,
    InvalidRequest,
    ProtectedResourceInvalid,
    ProtectedMaterializationFailed(obsession_runtime_protocol::RuntimeFailureDetail),
    RuntimeFailed,
    RuntimeProcessFailed(obsession_runtime_protocol::RuntimeFailureDetail),
    ReplacementRolledBack {
        code: ServiceErrorCode,
        detail: Option<obsession_runtime_protocol::RuntimeFailureDetail>,
        generation: u64,
    },
    ServiceUnavailable,
    Internal,
}

impl BackendError {
    fn wire_code(&self) -> ServiceErrorCode {
        match self {
            Self::Busy => ServiceErrorCode::Busy,
            Self::Conflict => ServiceErrorCode::Conflict,
            Self::InvalidRequest => ServiceErrorCode::InvalidRequest,
            Self::ProtectedResourceInvalid | Self::ProtectedMaterializationFailed(_) => ServiceErrorCode::ProtectedResourceInvalid,
            Self::RuntimeFailed | Self::RuntimeProcessFailed(_) => ServiceErrorCode::RuntimeFailed,
            Self::ReplacementRolledBack { code, .. } => *code,
            Self::ServiceUnavailable => ServiceErrorCode::ServiceUnavailable,
            Self::Internal => ServiceErrorCode::Internal,
        }
    }
}

pub trait RuntimeBackend {
    /// Bounded service-owned maintenance while the authenticated pipe is idle.
    /// Implementations must never wait for caller input or detach work past
    /// their own deadline.
    fn poll_background(&mut self) -> Result<(), BackendError> {
        Ok(())
    }

    fn capabilities(&self) -> Result<Capabilities, BackendError>;
    fn runtime_snapshot(&self) -> Result<RuntimeSnapshot, BackendError>;
    fn set_legacy_recovery_controls(
        &mut self,
        _request: LegacyRecoveryControlsRequest,
    ) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn approve_legacy_recovery(
        &mut self,
        _request: LegacyRecoveryApprovalRequest,
    ) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn dpi_start(&mut self, request: DpiStartRequest) -> Result<RuntimeStarted, BackendError>;
    fn dpi_replace(&mut self, _request: DpiReplaceRequest) -> Result<RuntimeStarted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn dpi_stop(&mut self, request: DpiStopRequest) -> Result<(), BackendError>;
    fn dpi_diagnostics(&self, _request: DpiStopRequest) -> Result<DpiDiagnostics, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn flush_system_dns(&mut self, _request: SystemDnsRequest) -> Result<SystemDnsStatus, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn repair_system_dns(&mut self, _request: SystemDnsRequest) -> Result<SystemDnsStatus, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn restore_system_dns(&mut self) -> Result<SystemDnsStatus, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn system_dns_status(&self) -> Result<SystemDnsStatus, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn hosts_install(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError>;
    fn hosts_check(
        &mut self,
        _request: HostsCheckRequest,
    ) -> Result<HostsHealthSnapshot, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn hosts_uninstall(&mut self) -> Result<OperationAccepted, BackendError>;
    fn hosts_refresh_gemini(&mut self, _preference: obsession_runtime_protocol::GeminiRoutePreference) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
    fn hosts_restore(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError>;
    fn firewall_open_proxy_lan(
        &mut self,
        request: FirewallOpenProxyLanRequest,
    ) -> Result<OperationAccepted, BackendError>;
    fn firewall_close_proxy_lan(&mut self) -> Result<(), BackendError>;
    fn subscribe_events(&mut self) -> Result<OperationAccepted, BackendError>;

    /// Removes the authenticated caller's fixed legacy per-user roots. The
    /// default remains available even when DPI preflight selected LockedBackend,
    /// because cleanup does not consume any mutable application resource.
    fn legacy_cleanup(&mut self, user_sid: &str) -> Result<(), BackendError> {
        #[cfg(windows)]
        {
            crate::legacy_cleanup::cleanup_authenticated_user(user_sid)
                .map_err(|error| error.backend_error())
        }
        #[cfg(not(windows))]
        {
            let _ = user_sid;
            Err(BackendError::ServiceUnavailable)
        }
    }
}

/// Fail-closed fallback used when production startup preflight cannot establish
/// a protected runtime. It remains queryable but exposes no privileged feature.
#[derive(Default)]
pub struct LockedBackend;

impl RuntimeBackend for LockedBackend {
    fn capabilities(&self) -> Result<Capabilities, BackendError> {
        Ok(Capabilities {
            service_version: env!("CARGO_PKG_VERSION").into(),
            features: Vec::new(),
        })
    }

    fn runtime_snapshot(&self) -> Result<RuntimeSnapshot, BackendError> {
        Ok(RuntimeSnapshot {
            dpi: None,
            legacy_reliability: None,
            legacy_access_activity: None,
            hosts: None,
            hosts_provider: None,
            proxy_lan: None,
        })
    }

    fn dpi_start(&mut self, _request: DpiStartRequest) -> Result<RuntimeStarted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn dpi_stop(&mut self, _request: DpiStopRequest) -> Result<(), BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn hosts_install(
        &mut self,
        _request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn hosts_check(
        &mut self,
        _request: HostsCheckRequest,
    ) -> Result<HostsHealthSnapshot, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn hosts_uninstall(&mut self) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn hosts_restore(
        &mut self,
        _request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn firewall_open_proxy_lan(
        &mut self,
        _request: FirewallOpenProxyLanRequest,
    ) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn firewall_close_proxy_lan(&mut self) -> Result<(), BackendError> {
        Err(BackendError::ServiceUnavailable)
    }

    fn subscribe_events(&mut self) -> Result<OperationAccepted, BackendError> {
        Err(BackendError::ServiceUnavailable)
    }
}

pub struct ServiceCore<B> {
    backend: B,
}

impl<B: RuntimeBackend> ServiceCore<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub fn handle(
        &mut self,
        client: &ClientIdentity,
        request: RequestEnvelope,
    ) -> ResponseEnvelope {
        let request_id = request.request_id.clone();
        let response = if request.validate().is_err() {
            error_response(ServiceErrorCode::InvalidRequest)
        } else if !client.is_allowed() {
            error_response(ServiceErrorCode::AccessDenied)
        } else {
            self.dispatch(client, request.request)
        };
        ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            response,
        }
    }

    pub(crate) fn poll_background(&mut self) {
        // Background recovery failure is reflected in backend-owned status;
        // it must not tear down the authenticated transport or SCM process.
        let _ = self.backend.poll_background();
    }

    fn dispatch(&mut self, client: &ClientIdentity, request: Request) -> Response {
        let result = match request {
            Request::GetCapabilities => self.backend.capabilities().map(Response::Capabilities),
            Request::GetRuntimeSnapshot => self
                .backend
                .runtime_snapshot()
                .map(Response::RuntimeSnapshot),
            Request::LegacyCleanup => self
                .backend
                .legacy_cleanup(&client.user_sid)
                .map(|()| Response::LegacyCleanupCompleted),
            Request::SetLegacyRecoveryControls(request) => self
                .backend
                .set_legacy_recovery_controls(request)
                .map(Response::Accepted),
            Request::ApproveLegacyRecovery(request) => self
                .backend
                .approve_legacy_recovery(request)
                .map(Response::Accepted),
            Request::DpiStart(request) => self.backend.dpi_start(request).map(Response::Started),
            Request::DpiReplace(request) => {
                self.backend.dpi_replace(request).map(Response::Started)
            }
            Request::DpiStop(request) => self.backend.dpi_stop(request).map(|()| Response::Stopped),
            Request::GetDpiDiagnostics(request) => self.backend.dpi_diagnostics(request).map(Response::DpiDiagnostics),
            // Keep old wire variants parseable, but deny even authenticated old
            // clients. Automatic DNS mutations are no longer supported.
            Request::FlushSystemDns(_) | Request::RepairSystemDns(_) => Err(BackendError::ServiceUnavailable),
            Request::RestoreSystemDns => self.backend.restore_system_dns().map(Response::SystemDns),
            Request::GetSystemDns => self.backend.system_dns_status().map(Response::SystemDns),
            Request::HostsInstall(request) => {
                self.backend.hosts_install(request).map(Response::Accepted)
            }
            Request::HostsCheck(request) => {
                self.backend.hosts_check(request).map(Response::HostsHealth)
            }
            Request::HostsUninstall => self.backend.hosts_uninstall().map(Response::Accepted),
            Request::HostsRefreshGemini(preference) => self.backend.hosts_refresh_gemini(preference).map(Response::Accepted),
            Request::HostsRestoreLastKnownGood(request) => {
                self.backend.hosts_restore(request).map(Response::Accepted)
            }
            Request::FirewallOpenProxyLan(request) => self
                .backend
                .firewall_open_proxy_lan(request)
                .map(Response::Accepted),
            Request::FirewallCloseProxyLan => self
                .backend
                .firewall_close_proxy_lan()
                .map(|()| Response::Stopped),
            Request::SubscribeEvents => self.backend.subscribe_events().map(Response::Accepted),
        };
        result.unwrap_or_else(|error| Response::Error(ServiceError {
            code: error.wire_code(),
            runtime_detail: match &error {
                BackendError::RuntimeProcessFailed(detail) | BackendError::ProtectedMaterializationFailed(detail) => Some(detail.clone()),
                BackendError::ReplacementRolledBack { detail, .. } => detail.clone(),
                _ => None,
            },
            restored_generation: match error { BackendError::ReplacementRolledBack { generation, .. } => Some(generation), _ => None },
        }))
    }
}

fn error_response(code: ServiceErrorCode) -> Response {
    Response::Error(ServiceError { code, runtime_detail: None, restored_generation: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use obsession_runtime_protocol::{
        DpiCategory, DpiEngine, DpiRuntimeOptions, DpiSelection, Feature, HostsProvider,
    };

    fn allowed_client() -> ClientIdentity {
        ClientIdentity {
            is_local: true,
            session_id: 1,
            user_sid: "S-1-5-21-1000".into(),
            integrity: ClientIntegrity::Medium,
        }
    }

    fn start_request() -> RequestEnvelope {
        RequestEnvelope::new(
            "start-1",
            Request::DpiStart(DpiStartRequest {
                engine: DpiEngine::Legacy,
                selections: vec![DpiSelection {
                    category: DpiCategory::Discord,
                    strategy_id: "discord_1".into(),
                }],
                options: DpiRuntimeOptions {
                    zapret2_level: 0, zapret2_clienthello: false, zapret2_profiles: Vec::new(),
                    legacy_reliability: true,
                    zapret2_overrides: Vec::new(),
                    zapret2_diagnostics: false,
                },
            }),
        )
    }

    #[test]
    fn locked_service_exposes_status_but_no_privileged_features() {
        let mut service = ServiceCore::new(LockedBackend);
        let response = service.handle(
            &allowed_client(),
            RequestEnvelope::new("cap-1", Request::GetCapabilities),
        );
        assert_eq!(
            response.response,
            Response::Capabilities(Capabilities {
                service_version: "0.1.0".into(),
                features: vec![],
            })
        );

        let response = service.handle(&allowed_client(), start_request());
        assert_eq!(
            response.response,
            Response::Error(ServiceError {
                code: ServiceErrorCode::ServiceUnavailable,
                runtime_detail: None,
                restored_generation: None,
            })
        );
    }

    #[test]
    fn remote_session_zero_and_service_clients_are_denied_before_dispatch() {
        let denied = [
            ClientIdentity {
                is_local: false,
                ..allowed_client()
            },
            ClientIdentity {
                session_id: 0,
                ..allowed_client()
            },
            ClientIdentity {
                integrity: ClientIntegrity::System,
                ..allowed_client()
            },
            ClientIdentity {
                user_sid: "invalid".into(),
                ..allowed_client()
            },
        ];
        for client in denied {
            let mut service = ServiceCore::new(LockedBackend);
            let response = service.handle(
                &client,
                RequestEnvelope::new("cap-1", Request::GetCapabilities),
            );
            assert_eq!(
                response.response,
                Response::Error(ServiceError {
                    code: ServiceErrorCode::AccessDenied,
                    runtime_detail: None,
                    restored_generation: None,
                })
            );
        }
    }

    #[derive(Default)]
    struct RecordingBackend {
        starts: Vec<DpiStartRequest>,
        replace_error: Option<BackendError>,
        controls: Vec<LegacyRecoveryControlsRequest>,
        approvals: Vec<LegacyRecoveryApprovalRequest>,
        hosts: Vec<HostsMutationRequest>,
        cleanup_sids: Vec<String>,
        dns_generations: Vec<u64>,
    }

    impl RuntimeBackend for RecordingBackend {
        fn repair_system_dns(&mut self, request: SystemDnsRequest) -> Result<SystemDnsStatus, BackendError> {
            self.dns_generations.push(request.generation);
            Ok(SystemDnsStatus::default())
        }
        fn capabilities(&self) -> Result<Capabilities, BackendError> {
            Ok(Capabilities {
                service_version: "1.0.0".into(),
                features: vec![Feature::Dpi, Feature::Hosts],
            })
        }

        fn runtime_snapshot(&self) -> Result<RuntimeSnapshot, BackendError> {
            Ok(RuntimeSnapshot {
                dpi: None,
                legacy_reliability: None,
                legacy_access_activity: None,
                hosts: None,
                hosts_provider: None,
                proxy_lan: None,
            })
        }

        fn set_legacy_recovery_controls(
            &mut self,
            request: LegacyRecoveryControlsRequest,
        ) -> Result<OperationAccepted, BackendError> {
            self.controls.push(request);
            Ok(OperationAccepted { operation_id: 7 })
        }

        fn approve_legacy_recovery(
            &mut self,
            request: LegacyRecoveryApprovalRequest,
        ) -> Result<OperationAccepted, BackendError> {
            self.approvals.push(request);
            Ok(OperationAccepted { operation_id: 11 })
        }

        fn dpi_start(&mut self, request: DpiStartRequest) -> Result<RuntimeStarted, BackendError> {
            self.starts.push(request);
            Ok(RuntimeStarted { generation: 1, materialized_fingerprint: None, initialization_retries: 0 })
        }

        fn dpi_stop(&mut self, _request: DpiStopRequest) -> Result<(), BackendError> {
            Ok(())
        }

        fn dpi_replace(&mut self, _request: DpiReplaceRequest) -> Result<RuntimeStarted, BackendError> {
            Err(self.replace_error.clone().unwrap_or(BackendError::ServiceUnavailable))
        }

        fn hosts_install(
            &mut self,
            request: HostsMutationRequest,
        ) -> Result<OperationAccepted, BackendError> {
            self.hosts.push(request);
            Ok(OperationAccepted { operation_id: 1 })
        }

        fn hosts_uninstall(&mut self) -> Result<OperationAccepted, BackendError> {
            Ok(OperationAccepted { operation_id: 2 })
        }

        fn hosts_restore(
            &mut self,
            request: HostsMutationRequest,
        ) -> Result<OperationAccepted, BackendError> {
            self.hosts.push(request);
            Ok(OperationAccepted { operation_id: 3 })
        }

        fn firewall_open_proxy_lan(
            &mut self,
            _request: FirewallOpenProxyLanRequest,
        ) -> Result<OperationAccepted, BackendError> {
            Ok(OperationAccepted { operation_id: 4 })
        }

        fn firewall_close_proxy_lan(&mut self) -> Result<(), BackendError> {
            Ok(())
        }

        fn subscribe_events(&mut self) -> Result<OperationAccepted, BackendError> {
            Ok(OperationAccepted { operation_id: 5 })
        }

        fn legacy_cleanup(&mut self, user_sid: &str) -> Result<(), BackendError> {
            self.cleanup_sids.push(user_sid.to_owned());
            Ok(())
        }
    }

    #[test]
    fn dispatcher_preserves_the_failed_replace_receipt_and_native_detail() {
        let detail = obsession_runtime_protocol::RuntimeFailureDetail::StartupFailed { exit_code: 87,
            diagnostics: obsession_runtime_protocol::DpiProcessDiagnostics {
                pid: 42, exit_code: Some(87), trace_enabled: true, stream_closed: true,
                observed_bytes: 40, discarded_lines: 0, truncated: false,
                lines: vec!["LUA ERROR: startup fixture".into()],
            } };
        let mut service = ServiceCore::new(RecordingBackend {
            replace_error: Some(BackendError::ReplacementRolledBack {
                code: ServiceErrorCode::RuntimeFailed, detail: Some(detail.clone()), generation: 5,
            }),
            ..Default::default()
        });
        let Request::DpiStart(mut runtime) = start_request().request else { unreachable!() };
        runtime.engine = DpiEngine::Zapret2;
        runtime.options.legacy_reliability = false;
        runtime.selections[0].strategy_id = "builtin-discord".into();
        let response = service.handle(&allowed_client(), RequestEnvelope::new(
            "replace-1", Request::DpiReplace(DpiReplaceRequest { expected_generation: 3, runtime }),
        ));
        assert_eq!(response.response, Response::Error(ServiceError {
            code: ServiceErrorCode::RuntimeFailed, runtime_detail: Some(detail), restored_generation: Some(5),
        }));
        response.validate().unwrap();
    }

    #[test]
    fn dispatcher_can_only_forward_typed_allowlisted_operations() {
        let mut service = ServiceCore::new(RecordingBackend::default());
        let response = service.handle(&allowed_client(), start_request());
        assert_eq!(
            response.response,
            Response::Started(RuntimeStarted { generation: 1, materialized_fingerprint: None, initialization_retries: 0 })
        );
        assert_eq!(service.backend.starts.len(), 1);
        assert_eq!(
            service.backend.starts[0].selections[0].strategy_id,
            "discord_1"
        );

        let controls = LegacyRecoveryControlsRequest {
            mode: obsession_runtime_protocol::LegacyRecoveryMode::Automatic,
            automatic_paused: false,
            frozen_categories: vec![DpiCategory::Discord],
        };
        let response = service.handle(
            &allowed_client(),
            RequestEnvelope::new(
                "legacy-controls-1",
                Request::SetLegacyRecoveryControls(controls.clone()),
            ),
        );
        assert_eq!(
            response.response,
            Response::Accepted(OperationAccepted { operation_id: 7 })
        );
        assert_eq!(service.backend.controls, [controls]);

        let approval = LegacyRecoveryApprovalRequest {
            proposal_id: 11,
            attempt_id: 13,
        };
        let response = service.handle(
            &allowed_client(),
            RequestEnvelope::new(
                "legacy-approval-1",
                Request::ApproveLegacyRecovery(approval),
            ),
        );
        assert_eq!(
            response.response,
            Response::Accepted(OperationAccepted { operation_id: 11 })
        );
        assert_eq!(service.backend.approvals, [approval]);

        let response = service.handle(
            &allowed_client(),
            RequestEnvelope::new(
                "hosts-1",
                Request::HostsInstall(HostsMutationRequest {
                    provider: HostsProvider::Malw,
                }),
            ),
        );
        assert_eq!(
            response.response,
            Response::Accepted(OperationAccepted { operation_id: 1 })
        );
        assert_eq!(service.backend.hosts.len(), 1);

        let response = service.handle(
            &allowed_client(),
            RequestEnvelope::new("cleanup-1", Request::LegacyCleanup),
        );
        assert_eq!(response.response, Response::LegacyCleanupCompleted);
        assert_eq!(service.backend.cleanup_sids, ["S-1-5-21-1000"]);
    }

    #[test]
    fn client_sid_text_cannot_inject_a_profile_registry_subkey() {
        for sid in [
            r"S-1-5-21-1000\\..\\S-1-5-18",
            "S-1-5/21/1000",
            "S-1--5",
            "S-2-5-21-1000",
            "S-1-",
        ] {
            assert!(!valid_user_sid_text(sid), "must reject {sid:?}");
        }
        assert!(valid_user_sid_text("S-1-5-21-1000"));
    }

    #[test]
    fn old_dns_mutations_are_denied_without_reaching_the_backend() {
        let mut service = ServiceCore::new(RecordingBackend::default());
        for (client, generation, expected) in [
            (ClientIdentity { is_local: false, ..allowed_client() }, 7, ServiceErrorCode::AccessDenied),
            (allowed_client(), 0, ServiceErrorCode::InvalidRequest),
        ] {
            let response = service.handle(&client, RequestEnvelope::new("dns-auth", Request::RepairSystemDns(SystemDnsRequest { generation })));
            assert!(matches!(response.response, Response::Error(ref error) if error.code == expected));
            assert!(service.backend.dns_generations.is_empty());
        }
        for request in [Request::RepairSystemDns(SystemDnsRequest { generation: 7 }), Request::FlushSystemDns(SystemDnsRequest { generation: 7 })] {
            let response = service.handle(&allowed_client(), RequestEnvelope::new("dns-valid", request));
            assert!(matches!(response.response, Response::Error(ref error) if error.code == ServiceErrorCode::ServiceUnavailable));
            assert!(service.backend.dns_generations.is_empty());
        }
    }
}
