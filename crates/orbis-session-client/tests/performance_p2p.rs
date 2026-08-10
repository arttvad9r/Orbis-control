//! Private P2P integration test полного пути Performance Mode:
//!
//! `SessionPerformanceProvider`
//! → `ZbusSessionPerformanceSource`
//! → generated `Session1Proxy` (`Performance` property)
//! → `SessionService` (wire `(yy)`)
//! → scripted server-side read-only `PerformanceProvider`.
//!
//! Используется пара локальных Unix streams (`std::os::unix::net::UnixStream::pair`);
//! внешний D-Bus daemon / system / session bus не задействованы.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::ChargeLimit;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{BatteryProvider, PerformanceProvider, Provider, ProviderHealth};
use orbis_session_client::{SessionPerformanceProvider, ZbusSessionPerformanceSource};
use orbis_sessiond::server::build_session_server;
use zbus::connection::Builder;

/// Заглушка BatteryProvider: ChargeLimit в этом тесте не читается.
struct NoBattery;
#[async_trait]
impl BatteryProvider for NoBattery {
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        Err(ProviderError::Unsupported("no battery".into()))
    }
    async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("no battery".into()))
    }
    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("no battery".into()))
    }
    fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
        ValidationResult::invalid("no battery")
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

/// Тестовый server-side PerformanceProvider.
struct ScriptedPerformance {
    current: PerformanceProfile,
    available: Vec<PerformanceProfile>,
}
impl Provider for ScriptedPerformance {
    fn id(&self) -> &'static str {
        "scripted-performance"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("scripted-performance")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        format!("scripted-performance: {feature} недоступен")
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}
#[async_trait]
impl PerformanceProvider for ScriptedPerformance {
    async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
        Ok(self.available.clone())
    }
    async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
        Ok(self.current)
    }
    async fn set_profile(
        &self,
        _profile: PerformanceProfile,
    ) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("read-only".into()))
    }
    async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        Ok(None)
    }
    async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
        Ok(None)
    }
    fn validate_set_profile(&self, _profile: PerformanceProfile) -> ValidationResult {
        ValidationResult::invalid("read-only")
    }
}

async fn connect_pair()
-> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream).server(guid)?.p2p();
    let client_builder = Builder::unix_stream(client_stream).p2p();

    let battery: Arc<dyn BatteryProvider> = Arc::new(NoBattery);
    let performance: Arc<dyn PerformanceProvider> = Arc::new(ScriptedPerformance {
        current: PerformanceProfile::Silent,
        available: vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
            PerformanceProfile::Turbo,
        ],
    });
    let (server_conn, client_conn) = tokio::try_join!(
        build_session_server(
            server_builder,
            battery,
            Default::default(),
            Some(performance),
        ),
        client_builder.build()
    )?;
    Ok((server_conn, client_conn))
}

#[tokio::test]
async fn performance_roundtrip_over_p2p() {
    let (_server_conn, client_conn) = connect_pair().await.expect("pair");

    let provider = SessionPerformanceProvider::new(ZbusSessionPerformanceSource::new(client_conn));

    assert_eq!(
        provider.current_profile().await.expect("current"),
        PerformanceProfile::Silent
    );
    assert_eq!(
        provider.profiles().await.expect("profiles"),
        vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
            PerformanceProfile::Turbo,
        ]
    );
    assert!(matches!(
        provider
            .set_profile(PerformanceProfile::Turbo)
            .await
            .expect_err("read-only set"),
        ProviderError::Unsupported(_)
    ));
}
