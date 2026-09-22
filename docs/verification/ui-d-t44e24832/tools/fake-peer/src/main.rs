//! Standalone scripted Session1 peer for live UI verification (evidence tool).
//!
//! Serves the read-only `io.github.orbiscontrol.Session1` D-Bus API from the
//! same `build_session_server` helper and scripted-provider pattern the repo's
//! own p2p integration tests use (`orbis-session-client/tests/
//! gpu_capabilities_p2p.rs`). Run against a private session bus:
//!
//! ```text
//! dbus-daemon --session --address=unix:path=$SCRATCH/bus --fork \
//!   --print-address=FILE --print-pid=FILE
//! orbis-fake-session-peer <address-file>
//! ```
//!
//! The peer performs no hardware access of any kind. Power limits are the
//! fixture triple also used by the repo tests: SPL 45 W (20..80 step 5,
//! default 45), NVIDIA Dynamic Boost 15 W (0..25 step 5, default 10),
//! GPU temp target 75 °C (60..87 step 1, default 80). `power-profiles-daemon`
//! is NOT served on this bus, so the GUI honestly reports the Hardware1
//! fallback path for performance mode.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::battery::ChargeLimit;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::identity::BackendIdentity;
use orbis_core::limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    BatteryProvider, GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PowerLimitProvider,
    Provider, ProviderHealth,
};
use orbis_sessiond::server::{GpuCapabilities, build_session_server};

fn main() {
    let mut args = std::env::args().skip(1);
    let address_file = args
        .next()
        .unwrap_or_else(|| "/tmp/fake-peer-bus-address".to_string());

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(run(address_file));
}

async fn run(address_file: String) {
    let mut address = String::new();
    for _ in 0..100 {
        if let Ok(text) = std::fs::read_to_string(&address_file) {
            address = text.trim().to_string();
            if !address.is_empty() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if address.is_empty() {
        eprintln!("fake-peer: no bus address in {address_file}");
        std::process::exit(2);
    }
    eprintln!("fake-peer: connecting to {address}");

    let builder = match zbus::connection::Builder::address(address.as_str()) {
        Ok(builder) => builder,
        Err(error) => {
            eprintln!("fake-peer: address {address}: {error}");
            std::process::exit(2);
        }
    };

    let battery: Arc<dyn BatteryProvider> = Arc::new(NoBattery);
    let gpu = GpuCapabilities {
        power: Some(Arc::new(StaticGpuPower(GpuPowerState::Suspended))),
        mux: Some(Arc::new(StaticGpuMux(GpuMuxState::Integrated))),
        access: Some(Arc::new(StaticGpuAccess(GpuAccessPolicy::Unblocked))),
        power_limits: Some(Arc::new(ScriptedPowerLimits)),
    };
    let _server = match build_session_server(builder, battery, gpu, None, None, None).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("fake-peer: build session server: {error}");
            std::process::exit(3);
        }
    };
    eprintln!(
        "fake-peer: Session1 up as {}",
        orbis_session_protocol::BUS_NAME
    );
    // Park forever; the connection object must stay alive to serve requests.
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

/// Provider plumbing shared by every scripted provider.
macro_rules! provider_plumbing {
    ($name:literal) => {
        fn id(&self) -> &'static str {
            $name
        }
        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple($name)
        }
        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }
        fn explain_unsupported(&self, feature: &str) -> String {
            format!("{}: {feature} недоступен", $name)
        }
        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }
        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    };
}

struct NoBattery;
impl Provider for NoBattery {
    provider_plumbing!("no-battery");
}
#[async_trait]
impl BatteryProvider for NoBattery {
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        Err(ProviderError::Unsupported("no battery".into()))
    }
    async fn set_charge_limit(
        &self,
        _percent: u8,
    ) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("no battery".into()))
    }
    async fn one_shot_full_charge(&self) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("no battery".into()))
    }
    fn validate_charge_limit(&self, _percent: u8) -> orbis_providers::error::ValidationResult {
        orbis_providers::error::ValidationResult::invalid("no battery")
    }
}

struct StaticGpuPower(GpuPowerState);
impl Provider for StaticGpuPower {
    provider_plumbing!("scripted-gpu-power");
}
#[async_trait]
impl GpuPowerProvider for StaticGpuPower {
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
        Ok(self.0)
    }
}

struct StaticGpuMux(GpuMuxState);
impl Provider for StaticGpuMux {
    provider_plumbing!("scripted-gpu-mux");
}
#[async_trait]
impl GpuMuxProvider for StaticGpuMux {
    async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
        Ok(self.0)
    }
}

struct StaticGpuAccess(GpuAccessPolicy);
impl Provider for StaticGpuAccess {
    provider_plumbing!("scripted-gpu-access");
}
#[async_trait]
impl GpuAccessProvider for StaticGpuAccess {
    async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
        Ok(self.0)
    }
}

struct ScriptedPowerLimits;
impl Provider for ScriptedPowerLimits {
    provider_plumbing!("scripted-power-limits");
}
#[async_trait]
impl PowerLimitProvider for ScriptedPowerLimits {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        let mut fields = BTreeMap::new();
        fields.insert(
            PowerLimitField::Spl,
            PowerLimitValue::new(45, 20, 80, 5, Some(45), Unit::Watts).unwrap(),
        );
        fields.insert(
            PowerLimitField::GpuDynamicBoost,
            PowerLimitValue::new(15, 0, 25, 5, Some(10), Unit::Watts).unwrap(),
        );
        fields.insert(
            PowerLimitField::GpuTempTarget,
            PowerLimitValue::new(75, 60, 87, 1, Some(80), Unit::DegreesC).unwrap(),
        );
        Ok(PowerLimits { fields })
    }
    async fn set_power_limit(
        &self,
        _: PowerLimitField,
        _: i32,
    ) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("read-only fixture".into()))
    }
    async fn restore_defaults(&self) -> Result<orbis_core::action::ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("read-only fixture".into()))
    }
    fn validate_power_limit(
        &self,
        _: &PowerLimitField,
        _: i32,
    ) -> orbis_providers::error::ValidationResult {
        orbis_providers::error::ValidationResult::invalid("read-only fixture")
    }
}
