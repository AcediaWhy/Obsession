//! Zapret2 diagnostics own a receive-only sensor, never a Legacy controller.
#![cfg(windows)]
use crate::protected_layout::{VerifiedResource, VerifiedRuntimeCatalog};
use obsession_runtime_protocol::{DpiEngine, DpiPacketCapture, DpiPacketCaptureState};
use obsession_runtime_reliability::{
    eyes::{capture::start_packet_diagnostics, diagnostic::PacketTracker, EyesHandle},
    legacy_reliability::health::AtomicHealthCounters,
    EyesTeardownTicket,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) struct PacketObserver {
    dll: Option<VerifiedResource>,
    generation: u64,
    state: DpiPacketCaptureState,
    startup_error: Option<String>,
    tracker: Arc<Mutex<PacketTracker>>,
    health: Arc<AtomicHealthCounters>,
    active: Option<EyesHandle>,
    pending: Option<EyesTeardownTicket>,
    startup_unresolved: bool,
    deadline: Option<Instant>,
}

impl PacketObserver {
    pub(crate) fn from_catalog(catalog: &VerifiedRuntimeCatalog) -> Self {
        let dll = catalog.engine(DpiEngine::Zapret2).and_then(|engine| {
            engine
                .strategies()
                .iter()
                .flat_map(|strategy| strategy.dependency_resources())
                .find(|resource| {
                    resource.relative_path().file_name().is_some_and(|name| {
                        name.to_string_lossy().eq_ignore_ascii_case("WinDivert.dll")
                    })
                })
                .cloned()
        });
        Self {
            dll,
            generation: 0,
            state: DpiPacketCaptureState::Unavailable,
            startup_error: None,
            tracker: Arc::default(),
            health: Arc::default(),
            active: None,
            pending: None,
            startup_unresolved: false,
            deadline: None,
        }
    }
    pub(crate) fn available(&self) -> bool {
        self.dll.is_some()
    }

    pub(crate) fn start(&mut self, generation: u64, requested: bool) {
        // Start is only called after the executor has started this exact runtime.
        // Failure degrades diagnostics and must not roll back a healthy engine.
        self.generation = if requested { generation } else { 0 };
        if !requested {
            return;
        }
        self.tracker = Arc::default();
        self.health = Arc::default();
        self.startup_error = None;
        if let Some(pending) = self.pending.as_mut() {
            if pending.wait_bounded(Duration::ZERO).resolved {
                self.pending = None;
            }
        }
        if self.pending.is_some() || self.startup_unresolved || self.active.is_some() {
            self.state = DpiPacketCaptureState::TeardownPending;
            return;
        }
        let Some(dll) = &self.dll else {
            self.state = DpiPacketCaptureState::Unavailable;
            return;
        };
        if dll.verify().is_err() {
            self.state = DpiPacketCaptureState::Unavailable;
            return;
        }
        match start_packet_diagnostics(dll.path(), self.tracker.clone(), self.health.clone()) {
            Ok(handle) => {
                self.active = Some(handle);
                self.state = DpiPacketCaptureState::Active;
                self.deadline = Some(Instant::now() + Duration::from_secs(300));
            }
            Err(error) => {
                let mut message = error
                    .to_string()
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .collect::<String>();
                while message.len() > 512 {
                    message.pop();
                }
                self.startup_error = Some(message);
                let retry_safe = error.safe_to_retry();
                self.pending = EyesTeardownTicket::from_start_error(error);
                self.startup_unresolved = !retry_safe && self.pending.is_none();
                self.state = DpiPacketCaptureState::StartupFailed;
            }
        }
    }

    pub(crate) fn stop(&mut self, generation: u64) {
        if self.generation != generation {
            return;
        }
        if let Some(handle) = self.active.take() {
            let mut ticket = EyesTeardownTicket::begin(handle);
            if !ticket.wait_bounded(Duration::from_millis(250)).resolved {
                self.pending = Some(ticket);
            }
        }
        self.generation = 0;
        self.deadline = None;
    }

    pub(crate) fn poll(&mut self, engine_exited: bool) {
        if let Some(pending) = self.pending.as_mut() {
            if pending.wait_bounded(Duration::ZERO).resolved {
                self.pending = None;
            }
        }
        if self.active.is_some()
            && (engine_exited
                || self
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline))
        {
            let generation = self.generation;
            self.stop(generation);
            self.generation = generation;
            self.state = if engine_exited {
                DpiPacketCaptureState::EngineExited
            } else {
                DpiPacketCaptureState::Expired
            };
        }
    }

    pub(crate) fn snapshot(&self, generation: u64) -> Option<DpiPacketCapture> {
        if self.generation != generation || generation == 0 {
            return None;
        }
        let health = self.health.snapshot();
        let tracker = self
            .tracker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(DpiPacketCapture {
            generation,
            state: self.state,
            startup_error: self.startup_error.clone(),
            captured_packets: health.counters.packet_count,
            queue_drops: health.counters.queue_drops,
            parse_errors: health.counters.parse_errors,
            sensor_failed: health.sensor_failed,
            capacity_evictions: tracker.capacity_evictions,
            flows: tracker.snapshot(),
        })
    }
}

impl Drop for PacketObserver {
    fn drop(&mut self) {
        self.stop(self.generation);
    }
}
