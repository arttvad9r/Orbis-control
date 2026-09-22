use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{PerformanceProvider, Provider, ProviderHealth};
use orbis_session_client::{
    PowerProfilesDaemonClient, PowerProfilesProfile, ZbusPowerProfilesDaemonClient,
};
use zbus::connection::Builder;

#[derive(Default)]
struct FakePowerProfiles {
    active: Mutex<String>,
    profiles: Vec<(String, String)>,
    deny: bool,
    mismatch: bool,
}

#[zbus::interface(name = "org.freedesktop.UPower.PowerProfiles")]
impl FakePowerProfiles {
    #[zbus(property)]
    async fn active_profile(&self) -> String {
        self.active.lock().unwrap().clone()
    }

    #[zbus(property)]
    async fn set_active_profile(&self, profile: String) -> zbus::Result<()> {
        if self.deny {
            return Err(zbus::fdo::Error::AccessDenied("denied".into()).into());
        }
        if !self.profiles.iter().any(|(name, _)| name == &profile) {
            return Err(zbus::fdo::Error::InvalidArgs("invalid profile".into()).into());
        }
        if !self.mismatch {
            *self.active.lock().unwrap() = profile;
        }
        Ok(())
    }

    #[zbus(property)]
    async fn profiles(&self) -> Vec<(String, String)> {
        self.profiles.clone()
    }
}

async fn fake_client(
    active: &str,
    deny: bool,
    mismatch: bool,
) -> (
    zbus::Connection,
    zbus::Connection,
    ZbusPowerProfilesDaemonClient,
) {
    let service = FakePowerProfiles {
        active: Mutex::new(active.into()),
        profiles: vec![
            ("power-saver".into(), "driver".into()),
            ("balanced".into(), "driver".into()),
            ("performance".into(), "driver".into()),
        ],
        deny,
        mismatch,
    };
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair().unwrap();
    let server = Builder::unix_stream(server_stream)
        .server(zbus::Guid::generate())
        .unwrap()
        .p2p()
        .name("org.freedesktop.UPower.PowerProfiles")
        .unwrap()
        .serve_at("/org/freedesktop/UPower/PowerProfiles", service)
        .unwrap();
    let client = Builder::unix_stream(client_stream).p2p();
    let (server, connection) = tokio::try_join!(server.build(), client.build()).unwrap();
    let daemon = ZbusPowerProfilesDaemonClient::new(connection.clone());
    (server, connection, daemon)
}

#[tokio::test]
async fn delegated_client_reads_profiles_and_applies_mapped_profile() {
    let (_server, _connection, client) = fake_client("balanced", false, false).await;
    assert_eq!(
        client.read_current().await.unwrap(),
        PerformanceProfile::Balanced
    );
    assert_eq!(
        client.read_profiles().await.unwrap(),
        vec![
            PerformanceProfile::Silent,
            PerformanceProfile::Balanced,
            PerformanceProfile::Turbo,
        ]
    );
    assert_eq!(
        client.set_profile(PerformanceProfile::Turbo).await.unwrap(),
        ApplyResult::Applied
    );
    assert_eq!(
        client.read_current().await.unwrap(),
        PerformanceProfile::Turbo
    );
}

#[tokio::test]
async fn delegated_client_preserves_invalid_profile_access_denied_and_readback_mismatch() {
    assert!(matches!(
        PowerProfilesProfile::from_wire("not-a-profile"),
        Err(ProviderError::InvalidRequest(message)) if message.contains("not-a-profile")
    ));
    let (_server, _connection, denied) = fake_client("balanced", true, false).await;
    let denied_error = denied.set_profile(PerformanceProfile::Turbo).await;
    assert!(
        matches!(denied_error, Err(ProviderError::PermissionDenied(_))),
        "{denied_error:?}"
    );
    let (_server, _connection, mismatch) = fake_client("balanced", false, true).await;
    assert!(matches!(
        mismatch.set_profile(PerformanceProfile::Turbo).await,
        Err(ProviderError::Conflict(_))
    ));
}

#[test]
fn fallback_provider_keeps_hardware_write_owner() {
    fn assert_performance_provider<T: PerformanceProvider>() {}
    assert_performance_provider::<
        orbis_session_client::DelegatedPerformanceProvider<NeverSession, NeverHardware>,
    >();
}

struct NeverSession;
#[async_trait]
impl orbis_session_client::SessionPerformanceSource for NeverSession {
    async fn read_performance(
        &self,
    ) -> Result<orbis_session_protocol::PerformanceInfo, ProviderError> {
        Err(ProviderError::BackendUnavailable("never".into()))
    }
}

struct NeverHardware;
#[async_trait]
impl orbis_session_client::HardwarePerformanceSource for NeverHardware {
    async fn set_performance(&self, _: u8) -> Result<u8, ProviderError> {
        Err(ProviderError::BackendUnavailable("never".into()))
    }
}

impl Provider for NeverSession {
    fn id(&self) -> &'static str {
        "never"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("never")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, _: &str) -> String {
        "never".into()
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}
