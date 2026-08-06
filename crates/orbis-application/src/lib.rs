//! # orbis-application
//!
//! Application boundary между пользовательским интерфейсом и provider traits.
//!
//! - crate не содержит UI/Slint и transport/D-Bus деталей (он от них не зависит);
//! - API остаётся async: вызов из синхронного Slint-callback решается отдельным
//!   worker/runtime boundary, здесь runtime не создаётся;
//! - provider state перечитывается после каждой команды (authoritative read-back);
//! - состояние не кэшируется внутри сервиса.
//!
//! На текущем этапе поддерживаются Performance Mode и Battery Charge Limit.
//! В будущем слой смогут использовать orbis-ui (через асинхронный worker),
//! orbis-sessiond, CLI и интеграционные тесты.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use orbis_core::action::{ActionRequirement, ApplyResult};
use orbis_core::battery::ChargeLimit;
use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::{BatteryProvider, GpuProvider, PerformanceProvider};

/// Authoritative состояние Performance Mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceState {
    /// Текущий профиль (из provider read-back).
    pub current: PerformanceProfile,
    /// Доступные профили; порядок совпадает с ответом provider.
    pub available: Vec<PerformanceProfile>,
}

/// Общий результат application-команды: исходный `ApplyResult` + authoritative
/// состояние после операции.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome<S> {
    /// Результат мутации, возвращённый provider-ом без преобразований.
    pub result: ApplyResult,
    /// Состояние, перечитанное из provider после команды.
    pub state: S,
}

/// Результат команды Performance Mode (alias общего `CommandOutcome`).
pub type PerformanceCommandOutcome = CommandOutcome<PerformanceState>;

/// Результат команды Battery Charge Limit: authoritative состояние — это
/// существующий `ChargeLimit` из provider read-back.
pub type ChargeLimitCommandOutcome = CommandOutcome<ChargeLimit>;

/// Общая ошибка application-команды.
///
/// Различает два принципиально разных случая:
/// - команда provider не выполнилась (или была отклонена);
/// - команда выполнилась (есть `ApplyResult`), но обязательный authoritative
///   read-back после неё завершился ошибкой.
#[derive(Debug)]
pub enum CommandError {
    /// Ошибка самой команды provider: мутация не выполнена или отклонена.
    Command(ProviderError),
    /// Команда выполнена (сохранён `ApplyResult`), но повторное чтение
    /// authoritative state после неё завершилось ошибкой.
    ReadBack {
        /// Результат выполненной мутации.
        result: ApplyResult,
        /// Ошибка повторного чтения состояния.
        source: ProviderError,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(e) => write!(f, "application-команда не выполнена: {e}"),
            Self::ReadBack { result, source } => write!(
                f,
                "application-команда выполнена ({result:?}), но read-back не удался: {source}"
            ),
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Command(e) => Some(e),
            Self::ReadBack { source, .. } => Some(source),
        }
    }
}

/// Ошибка команды Performance Mode (alias общего `CommandError`).
pub type SetPerformanceError = CommandError;

/// Ошибка команды Battery Charge Limit (alias общего `CommandError`).
pub type SetChargeLimitError = CommandError;

/// Application service.
///
/// Владеет provider через `Arc<P>` и предоставляет типизированные async-команды.
/// Не привязан к конкретному provider: методы доступны в зависимости от того,
/// какие provider traits реализует `P` (независимые impl-блоки).
pub struct AppService<P> {
    provider: Arc<P>,
}

impl<P> AppService<P> {
    /// Создать сервис над провайдером.
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }
}

impl<P> AppService<P>
where
    P: PerformanceProvider + Send + Sync,
{
    /// Прочитать authoritative состояние Performance Mode.
    ///
    /// Данные берутся только из ответов provider; внутренний кэш отсутствует.
    pub async fn performance_state(&self) -> Result<PerformanceState, ProviderError> {
        let current = self.provider.current_profile().await?;
        let available = self.provider.profiles().await?;
        Ok(PerformanceState { current, available })
    }

    /// Установить профиль производительности.
    ///
    /// 1. Вызывает `PerformanceProvider::set_profile`.
    /// 2. После успешного provider-вызова перечитывает состояние.
    /// 3. Возвращает исходный `ApplyResult` и authoritative `PerformanceState`
    ///    (итоговый `current` приходит из read-back, а не из предположения).
    ///
    /// Ошибка мутации возвращается как `SetPerformanceError::Command`;
    /// ошибка read-back после успешной мутации — как
    /// `SetPerformanceError::ReadBack` с сохранённым `ApplyResult`.
    pub async fn set_performance(
        &self,
        profile: PerformanceProfile,
    ) -> Result<PerformanceCommandOutcome, SetPerformanceError> {
        let result = self
            .provider
            .set_profile(profile)
            .await
            .map_err(CommandError::Command)?;
        let state = self
            .performance_state()
            .await
            .map_err(|source| CommandError::ReadBack {
                result: result.clone(),
                source,
            })?;
        Ok(CommandOutcome { result, state })
    }
}

impl<P> AppService<P>
where
    P: BatteryProvider + Send + Sync,
{
    /// Прочитать authoritative Battery Charge Limit.
    ///
    /// Данные берутся только из `BatteryProvider::charge_limit`; кэш отсутствует.
    pub async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
        self.provider.charge_limit().await
    }

    /// Установить лимит зарядки.
    ///
    /// 1. Вызывает `BatteryProvider::set_charge_limit(percent)` без нормализации
    ///    (диапазон/шаг проверяет сам provider).
    /// 2. После успешного provider-вызова перечитывает authoritative лимит.
    /// 3. Возвращает исходный `ApplyResult` и `ChargeLimit` из read-back.
    ///
    /// Ошибка мутации — `SetChargeLimitError::Command`; ошибка read-back после
    /// успешной мутации — `SetChargeLimitError::ReadBack` с сохранённым
    /// `ApplyResult`.
    pub async fn set_charge_limit(
        &self,
        percent: u8,
    ) -> Result<ChargeLimitCommandOutcome, SetChargeLimitError> {
        let result = self
            .provider
            .set_charge_limit(percent)
            .await
            .map_err(CommandError::Command)?;
        let state = self
            .charge_limit()
            .await
            .map_err(|source| CommandError::ReadBack {
                result: result.clone(),
                source,
            })?;
        Ok(CommandOutcome { result, state })
    }
}

/// Authoritative состояние GPU Mode.
///
/// Собирается только через существующий `GpuProvider` (несколько независимых
/// async reads); отдельные поля не объединяются в упрощённый enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuState {
    /// Запрошенный режим (`GpuProvider::requested_mode`).
    pub requested: GpuMode,
    /// Applied физический MUX (`GpuProvider::mux_state`).
    pub mux: GpuMuxState,
    /// Applied доступ приложений к dGPU (`GpuProvider::access_policy`).
    pub access_policy: GpuAccessPolicy,
    /// Applied power state dGPU (`GpuProvider::power_state`).
    pub power_state: GpuPowerState,
    /// Требование для запрошенного режима (`GpuProvider::requirement_for`).
    pub requirement: ActionRequirement,
}

/// Результат команды GPU Mode (alias общего `CommandOutcome`).
pub type GpuCommandOutcome = CommandOutcome<GpuState>;

/// Ошибка команды GPU Mode (alias общего `CommandError`).
pub type SetGpuModeError = CommandError;

impl<P> AppService<P>
where
    P: GpuProvider + Send + Sync,
{
    /// Прочитать authoritative GPU-состояние.
    ///
    /// Поля читаются через отдельные provider методы; внутренний кэш отсутствует.
    /// Примечание: составной snapshot неатомарен (см. известные ограничения).
    pub async fn gpu_state(&self) -> Result<GpuState, ProviderError> {
        let requested = self.provider.requested_mode().await?;
        let mux = self.provider.mux_state().await?;
        let access_policy = self.provider.access_policy().await?;
        let power_state = self.provider.power_state().await?;
        let requirement = self.provider.requirement_for(requested);
        Ok(GpuState {
            requested,
            mux,
            access_policy,
            power_state,
            requirement,
        })
    }

    /// Установить GPU Mode.
    ///
    /// 1. Вызывает `GpuProvider::set_mode(mode, confirmed)` без преобразования
    ///    флага `confirmed` и без собственной валидации режима.
    /// 2. После успешного provider-вызова перечитывает authoritative GPU state.
    /// 3. Возвращает исходный `ApplyResult` и `GpuState` из read-back.
    ///
    /// Ошибка мутации — `SetGpuModeError::Command`; ошибка read-back после
    /// успешной мутации — `SetGpuModeError::ReadBack` с сохранённым
    /// `ApplyResult`.
    pub async fn set_gpu_mode(
        &self,
        mode: GpuMode,
        confirmed: bool,
    ) -> Result<GpuCommandOutcome, SetGpuModeError> {
        let result = self
            .provider
            .set_mode(mode, confirmed)
            .await
            .map_err(CommandError::Command)?;
        let state = self
            .gpu_state()
            .await
            .map_err(|source| CommandError::ReadBack {
                result: result.clone(),
                source,
            })?;
        Ok(CommandOutcome { result, state })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use orbis_core::action::{ActionRequirement, ApplyResult};
    use orbis_core::battery::ChargeLimit;
    use orbis_core::gpu::{GpuAccessPolicy, GpuMode, GpuMuxState, GpuPowerState};
    use orbis_core::identity::BackendIdentity;
    use orbis_core::newtypes::Percent;
    use orbis_core::profile::PerformanceProfile;
    use orbis_providers::error::{ProviderError, ValidationResult};
    use orbis_providers::mock::{MockErrorMode, MockProvider};
    use orbis_providers::traits::{
        BatteryProvider, GpuProvider, PerformanceProvider, Provider, ProviderHealth,
    };
    use orbis_test_support::devices::build_state_arc;

    use super::{
        AppService, ChargeLimitCommandOutcome, CommandError, GpuCommandOutcome, PerformanceState,
    };

    fn service() -> (Arc<MockProvider>, AppService<MockProvider>) {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state));
        let svc = AppService::new(provider.clone());
        (provider, svc)
    }

    // -----------------------------------------------------------------------
    // Performance
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn performance_state_zephyrus() {
        let (_provider, svc) = service();
        let st: PerformanceState = svc.performance_state().await.unwrap();
        assert_eq!(st.current, PerformanceProfile::Balanced);
        assert_eq!(
            st.available,
            vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ]
        );
    }

    #[tokio::test]
    async fn balanced_to_silent() {
        let (_provider, svc) = service();
        let outcome = svc
            .set_performance(PerformanceProfile::Silent)
            .await
            .unwrap();
        assert!(outcome.result.is_applied());
        assert_eq!(outcome.state.current, PerformanceProfile::Silent);
        assert_eq!(outcome.state.available.len(), 3);
    }

    #[tokio::test]
    async fn silent_to_turbo() {
        let (_provider, svc) = service();
        svc.set_performance(PerformanceProfile::Silent)
            .await
            .unwrap();
        let outcome = svc
            .set_performance(PerformanceProfile::Turbo)
            .await
            .unwrap();
        assert_eq!(outcome.state.current, PerformanceProfile::Turbo);
    }

    #[tokio::test]
    async fn repeated_current_is_idempotent() {
        let (_provider, svc) = service();
        let first = svc.performance_state().await.unwrap();
        let outcome = svc
            .set_performance(PerformanceProfile::Balanced)
            .await
            .unwrap();
        assert!(outcome.result.is_applied());
        assert_eq!(outcome.state.current, PerformanceProfile::Balanced);
        assert_eq!(outcome.state.available, first.available);
    }

    #[tokio::test]
    async fn backend_down_returns_command_error_without_mutation() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let svc = AppService::new(provider.clone());

        state.write().await.error_mode = MockErrorMode::BackendDown;
        let err = svc
            .set_performance(PerformanceProfile::Silent)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            CommandError::Command(ProviderError::BackendUnavailable(_))
        ));

        // После снятия ошибки исходный профиль прежний (мутация не применялась).
        state.write().await.error_mode = MockErrorMode::None;
        let st = svc.performance_state().await.unwrap();
        assert_eq!(st.current, PerformanceProfile::Balanced);
    }

    #[tokio::test]
    async fn permission_denied_returns_command_error() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let svc = AppService::new(provider.clone());

        state.write().await.error_mode = MockErrorMode::PermissionDenied;
        let err = svc
            .set_performance(PerformanceProfile::Silent)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            CommandError::Command(ProviderError::PermissionDenied(_))
        ));

        state.write().await.error_mode = MockErrorMode::None;
        let st = svc.performance_state().await.unwrap();
        assert_eq!(st.current, PerformanceProfile::Balanced);
    }

    #[tokio::test]
    async fn snapshot_is_readback_not_cached() {
        let (provider, svc) = service();
        // Прямое изменение provider через trait (вне AppService).
        provider
            .set_profile(PerformanceProfile::Turbo)
            .await
            .unwrap();
        // Следующий snapshot отражает новое provider state (кэша нет).
        let st = svc.performance_state().await.unwrap();
        assert_eq!(st.current, PerformanceProfile::Turbo);
    }

    // -----------------------------------------------------------------------
    // Battery Charge Limit
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn initial_charge_limit() {
        let (_provider, svc) = service();
        let cl = svc.charge_limit().await.unwrap();
        assert_eq!(cl.percent, Some(Percent::new(80).unwrap()));
        assert!(cl.enabled);
        assert_eq!(cl.min, Percent::new(40).unwrap());
        assert_eq!(cl.max, Percent::new(100).unwrap());
    }

    #[tokio::test]
    async fn charge_limit_80_to_40() {
        let (_provider, svc) = service();
        let outcome: ChargeLimitCommandOutcome = svc.set_charge_limit(40).await.unwrap();
        assert!(outcome.result.is_applied());
        assert_eq!(outcome.state.percent, Some(Percent::new(40).unwrap()));
        // Остальные поля не повреждены.
        assert!(outcome.state.enabled);
        assert_eq!(outcome.state.min, Percent::new(40).unwrap());
        assert_eq!(outcome.state.max, Percent::new(100).unwrap());
    }

    #[tokio::test]
    async fn charge_limit_40_to_100() {
        let (_provider, svc) = service();
        svc.set_charge_limit(40).await.unwrap();
        let outcome = svc.set_charge_limit(100).await.unwrap();
        assert_eq!(outcome.state.percent, Some(Percent::new(100).unwrap()));
    }

    #[tokio::test]
    async fn repeated_charge_limit_is_idempotent() {
        let (_provider, svc) = service();
        let first = svc.set_charge_limit(100).await.unwrap();
        let second = svc.set_charge_limit(100).await.unwrap();
        assert!(second.result.is_applied());
        assert_eq!(second.state.percent, Some(Percent::new(100).unwrap()));
        assert_eq!(second.state, first.state);
    }

    #[tokio::test]
    async fn value_83_follows_provider_semantics() {
        let (_provider, svc) = service();
        // Фактическая семантика MockProvider: проверяется только диапазон
        // 40..=100 (шаг 5 провайдер не проверяет), поэтому 83 принимается.
        // AppService не нормализует и не округляет значение.
        let outcome = svc.set_charge_limit(83).await.unwrap();
        assert!(outcome.result.is_applied());
        assert_eq!(outcome.state.percent, Some(Percent::new(83).unwrap()));
    }

    #[tokio::test]
    async fn charge_backend_down_returns_command_error_without_mutation() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let svc = AppService::new(provider.clone());

        state.write().await.error_mode = MockErrorMode::BackendDown;
        let err = svc.set_charge_limit(60).await.unwrap_err();
        assert!(matches!(
            err,
            CommandError::Command(ProviderError::BackendUnavailable(_))
        ));

        state.write().await.error_mode = MockErrorMode::None;
        let cl = svc.charge_limit().await.unwrap();
        assert_eq!(cl.percent, Some(Percent::new(80).unwrap()));
    }

    #[tokio::test]
    async fn charge_permission_denied_returns_command_error() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let svc = AppService::new(provider.clone());

        state.write().await.error_mode = MockErrorMode::PermissionDenied;
        let err = svc.set_charge_limit(60).await.unwrap_err();
        assert!(matches!(
            err,
            CommandError::Command(ProviderError::PermissionDenied(_))
        ));

        state.write().await.error_mode = MockErrorMode::None;
        let cl = svc.charge_limit().await.unwrap();
        assert_eq!(cl.percent, Some(Percent::new(80).unwrap()));
    }

    #[tokio::test]
    async fn charge_snapshot_is_readback_not_cached() {
        let (provider, svc) = service();
        // Прямое изменение provider через trait (вне AppService).
        provider.set_charge_limit(60).await.unwrap();
        let cl = svc.charge_limit().await.unwrap();
        assert_eq!(cl.percent, Some(Percent::new(60).unwrap()));
    }

    // -----------------------------------------------------------------------
    // Scripted providers: команда выполняется, но следующий read-back падает.
    // -----------------------------------------------------------------------

    /// Тестовый провайдер Performance: `set_profile` всегда успешен и применяет
    /// профиль; `current_profile` может быть принудительно переведён в ошибку.
    struct ScriptedProvider {
        current: tokio::sync::RwLock<PerformanceProfile>,
        fail_reads: AtomicBool,
    }

    impl ScriptedProvider {
        fn new() -> Self {
            Self {
                current: tokio::sync::RwLock::new(PerformanceProfile::Balanced),
                fail_reads: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn id(&self) -> &'static str {
            "scripted"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted")
        }

        fn timeout(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<orbis_core::diagnostics::DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl PerformanceProvider for ScriptedProvider {
        async fn profiles(&self) -> Result<Vec<PerformanceProfile>, ProviderError> {
            Ok(vec![
                PerformanceProfile::Silent,
                PerformanceProfile::Balanced,
                PerformanceProfile::Turbo,
            ])
        }

        async fn current_profile(&self) -> Result<PerformanceProfile, ProviderError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable(
                    "scripted read failure".into(),
                ));
            }
            Ok(*self.current.read().await)
        }

        async fn set_profile(
            &self,
            profile: PerformanceProfile,
        ) -> Result<ApplyResult, ProviderError> {
            *self.current.write().await = profile;
            Ok(ApplyResult::Applied)
        }

        async fn profile_on_ac(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        async fn profile_on_battery(&self) -> Result<Option<PerformanceProfile>, ProviderError> {
            Ok(None)
        }

        fn validate_set_profile(&self, profile: PerformanceProfile) -> ValidationResult {
            match profile {
                PerformanceProfile::Silent
                | PerformanceProfile::Balanced
                | PerformanceProfile::Turbo => ValidationResult::Valid,
            }
        }
    }

    #[tokio::test]
    async fn performance_readback_error_preserves_apply_result() {
        let provider = Arc::new(ScriptedProvider::new());
        let svc = AppService::new(provider.clone());

        provider.fail_reads.store(true, Ordering::SeqCst);
        let err = svc
            .set_performance(PerformanceProfile::Silent)
            .await
            .unwrap_err();

        match err {
            CommandError::ReadBack { result, source } => {
                assert!(result.is_applied());
                assert!(matches!(source, ProviderError::BackendUnavailable(_)));
            }
            other => panic!("ожидался ReadBack, получен: {other:?}"),
        }

        assert_eq!(*provider.current.read().await, PerformanceProfile::Silent);
    }

    /// Тестовый провайдер Battery: `set_charge_limit` всегда успешен и применяет
    /// значение; `charge_limit` может быть принудительно переведён в ошибку.
    struct ScriptedBatteryProvider {
        limit: tokio::sync::RwLock<u8>,
        fail_reads: AtomicBool,
    }

    impl ScriptedBatteryProvider {
        fn new() -> Self {
            Self {
                limit: tokio::sync::RwLock::new(80),
                fail_reads: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedBatteryProvider {
        fn id(&self) -> &'static str {
            "scripted-battery"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-battery")
        }

        fn timeout(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-battery: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<orbis_core::diagnostics::DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl BatteryProvider for ScriptedBatteryProvider {
        async fn charge_limit(&self) -> Result<ChargeLimit, ProviderError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable(
                    "scripted battery read failure".into(),
                ));
            }
            let p = *self.limit.read().await;
            Ok(ChargeLimit::new(
                true,
                Some(Percent::new(p).expect("range")),
                Percent::new(40).expect("const"),
                Percent::new(100).expect("const"),
                1,
            )
            .expect("valid"))
        }

        async fn set_charge_limit(&self, percent: u8) -> Result<ApplyResult, ProviderError> {
            *self.limit.write().await = percent;
            Ok(ApplyResult::Applied)
        }

        async fn one_shot_full_charge(&self) -> Result<ApplyResult, ProviderError> {
            Ok(ApplyResult::Applied)
        }

        fn validate_charge_limit(&self, _percent: u8) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    #[tokio::test]
    async fn battery_readback_error_preserves_apply_result() {
        let provider = Arc::new(ScriptedBatteryProvider::new());
        let svc = AppService::new(provider.clone());

        provider.fail_reads.store(true, Ordering::SeqCst);
        let err = svc.set_charge_limit(40).await.unwrap_err();

        match err {
            CommandError::ReadBack { result, source } => {
                assert!(result.is_applied());
                assert!(matches!(source, ProviderError::BackendUnavailable(_)));
            }
            other => panic!("ожидался ReadBack, получен: {other:?}"),
        }

        // Команда действительно применилась, несмотря на неудачный read-back.
        assert_eq!(*provider.limit.read().await, 40);
    }

    // -----------------------------------------------------------------------
    // GPU Mode
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn initial_gpu_state_zephyrus() {
        let (_provider, svc) = service();
        let st = svc.gpu_state().await.unwrap();
        assert_eq!(st.requested, GpuMode::Standard);
        assert_eq!(st.mux, GpuMuxState::Integrated);
        assert_eq!(st.access_policy, GpuAccessPolicy::Unblocked);
        assert_eq!(st.power_state, GpuPowerState::Active);
        assert_eq!(st.requirement, ActionRequirement::None);
    }

    #[tokio::test]
    async fn applied_mode_optimized_readback() {
        let (_provider, svc) = service();
        let outcome: GpuCommandOutcome = svc.set_gpu_mode(GpuMode::Optimized, true).await.unwrap();
        // MockProvider применяет Optimized немедленно.
        assert!(outcome.result.is_applied());
        assert_eq!(outcome.state.requested, GpuMode::Optimized);
        assert_eq!(outcome.state.mux, GpuMuxState::Integrated);
        assert_eq!(outcome.state.access_policy, GpuAccessPolicy::Blocked);
        assert_eq!(outcome.state.power_state, GpuPowerState::Active);
    }

    #[tokio::test]
    async fn standard_to_ultimate_pending_keeps_applied() {
        let (_provider, svc) = service();
        let outcome = svc.set_gpu_mode(GpuMode::Ultimate, true).await.unwrap();
        assert!(matches!(
            outcome.result,
            ApplyResult::Pending {
                requirement: ActionRequirement::Reboot
            }
        ));
        assert_eq!(outcome.state.requested, GpuMode::Ultimate);
        // applied state не меняется до перезагрузки
        assert_eq!(outcome.state.mux, GpuMuxState::Integrated);
        assert_eq!(outcome.state.access_policy, GpuAccessPolicy::Unblocked);
        assert_eq!(outcome.state.power_state, GpuPowerState::Active);
        assert_eq!(outcome.state.requirement, ActionRequirement::Reboot);
    }

    #[tokio::test]
    async fn repeated_ultimate_is_idempotent() {
        let (_provider, svc) = service();
        let first = svc.set_gpu_mode(GpuMode::Ultimate, true).await.unwrap();
        let second = svc.set_gpu_mode(GpuMode::Ultimate, true).await.unwrap();
        assert!(matches!(second.result, ApplyResult::Pending { .. }));
        assert_eq!(second.state.requested, GpuMode::Ultimate);
        assert_eq!(second.state, first.state);
    }

    #[tokio::test]
    async fn eco_keeps_applied_and_returns_logout_pending() {
        let (_provider, svc) = service();
        let outcome = svc.set_gpu_mode(GpuMode::Eco, true).await.unwrap();
        // Фактическая семантика MockProvider: Eco -> Pending/Logout, applied не меняется.
        assert!(matches!(
            outcome.result,
            ApplyResult::Pending {
                requirement: ActionRequirement::Logout
            }
        ));
        assert_eq!(outcome.state.requested, GpuMode::Eco);
        assert_eq!(outcome.state.mux, GpuMuxState::Integrated);
        assert_eq!(outcome.state.access_policy, GpuAccessPolicy::Unblocked);
        assert_eq!(outcome.state.requirement, ActionRequirement::Logout);
    }

    #[tokio::test]
    async fn gpu_backend_down_returns_command_error_without_mutation() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let svc = AppService::new(provider.clone());

        state.write().await.error_mode = MockErrorMode::BackendDown;
        let err = svc.set_gpu_mode(GpuMode::Ultimate, true).await.unwrap_err();
        assert!(matches!(
            err,
            CommandError::Command(ProviderError::BackendUnavailable(_))
        ));

        state.write().await.error_mode = MockErrorMode::None;
        let st = svc.gpu_state().await.unwrap();
        assert_eq!(st.requested, GpuMode::Standard);
        assert_eq!(st.mux, GpuMuxState::Integrated);
    }

    #[tokio::test]
    async fn gpu_permission_denied_returns_command_error() {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state.clone()));
        let svc = AppService::new(provider.clone());

        state.write().await.error_mode = MockErrorMode::PermissionDenied;
        let err = svc.set_gpu_mode(GpuMode::Ultimate, true).await.unwrap_err();
        assert!(matches!(
            err,
            CommandError::Command(ProviderError::PermissionDenied(_))
        ));

        state.write().await.error_mode = MockErrorMode::None;
        let st = svc.gpu_state().await.unwrap();
        assert_eq!(st.requested, GpuMode::Standard);
    }

    #[tokio::test]
    async fn gpu_snapshot_is_readback_not_cached() {
        let (provider, svc) = service();
        // Прямое изменение provider через trait (вне AppService).
        provider.set_mode(GpuMode::Optimized, true).await.unwrap();
        let st = svc.gpu_state().await.unwrap();
        assert_eq!(st.requested, GpuMode::Optimized);
        assert_eq!(st.access_policy, GpuAccessPolicy::Blocked);
    }

    // -----------------------------------------------------------------------
    // Scripted GPU provider
    // -----------------------------------------------------------------------

    /// Тестовый GPU-провайдер: фиксирует полученные mode/confirmed, применяет
    /// requested; чтение может быть принудительно переведено в ошибку.
    struct ScriptedGpuProvider {
        requested: tokio::sync::RwLock<GpuMode>,
        mux: tokio::sync::RwLock<GpuMuxState>,
        access: tokio::sync::RwLock<GpuAccessPolicy>,
        power: tokio::sync::RwLock<GpuPowerState>,
        fail_reads: AtomicBool,
        last_mode: tokio::sync::RwLock<Option<GpuMode>>,
        last_confirmed: tokio::sync::RwLock<Option<bool>>,
    }

    impl ScriptedGpuProvider {
        fn new() -> Self {
            Self {
                requested: tokio::sync::RwLock::new(GpuMode::Standard),
                mux: tokio::sync::RwLock::new(GpuMuxState::Integrated),
                access: tokio::sync::RwLock::new(GpuAccessPolicy::Unblocked),
                power: tokio::sync::RwLock::new(GpuPowerState::Active),
                fail_reads: AtomicBool::new(false),
                last_mode: tokio::sync::RwLock::new(None),
                last_confirmed: tokio::sync::RwLock::new(None),
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedGpuProvider {
        fn id(&self) -> &'static str {
            "scripted-gpu"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-gpu")
        }

        fn timeout(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("scripted-gpu: {feature} недоступен")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<orbis_core::diagnostics::DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl GpuProvider for ScriptedGpuProvider {
        async fn requested_mode(&self) -> Result<GpuMode, ProviderError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable(
                    "scripted gpu read failure".into(),
                ));
            }
            Ok(*self.requested.read().await)
        }

        async fn set_mode(
            &self,
            mode: GpuMode,
            confirmed: bool,
        ) -> Result<ApplyResult, ProviderError> {
            *self.last_mode.write().await = Some(mode);
            *self.last_confirmed.write().await = Some(confirmed);
            *self.requested.write().await = mode;
            Ok(ApplyResult::Applied)
        }

        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable(
                    "scripted gpu read failure".into(),
                ));
            }
            Ok(*self.mux.read().await)
        }

        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable(
                    "scripted gpu read failure".into(),
                ));
            }
            Ok(*self.access.read().await)
        }

        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(ProviderError::BackendUnavailable(
                    "scripted gpu read failure".into(),
                ));
            }
            Ok(*self.power.read().await)
        }

        fn requirement_for(&self, _mode: GpuMode) -> ActionRequirement {
            ActionRequirement::None
        }

        fn validate_mode(&self, _mode: GpuMode) -> ValidationResult {
            ValidationResult::Valid
        }
    }

    #[tokio::test]
    async fn gpu_confirmed_flag_passed_unmodified() {
        let provider = Arc::new(ScriptedGpuProvider::new());
        let svc = AppService::new(provider.clone());

        // false передаётся без изменения
        svc.set_gpu_mode(GpuMode::Optimized, false).await.unwrap();
        assert_eq!(*provider.last_mode.read().await, Some(GpuMode::Optimized));
        assert_eq!(*provider.last_confirmed.read().await, Some(false));

        // true передаётся без изменения
        svc.set_gpu_mode(GpuMode::Eco, true).await.unwrap();
        assert_eq!(*provider.last_mode.read().await, Some(GpuMode::Eco));
        assert_eq!(*provider.last_confirmed.read().await, Some(true));
    }

    #[tokio::test]
    async fn gpu_readback_error_preserves_apply_result() {
        let provider = Arc::new(ScriptedGpuProvider::new());
        let svc = AppService::new(provider.clone());

        provider.fail_reads.store(true, Ordering::SeqCst);
        let err = svc
            .set_gpu_mode(GpuMode::Optimized, false)
            .await
            .unwrap_err();

        match err {
            CommandError::ReadBack { result, source } => {
                assert!(result.is_applied());
                assert!(matches!(source, ProviderError::BackendUnavailable(_)));
            }
            other => panic!("ожидался ReadBack, получен: {other:?}"),
        }

        // Команда применилась: mode/confirmed переданы правильно.
        assert_eq!(*provider.last_mode.read().await, Some(GpuMode::Optimized));
        assert_eq!(*provider.last_confirmed.read().await, Some(false));
        assert_eq!(*provider.requested.read().await, GpuMode::Optimized);
    }
}
