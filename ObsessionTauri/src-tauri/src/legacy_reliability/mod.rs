//! Typed contracts for the Legacy reliability pipeline.

// Phase 1 builds the contracts before the production runtime is wired to them.
#![allow(dead_code)]

pub mod adapter;
pub mod contracts;
pub mod health;
pub mod ingress;
pub mod manager;
pub mod registry_loader;
pub mod runtime;
pub mod status;
pub mod target_registry;
