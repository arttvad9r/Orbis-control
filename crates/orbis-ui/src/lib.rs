//! # Orbis UI
//!
//! Slint presentation plus application/runtime coordination for Orbis Control.
//! Hardware mutation remains owned by typed application/provider boundaries and
//! the sequential worker; UI-facing modules must not invent support state.

#![forbid(unsafe_code)]

pub mod automation_capability;
pub mod automation_execution_guard;
pub mod automation_execution_promotion;
pub mod automation_execution_scope;
pub mod automation_lifecycle_revision;
mod automation_performance_executor;
pub mod automation_policy_sync;
pub mod automation_recovery;
pub mod automation_registry;
pub mod automation_retry;
pub mod automation_serialization;
pub mod automation_shadow_runtime;
pub mod automation_worker_coordinator;
pub mod automation_worker_driver;
pub mod automation_worker_runtime;
pub mod composition;
pub mod diagnostics_dto;
pub mod diagnostics_export;
pub mod diagnostics_metadata;
pub mod diagnostics_runtime;
pub mod diagnostics_window_model;
pub mod display_refresh_service;
pub mod product_mutation_promotion;
#[path = "worker_runtime.rs"]
pub mod worker;
