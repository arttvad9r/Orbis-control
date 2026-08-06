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
//! На текущем этапе поддерживается только Performance Mode. В будущем слой
//! смогут использовать orbis-ui (через асинхронный worker), orbis-sessiond, CLI
//! и интеграционные тесты.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use orbis_core::action::ApplyResult;
use orbis_core::profile::PerformanceProfile;
use orbis_providers::error::ProviderError;
use orbis_providers::traits::PerformanceProvider;

/// Authoritative состояние Performance Mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceState {
    /// Текущий профиль (из provider read-back).
    pub current: PerformanceProfile,
    /// Доступные профили; порядок совпадает с ответом provider.
    pub available: Vec<PerformanceProfile>,
}

/// Результат команды: исходный `ApplyResult` + authoritative состояние после
/// операции.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceCommandOutcome {
    /// Результат мутации, возвращённый provider-ом без преобразований.
    pub result: ApplyResult,
    /// Состояние, перечитанное из provider после команды.
    pub state: PerformanceState,
}

/// Ошибка команды Performance Mode.
///
/// Различает два принципиально разных случая:
/// - команда provider не выполнилась (или была отклонена);
/// - команда выполнилась (есть `ApplyResult`), но обязательный authoritative
///   read-back после неё завершился ошибкой.
#[derive(Debug)]
pub enum SetPerformanceError {
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

impl std::fmt::Display for SetPerformanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(e) => write!(f, "команда Performance Mode не выполнена: {e}"),
            Self::ReadBack { result, source } => write!(
                f,
                "команда Performance Mode выполнена ({result:?}), но read-back не удался: {source}"
            ),
        }
    }
}

impl std::error::Error for SetPerformanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Command(e) => Some(e),
            Self::ReadBack { source, .. } => Some(source),
        }
    }
}

/// Application service для Performance Mode.
///
/// Владеет provider через `Arc<P>` и предоставляет типизированные async-команды.
/// Не привязан к конкретному provider: работает с любым `P: PerformanceProvider`.
pub struct AppService<P> {
    provider: Arc<P>,
}

impl<P> AppService<P>
where
    P: PerformanceProvider + Send + Sync,
{
    /// Создать сервис над провайдером.
    pub fn new(provider: Arc<P>) -> Self {
        Self { provider }
    }

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
            .map_err(SetPerformanceError::Command)?;
        let state =
            self.performance_state()
                .await
                .map_err(|source| SetPerformanceError::ReadBack {
                    result: result.clone(),
                    source,
                })?;
        Ok(PerformanceCommandOutcome { result, state })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use orbis_core::action::ApplyResult;
    use orbis_core::identity::BackendIdentity;
    use orbis_core::profile::PerformanceProfile;
    use orbis_providers::error::{ProviderError, ValidationResult};
    use orbis_providers::mock::{MockErrorMode, MockProvider};
    use orbis_providers::traits::{PerformanceProvider, Provider, ProviderHealth};
    use orbis_test_support::devices::build_state_arc;

    use super::{AppService, PerformanceState, SetPerformanceError};

    fn service() -> (Arc<MockProvider>, AppService<MockProvider>) {
        let state = build_state_arc("zephyrus-full").expect("profile exists");
        let provider = Arc::new(MockProvider::new(state));
        let svc = AppService::new(provider.clone());
        (provider, svc)
    }

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
            SetPerformanceError::Command(ProviderError::BackendUnavailable(_))
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
            SetPerformanceError::Command(ProviderError::PermissionDenied(_))
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
    // Scripted provider: команда выполняется, но следующий read-back падает.
    // -----------------------------------------------------------------------

    /// Тестовый провайдер: `set_profile` всегда успешен и применяет профиль;
    /// `current_profile` может быть принудительно переведён в ошибку.
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
    async fn readback_error_preserves_apply_result() {
        let provider = Arc::new(ScriptedProvider::new());
        let svc = AppService::new(provider.clone());

        // Мутация успешна, но последующий authoritative read-back падает.
        provider.fail_reads.store(true, Ordering::SeqCst);
        let err = svc
            .set_performance(PerformanceProfile::Silent)
            .await
            .unwrap_err();

        match err {
            SetPerformanceError::ReadBack { result, source } => {
                assert!(result.is_applied());
                assert!(matches!(source, ProviderError::BackendUnavailable(_)));
            }
            other => panic!("ожидался ReadBack, получен: {other:?}"),
        }

        // Команда действительно применилась, несмотря на неудачный read-back.
        assert_eq!(*provider.current.read().await, PerformanceProfile::Silent);
    }
}
