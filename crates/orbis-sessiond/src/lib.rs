//! # orbis-sessiond
//!
//! Пользовательский демон (user session daemon) и системные адаптеры.
//!
//! На текущем этапе реализован read-only UPower adapter для Battery Charge
//! Limit (`upower`); D-Bus API sessiond и запуск демона наполняются по мере
//! реализации.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod upower;
