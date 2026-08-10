//! Platform-neutral dispatch core for the privileged Obsession runtime.
//!
//! Windows named-pipe authentication and the SCM entry point wrap this crate.
//! The service exposes only preflighted protected DPI engines (Legacy and the
//! bundled Zapret2 Strategy Pack); any initialization failure selects a locked
//! backend with no privileged capabilities.

pub mod dpi_materializer;
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
pub mod named_pipe;

#[cfg(windows)]
pub mod scm;

use obsession_runtime_protocol::{
    Capabilities, DpiReplaceRequest, DpiStartRequest, DpiStopRequest, FirewallOpenProxyLanRequest,
    HostsMutationRequest, LegacyRecoveryApprovalRequest, LegacyRecoveryControlsRequest,
    OperationAccepted, Request, RequestEnvelope, Response, ResponseEnvelope, RuntimeSnapshot,
    RuntimeStarted, ServiceError, ServiceErrorCode, PROTOCOL_VERSION,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendError {
    Busy,
    Conflict,
    InvalidRequest,
    ProtectedResourceInvalid,
    RuntimeFailed,
    ServiceUnavailable,
    Internal,
}

impl BackendError {
    fn wire_code(self) -> ServiceErrorCode {
        match self {
            Self::Busy => ServiceErrorCode::Busy,
            Self::Conflict => ServiceErrorCode::Conflict,
            Self::InvalidRequest => ServiceErrorCode::InvalidRequest,
            Self::ProtectedResourceInvalid => ServiceErrorCode::ProtectedResourceInvalid,
            Self::RuntimeFailed => ServiceErrorCode::RuntimeFailed,
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
    fn hosts_install(
        &mut self,
        request: HostsMutationRequest,
    ) -> Result<OperationAccepted, BackendError>;
    fn hosts_uninstall(&mut self) -> Result<OperationAccepted, BackendError>;
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
            Request::HostsInstall(request) => {
                self.backend.hosts_install(request).map(Response::Accepted)
            }
            Request::HostsUninstall => self.backend.hosts_uninstall().map(Response::Accepted),
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
        result.unwrap_or_else(|error| error_response(error.wire_code()))
    }
}

fn error_response(code: ServiceErrorCode) -> Response {
    Response::Error(ServiceError { code })
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
                    zapret2_level: 0,
                    legacy_reliability: true,
                    zapret2_overrides: Vec::new(),
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
                })
            );
        }
    }

    #[derive(Default)]
    struct RecordingBackend {
        starts: Vec<DpiStartRequest>,
        controls: Vec<LegacyRecoveryControlsRequest>,
        approvals: Vec<LegacyRecoveryApprovalRequest>,
        hosts: Vec<HostsMutationRequest>,
        cleanup_sids: Vec<String>,
    }

    impl RuntimeBackend for RecordingBackend {
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
            Ok(RuntimeStarted { generation: 1 })
        }

        fn dpi_stop(&mut self, _request: DpiStopRequest) -> Result<(), BackendError> {
            Ok(())
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
    fn dispatcher_can_only_forward_typed_allowlisted_operations() {
        let mut service = ServiceCore::new(RecordingBackend::default());
        let response = service.handle(&allowed_client(), start_request());
        assert_eq!(
            response.response,
            Response::Started(RuntimeStarted { generation: 1 })
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
}
