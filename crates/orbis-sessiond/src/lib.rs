//! # orbis-sessiond
//!
//! Пользовательский демон (user session daemon) и системные адаптеры.
//!
//! На текущем этапе реализованы:
//! - read-only UPower adapter для Battery Charge Limit (`upower`);
//! - read-only D-Bus service object (`service`), выдающий Charge Limit через
//!   интерфейс `io.github.orbiscontrol.Session1`.
//!
//! Bus bootstrap (Connection/ObjectServer/registration) и запуск демона
//! наполняются отдельными микрошагами.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod service;
pub mod upower;
