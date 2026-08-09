//! Read-only supergfxd adapter для dGPU runtime power state.
//!
//! Реализует `GpuPowerProvider` (read-only power concept) через D-Bus метод
//! `Power()` интерфейса `org.supergfxctl.Daemon` (объект
//! `/org/supergfxctl/Gfx`).
//!
//! - source получает готовую `zbus::Connection` извне; crate сам bus не
//!   открывает и не создаёт runtime;
//! - каждый вызов `power_state()` выполняет новый authoritative D-Bus method
//!   call (кэш отсутствует);
//! - этот provider реализует ТОЛЬКО power capability (`GpuPowerProvider`) и НЕ
//!   предоставляет requested mode / MUX / access policy — эти concepts просто
//!   отсутствуют у capability, а не возвращают `Unsupported`;
//! - mapping `Power()` → `GpuPowerState` использует PROVEN enum definitions из
//!   локального authoritative API evidence (XML introspection установленного
//!   supergfxctl 5.2.7): 0=Active, 1=Suspended, 2=Off, 3=AsusDisabled,
//!   4=Unknown;
//! - `AsusDisabled` не моделируется текущим `GpuPowerState` и консервативно
//!   отображается в `Unknown` (НЕ в Off); неизвестное raw значение также →
//!   `Unknown` без clamp/fallback;
//! - никаких SetMode/SetConfig, никаких hardware writes.

use std::time::Duration;

use async_trait::async_trait;
use orbis_core::diagnostics::DiagnosticEntry;
use orbis_core::gpu::GpuPowerState;
use orbis_core::identity::BackendIdentity;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{GpuPowerProvider, Provider, ProviderHealth};
use zbus::proxy::CacheProperties;

/// Testable источник raw dGPU power state через supergfxd.
#[async_trait]
pub trait SupergfxdGpuPowerSource: Send + Sync {
    /// Прочитать authoritative raw power value (u32, без domain conversion).
    async fn read_power(&self) -> Result<u32, ProviderError>;
}

/// Реальный zbus источник через `org.supergfxctl.Daemon.Power()`.
///
/// Хранит переданную извне готовую `Connection`; I/O начинается только в
/// `read_power().await`. Конструктор не выполняет I/O.
pub struct ZbusSupergfxdGpuPowerSource {
    connection: zbus::Connection,
}

impl ZbusSupergfxdGpuPowerSource {
    /// Создать источник с готовой Connection.
    pub fn new(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

/// Минимальный read-only proxy-контракт supergfxd Daemon (только Power).
#[zbus::proxy(
    interface = "org.supergfxctl.Daemon",
    default_service = "org.supergfxctl.Daemon",
    default_path = "/org/supergfxctl/Gfx"
)]
trait SupergfxdDaemon {
    /// Текущий power state dGPU (read-only; не будит GPU).
    fn power(&self) -> zbus::Result<u32>;
}

#[async_trait]
impl SupergfxdGpuPowerSource for ZbusSupergfxdGpuPowerSource {
    async fn read_power(&self) -> Result<u32, ProviderError> {
        let proxy = SupergfxdDaemonProxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))?;
        proxy
            .power()
            .await
            .map_err(|e| ProviderError::Dbus(e.to_string()))
    }
}

/// Read-only GpuProvider для dGPU runtime power state над supergfxd source.
///
/// `S` — source (реальный zbus или scripted в тестах); source передаётся в
/// конструкторе, который не выполняет I/O.
pub struct SupergfxdGpuPowerProvider<S> {
    source: S,
}

impl<S> SupergfxdGpuPowerProvider<S> {
    /// Создать provider над source.
    ///
    /// Не открывает D-Bus connection и не выполняет I/O.
    pub fn new(source: S) -> Self {
        Self { source }
    }
}

impl<S> Provider for SupergfxdGpuPowerProvider<S>
where
    S: SupergfxdGpuPowerSource,
{
    fn id(&self) -> &'static str {
        "supergfxd-gpu-power"
    }

    fn backend(&self) -> BackendIdentity {
        BackendIdentity::simple("supergfxd-gpu-power")
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn explain_unsupported(&self, feature: &str) -> String {
        format!("supergfxd read-only power backend: функция '{feature}' недоступна")
    }

    fn health(&self) -> ProviderHealth {
        ProviderHealth::Healthy
    }

    fn diagnostics(&self) -> Vec<DiagnosticEntry> {
        vec![DiagnosticEntry::new(
            "provider.supergfxd-gpu-power",
            "read-only supergfxd dGPU power backend (без hardware writes)",
        )]
    }
}

/// Преобразовать raw `Power()` значение в `GpuPowerState` по PROVEN enum.
///
/// 0=Active, 1=Suspended, 2=Off — прямое отображение.
/// 3=AsusDisabled — не моделируется текущим domain; консервативно → Unknown
/// (НЕ Off).
/// 4=Unknown → Unknown.
/// Неизвестное значение → Unknown (без clamp/fallback/modulo).
fn power_from_raw(raw: u32) -> GpuPowerState {
    match raw {
        0 => GpuPowerState::Active,
        1 => GpuPowerState::Suspended,
        2 => GpuPowerState::Off,
        // AsusDisabled (3) и Unknown (4): специфические состояния, частично
        // отсутствующие в текущем GpuPowerState; консервативно → Unknown
        // (AsusDisabled НЕ отображаем как Off без contract evidence).
        _ => GpuPowerState::Unknown,
    }
}

#[async_trait]
impl<S> GpuPowerProvider for SupergfxdGpuPowerProvider<S>
where
    S: SupergfxdGpuPowerSource,
{
    async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
        let raw = self.source.read_power().await?;
        Ok(power_from_raw(raw))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Тестовый источник: очередь заранее заданных raw values, счётчик вызовов.
    struct ScriptedSource {
        results: std::sync::Mutex<std::collections::VecDeque<Result<u32, ProviderError>>>,
        reads: AtomicUsize,
    }

    impl ScriptedSource {
        fn new(results: Vec<Result<u32, ProviderError>>) -> Self {
            Self {
                results: std::sync::Mutex::new(results.into()),
                reads: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SupergfxdGpuPowerSource for ScriptedSource {
        async fn read_power(&self) -> Result<u32, ProviderError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.results.lock().unwrap().pop_front().ok_or_else(|| {
                ProviderError::Internal("scripted source: очередь исчерпана".into())
            })?
        }
    }

    fn provider(
        results: Vec<Result<u32, ProviderError>>,
    ) -> SupergfxdGpuPowerProvider<ScriptedSource> {
        SupergfxdGpuPowerProvider::new(ScriptedSource::new(results))
    }

    #[tokio::test]
    async fn maps_proven_power_values() {
        let cases = [
            (0, GpuPowerState::Active),
            (1, GpuPowerState::Suspended),
            (2, GpuPowerState::Off),
            // 3=AsusDisabled консервативно → Unknown (не Off).
            (3, GpuPowerState::Unknown),
            // 4=Unknown → Unknown.
            (4, GpuPowerState::Unknown),
        ];
        for (raw, expected) in cases {
            let p = provider(vec![Ok(raw)]);
            assert_eq!(p.power_state().await.expect("power"), expected, "raw {raw}");
        }
    }

    #[tokio::test]
    async fn unknown_raw_is_unknown_without_guess() {
        let p = provider(vec![Ok(7)]);
        assert_eq!(
            p.power_state().await.expect("power"),
            GpuPowerState::Unknown
        );
    }

    #[tokio::test]
    async fn source_error_propagates() {
        let p = provider(vec![Err(ProviderError::Dbus("backend down".into()))]);
        let err = p.power_state().await.expect_err("source error");
        assert!(matches!(err, ProviderError::Dbus(_)));
    }

    #[tokio::test]
    async fn reads_are_fresh_not_cached() {
        let source = ScriptedSource::new(vec![Ok(0), Ok(1)]);
        let p = SupergfxdGpuPowerProvider::new(source);
        assert_eq!(p.power_state().await.expect("first"), GpuPowerState::Active);
        assert_eq!(
            p.power_state().await.expect("second"),
            GpuPowerState::Suspended
        );
        // Каждый вызов делает новый source call; кэш отсутствует.
        assert_eq!(p.source.reads(), 2);
    }
}
