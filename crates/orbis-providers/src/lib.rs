//! # Orbis Providers
//!
//! Typed provider interfaces and production/test implementations for Orbis
//! hardware and platform capabilities.
//!
//! Rules:
//! - file/service presence alone is not proof of write support;
//! - read and write evidence remain independent;
//! - provider errors preserve Unsupported / Unavailable / PermissionDenied /
//!   Unknown semantics instead of fabricating values;
//! - privileged mutation is exposed only through bounded typed owners;
//! - `mock` exists for tests/development and is not the production composition.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod asus_armoury;
pub mod asus_diagnostics;
pub mod aura;
pub mod display_diagnostics;
pub mod display_refresh_owner;
pub mod error;
pub mod fan_defaults;
pub mod gpu_diagnostics;
pub mod hardware_identity;
pub mod keyboard_backlight;
pub mod native_asus_eco;
pub mod probes;
pub mod service_presence;
pub mod supergfxd;
pub mod sysfs_telemetry;
pub mod system_metadata;
pub mod telemetry_diagnostics;
pub mod traits;
pub mod wayland_output;
pub mod wayland_output_management;

#[cfg(feature = "mock")]
pub mod mock;

pub use asus_armoury::*;
pub use asus_diagnostics::{
    probe_asus_diagnostics_capabilities, probe_aura, probe_keyboard_backlight,
};
pub use aura::*;
pub use display_diagnostics::display_diagnostics_snapshot;
pub use display_refresh_owner::{
    DisplayRefreshMutationOwner, validate_display_refresh_readback,
    validate_display_refresh_request,
};
pub use error::{OperationId, ProviderError, ValidationResult};
pub use fan_defaults::{FanCurveDefaultsMutationProvider, Hardware1FanDefaultsProvider};
pub use gpu_diagnostics::gpu_diagnostics_snapshot;
pub use hardware_identity::HardwareIdentityProvider;
pub use keyboard_backlight::*;
pub use native_asus_eco::*;
pub use probes::{
    probe_charge_limit, probe_display_output, probe_fan_curve, probe_gpu_access, probe_gpu_mux,
    probe_gpu_power, probe_mini_led_mode, probe_panel_overdrive, probe_performance,
    probe_screen_auto_brightness,
};
pub use service_presence::*;
pub use sysfs_telemetry::SysfsTelemetryProvider;
pub use system_metadata::SystemMetadataProvider;
pub use telemetry_diagnostics::telemetry_diagnostics_after_attempt;
pub use traits::*;
pub use wayland_output::*;
pub use wayland_output_management::*;

#[cfg(feature = "mock")]
pub use mock::{MockErrorMode, MockProvider, MockState, MockStateError};
