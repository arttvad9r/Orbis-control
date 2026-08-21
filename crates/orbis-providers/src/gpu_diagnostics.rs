//! Independent read-only GPU primitive diagnostics snapshot.

use orbis_core::diagnostics::{DiagnosticObservation, GpuDiagnostics};

use crate::{
    ProviderError, bounded_provider_call,
    traits::{GpuAccessProvider, GpuMuxProvider, GpuPowerProvider},
};

/// Collect the three authoritative GPU primitives independently.
///
/// This function never reads or synthesizes a product `GpuMode`. Each primitive
/// keeps its own observation result, so failure of one source does not erase a
/// successfully observed value from either of the other sources.
pub async fn gpu_diagnostics_snapshot<M, A, P>(mux: &M, access: &A, power: &P) -> GpuDiagnostics
where
    M: GpuMuxProvider + ?Sized,
    A: GpuAccessProvider + ?Sized,
    P: GpuPowerProvider + ?Sized,
{
    let mux =
        observation_from_result(bounded_provider_call(mux, "gpu.mux_state", mux.mux_state()).await);
    let access_policy = observation_from_result(
        bounded_provider_call(access, "gpu.access_policy", access.access_policy()).await,
    );
    let runtime_power = observation_from_result(
        bounded_provider_call(power, "gpu.power_state", power.power_state()).await,
    );

    GpuDiagnostics {
        mux,
        access_policy,
        runtime_power,
    }
}

fn observation_from_result<T>(result: Result<T, ProviderError>) -> DiagnosticObservation<T> {
    match result {
        Ok(value) => DiagnosticObservation::Value(value),
        Err(ProviderError::PermissionDenied(_)) => DiagnosticObservation::PermissionDenied,
        Err(
            ProviderError::BackendUnavailable(_)
            | ProviderError::Unsupported(_)
            | ProviderError::Timeout(_),
        ) => DiagnosticObservation::Unavailable,
        Err(
            ProviderError::InvalidRequest(_)
            | ProviderError::Io(_)
            | ProviderError::Dbus(_)
            | ProviderError::Internal(_)
            | ProviderError::Conflict(_),
        ) => DiagnosticObservation::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_core::{
        diagnostics::DiagnosticEntry,
        gpu::{GpuAccessPolicy, GpuMuxState, GpuPowerState},
        identity::BackendIdentity,
    };

    use super::*;
    use crate::{Provider, ProviderHealth};

    #[derive(Debug, Clone, Copy)]
    enum Read<T> {
        Value(T),
        BackendUnavailable,
        Unsupported,
        PermissionDenied,
        Timeout,
        Dbus,
        Internal,
    }

    fn read_result<T: Copy>(read: Read<T>) -> Result<T, ProviderError> {
        match read {
            Read::Value(value) => Ok(value),
            Read::BackendUnavailable => Err(ProviderError::BackendUnavailable("offline".into())),
            Read::Unsupported => Err(ProviderError::Unsupported("unsupported".into())),
            Read::PermissionDenied => Err(ProviderError::PermissionDenied("denied".into())),
            Read::Timeout => Err(ProviderError::Timeout("timeout".into())),
            Read::Dbus => Err(ProviderError::Dbus("transport failure".into())),
            Read::Internal => Err(ProviderError::Internal("invalid remote payload".into())),
        }
    }

    struct Fixture {
        mux: Read<GpuMuxState>,
        access: Read<GpuAccessPolicy>,
        power: Read<GpuPowerState>,
    }

    impl Provider for Fixture {
        fn id(&self) -> &'static str {
            "diagnostics-gpu-fixture"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("diagnostics-gpu-fixture")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("fixture does not support {feature}")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl GpuMuxProvider for Fixture {
        async fn mux_state(&self) -> Result<GpuMuxState, ProviderError> {
            read_result(self.mux)
        }
    }

    #[async_trait]
    impl GpuAccessProvider for Fixture {
        async fn access_policy(&self) -> Result<GpuAccessPolicy, ProviderError> {
            read_result(self.access)
        }
    }

    #[async_trait]
    impl GpuPowerProvider for Fixture {
        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            read_result(self.power)
        }
    }

    #[tokio::test]
    async fn successful_domain_unknown_values_remain_successful_observations() {
        let fixture = Fixture {
            mux: Read::Value(GpuMuxState::Unknown),
            access: Read::Value(GpuAccessPolicy::Unknown),
            power: Read::Value(GpuPowerState::Unknown),
        };

        let snapshot = gpu_diagnostics_snapshot(&fixture, &fixture, &fixture).await;

        assert_eq!(
            snapshot.mux,
            DiagnosticObservation::Value(GpuMuxState::Unknown)
        );
        assert_eq!(
            snapshot.access_policy,
            DiagnosticObservation::Value(GpuAccessPolicy::Unknown)
        );
        assert_eq!(
            snapshot.runtime_power,
            DiagnosticObservation::Value(GpuPowerState::Unknown)
        );
    }

    #[tokio::test]
    async fn primitive_failures_are_classified_independently() {
        let fixture = Fixture {
            mux: Read::PermissionDenied,
            access: Read::BackendUnavailable,
            power: Read::Internal,
        };

        let snapshot = gpu_diagnostics_snapshot(&fixture, &fixture, &fixture).await;

        assert_eq!(snapshot.mux, DiagnosticObservation::PermissionDenied);
        assert_eq!(snapshot.access_policy, DiagnosticObservation::Unavailable);
        assert_eq!(snapshot.runtime_power, DiagnosticObservation::Unknown);
    }

    #[tokio::test]
    async fn unavailable_and_transport_failures_are_not_fake_domain_values() {
        let fixture = Fixture {
            mux: Read::Unsupported,
            access: Read::Timeout,
            power: Read::Dbus,
        };

        let snapshot = gpu_diagnostics_snapshot(&fixture, &fixture, &fixture).await;

        assert_eq!(snapshot.mux, DiagnosticObservation::Unavailable);
        assert_eq!(snapshot.access_policy, DiagnosticObservation::Unavailable);
        assert_eq!(snapshot.runtime_power, DiagnosticObservation::Unknown);
    }

    #[tokio::test]
    async fn successful_primitives_are_preserved_without_product_mode_inference() {
        let fixture = Fixture {
            mux: Read::Value(GpuMuxState::Discrete),
            access: Read::Value(GpuAccessPolicy::Blocked),
            power: Read::Value(GpuPowerState::Suspended),
        };

        let snapshot = gpu_diagnostics_snapshot(&fixture, &fixture, &fixture).await;

        assert_eq!(
            snapshot.mux,
            DiagnosticObservation::Value(GpuMuxState::Discrete)
        );
        assert_eq!(
            snapshot.access_policy,
            DiagnosticObservation::Value(GpuAccessPolicy::Blocked)
        );
        assert_eq!(
            snapshot.runtime_power,
            DiagnosticObservation::Value(GpuPowerState::Suspended)
        );
    }
}
