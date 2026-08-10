//! # orbis-sessiond
//!
//! Пользовательский демон (user session daemon) и системные адаптеры.
//!
//! На текущем этапе реализованы:
//! - read-only UPower adapter для Battery Charge Limit (`upower`);
//! - read-only D-Bus service object (`service`), выдающий Charge Limit через
//!   интерфейс `io.github.orbiscontrol.Session1`;
//! - server bootstrap helper (`server`), принимающий transport-configured
//!   `zbus::connection::Builder` и регистрирующий protocol name/path;
//! - composition helper (`composition`) полного read-only пути: готовая UPower
//!   Connection → source → provider → session server;
//! - real D-Bus bootstrap helper (`bootstrap`), открывающий system/session bus
//!   и передающий connections в composition layer, включая UPower battery
//!   discovery;
//! - read-only discovery системной батареи через UPower (`discovery`);
//! - production lifecycle helper (`runtime`), запускающий discovered bootstrap
//!   и удерживающий D-Bus service живым до shutdown signal.
//!
//! Запуск демона (main) наполняется отдельными микрошагами.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod armoury;
pub mod bootstrap;
pub mod composition;
pub mod discovery;
pub mod performance;
pub mod runtime;
pub mod server;
pub mod service;
pub mod supergfxd;
pub mod upower;
