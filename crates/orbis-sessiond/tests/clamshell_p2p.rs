use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::ChargeLimit;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
use orbis_session_protocol::{Session1Proxy, clamshell};
use orbis_sessiond::clamshell::ClamshellInhibitor;
use orbis_sessiond::server::{GpuCapabilities, build_session_server_with_clamshell};
use zbus::connection::Builder;
use zbus::proxy::CacheProperties;

struct Battery;

impl Provider for Battery {
    fn id(&self) -> &'static str {
        "test-battery"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("test-battery")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        feature.into()
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}

#[async_trait]
impl BatteryProvider for Battery {
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        Err(ProviderError::Unsupported("unused".into()))
    }
    async fn set_charge_limit(&self, _: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("unused".into()))
    }
    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("unused".into()))
    }
    fn validate_charge_limit(&self, _: u8) -> ValidationResult {
        ValidationResult::invalid("unused")
    }
}

struct FakeInhibitor {
    state: Mutex<u8>,
}

impl FakeInhibitor {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(clamshell::INACTIVE),
        })
    }
}

impl ClamshellInhibitor for FakeInhibitor {
    fn status(&self) -> u8 {
        *self.state.lock().unwrap()
    }

    fn set_enabled(&self, enabled: bool) -> u8 {
        *self.state.lock().unwrap() = if enabled {
            clamshell::ACTIVE
        } else {
            clamshell::INACTIVE
        };
        self.status()
    }
}

async fn pair(inhibitor: Arc<dyn ClamshellInhibitor>) -> (zbus::Connection, zbus::Connection) {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
    let guid = zbus::Guid::generate();
    let server = Builder::unix_stream(server_stream)
        .server(guid)
        .unwrap()
        .p2p();
    let client = Builder::unix_stream(client_stream).p2p();
    tokio::try_join!(
        build_session_server_with_clamshell(
            server,
            Arc::new(Battery),
            GpuCapabilities::default(),
            None,
            None,
            Some(inhibitor),
        ),
        client.build(),
    )
    .unwrap()
}

#[tokio::test]
async fn p2p_lifecycle_and_refresh_are_authoritative() {
    let (server, client) = pair(FakeInhibitor::new()).await;
    let proxy = Session1Proxy::builder(&client)
        .cache_properties(CacheProperties::No)
        .build()
        .await
        .unwrap();

    assert_eq!(
        proxy.clamshell_inhibitor().await.unwrap(),
        clamshell::INACTIVE
    );
    assert_eq!(
        proxy.clamshell_inhibitor().await.unwrap(),
        clamshell::INACTIVE
    );
    assert_eq!(
        proxy.set_clamshell_inhibitor(true).await.unwrap(),
        clamshell::ACTIVE
    );
    assert_eq!(
        proxy.clamshell_inhibitor().await.unwrap(),
        clamshell::ACTIVE
    );
    assert_eq!(
        proxy.set_clamshell_inhibitor(false).await.unwrap(),
        clamshell::INACTIVE
    );
    drop(server);
}
