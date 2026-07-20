//! Typed contracts for the Legacy reliability pipeline.

// Some forward-compatible Phase 3/4 contract variants are intentionally not
// consumed by the observe-only Phase 2 runtime yet.
#![allow(dead_code)]

pub mod adapter;
pub mod assessment;
pub mod contracts;
pub mod environment_gate;
pub mod health;
pub mod ingress;
pub mod manager;
pub mod policy;
pub mod registry_loader;
pub mod reliability_log;
pub mod runtime;
pub mod status;
pub mod target_registry;
