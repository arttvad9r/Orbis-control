//! Private P2P D-Bus integration tests.
//!
//! Фактически проверяют совместимость между server property
//! `SessionService::ChargeLimit` (wire tuple `(bbyyyy)`) и client
//! `orbis_session_protocol::Session1Proxy` -> `ChargeLimitInfo`.
//!
//! Используется пара локальных Unix streams (`std::os::unix::net::UnixStream::pair`),
//! внешний D-Bus daemon / system / session bus не задействованы; все соединения
//! существуют только внутри test process.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::ChargeLimit;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::Percent;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
use orbis_session_protocol::{BUS_NAME, ChargeLimitInfo, OBJECT_PATH, Session1Proxy};
use orbis_sessiond::service::SessionService;
use zbus::connection::Builder;
use zbus::proxy::CacheProperties;

/// Заранее заданный исход scripted provider (integration test).
#[derive(Debug, Clone, Copy)]
enum Read {
    Limit(ChargeLimit),
    Unsupported,
}

/// Тестовый BatteryProvider: очередь результатов, счётчик чтений; без D-Bus,
/// UPower, sleep, глобального состояния; mutation -> Unsupported.
struct ScriptedBatteryProvider {
    results: Mutex<VecDeque<Read>>,
    reads: AtomicUsize,
}

impl ScriptedBatteryProvider {
    fn new(reads: Vec<Read>) -> Self {
        Self {
            results: Mutex::new(reads.into()),
            reads: AtomicUsize::new(0),
        }
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn to_result(read: Read) -> Result<ChargeLimit, ProviderError> {
        match read {
            Read::Limit(l) => Ok(l),
            Read::Unsupported => Err(ProviderError::Unsupported("scripted unsupported".into())),
        }
    }
}

impl Provider for ScriptedBatteryProvider {
    fn id(&self) -> &'static str {
        "scripted-battery"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("scripted-battery")
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("scripted-battery: {feature} недоступен")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        Vec::new()
    }
}

#[async_trait]
impl BatteryProvider for ScriptedBatteryProvider {
    async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let read = self
            .results
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted provider: очередь результатов исчерпана");
        Self::to_result(read)
    }

    async fn set_charge_limit(&self, _percent: u8) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "scripted read-only backend: set_charge_limit недоступна".into(),
        ))
    }

    async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
        Err(ProviderError::Unsupported(
            "scripted read-only backend: one_shot_full_charge недоступна".into(),
        ))
    }

    fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
        ValidationResult::invalid("read-only backend: запись charge limit не поддерживается")
    }
}

/// Test-only domain fixture через существующий конструктор.
fn limit(enabled: bool, percent: Option<u8>, min: u8, max: u8, step: u8) -> ChargeLimit {
    ChargeLimit::new(
        enabled,
        percent.map(|p| Percent::new(p).expect("range")),
        Percent::new(min).expect("range"),
        Percent::new(max).expect("range"),
        step,
    )
    .expect("valid")
}

/// Поднять пару P2P соединений: (server, client).
///
/// Server: unix_stream + server(guid) + p2p + name(BUS_NAME) + serve_at(OBJECT_PATH).
/// Client: unix_stream + p2p.
/// Обе стороны строятся конкурентно через `tokio::try_join!` (без spawn/daemon).
async fn connect_service(
    service: SessionService,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream)
        .server(guid)?
        .p2p()
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, service)?;
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server_conn, client_conn) =
        tokio::try_join!(server_builder.build(), client_builder.build())?;
    Ok((server_conn, client_conn))
}

async fn proxy_for(client: &zbus::Connection) -> zbus::Result<Session1Proxy<'_>> {
    Session1Proxy::builder(client)
        .cache_properties(CacheProperties::No)
        .build()
        .await
}

#[tokio::test]
async fn protocol_proxy_deserializes_server_tuple() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = Arc::new(ScriptedBatteryProvider::new(vec![Read::Limit(limit(
            true,
            Some(80),
            40,
            100,
            5,
        ))]));
        let service = SessionService::new(provider.clone());
        let (_server, client) = connect_service(service).await.expect("p2p connect");
        let proxy = proxy_for(&client).await.expect("proxy");

        let info: ChargeLimitInfo = proxy.charge_limit().await.expect("charge limit");

        assert!(info.enabled);
        assert!(info.percent_present);
        assert_eq!(info.percent, 80);
        assert_eq!(info.percent(), Some(80));
        assert_eq!(info.min_percent, 40);
        assert_eq!(info.max_percent, 100);
        assert_eq!(info.step_percent, 5);
        assert_eq!(provider.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn protocol_proxy_preserves_canonical_missing_percent() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = Arc::new(ScriptedBatteryProvider::new(vec![Read::Limit(limit(
            false, None, 40, 100, 5,
        ))]));
        let service = SessionService::new(provider.clone());
        let (_server, client) = connect_service(service).await.expect("p2p connect");
        let proxy = proxy_for(&client).await.expect("proxy");

        let info: ChargeLimitInfo = proxy.charge_limit().await.expect("charge limit");

        assert!(!info.enabled);
        assert!(!info.percent_present);
        assert_eq!(info.percent, 0);
        assert_eq!(info.percent(), None);
        assert_eq!(info.min_percent, 40);
        assert_eq!(info.max_percent, 100);
        assert_eq!(info.step_percent, 5);
        assert_eq!(provider.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn protocol_proxy_reads_fresh_authoritative_values() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = Arc::new(ScriptedBatteryProvider::new(vec![
            Read::Limit(limit(true, Some(80), 40, 100, 5)),
            Read::Limit(limit(true, Some(60), 40, 100, 5)),
        ]));
        let service = SessionService::new(provider.clone());
        let (_server, client) = connect_service(service).await.expect("p2p connect");
        let proxy = proxy_for(&client).await.expect("proxy");

        let first: ChargeLimitInfo = proxy.charge_limit().await.expect("read1");
        let second: ChargeLimitInfo = proxy.charge_limit().await.expect("read2");

        assert_eq!(first.percent, 80);
        assert_eq!(second.percent, 60);
        assert_eq!(provider.reads(), 2);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn protocol_proxy_receives_not_supported_error() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = Arc::new(ScriptedBatteryProvider::new(vec![Read::Unsupported]));
        let service = SessionService::new(provider.clone());
        let (_server, client) = connect_service(service).await.expect("p2p connect");
        let proxy = proxy_for(&client).await.expect("proxy");

        let err = proxy.charge_limit().await.expect_err("unsupported");

        // zbus proxy преобразует remote fdo error в `Error::FDO` напрямую.
        match err {
            zbus::Error::FDO(boxed) => {
                assert!(matches!(&*boxed, zbus::fdo::Error::NotSupported(_)));
            }
            other => panic!("ожидался FDO(NotSupported), получено: {other:?}"),
        }
        assert_eq!(provider.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}
