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

pub mod asus_armoury;
pub mod aura;
pub mod error;
pub mod fan_defaults;
pub mod keyboard_backlight;
pub mod native_asus_eco;
pub mod probes;
pub mod supergfxd;
pub mod sysfs_telemetry;
pub mod system_metadata;
pub mod traits;
pub mod wayland_output;

#[cfg(feature = "mock")]
pub mod mock;

pub use asus_armoury::*;
pub use aura::*;
pub use error::{OperationId, ProviderError, ValidationResult};
pub use fan_defaults::{FanCurveDefaultsMutationProvider, Hardware1FanDefaultsProvider};
pub use keyboard_backlight::*;
pub use native_asus_eco::*;
pub use probes::{
    probe_charge_limit, probe_display_output, probe_fan_curve, probe_gpu_access, probe_gpu_mux,
    probe_gpu_power, probe_mini_led_mode, probe_panel_overdrive, probe_performance,
    probe_screen_auto_brightness,
};
pub use sysfs_telemetry::SysfsTelemetryProvider;
pub use system_metadata::SystemMetadataProvider;
pub use traits::*;
pub use wayland_output::*;

#[cfg(feature = "mock")]
pub use mock::{MockErrorMode, MockProvider, MockState, MockStateError};
