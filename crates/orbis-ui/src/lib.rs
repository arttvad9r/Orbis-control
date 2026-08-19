//! # orbis-ui
//!
//! GUI на Slint (только представление). Аппаратный I/O и provider-вызовы здесь
//! не выполняются: команды уходят в application/worker слой.
//!
//! `worker` — независимый от Slint последовательный async-worker для команд
//! Performance Mode (инфраструктура, подключается к callbacks позже).

#![forbid(unsafe_code)]

pub mod composition;
pub mod diagnostics_dto;
pub mod diagnostics_export;
pub mod diagnostics_metadata;
pub mod worker;
