//! Bounded execution helpers for provider operations.
//!
//! `Provider::timeout()` is part of the provider contract. This module turns
//! that declaration into one canonical Tokio timeout wrapper so callers do not
//! need to duplicate elapsed-time classification.
//!
//! The helper deliberately performs no retry. A timeout only proves that the
//! operation did not complete within the provider contract; for mutations the
//! real hardware outcome can be unknown, so retry policy must remain with the
//! higher-level transaction/reconciliation layer.

use std::future::Future;

use crate::error::ProviderError;
use crate::traits::Provider;

/// Execute one provider operation within the provider-declared timeout.
///
/// Successful values and provider errors are returned unchanged. If the
/// operation does not finish before `Provider::timeout()`, the result is
/// `ProviderError::Timeout` with provider and operation identity preserved in
/// the diagnostic detail.
///
/// This function never retries the supplied future.
pub async fn bounded_provider_call<P, T, F>(
    provider: &P,
    operation: &str,
    future: F,
) -> Result<T, ProviderError>
where
    P: Provider + ?Sized,
    F: Future<Output = Result<T, ProviderError>>,
{
    let limit = provider.timeout();
    match tokio::time::timeout(limit, future).await {
        Ok(result) => result,
        Err(_) => Err(ProviderError::Timeout(format!(
            "provider '{}' operation '{}' exceeded {} ms",
            provider.id(),
            operation,
            limit.as_millis()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::time::Duration;

    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::identity::BackendIdentity;

    use super::bounded_provider_call;
    use crate::error::ProviderError;
    use crate::traits::{Provider, ProviderHealth};

    struct TestProvider {
        timeout: Duration,
    }

    #[async_trait::async_trait]
    impl Provider for TestProvider {
        fn id(&self) -> &'static str {
            "bounded-test"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("bounded-test")
        }

        fn timeout(&self) -> Duration {
            self.timeout
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("unsupported test feature: {feature}")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn never_completing_operation_becomes_timeout() {
        let provider = TestProvider {
            timeout: Duration::from_millis(250),
        };

        let result = bounded_provider_call(
            &provider,
            "read_status",
            pending::<Result<(), ProviderError>>(),
        )
        .await;

        match result {
            Err(ProviderError::Timeout(detail)) => {
                assert!(detail.contains("bounded-test"));
                assert!(detail.contains("read_status"));
                assert!(detail.contains("250 ms"));
            }
            other => panic!("expected timeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn provider_error_is_preserved() {
        let provider = TestProvider {
            timeout: Duration::from_secs(1),
        };

        let result = bounded_provider_call(&provider, "read", async {
            Err::<(), _>(ProviderError::Unsupported("not present".into()))
        })
        .await;

        assert!(matches!(
            result,
            Err(ProviderError::Unsupported(detail)) if detail == "not present"
        ));
    }

    #[tokio::test]
    async fn successful_value_is_preserved() {
        let provider = TestProvider {
            timeout: Duration::from_secs(1),
        };

        let result = bounded_provider_call(&provider, "read", async { Ok::<_, ProviderError>(42) })
            .await
            .expect("bounded call should succeed");

        assert_eq!(result, 42);
    }
}
