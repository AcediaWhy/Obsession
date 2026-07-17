use tauri::{AppHandle, Manager};

use super::dsl::StrategyCandidate;
use crate::state::{AppState, DpiRuntimeSnapshot};

#[derive(Debug)]
pub(super) struct CandidateStartOutcome {
    pub(super) result: Result<u64, String>,
    pub(super) generation_after: u64,
}

pub(super) async fn start(
    app: &AppHandle,
    original: &DpiRuntimeSnapshot,
    expected_generation: u64,
    category: &str,
    candidate: StrategyCandidate,
) -> CandidateStartOutcome {
    let (result, generation_after) = {
        let state = app.state::<AppState>();
        let _gate = state.dpi_gate.lock().await;
        let result = crate::dpi::start_adaptive_candidate_locked(
            app,
            original,
            expected_generation,
            category,
            candidate,
        )
        .await;
        let generation_after = crate::dpi::runtime_snapshot(app).generation;
        (result, generation_after)
    };
    CandidateStartOutcome {
        result,
        generation_after,
    }
}

pub(super) fn failed(app: &AppHandle, error: impl Into<String>) -> CandidateStartOutcome {
    CandidateStartOutcome {
        result: Err(error.into()),
        generation_after: crate::dpi::runtime_snapshot(app).generation,
    }
}
