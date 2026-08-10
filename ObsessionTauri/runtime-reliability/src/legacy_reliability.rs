//! Pure Legacy reliability modules shared with the Tauri crate.

#[path = "../../src-tauri/src/legacy_reliability/adapter.rs"]
pub mod adapter;
#[path = "../../src-tauri/src/legacy_reliability/assessment.rs"]
pub mod assessment;
#[path = "../../src-tauri/src/legacy_reliability/contracts.rs"]
pub mod contracts;
#[path = "../../src-tauri/src/legacy_reliability/environment_gate.rs"]
pub mod environment_gate;
#[path = "../../src-tauri/src/legacy_reliability/health.rs"]
pub mod health;
#[path = "../../src-tauri/src/legacy_reliability/ingress.rs"]
pub mod ingress;
#[path = "../../src-tauri/src/legacy_reliability/manager.rs"]
pub mod manager;
#[path = "../../src-tauri/src/legacy_reliability/policy.rs"]
pub mod policy;
#[path = "../../src-tauri/src/legacy_reliability/recovery.rs"]
pub mod recovery;
#[path = "../../src-tauri/src/legacy_reliability/recovery_runtime.rs"]
pub mod recovery_runtime;
#[path = "../../src-tauri/src/legacy_reliability/target_registry.rs"]
pub mod target_registry;
