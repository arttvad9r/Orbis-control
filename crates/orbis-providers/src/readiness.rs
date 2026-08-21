//! Bounded readiness probe adapter.
//!
//! Converts one provider operation into structured readiness evidence while
//! keeping dependency reachability and permission evidence independent.

use std::future::Future;

use orbis_core::FeatureId;
use orbis_core::readiness::{PermissionState, ReadinessItem, ReadinessState};

use crate::error::ProviderError;
use crate::execution::bounded_provider_call;
use crate::traits::Provider;

/// Execute one read-only readiness operation within the provider timeout.
///
/// The supplied future must not perform a mutation. This helper is intended for
/// setup/status/owner/readiness evidence.
pub async fn bounded_readiness_probe<P, T, F>(
    provider: &P,
    id: impl Into<String>,
    operation: &str,
    required_for: Vec<FeatureId>,
    permission_required: bool,
    future: F,
) -> ReadinessItem
where
    P: Provider + ?Sized,
    T: std::fmt::Debug,
    F: Future<Output = Result<T, ProviderError>>,
{
    let id = id.into();
    let permission_unknown = || {
        if permission_required {
            PermissionState::Unknown
        } else {
            PermissionState::NotRequired
        }
    };

    match bounded_provider_call(provider, operation, future).await {
        Ok(value) => ReadinessItem {
            id,
            state: ReadinessState::Ready,
            required_for,
            evidence: vec![format!("{operation}: {value:?}")],
            permission: if permission_required {
                PermissionState::Granted
            } else {
                PermissionState::NotRequired
            },
        },
        Err(ProviderError::PermissionDenied(detail)) => ReadinessItem {
            id,
            state: ReadinessState::Ready,
            required_for,
            evidence: vec![detail],
            permission: PermissionState::Denied,
        },
        Err(ProviderError::Unsupported(detail)) => ReadinessItem {
            id,
            state: ReadinessState::NotAvailable,
            required_for,
            evidence: vec![detail],
            permission: permission_unknown(),
        },
        Err(ProviderError::BackendUnavailable(detail)) => ReadinessItem {
            id,
            state: ReadinessState::Inactive,
            required_for,
            evidence: vec![detail],
            permission: permission_unknown(),
        },
        Err(ProviderError::Timeout(detail)) | Err(ProviderError::Dbus(detail)) => ReadinessItem {
            id,
            state: ReadinessState::Unreachable,
            required_for,
            evidence: vec![detail],
            permission: permission_unknown(),
        },
        Err(ProviderError::Io(error)) => ReadinessItem {
            id,
            state: ReadinessState::Unreachable,
            required_for,
            evidence: vec![error.to_string()],
            permission: permission_unknown(),
        },
        Err(ProviderError::Internal(detail)) | Err(ProviderError::InvalidRequest(detail)) => {
            ReadinessItem {
                id,
                state: ReadinessState::Unknown,
                required_for,
                evidence: vec![detail],
                permission: permission_unknown(),
            }
        }
    }
}

/// Run two independent readiness futures concurrently.
///
/// This small primitive keeps concurrency explicit at composition sites while
/// avoiding a generic dynamic task registry or a second source of truth.
pub async fn join_readiness2<A, B>(a: A, b: B) -> (A::Output, B::Output)
where
    A: Future,
    B: Future,
{
    tokio::join!(a, b)
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::time::Duration;

    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::identity::BackendIdentity;

    use super::*;
    use crate::traits::ProviderHealth;

    struct TestProvider;

    #[async_trait::async_trait]
    impl Provider for TestProvider {
        fn id(&self) -> &'static str {
            "readiness-test"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("readiness-test")
        }

        fn timeout(&self) -> Duration {
            Duration::from_millis(100)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("unsupported {feature}")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_becomes_unreachable_not_unsupported() {
        let item = bounded_readiness_probe(
            &TestProvider,
            "service",
            "owner",
            vec![FeatureId::Performance],
            false,
            pending::<Result<(), ProviderError>>(),
        )
        .await;
        assert_eq!(item.state, ReadinessState::Unreachable);
        assert_eq!(item.permission, PermissionState::NotRequired);
    }

    #[tokio::test]
    async fn permission_is_separate_from_dependency_state() {
        let item = bounded_readiness_probe(
            &TestProvider,
            "hardware1",
            "authorize",
            vec![FeatureId::ChargeLimit],
            true,
            async { Err::<(), _>(ProviderError::PermissionDenied("polkit".into())) },
        )
        .await;
        assert_eq!(item.state, ReadinessState::Ready);
        assert_eq!(item.permission, PermissionState::Denied);
    }

    #[tokio::test]
    async fn internal_contract_failure_is_unknown_not_unreachable() {
        let item = bounded_readiness_probe(
            &TestProvider,
            "hardware1",
            "status",
            vec![FeatureId::ChargeLimit],
            false,
            async { Err::<(), _>(ProviderError::Internal("malformed payload".into())) },
        )
        .await;
        assert_eq!(item.state, ReadinessState::Unknown);
    }

    #[tokio::test]
    async fn independent_probes_can_run_concurrently() {
        let (a, b) = join_readiness2(async { 1u8 }, async { 2u8 }).await;
        assert_eq!((a, b), (1, 2));
    }
}
