//! Private P2P tests for the application-side Battery mutation route.
//!
//! Hardware1 is served by a local test object only; no system bus, asusd,
//! session bus or real hardware is involved.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::ChargeLimit;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::Percent;
use orbis_hardwared::{DBUS_NAME, DBUS_OBJECT_PATH};
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
use orbis_session_client::{
    HardwareBatterySource, SessionHardwareBatteryProvider, ZbusHardwareBatterySource,
};
use zbus::connection::Builder;

struct HardwareObject {
    calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<u8>>>,
    reply: Result<u8, String>,
}

#[zbus::interface(name = "io.github.orbiscontrol.Hardware1")]
impl HardwareObject {
    async fn set_charge_limit(&self, percent: u8) -> zbus::fdo::Result<u8> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(percent);
        match &self.reply {
            Ok(value) => Ok(*value),
            Err(message) => Err(zbus::fdo::Error::Failed(message.clone())),
        }
    }
}

async fn connect_hardware(
    object: HardwareObject,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream)
        .server(guid)?
        .p2p()
        .name(DBUS_NAME)?
        .serve_at(DBUS_OBJECT_PATH, object)?;
    let client_builder = Builder::unix_stream(client_stream).p2p();
    Ok(tokio::try_join!(
        server_builder.build(),
        client_builder.build()
    )?)
}

#[tokio::test]
async fn hardware_source_sends_exact_request_once_and_returns_confirmation() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let calls = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let object = HardwareObject {
            calls: calls.clone(),
            requests: requests.clone(),
            reply: Ok(83),
        };
        let (_server, client) = connect_hardware(object).await.expect("p2p");
        let source = ZbusHardwareBatterySource::new(client);

        assert_eq!(source.set_charge_limit(83).await.expect("reply"), 83);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(*requests.lock().unwrap(), vec![83]);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn hardware_source_propagates_dbus_error() {
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let object = HardwareObject {
        calls: calls.clone(),
        requests: requests.clone(),
        reply: Err("scripted failure".into()),
    };
    let (_server, client) = connect_hardware(object).await.expect("p2p");
    let source = ZbusHardwareBatterySource::new(client);

    assert!(matches!(
        source.set_charge_limit(80).await,
        Err(ProviderError::Dbus(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(*requests.lock().unwrap(), vec![80]);
}

struct ReadProvider;

impl Provider for ReadProvider {
    fn id(&self) -> &'static str {
        "scripted-session-battery"
    }
    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("scripted-session-battery")
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }
    fn explain_unsupported(&self, feature: &str) -> String {
        feature.to_string()
    }
    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }
    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}

#[async_trait]
impl BatteryProvider for ReadProvider {
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        ChargeLimit::new(
            false,
            Some(Percent::new(80).unwrap()),
            Some(Percent::new(100).unwrap()),
            None,
        )
        .map_err(|error| ProviderError::Internal(error.to_string()))
    }
    async fn set_charge_limit(&self, _: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("read-only test".into()))
    }
    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported("read-only test".into()))
    }
    fn validate_charge_limit(&self, _: u8) -> ValidationResult {
        ValidationResult::invalid("read-only test")
    }
}

struct ScriptedHardware {
    calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<u8>>>,
}

#[async_trait]
impl HardwareBatterySource for ScriptedHardware {
    async fn set_charge_limit(&self, percent: u8) -> Result<u8, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(percent);
        Ok(percent)
    }
}

#[tokio::test]
async fn composed_provider_validates_range_and_keeps_session_reads() {
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let hardware = ScriptedHardware {
        calls: calls.clone(),
        requests: requests.clone(),
    };
    let session = ReadProvider;
    let provider = SessionHardwareBatteryProvider::new(session, hardware);

    assert!(matches!(
        provider.set_charge_limit(19).await,
        Err(ProviderError::InvalidRequest(_))
    ));
    assert!(provider.set_charge_limit(20).await.is_ok());
    assert!(provider.set_charge_limit(80).await.is_ok());
    assert!(provider.set_charge_limit(100).await.is_ok());
    assert!(matches!(
        provider.set_charge_limit(101).await,
        Err(ProviderError::InvalidRequest(_))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(*requests.lock().unwrap(), vec![20, 80, 100]);

    let read = provider.charge_limit().await.expect("Session1 read");
    assert_eq!(read.configured_percent.map(|value| value.get()), Some(80));
    assert_eq!(read.effective_percent.map(|value| value.get()), Some(100));
}
