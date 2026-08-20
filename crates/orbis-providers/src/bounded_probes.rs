//! Provider-timeout enforcement for public read-only capability probes.
//!
//! The existing probe implementations own capability classification semantics.
//! This module adds one outer provider-declared deadline around each complete
//! read-only probe. If the deadline expires, only that capability becomes
//! `TemporarilyUnavailable`; the timeout never becomes `Unsupported`, never
//! aborts unrelated capability discovery, and never triggers a retry or write.

use std::future::Future;

use orbis_capabilities::{ProbeError, capability_from_operations};
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, CapabilityReason, CapabilityStatus,
    OperationCapability, RiskLevel,
};

use crate::bounded_provider_call;
use crate::error::ProviderError;
use crate::traits::{
    BatteryProvider, DisplayOutputProvider, FanProvider, GpuAccessProvider, GpuMuxProvider,
    GpuPowerProvider, MiniLedModeProvider, PanelOverdriveProvider, PerformanceProvider, Provider,
    ScreenAutoBrightnessProvider,
};

fn operation(status: CapabilityStatus, reason: String) -> OperationCapability {
    OperationCapability::with_reason(
        status,
        CapabilityReason {
            reason,
            suggestion: String::new(),
            backend: None,
            endpoint: None,
            requirement: None,
            risk: RiskLevel::Safe,
            checked_at: None,
        },
    )
}

fn timeout_capability(detail: String, write_status: CapabilityStatus) -> Capability {
    let read = operation(CapabilityStatus::TemporarilyUnavailable, detail.clone());
    let write = operation(
        write_status,
        if write_status == CapabilityStatus::TemporarilyUnavailable {
            "write capability cannot be established while the read-only probe timed out".into()
        } else {
            format!("write status remains {write_status:?} while the read-only probe timed out")
        },
    );
    capability_from_operations(
        CapabilityOperations { read, write },
        CapabilityConstraints::Unknown,
    )
}

async fn bounded_probe<P, F>(
    provider: &P,
    operation: &'static str,
    timeout_write_status: CapabilityStatus,
    future: F,
) -> Result<Capability, ProbeError>
where
    P: Provider + ?Sized,
    F: Future<Output = Result<Capability, ProbeError>>,
{
    let wrapped = async move { Ok::<_, ProviderError>(future.await) };
    match bounded_provider_call(provider, operation, wrapped).await {
        Ok(result) => result,
        Err(ProviderError::Timeout(detail)) => {
            Ok(timeout_capability(detail, timeout_write_status))
        }
        Err(error) => Err(ProbeError::Internal(format!(
            "bounded read-only probe wrapper returned unexpected provider error: {error}"
        ))),
    }
}

/// Bounded Performance capability probe.
pub async fn probe_performance<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: PerformanceProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_performance",
        CapabilityStatus::TemporarilyUnavailable,
        crate::probes::probe_performance(provider, mutation_status),
    )
    .await
}

/// Bounded Battery charge-limit capability probe.
pub async fn probe_charge_limit<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: BatteryProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_charge_limit",
        CapabilityStatus::TemporarilyUnavailable,
        crate::probes::probe_charge_limit(provider, mutation_status),
    )
    .await
}

/// Bounded fan-curve capability probe.
pub async fn probe_fan_curve<P>(
    provider: &P,
    fan: &orbis_core::fan::FanId,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: FanProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_fan_curve",
        CapabilityStatus::TemporarilyUnavailable,
        crate::probes::probe_fan_curve(provider, fan, mutation_status),
    )
    .await
}

/// Bounded dGPU power-state capability probe.
pub async fn probe_gpu_power<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: GpuPowerProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_gpu_power",
        CapabilityStatus::Unsupported,
        crate::probes::probe_gpu_power(provider),
    )
    .await
}

/// Bounded physical GPU-MUX capability probe.
pub async fn probe_gpu_mux<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: GpuMuxProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_gpu_mux",
        CapabilityStatus::Unsupported,
        crate::probes::probe_gpu_mux(provider),
    )
    .await
}

/// Bounded dGPU-access capability probe.
pub async fn probe_gpu_access<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: GpuAccessProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_gpu_access",
        CapabilityStatus::Unsupported,
        crate::probes::probe_gpu_access(provider),
    )
    .await
}

/// Bounded Panel Overdrive capability probe.
pub async fn probe_panel_overdrive<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: PanelOverdriveProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_panel_overdrive",
        CapabilityStatus::TemporarilyUnavailable,
        crate::probes::probe_panel_overdrive(provider, mutation_status),
    )
    .await
}

/// Bounded MiniLED read-only capability probe.
pub async fn probe_mini_led_mode<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: MiniLedModeProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_mini_led_mode",
        CapabilityStatus::ReadOnly,
        crate::probes::probe_mini_led_mode(provider),
    )
    .await
}

/// Bounded screen-auto-brightness read-only capability probe.
pub async fn probe_screen_auto_brightness<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: ScreenAutoBrightnessProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_screen_auto_brightness",
        CapabilityStatus::ReadOnly,
        crate::probes::probe_screen_auto_brightness(provider),
    )
    .await
}

/// Bounded display-output read-only capability probe.
pub async fn probe_display_output<P>(provider: &P) -> Result<Capability, ProbeError>
where
    P: DisplayOutputProvider + ?Sized,
{
    bounded_probe(
        provider,
        "probe_display_output",
        CapabilityStatus::ReadOnly,
        crate::probes::probe_display_output(provider),
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::gpu::GpuPowerState;
    use orbis_core::identity::BackendIdentity;

    use super::*;
    use crate::traits::{Provider, ProviderHealth};

    struct HangingGpuPowerProvider;

    impl Provider for HangingGpuPowerProvider {
        fn id(&self) -> &'static str {
            "hanging-gpu-power"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("hanging-gpu-power")
        }

        fn timeout(&self) -> Duration {
            Duration::from_millis(50)
        }

        fn explain_unsupported(&self, feature: &str) -> String {
            format!("unsupported: {feature}")
        }

        fn health(&self) -> ProviderHealth {
            ProviderHealth::Healthy
        }

        fn diagnostics(&self) -> Vec<DiagnosticEntry> {
            Vec::new()
        }
    }

    #[async_trait]
    impl GpuPowerProvider for HangingGpuPowerProvider {
        async fn power_state(&self) -> Result<GpuPowerState, ProviderError> {
            pending::<Result<GpuPowerState, ProviderError>>().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn timed_out_probe_is_temporarily_unavailable_not_unsupported() {
        let capability = probe_gpu_power(&HangingGpuPowerProvider)
            .await
            .expect("timeout is capability evidence, not a probe abort");

        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::TemporarilyUnavailable
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
        let reason = capability
            .operations
            .read
            .reason
            .expect("timeout reason must be preserved");
        assert!(reason.reason.contains("hanging-gpu-power"));
        assert!(reason.reason.contains("probe_gpu_power"));
    }
}
