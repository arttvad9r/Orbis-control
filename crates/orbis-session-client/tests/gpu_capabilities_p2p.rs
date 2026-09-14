//! Private P2P D-Bus integration tests для read-only GPU capabilities.
//!
//! Проверяют совместимость между server properties `SessionService::GpuPower/
//! GpuMux/GpuAccess` (wire `y`) и client-side capability providers
//! (`SessionGpuPowerProvider`/`SessionGpuMuxProvider`/`SessionGpuAccessProvider`).
//!
//! Используется пара локальных Unix streams; внешний D-Bus daemon / system /
//! session bus не задействованы; все соединения существуют внутри test process.
//! Никаких hardware reads/writes.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState};
use orbis_core::identity::BackendIdentity;
use orbis_core::limits::{PowerLimitField, PowerLimitValue, PowerLimits, Unit};
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{
    GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, PowerLimitProvider, Provider,
    ProviderHealth,
};
use orbis_session_client::{
    SessionGpuAccessProvider, SessionGpuMuxProvider, SessionGpuPowerProvider,
    SessionPowerLimitProvider, ZbusSessionGpuSource, ZbusSessionPowerLimitSource,
};
use orbis_sessiond::server::{GpuCapabilities, build_session_server};
use zbus::connection::Builder;

// Scripted GPU capability providers для server.

struct ScriptedGpuPower {
    value: GpuPowerState,
}
impl Provider for ScriptedGpuPower {
    fn id(&self) -> &'static str {
        "scripted-gpu-power"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("scripted-gpu-power")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("scripted-gpu-power: {feature} недоступен")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}
#[async_trait]
impl GpuPowerProvider for ScriptedGpuPower {
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
        Ok(self.value)
    }
}

struct ScriptedPowerLimits;
impl Provider for ScriptedPowerLimits {
    fn id(&self) -> &'static str {
        "scripted-power-limits"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("fixture-power-limits")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("fixture: {feature}")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}
#[async_trait]
impl PowerLimitProvider for ScriptedPowerLimits {
    async fn power_limits(&self) -> Result<PowerLimits, ProviderError> {
        let mut fields = std::collections::BTreeMap::new();
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
    fn validate_power_limit(&self, _: &PowerLimitField, _: i32) -> ValidationResult {
        ValidationResult::invalid("read-only fixture")
    }
}

struct ScriptedGpuMux {
    value: GpuMuxState,
}
impl Provider for ScriptedGpuMux {
    fn id(&self) -> &'static str {
        "scripted-gpu-mux"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("scripted-gpu-mux")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("scripted-gpu-mux: {feature} недоступен")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}
#[async_trait]
impl GpuMuxProvider for ScriptedGpuMux {
    async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
        Ok(self.value)
    }
}

struct ScriptedGpuAccess {
    value: GpuAccessPolicy,
}
impl Provider for ScriptedGpuAccess {
    fn id(&self) -> &'static str {
        "scripted-gpu-access"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("scripted-gpu-access")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("scripted-gpu-access: {feature} недоступен")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}
#[async_trait]
impl GpuAccessProvider for ScriptedGpuAccess {
    async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
        Ok(self.value)
    }
}

/// Заглушка BatteryProvider: ChargeLimit в этом тесте не читается.
struct NoBattery;
#[async_trait]
impl orbis_providers::traits::BatteryProvider for NoBattery {
    async fn charge_limit(&self) -> Result<orbis_core::battery::ChargeLimit, ProviderError> {
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
impl Provider for NoBattery {
    fn id(&self) -> &'static str {
        "no-battery"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("no-battery")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("no-battery: {feature} недоступен")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}

async fn connect_pair()
-> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream).server(guid)?.p2p();
    let client_builder = Builder::unix_stream(client_stream).p2p();

    let battery: Arc<dyn orbis_providers::traits::BatteryProvider> = Arc::new(NoBattery);
    let gpu = GpuCapabilities {
        power: Some(Arc::new(ScriptedGpuPower {
            value: GpuPowerState::Suspended,
        })),
        mux: Some(Arc::new(ScriptedGpuMux {
            value: GpuMuxState::Integrated,
        })),
        access: Some(Arc::new(ScriptedGpuAccess {
            value: GpuAccessPolicy::Blocked,
        })),
        power_limits: Some(Arc::new(ScriptedPowerLimits)),
    };
    let (server_conn, client_conn) = tokio::try_join!(
        build_session_server(server_builder, battery, gpu, None, None, None),
        client_builder.build()
    )?;
    Ok((server_conn, client_conn))
}

#[tokio::test]
async fn gpu_capabilities_roundtrip_over_p2p() {
    let (_server_conn, client_conn) = connect_pair().await.expect("pair");

    // Три независимых client capability providers поверх одной Connection.
    let power = SessionGpuPowerProvider::new(ZbusSessionGpuSource::new(client_conn.clone()));
    let mux = SessionGpuMuxProvider::new(ZbusSessionGpuSource::new(client_conn.clone()));
    let access = SessionGpuAccessProvider::new(ZbusSessionGpuSource::new(client_conn.clone()));

    assert_eq!(
        power.power_state().await.expect("power"),
        GpuPowerState::Suspended
    );
    assert_eq!(mux.mux_state().await.expect("mux"), GpuMuxState::Integrated);
    assert_eq!(
        access.access_policy().await.expect("access"),
        GpuAccessPolicy::Blocked
    );
}

#[tokio::test]
async fn power_limits_roundtrip_over_p2p_and_writes_stay_unsupported() {
    let (_server_conn, client_conn) = connect_pair().await.expect("pair");
    let provider = SessionPowerLimitProvider::new(ZbusSessionPowerLimitSource::new(client_conn));
    let limits = provider.power_limits().await.expect("power limits");
    assert_eq!(limits.get(&PowerLimitField::Spl).unwrap().value, 45);
    assert_eq!(
        limits.get(&PowerLimitField::GpuDynamicBoost).unwrap().unit,
        Unit::Watts
    );
    assert_eq!(limits.get(&PowerLimitField::GpuTempTarget).unwrap().max, 87);
    assert!(matches!(
        provider.set_power_limit(PowerLimitField::Spl, 50).await,
        Err(ProviderError::Unsupported(_))
    ));
}
