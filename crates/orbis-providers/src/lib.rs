//! # Orbis Providers
//!
//! Trait-ы провайдеров аппаратных функций и mock-реализации (Этап 2).
//!
//! Правила:
//! - провайдер не считает наличие файла доказательством поддержки записи;
//! - каждый провайдер обязан: probe, capabilities, read_state, write-методы с
//!   validate_request, health, diagnostics, timeout, объяснение отсутствия поддержки,
//!   идентификатор/версию backend, классификацию риска.
//! - на Этапе 2 реализованы только интерфейсы и mock-провайдеры; настоящие
//!   sysfs/asusd-провайдеры — Этап 3+; внешние процессы не вызываются.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod fan_defaults;
pub mod native_asus_eco;
pub mod probes;
pub mod supergfxd;
pub mod sysfs_telemetry;
pub mod traits;

#[cfg(feature = "mock")]
pub mod mock;

pub use error::{OperationId, ProviderError, ValidationResult};
pub use fan_defaults::{FanCurveDefaultsMutationProvider, Hardware1FanDefaultsProvider};
pub use native_asus_eco::*;
pub use probes::{
    probe_charge_limit, probe_fan_curve, probe_gpu_access, probe_gpu_mux, probe_gpu_power,
    probe_performance,
};
pub use sysfs_telemetry::SysfsTelemetryProvider;
pub use traits::*;

#[cfg(feature = "mock")]
pub use mock::{MockErrorMode, MockProvider, MockState, MockStateError};
