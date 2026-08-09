//! Private P2P integration tests server bootstrap helper
//! (`orbis_sessiond::server::build_session_server`).
//!
//! Проверяется, что production helper регистрирует protocol BUS_NAME /
//! OBJECT_PATH / SessionService на transport-configured builder; тест не
//! вызывает `.name()`/`.serve_at()` напрямую и не создаёт SessionService
//! вручную.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use orbis_core::action::ApplyResult;
use orbis_core::battery::ChargeLimit;
use orbis_core::battery::ChargeLimitBounds;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::identity::BackendIdentity;
use orbis_core::newtypes::Percent;
use orbis_providers::error::{ProviderError, ValidationResult};
use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
use orbis_session_protocol::Session1Proxy;
use orbis_sessiond::server::build_session_server;
use zbus::connection::Builder;
use zbus::proxy::CacheProperties;

/// Заранее заданный исход scripted server provider.
#[derive(Debug, Clone, Copy)]
enum ServerRead {
    Limit(ChargeLimit),
    Unsupported,
}

/// Тестовый server-side BatteryProvider: очередь результатов, счётчик чтений.
struct ScriptedBatteryProvider {
    results: Mutex<VecDeque<ServerRead>>,
    reads: AtomicUsize,
}

impl ScriptedBatteryProvider {
    fn new(reads: Vec<ServerRead>) -> Self {
        Self {
            results: Mutex::new(reads.into()),
            reads: AtomicUsize::new(0),
        }
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn to_result(read: ServerRead) -> Result<ChargeLimit, ProviderError> {
        match read {
            ServerRead::Limit(l) => Ok(l),
            ServerRead::Unsupported => {
                Err(ProviderError::Unsupported("scripted unsupported".into()))
            }
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
        Some(
            ChargeLimitBounds::new(
                Percent::new(min).expect("range"),
                Percent::new(max).expect("range"),
                step,
            )
            .expect("valid"),
        ),
    )
    .expect("valid")
}

/// Поднять P2P pair: server через production helper, client напрямую.
///
/// Server builder передаётся helper без `.name()`/`.serve_at()` — их
/// выполняет `build_session_server`. Обе Connection живы до конца вызовов.
async fn connect_via_helper(
    provider: Arc<dyn BatteryProvider>,
) -> Result<(zbus::Connection, zbus::Connection), Box<dyn std::error::Error + Send + Sync>> {
    let (server_stream, client_stream) = std::os::unix::net::UnixStream::pair()?;
    let guid = zbus::Guid::generate();
    let server_builder = Builder::unix_stream(server_stream).server(guid)?.p2p();
    let client_builder = Builder::unix_stream(client_stream).p2p();
    let (server_conn, client_conn) = tokio::try_join!(
        build_session_server(server_builder, provider, Default::default(), None),
        client_builder.build()
    )?;
    Ok((server_conn, client_conn))
}

#[tokio::test]
async fn server_helper_registers_protocol_service() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = Arc::new(ScriptedBatteryProvider::new(vec![ServerRead::Limit(
            limit(true, Some(80), 40, 100, 5),
        )]));
        let (_server_conn, client_conn) = connect_via_helper(provider.clone())
            .await
            .expect("p2p connect");

        let proxy = Session1Proxy::builder(&client_conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("proxy");

        let info = proxy.charge_limit().await.expect("charge limit");

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
async fn server_helper_preserves_remote_error() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = Arc::new(ScriptedBatteryProvider::new(vec![ServerRead::Unsupported]));
        let (_server_conn, client_conn) = connect_via_helper(provider.clone())
            .await
            .expect("p2p connect");

        let proxy = Session1Proxy::builder(&client_conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("proxy");

        let err = proxy.charge_limit().await.expect_err("unsupported");

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

#[tokio::test]
async fn server_helper_reads_fresh_values() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let provider = Arc::new(ScriptedBatteryProvider::new(vec![
            ServerRead::Limit(limit(true, Some(80), 40, 100, 5)),
            ServerRead::Limit(limit(true, Some(60), 40, 100, 5)),
        ]));
        let (_server_conn, client_conn) = connect_via_helper(provider.clone())
            .await
            .expect("p2p connect");

        let proxy = Session1Proxy::builder(&client_conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .expect("proxy");

        let first = proxy.charge_limit().await.expect("read1");
        let second = proxy.charge_limit().await.expect("read2");

        assert_eq!(first.percent, 80);
        assert_eq!(second.percent, 60);
        assert_eq!(provider.reads(), 2);
    })
    .await
    .expect("p2p test timeout");
}
