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
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{
    GpuAccessProvider, GpuMuxProvider, GpuPowerProvider, Provider, ProviderHealth,
};
use orbis_session_client::{
    SessionGpuAccessProvider, SessionGpuMuxProvider, SessionGpuPowerProvider, ZbusSessionGpuSource,
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
    };
    let (server_conn, client_conn) = tokio::try_join!(
        build_session_server(server_builder, battery, gpu, None, None),
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
