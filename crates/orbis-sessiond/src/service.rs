//! Read-only D-Bus service object для Battery Charge Limit.
//!
//! Серверная сторона интерфейса `io.github.orbiscontrol.Session1` (getter-only
//! property `ChargeLimit`). Этот модуль только определяет service object и
//! domain-to-wire conversion; он не создаёт Connection, ObjectServer, runtime и
//! не регистрирует bus name — bus bootstrap выполняется отдельным микрошагом.

use std::sync::Arc;

use orbis_core::battery::ChargeLimit;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::BatteryProvider;
use orbis_session_protocol::ChargeLimitInfo;

/// Read-only service object session интерфейса.
///
/// Владеет только `Arc<dyn BatteryProvider>`; provider передаётся извне.
/// Конструктор не выполняет I/O, не читает состояние, не открывает D-Bus и не
/// создаёт runtime; кэш/last error/mutable state отсутствуют.
pub struct SessionService {
    battery: Arc<dyn BatteryProvider>,
}

impl SessionService {
    /// Создать service object над provider.
    pub fn new(battery: Arc<dyn BatteryProvider>) -> Self {
        Self { battery }
    }

    /// Прочитать authoritative domain Charge Limit и вернуть wire DTO.
    ///
    /// Provider вызывается ровно один раз; каждое чтение — новое; кэш
    /// отсутствует; `ProviderError` сохраняется без преобразования внутри
    /// helper; panic/retry/fallback отсутствуют.
    pub async fn read_charge_limit(&self) -> Result<ChargeLimitInfo, ProviderError> {
        let value = self.battery.charge_limit().await?;
        Ok(charge_limit_to_wire(value))
    }
}

/// Преобразовать domain `ChargeLimit` в canonical wire `ChargeLimitInfo`.
///
/// Значения переносятся без clamp/округления/изменения шага; UI step не
/// применяется; при `percent == None` wire payload канонизируется
/// (`percent_present = false`, `percent = 0`).
pub fn charge_limit_to_wire(value: ChargeLimit) -> ChargeLimitInfo {
    match (value.percent, value.bounds) {
        (Some(percent), Some(bounds)) => ChargeLimitInfo::with_percent(
            value.enabled,
            percent.get(),
            bounds.min.get(),
            bounds.max.get(),
            bounds.step,
        ),
        (Some(percent), None) => {
            ChargeLimitInfo::with_percent_unknown_bounds(value.enabled, percent.get())
        }
        (None, Some(bounds)) => ChargeLimitInfo::without_percent(
            value.enabled,
            bounds.min.get(),
            bounds.max.get(),
            bounds.step,
        ),
        (None, None) => ChargeLimitInfo::without_percent_unknown_bounds(value.enabled),
    }
}

/// Преобразовать `ProviderError` в `zbus::fdo::Error`.
///
/// Детерминированное отображение классов ошибок; диагностический смысл строки
/// сохраняется; чистый mapper не логирует.
fn provider_error_to_dbus(error: ProviderError) -> zbus::fdo::Error {
    match error {
        ProviderError::Unsupported(msg) => zbus::fdo::Error::NotSupported(msg),
        ProviderError::PermissionDenied(msg) => zbus::fdo::Error::AccessDenied(msg),
        ProviderError::InvalidRequest(msg) => zbus::fdo::Error::InvalidArgs(msg),
        ProviderError::BackendUnavailable(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Timeout(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Io(e) => zbus::fdo::Error::Failed(e.to_string()),
        ProviderError::Dbus(msg) => zbus::fdo::Error::Failed(msg),
        ProviderError::Internal(msg) => zbus::fdo::Error::Failed(msg),
    }
}

/// Wire-кодирование `ChargeLimitInfo` в D-Bus tuple `(bbyyyy)`.
///
/// zbus 5.13.2 server-side interface macro требует `Value: From<T>` и `T: Type`
/// для типа property; кастомный struct (`ChargeLimitInfo`) не конвертируется
/// без impl в protocol crate (за пределами scope). Кортеж из шести полей имеет
/// ту же D-Bus signature `(bbyyyy)`, что и `ChargeLimitInfo`, поэтому client
/// proxy остаётся без изменений.
type ChargeLimitTuple = (bool, bool, u8, bool, u8, u8, u8);

fn charge_limit_info_to_tuple(info: ChargeLimitInfo) -> ChargeLimitTuple {
    (
        info.enabled,
        info.percent_present,
        info.percent,
        info.bounds_present,
        info.min_percent,
        info.max_percent,
        info.step_percent,
    )
}

/// Серверный интерфейс `io.github.orbiscontrol.Session1` (getter-only).
#[zbus::interface(name = "io.github.orbiscontrol.Session1")]
impl SessionService {
    /// Текущий Battery Charge Limit (read-only property, wire signature
    /// `(bbyyyy)`).
    #[zbus(property)]
    async fn charge_limit(&self) -> zbus::fdo::Result<ChargeLimitTuple> {
        let info = self
            .read_charge_limit()
            .await
            .map_err(provider_error_to_dbus)?;
        Ok(charge_limit_info_to_tuple(info))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_core::action::ApplyResult;
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::identity::BackendIdentity;
    use orbis_core::newtypes::Percent;
    use orbis_providers::error::ValidationResult;
    use orbis_providers::traits::{BatteryProvider, Provider, ProviderHealth};
    use zbus::object_server::Interface;

    use super::*;
    use orbis_core::battery::ChargeLimitBounds;

    /// Заранее заданный исход scripted provider.
    #[derive(Debug, Clone, Copy)]
    enum ScriptedRead {
        Limit(ChargeLimit),
        Unsupported,
    }

    /// Тестовый BatteryProvider: очередь заранее заданных результатов,
    /// счётчик чтений; mutation-методы возвращают Unsupported без I/O.
    struct ScriptedBatteryProvider {
        results: Mutex<VecDeque<ScriptedRead>>,
        reads: AtomicUsize,
    }

    impl ScriptedBatteryProvider {
        fn new(reads: Vec<ScriptedRead>) -> Self {
            Self {
                results: Mutex::new(reads.into()),
                reads: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }

        fn to_result(read: ScriptedRead) -> Result<ChargeLimit, ProviderError> {
            match read {
                ScriptedRead::Limit(l) => Ok(l),
                ScriptedRead::Unsupported => {
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

    fn service(reads: Vec<ScriptedRead>) -> (SessionService, Arc<ScriptedBatteryProvider>) {
        let provider = Arc::new(ScriptedBatteryProvider::new(reads));
        let svc = SessionService::new(provider.clone());
        (svc, provider)
    }

    #[test]
    fn maps_domain_charge_limit_with_percent_to_wire() {
        let value = limit(true, Some(80), 40, 100, 5);
        let wire = charge_limit_to_wire(value);
        assert!(wire.enabled);
        assert!(wire.percent_present);
        assert_eq!(wire.percent, 80);
        assert_eq!(wire.percent(), Some(80));
        assert_eq!(wire.min_percent, 40);
        assert_eq!(wire.max_percent, 100);
        assert_eq!(wire.step_percent, 5);
    }

    #[test]
    fn maps_domain_charge_limit_without_percent_to_canonical_wire() {
        let value = limit(false, None, 40, 100, 5);
        let wire = charge_limit_to_wire(value);
        assert!(!wire.enabled);
        assert!(!wire.percent_present);
        assert_eq!(wire.percent, 0);
        assert_eq!(wire.percent(), None);
        assert_eq!(wire.min_percent, 40);
        assert_eq!(wire.max_percent, 100);
        assert_eq!(wire.step_percent, 5);
    }

    #[tokio::test]
    async fn service_reads_provider_once() {
        let (svc, provider) = service(vec![ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5))]);
        let wire = svc.read_charge_limit().await.expect("read");
        assert_eq!(wire.percent, 80);
        assert_eq!(provider.reads(), 1);
    }

    #[tokio::test]
    async fn service_does_not_cache_charge_limit() {
        let (svc, provider) = service(vec![
            ScriptedRead::Limit(limit(true, Some(80), 40, 100, 5)),
            ScriptedRead::Limit(limit(true, Some(60), 40, 100, 5)),
        ]);
        let first = svc.read_charge_limit().await.expect("read1");
        let second = svc.read_charge_limit().await.expect("read2");
        assert_eq!(first.percent, 80);
        assert_eq!(second.percent, 60);
        assert_eq!(provider.reads(), 2);
    }

    #[test]
    fn unsupported_maps_to_dbus_not_supported() {
        let err = provider_error_to_dbus(ProviderError::Unsupported("x".into()));
        assert!(matches!(err, zbus::fdo::Error::NotSupported(_)));
    }

    #[test]
    fn permission_denied_maps_to_dbus_access_denied() {
        let err = provider_error_to_dbus(ProviderError::PermissionDenied("x".into()));
        assert!(matches!(err, zbus::fdo::Error::AccessDenied(_)));
    }

    #[test]
    fn invalid_request_maps_to_dbus_invalid_args() {
        let err = provider_error_to_dbus(ProviderError::InvalidRequest("x".into()));
        assert!(matches!(err, zbus::fdo::Error::InvalidArgs(_)));
    }

    #[test]
    fn backend_errors_map_to_dbus_failed() {
        for err in [
            ProviderError::BackendUnavailable("b".into()),
            ProviderError::Timeout("t".into()),
            ProviderError::Dbus("d".into()),
            ProviderError::Internal("i".into()),
        ] {
            assert!(matches!(
                provider_error_to_dbus(err),
                zbus::fdo::Error::Failed(_)
            ));
        }
        let io = provider_error_to_dbus(ProviderError::Io(std::io::Error::other("io")));
        assert!(matches!(io, zbus::fdo::Error::Failed(_)));
    }

    #[test]
    fn server_interface_name_matches_protocol() {
        let name = <SessionService as Interface>::name();
        assert_eq!(name.to_string(), orbis_session_protocol::INTERFACE_NAME);
    }

    #[tokio::test]
    async fn dbus_property_preserves_authoritative_wire_value() {
        let (svc, provider) = service(vec![ScriptedRead::Limit(limit(true, Some(60), 40, 100, 5))]);
        // Прямой вызов async property getter (без ObjectServer).
        let wire = svc.charge_limit().await.expect("property");
        // (enabled, percent_present, percent, bounds_present, min, max, step)
        assert_eq!(wire, (true, true, 60, true, 40, 100, 5));
        assert_eq!(provider.reads(), 1);
    }

    #[tokio::test]
    async fn dbus_property_maps_unsupported_to_not_supported() {
        let (svc, _) = service(vec![ScriptedRead::Unsupported]);
        let err = svc.charge_limit().await.expect_err("unsupported");
        assert!(matches!(err, zbus::fdo::Error::NotSupported(_)));
    }
}
