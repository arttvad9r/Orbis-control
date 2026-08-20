//! Read-only production source orchestration for the Diagnostics window.
//!
//! This layer owns no Slint objects and exposes no mutation API. It performs
//! one-shot reads through existing typed providers/adapters, then delegates
//! immutable assembly to `orbis_application::diagnostics::DiagnosticsCollector`.
//! The UI only consumes the resulting snapshot/DTO.

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use orbis_application::diagnostics::DiagnosticsCollector;
use orbis_capabilities::CapabilityRegistrySnapshot;
use orbis_core::diagnostics::{DiagnosticsSnapshot, ServiceCriticality, TelemetryDiagnostics};
use orbis_providers::traits::TelemetryProvider;
use orbis_providers::{
    ASUSD_SERVICE, HardwareIdentityProvider, ORBIS_HARDWARE_SERVICE, ORBIS_SESSION_SERVICE,
    SUPERGFXD_SERVICE, ServicePresenceProvider, SysfsTelemetryProvider, SystemMetadataProvider,
    WaylandCompositorOutputSource, WaylandDisplayOutputProvider, display_diagnostics_snapshot,
    gpu_diagnostics_snapshot, telemetry_diagnostics_after_attempt,
};
use orbis_session_client::{
    SessionGpuAccessProvider, SessionGpuMuxProvider, SessionGpuPowerProvider, ZbusSessionGpuSource,
};

use crate::diagnostics_metadata::application_diagnostics;

/// Application-owned read-only production source context for diagnostics.
///
/// Construction performs no I/O. A call to [`snapshot`](Self::snapshot) performs
/// only one-shot read operations. There is intentionally no timer/polling loop;
/// refresh lifecycle policy remains owned by the UI/application wiring.
#[derive(Clone)]
pub struct DiagnosticsRuntime {
    session_connection: zbus::Connection,
    system_connection: zbus::Connection,
    capabilities: Arc<RwLock<Arc<CapabilityRegistrySnapshot>>>,
    previous_telemetry: Arc<Mutex<Option<TelemetryDiagnostics>>>,
    telemetry_freshness_threshold: Duration,
}

impl DiagnosticsRuntime {
    /// Create a diagnostics source context over already-open bus connections and
    /// the current immutable capability snapshot.
    pub fn new(
        session_connection: zbus::Connection,
        system_connection: zbus::Connection,
        capabilities: Arc<CapabilityRegistrySnapshot>,
        telemetry_freshness_threshold: Duration,
    ) -> Self {
        Self {
            session_connection,
            system_connection,
            capabilities: Arc::new(RwLock::new(capabilities)),
            previous_telemetry: Arc::new(Mutex::new(None)),
            telemetry_freshness_threshold,
        }
    }

    /// Replace the capability snapshot used by subsequent diagnostics refreshes.
    ///
    /// The worker still owns probing/lifecycle. Diagnostics only follows a
    /// successfully published immutable registry snapshot; it never probes or
    /// edits registry entries itself.
    pub fn replace_capabilities(&self, snapshot: Arc<CapabilityRegistrySnapshot>) {
        *self
            .capabilities
            .write()
            .expect("diagnostics capability lock poisoned") = snapshot;
    }

    /// Clone the current immutable capability snapshot handle.
    ///
    /// This is a read-only sharing point for other UI-side observers that must
    /// make a decision against the same whole-swap registry generation as
    /// Diagnostics. It never probes or edits capabilities.
    pub fn capabilities_arc(&self) -> Arc<CapabilityRegistrySnapshot> {
        self.capabilities
            .read()
            .expect("diagnostics capability lock poisoned")
            .clone()
    }

    /// Collect one immutable production diagnostics snapshot using read-only
    /// sources only.
    pub async fn snapshot(&self) -> DiagnosticsSnapshot {
        let generated_at = SystemTime::now();

        let application = application_diagnostics();
        let system = SystemMetadataProvider::new().snapshot();
        let hardware = HardwareIdentityProvider::new().snapshot();

        let service_provider = ServicePresenceProvider::new(
            self.system_connection.clone(),
            self.session_connection.clone(),
        );
        let services = vec![
            service_provider
                .check(
                    ORBIS_HARDWARE_SERVICE,
                    ServiceCriticality::CapabilityLocal,
                    Some(generated_at),
                )
                .await,
            service_provider
                .check(
                    ORBIS_SESSION_SERVICE,
                    ServiceCriticality::CoreReadPath,
                    Some(generated_at),
                )
                .await,
            service_provider
                .check(
                    ASUSD_SERVICE,
                    ServiceCriticality::CapabilityLocal,
                    Some(generated_at),
                )
                .await,
            service_provider
                .check(
                    SUPERGFXD_SERVICE,
                    ServiceCriticality::Optional,
                    Some(generated_at),
                )
                .await,
        ];

        let registry = self.capabilities_arc();
        let capabilities =
            orbis_capabilities::diagnostics::capability_snapshot_diagnostics(&registry);

        let gpu_power = SessionGpuPowerProvider::new(ZbusSessionGpuSource::new(
            self.session_connection.clone(),
        ));
        let gpu_mux =
            SessionGpuMuxProvider::new(ZbusSessionGpuSource::new(self.session_connection.clone()));
        let gpu_access = SessionGpuAccessProvider::new(ZbusSessionGpuSource::new(
            self.session_connection.clone(),
        ));
        let gpu = gpu_diagnostics_snapshot(&gpu_mux, &gpu_access, &gpu_power).await;

        let telemetry_provider = SysfsTelemetryProvider::default();
        let telemetry_result = telemetry_provider.snapshot().await;
        let previous_telemetry = self
            .previous_telemetry
            .lock()
            .expect("diagnostics telemetry lock poisoned")
            .clone();
        let telemetry = telemetry_diagnostics_after_attempt(
            previous_telemetry.as_ref(),
            telemetry_result,
            generated_at,
            self.telemetry_freshness_threshold,
        );
        *self
            .previous_telemetry
            .lock()
            .expect("diagnostics telemetry lock poisoned") = Some(telemetry.clone());

        let display_provider =
            WaylandDisplayOutputProvider::new(WaylandCompositorOutputSource::new());
        let display = display_diagnostics_snapshot(&display_provider, Some(generated_at)).await;

        DiagnosticsCollector::new().collect(
            generated_at,
            application,
            system,
            hardware,
            services,
            capabilities,
            gpu,
            telemetry,
            display,
        )
    }
}
