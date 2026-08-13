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

/// Typed supergfxd 5.2.7 graphics mode, kept separate from product `GpuMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupergfxdMode {
    /// Hybrid/Optimus mode.
    Hybrid,
    /// Integrated GPU mode.
    Integrated,
    /// NVIDIA without modeset.
    NvidiaNoModeset,
    /// VFIO mode.
    Vfio,
    /// ASUS eGPU mode.
    AsusEgpu,
    /// ASUS MUX discrete GPU mode.
    AsusMuxDgpu,
    /// No current/pending mode.
    None,
    /// A future wire value not known by this version.
    Unknown(u32),
}

impl SupergfxdMode {
    /// Decode the exact supergfxd 5.2.7 wire discriminant.
    pub fn from_wire(raw: u32) -> Self {
        match raw {
            0 => Self::Hybrid,
            1 => Self::Integrated,
            2 => Self::NvidiaNoModeset,
            3 => Self::Vfio,
            4 => Self::AsusEgpu,
            5 => Self::AsusMuxDgpu,
            6 => Self::None,
            other => Self::Unknown(other),
        }
    }
}

/// Typed supergfxd 5.2.7 user action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupergfxdUserAction {
    /// Logout is required.
    Logout,
    /// Reboot is required.
    Reboot,
    /// Switch to Integrated first.
    SwitchToIntegrated,
    /// Disable ASUS eGPU.
    AsusEgpuDisable,
    /// No user action is required.
    Nothing,
    /// A future wire value not known by this version.
    Unknown(u32),
}

impl SupergfxdUserAction {
    /// Decode the exact supergfxd 5.2.7 wire discriminant.
    pub fn from_wire(raw: u32) -> Self {
        match raw {
            0 => Self::Logout,
            1 => Self::Reboot,
            2 => Self::SwitchToIntegrated,
            3 => Self::AsusEgpuDisable,
            4 => Self::Nothing,
            other => Self::Unknown(other),
        }
    }
}

/// Read-only supergfxd state required to classify a staged request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupergfxdSnapshot {
    /// Current backend mode.
    pub current_mode: SupergfxdMode,
    /// Pending backend mode, or `None` when no mode is pending.
    pub pending_mode: SupergfxdMode,
    /// Pending user action, or `Nothing` when none is pending.
    pub pending_user_action: SupergfxdUserAction,
    /// Current dGPU power state.
    pub power: GpuPowerState,
    /// Supported backend modes.
    pub supported_modes: Vec<SupergfxdMode>,
}

/// Classification of a fresh staged supergfxd snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupergfxdStagedState {
    /// The requested mode is applied and no staged state remains.
    Applied,
    /// The requested mode is still staged without a user action.
    Pending,
    /// The requested mode is staged and requires this user action.
    RequiresUserAction(SupergfxdUserAction),
    /// The snapshot cannot honestly be classified as success.
    Inconsistent,
}

/// Classify a requested backend mode against one fresh snapshot.
pub fn classify_supergfxd_state(
    requested: SupergfxdMode,
    snapshot: &SupergfxdSnapshot,
) -> SupergfxdStagedState {
    let mode_values_are_known = !matches!(requested, SupergfxdMode::Unknown(_))
        && !matches!(snapshot.current_mode, SupergfxdMode::Unknown(_))
        && !matches!(snapshot.pending_mode, SupergfxdMode::Unknown(_));
    let pending_matches = snapshot.pending_mode == requested;
    let pending_is_none = snapshot.pending_mode == SupergfxdMode::None;
    let action_is_nothing = snapshot.pending_user_action == SupergfxdUserAction::Nothing;
    let action_is_known = !matches!(
        snapshot.pending_user_action,
        SupergfxdUserAction::Unknown(_)
    );

    if !mode_values_are_known
        || !action_is_known
        || (snapshot.pending_user_action != SupergfxdUserAction::Nothing && pending_is_none)
        || (!pending_is_none && !pending_matches)
        || (snapshot.current_mode == requested && !pending_is_none)
    {
        return SupergfxdStagedState::Inconsistent;
    }

    if snapshot.current_mode == requested && pending_is_none && action_is_nothing {
        return SupergfxdStagedState::Applied;
    }

    if pending_matches {
        return if action_is_nothing {
            SupergfxdStagedState::Pending
        } else {
            SupergfxdStagedState::RequiresUserAction(snapshot.pending_user_action)
        };
    }

    SupergfxdStagedState::Inconsistent
}

/// Testable источник raw dGPU power state через supergfxd.
#[async_trait]
pub trait SupergfxdGpuPowerSource: Send + Sync {
    /// Прочитать authoritative raw power value (u32, без domain conversion).
    async fn read_power(&self) -> Result<u32, ProviderError>;
}

/// Read-only source for the complete supergfxd staged snapshot.
#[async_trait]
pub trait SupergfxdGpuSnapshotSource: Send + Sync {
    /// Read all staged-contract fields from fresh D-Bus calls.
    async fn read_snapshot(&self) -> Result<SupergfxdSnapshot, ProviderError>;
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
    /// Current backend mode.
    fn mode(&self) -> zbus::Result<u32>;
    /// Pending backend mode.
    fn pending_mode(&self) -> zbus::Result<u32>;
    /// Pending user action.
    fn pending_user_action(&self) -> zbus::Result<u32>;
    /// Текущий power state dGPU (read-only; не будит GPU).
    fn power(&self) -> zbus::Result<u32>;
    /// Supported backend modes.
    fn supported(&self) -> zbus::Result<Vec<u32>>;
}

fn zbus_error_to_provider(error: zbus::Error) -> ProviderError {
    if let zbus::Error::FDO(boxed) = &error {
        return match &**boxed {
            zbus::fdo::Error::NotSupported(msg) => ProviderError::Unsupported(msg.clone()),
            zbus::fdo::Error::AccessDenied(msg) => ProviderError::PermissionDenied(msg.clone()),
            zbus::fdo::Error::InvalidArgs(msg) => ProviderError::InvalidRequest(msg.clone()),
            _ => ProviderError::Dbus(error.to_string()),
        };
    }
    ProviderError::Dbus(error.to_string())
}

impl ZbusSupergfxdGpuPowerSource {
    async fn proxy(&self) -> Result<SupergfxdDaemonProxy<'_>, ProviderError> {
        SupergfxdDaemonProxy::builder(&self.connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(zbus_error_to_provider)
    }
}

#[async_trait]
impl SupergfxdGpuPowerSource for ZbusSupergfxdGpuPowerSource {
    async fn read_power(&self) -> Result<u32, ProviderError> {
        let proxy = self.proxy().await?;
        proxy.power().await.map_err(zbus_error_to_provider)
    }
}

#[async_trait]
impl SupergfxdGpuSnapshotSource for ZbusSupergfxdGpuPowerSource {
    async fn read_snapshot(&self) -> Result<SupergfxdSnapshot, ProviderError> {
        let proxy = self.proxy().await?;
        let current_mode =
            SupergfxdMode::from_wire(proxy.mode().await.map_err(zbus_error_to_provider)?);
        let pending_mode =
            SupergfxdMode::from_wire(proxy.pending_mode().await.map_err(zbus_error_to_provider)?);
        let pending_user_action = SupergfxdUserAction::from_wire(
            proxy
                .pending_user_action()
                .await
                .map_err(zbus_error_to_provider)?,
        );
        let power = power_from_raw(proxy.power().await.map_err(zbus_error_to_provider)?);
        let supported_modes = proxy
            .supported()
            .await
            .map_err(zbus_error_to_provider)?
            .into_iter()
            .map(SupergfxdMode::from_wire)
            .collect();

        Ok(SupergfxdSnapshot {
            current_mode,
            pending_mode,
            pending_user_action,
            power,
            supported_modes,
        })
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

    fn snapshot(
        current_mode: SupergfxdMode,
        pending_mode: SupergfxdMode,
        pending_user_action: SupergfxdUserAction,
    ) -> SupergfxdSnapshot {
        SupergfxdSnapshot {
            current_mode,
            pending_mode,
            pending_user_action,
            power: GpuPowerState::Suspended,
            supported_modes: vec![
                SupergfxdMode::Hybrid,
                SupergfxdMode::Integrated,
                SupergfxdMode::AsusMuxDgpu,
            ],
        }
    }

    #[test]
    fn decodes_exact_modes_and_actions_without_panicking() {
        assert_eq!(SupergfxdMode::from_wire(0), SupergfxdMode::Hybrid);
        assert_eq!(SupergfxdMode::from_wire(1), SupergfxdMode::Integrated);
        assert_eq!(SupergfxdMode::from_wire(2), SupergfxdMode::NvidiaNoModeset);
        assert_eq!(SupergfxdMode::from_wire(3), SupergfxdMode::Vfio);
        assert_eq!(SupergfxdMode::from_wire(4), SupergfxdMode::AsusEgpu);
        assert_eq!(SupergfxdMode::from_wire(5), SupergfxdMode::AsusMuxDgpu);
        assert_eq!(SupergfxdMode::from_wire(6), SupergfxdMode::None);
        assert_eq!(SupergfxdMode::from_wire(99), SupergfxdMode::Unknown(99));
        assert_eq!(
            SupergfxdUserAction::from_wire(0),
            SupergfxdUserAction::Logout
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(1),
            SupergfxdUserAction::Reboot
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(2),
            SupergfxdUserAction::SwitchToIntegrated
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(3),
            SupergfxdUserAction::AsusEgpuDisable
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(4),
            SupergfxdUserAction::Nothing
        );
        assert_eq!(
            SupergfxdUserAction::from_wire(99),
            SupergfxdUserAction::Unknown(99)
        );
    }

    #[test]
    fn classifies_staged_contract_without_product_mode_mapping() {
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::Integrated,
                &snapshot(
                    SupergfxdMode::Integrated,
                    SupergfxdMode::None,
                    SupergfxdUserAction::Nothing,
                ),
            ),
            SupergfxdStagedState::Applied
        );
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::Integrated,
                &snapshot(
                    SupergfxdMode::Hybrid,
                    SupergfxdMode::Integrated,
                    SupergfxdUserAction::Logout,
                ),
            ),
            SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Logout)
        );
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::AsusMuxDgpu,
                &snapshot(
                    SupergfxdMode::Hybrid,
                    SupergfxdMode::AsusMuxDgpu,
                    SupergfxdUserAction::Reboot,
                ),
            ),
            SupergfxdStagedState::RequiresUserAction(SupergfxdUserAction::Reboot)
        );
        assert_eq!(
            classify_supergfxd_state(
                SupergfxdMode::Hybrid,
                &snapshot(
                    SupergfxdMode::Integrated,
                    SupergfxdMode::Hybrid,
                    SupergfxdUserAction::Nothing,
                ),
            ),
            SupergfxdStagedState::Pending
        );
    }

    #[test]
    fn rejects_contradictory_staged_snapshots() {
        let cases = [
            (
                SupergfxdMode::Integrated,
                snapshot(
                    SupergfxdMode::Hybrid,
                    SupergfxdMode::AsusMuxDgpu,
                    SupergfxdUserAction::Nothing,
                ),
            ),
            (
                SupergfxdMode::Integrated,
                snapshot(
                    SupergfxdMode::Hybrid,
                    SupergfxdMode::None,
                    SupergfxdUserAction::Logout,
                ),
            ),
            (
                SupergfxdMode::Integrated,
                snapshot(
                    SupergfxdMode::Integrated,
                    SupergfxdMode::Hybrid,
                    SupergfxdUserAction::Nothing,
                ),
            ),
            (
                SupergfxdMode::Integrated,
                snapshot(
                    SupergfxdMode::Hybrid,
                    SupergfxdMode::Integrated,
                    SupergfxdUserAction::Unknown(9),
                ),
            ),
            (
                SupergfxdMode::Unknown(9),
                snapshot(
                    SupergfxdMode::Unknown(9),
                    SupergfxdMode::None,
                    SupergfxdUserAction::Nothing,
                ),
            ),
        ];
        for (requested, state) in cases {
            assert_eq!(
                classify_supergfxd_state(requested, &state),
                SupergfxdStagedState::Inconsistent
            );
        }
    }
}
