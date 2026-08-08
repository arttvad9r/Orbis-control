//! Private P2P integration tests полного read-only пути Battery Charge Limit:
//!
//! `SessionChargeLimitProvider`
//! → `ZbusSessionChargeLimitSource`
//! → generated `Session1Proxy`
//! → `SessionService`
//! → scripted server-side `BatteryProvider`.
//!
//! Используется пара локальных Unix streams (`std::os::unix::net::UnixStream::pair`);
//! внешний D-Bus daemon / system / session bus не задействованы. Тест вызывает
//! только `provider.charge_limit().await`; wire DTO и proxy остаются
//! внутренними этапами production client реализации.

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
use orbis_session_client::{SessionChargeLimitProvider, ZbusSessionChargeLimitSource};
use orbis_session_protocol::{BUS_NAME, OBJECT_PATH};
use orbis_sessiond::service::SessionService;
use zbus::connection::Builder;

/// Заранее заданный исход scripted server provider.
#[derive(Debug, Clone, Copy)]
enum ServerRead {
    Limit(ChargeLimit),
    Unsupported,
    PermissionDenied,
}

/// Тестовый server-side BatteryProvider для SessionService.
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
            ServerRead::PermissionDenied => {
                Err(ProviderError::PermissionDenied("scripted denied".into()))
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

/// Поднять полный production путь через пару P2P соединений.
///
/// Возвращает (server conn, client conn, client provider); обе connection живы
/// до конца вызовов (обычный drop при выходе из scope).
async fn connect_full_path(
    provider: Arc<dyn BatteryProvider>,
) -> Result<
    (
        zbus::Connection,
        zbus::Connection,
        SessionChargeLimitProvider<ZbusSessionChargeLimitSource>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let service = SessionService::new(provider);
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

    let source = ZbusSessionChargeLimitSource::new(client_conn.clone());
    let client_provider = SessionChargeLimitProvider::new(source);

    Ok((server_conn, client_conn, client_provider))
}

#[tokio::test]
async fn full_provider_path_reads_charge_limit() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Arc::new(ScriptedBatteryProvider::new(vec![ServerRead::Limit(
            limit(true, Some(80), 40, 100, 5),
        )]));
        let (_server_conn, _client_conn, client_provider) = connect_full_path(server.clone())
            .await
            .expect("p2p connect");

        let limit = client_provider.charge_limit().await.expect("charge limit");

        assert!(limit.enabled);
        assert_eq!(limit.percent.map(|p| p.get()), Some(80));
        let b = limit.bounds.expect("known bounds");
        assert_eq!(b.min.get(), 40);
        assert_eq!(b.max.get(), 100);
        assert_eq!(b.step, 5);
        assert_eq!(server.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn full_provider_path_preserves_disabled_known_threshold() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Arc::new(ScriptedBatteryProvider::new(vec![ServerRead::Limit(
            limit(false, Some(80), 40, 100, 5),
        )]));
        let (_server_conn, _client_conn, client_provider) = connect_full_path(server.clone())
            .await
            .expect("p2p connect");

        let limit = client_provider.charge_limit().await.expect("charge limit");

        assert!(!limit.enabled);
        assert_eq!(limit.percent.map(|p| p.get()), Some(80));
        let b = limit.bounds.expect("known bounds");
        assert_eq!(b.min.get(), 40);
        assert_eq!(b.max.get(), 100);
        assert_eq!(b.step, 5);
        assert_eq!(server.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn full_provider_path_preserves_missing_threshold() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Arc::new(ScriptedBatteryProvider::new(vec![ServerRead::Limit(
            limit(false, None, 40, 100, 5),
        )]));
        let (_server_conn, _client_conn, client_provider) = connect_full_path(server.clone())
            .await
            .expect("p2p connect");

        let limit = client_provider.charge_limit().await.expect("charge limit");

        assert!(!limit.enabled);
        assert_eq!(limit.percent, None);
        let b = limit.bounds.expect("known bounds");
        assert_eq!(b.min.get(), 40);
        assert_eq!(b.max.get(), 100);
        assert_eq!(b.step, 5);
        assert_eq!(server.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn full_provider_path_reads_fresh_values() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Arc::new(ScriptedBatteryProvider::new(vec![
            ServerRead::Limit(limit(true, Some(80), 40, 100, 5)),
            ServerRead::Limit(limit(true, Some(60), 40, 100, 5)),
        ]));
        let (_server_conn, _client_conn, client_provider) = connect_full_path(server.clone())
            .await
            .expect("p2p connect");

        let first = client_provider.charge_limit().await.expect("read1");
        let second = client_provider.charge_limit().await.expect("read2");

        assert_eq!(first.percent.map(|p| p.get()), Some(80));
        assert_eq!(second.percent.map(|p| p.get()), Some(60));
        assert_eq!(server.reads(), 2);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn full_provider_path_preserves_unsupported_error() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Arc::new(ScriptedBatteryProvider::new(vec![ServerRead::Unsupported]));
        let (_server_conn, _client_conn, client_provider) = connect_full_path(server.clone())
            .await
            .expect("p2p connect");

        let err = client_provider
            .charge_limit()
            .await
            .expect_err("unsupported");

        assert!(matches!(err, ProviderError::Unsupported(_)));
        assert_eq!(server.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}

#[tokio::test]
async fn full_provider_path_preserves_permission_denied() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let server = Arc::new(ScriptedBatteryProvider::new(vec![
            ServerRead::PermissionDenied,
        ]));
        let (_server_conn, _client_conn, client_provider) = connect_full_path(server.clone())
            .await
            .expect("p2p connect");

        let err = client_provider
            .charge_limit()
            .await
            .expect_err("permission denied");

        assert!(matches!(err, ProviderError::PermissionDenied(_)));
        assert_eq!(server.reads(), 1);
    })
    .await
    .expect("p2p test timeout");
}
