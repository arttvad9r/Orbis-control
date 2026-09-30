//! # Orbis UI
//!
//! Slint presentation plus application/runtime coordination for Orbis Control.
//! Hardware mutation remains owned by typed application/provider boundaries and
//! the sequential worker; UI-facing modules must not invent support state.

#![forbid(unsafe_code)]

pub mod composition;
pub mod cpu_tuning_runtime;
pub mod diagnostics_dto;
pub mod diagnostics_export;
pub mod diagnostics_metadata;
pub mod diagnostics_runtime;
pub mod diagnostics_window_model;
pub mod display_refresh_service;
pub mod keyboard_timeout_runtime;
pub mod nvidia_tuning_runtime;
pub mod power_rules_runtime;
pub mod product_mutation_promotion;
pub mod profile_limits_runtime;
pub mod sparkline;
pub mod system_summary;
pub mod update_check;
#[path = "worker_runtime.rs"]
pub mod worker;
