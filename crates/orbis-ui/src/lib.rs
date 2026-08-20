//! # orbis-ui
//!
//! GUI на Slint (только представление). Аппаратный I/O и provider-вызовы здесь
//! не выполняются: команды уходят в application/worker слой.
//!
//! `worker` — независимый от Slint последовательный async-worker для команд
//! Performance Mode (инфраструктура, подключается к callbacks позже).

#![forbid(unsafe_code)]

pub mod automation_capability;
pub mod automation_execution_guard;
pub mod automation_execution_scope;
pub mod automation_lifecycle_revision;
#[cfg(test)]
mod automation_performance_executor;
pub mod automation_serialization;
pub mod automation_shadow_runtime;
pub mod automation_worker_runtime;
pub mod composition;
pub mod diagnostics_dto;
pub mod diagnostics_export;
pub mod diagnostics_metadata;
pub mod diagnostics_runtime;
pub mod diagnostics_window_model;
pub mod worker;