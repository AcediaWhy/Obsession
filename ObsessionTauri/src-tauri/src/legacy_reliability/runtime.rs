//! Async shell for the side-effect-free observe-only Manager.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;

use super::contracts::{LaneGeneration, LegacySessionContext, SensorGeneration};
use super::health::AtomicHealthCounters;
use super::ingress::{channel, LegacyIngress};
use super::manager::{ObserveOnlyManager, ObserveOnlySnapshot, ReceiveOutcome};
use super::target_registry::TargetRegistry;

const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Handle kept by AppState for one Legacy observe-only session.
pub struct LegacyReliabilityHandle {
    pub ingress: LegacyIngress,
    status: watch::Receiver<ObserveOnlySnapshot>,
    stop: watch::Sender<bool>,
    wake: Arc<Notify>,
    join: JoinHandle<()>,
}

impl LegacyReliabilityHandle {
    pub fn snapshot(&self) -> ObserveOnlySnapshot {
        self.status.borrow().clone()
    }

    /// Forwards health/lifecycle snapshots into the app-owned public status.
    /// The publisher performs exact session+sensor fencing and deduplicates the
    /// public projection, so frequent manager polls cannot create an event storm.
    pub fn forward_public_status(&self, app: AppHandle) {
        let mut status = self.status.clone();
        let owner = self.snapshot();
        let session_id = owner.session.session_id;
        let sensor_generation = owner.session.sensor_generation;
        let active_categories = owner.session.active_categories;
        tauri::async_runtime::spawn(async move {
            loop {
                match status.changed().await {
                    Ok(()) => {
                        let snapshot = status.borrow_and_update().clone();
                        super::status::publish_snapshot_if_owned(&app, &snapshot);
                    }
                    Err(_) => {
                        // A panic/abort can close the watch sender without a
                        // terminal snapshot. Normal stop already published
                        // inactive, so its old owner is rejected here.
                        super::status::publish_if_owned(
                            &app,
                            session_id,
                            sensor_generation,
                            super::status::LegacyReliabilityStatus::blind(
                                active_categories,
                                session_id,
                                sensor_generation,
                            ),
                        );
                        break;
                    }
                }
            }
        });
    }

    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        self.wake.notify_one();
        let mut join = self.join;
        if tokio::time::timeout(Duration::from_secs(1), &mut join)
            .await
            .is_err()
        {
            join.abort();
            let _ = join.await;
        }
    }
}

/// Creates an observe-only manager and its capture-side sender. No process,
/// cache, UI, or Brain action is performed by this function or its task.
pub fn spawn(
    context: LegacySessionContext,
    sensor_generation: SensorGeneration,
    registry: &TargetRegistry,
    lane_generations: BTreeMap<String, LaneGeneration>,
) -> Result<LegacyReliabilityHandle, super::ingress::FenceBuildError> {
    let (ingress, receiver) = channel();
    let counters: Arc<AtomicHealthCounters> = ingress.counters();
    let manager = ObserveOnlyManager::new(
        context,
        sensor_generation,
        registry.version(),
        lane_generations,
        counters,
        receiver,
    )?;
    let initial = manager.snapshot();
    let (status_tx, status_rx) = watch::channel(initial);
    let (stop, mut stop_rx) = watch::channel(false);
    let wake = Arc::new(Notify::new());
    let wake_task = Arc::clone(&wake);
    let join = tokio::spawn(async move {
        run_manager(manager, status_tx, &mut stop_rx, wake_task).await;
    });

    Ok(LegacyReliabilityHandle {
        ingress,
        status: status_rx,
        stop,
        wake,
        join,
    })
}

async fn run_manager(
    mut manager: ObserveOnlyManager,
    status_tx: watch::Sender<ObserveOnlySnapshot>,
    stop_rx: &mut watch::Receiver<bool>,
    wake: Arc<Notify>,
) {
    let started = Instant::now();
    loop {
        let now_ms = started.elapsed().as_millis() as u64;
        tokio::select! {
            biased;
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    manager.shutdown(started.elapsed().as_millis() as u64);
                    let _ = status_tx.send(manager.snapshot());
                    break;
                }
            }
            outcome = tokio::time::timeout(HEALTH_POLL_INTERVAL, manager.recv_next(now_ms)) => {
                match outcome {
                    Ok(ReceiveOutcome::Event { .. }) => {
                        let _ = status_tx.send(manager.snapshot());
                    }
                    Ok(ReceiveOutcome::ReceiverClosed | ReceiveOutcome::ManagerClosed) => {
                        let _ = status_tx.send(manager.snapshot());
                        break;
                    }
                    Err(_) => {
                        let now_ms = started.elapsed().as_millis() as u64;
                        let _ = status_tx.send(manager.poll(now_ms));
                    }
                }
            }
            _ = wake.notified() => {
                let now_ms = started.elapsed().as_millis() as u64;
                let _ = status_tx.send(manager.poll(now_ms));
            }
        }
    }
}
