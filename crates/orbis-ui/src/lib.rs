//! # Orbis UI
//!
//! Slint presentation plus application/runtime coordination for Orbis Control.
//! Hardware mutation remains owned by typed application/provider boundaries and
//! the sequential worker; UI-facing modules must not invent support state.

#![forbid(unsafe_code)]

pub mod composition;
pub mod diagnostics_dto;
pub mod diagnostics_export;
pub mod diagnostics_metadata;
pub mod diagnostics_runtime;
pub mod diagnostics_window_model;
pub mod product_mutation_promotion;
#[path = "worker_runtime.rs"]
pub mod worker;
