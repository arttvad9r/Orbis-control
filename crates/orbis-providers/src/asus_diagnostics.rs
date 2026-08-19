//! Read-only ASUS diagnostics capability probes and deterministic aggregation.
//!
//! This module deliberately produces canonical capability metadata only. It
//! does not create a parallel ASUS diagnostics state model and never performs
//! a mutation. Observed Panel/MiniLED/auto-brightness/keyboard/Aura values are
//! used only to prove the corresponding read contract and are then discarded.

use orbis_capabilities::{
    ProbeClassification, ProbeContext, ProbeError, ProbeOperationResult, capability_from_operations,
};
use orbis_core::capability::{
    Capability, CapabilityConstraints, CapabilityOperations, CapabilityReason, CapabilityStatus,
    FeatureId, OperationCapability, RiskLevel,
};

use crate::error::ProviderError;
use crate::probes::{probe_mini_led_mode, probe_panel_overdrive, probe_screen_auto_brightness};
use crate::traits::{
    AuraProvider, KeyboardBacklightProvider, MiniLedModeProvider, PanelOverdriveProvider,
    ScreenAutoBrightnessProvider,
};

fn operation_from_error(
    error: &ProviderError,
    context: ProbeContext,
) -> Result<OperationCapability, ProbeError> {
    error
        .into_probe_result(context)
        .map(ProbeOperationResult::into_operation)
}

fn write_from_mutation_status(
    status: CapabilityStatus,
    feature: &'static str,
) -> OperationCapability {
    OperationCapability {
        status,
        reason: Some(CapabilityReason {
            reason: format!("Hardware1 {feature} mutation backend runtime evidence"),
            suggestion: String::new(),
            backend: None,
            endpoint: None,
            requirement: None,
            risk: RiskLevel::Safe,
            checked_at: None,
        }),
    }
}

fn write_from_read_failure(read: &OperationCapability) -> OperationCapability {
    let status = match read.status {
        CapabilityStatus::BackendMissing => CapabilityStatus::BackendMissing,
        CapabilityStatus::Unsupported => CapabilityStatus::Unsupported,
        CapabilityStatus::TemporarilyUnavailable => CapabilityStatus::TemporarilyUnavailable,
        CapabilityStatus::Unknown => CapabilityStatus::Unknown,
        // A denied read is not evidence that a write path exists or that its
        // authorization semantics are the same, so remain conservative.
        _ => CapabilityStatus::Unsupported,
    };
    OperationCapability {
        status,
        reason: Some(CapabilityReason {
            reason: "write capability cannot be established while the read probe failed".into(),
            suggestion: String::new(),
            backend: None,
            endpoint: None,
            requirement: None,
            risk: RiskLevel::Safe,
            checked_at: None,
        }),
    }
}

fn capability_from_read(read: OperationCapability, write: OperationCapability) -> Capability {
    capability_from_operations(
        CapabilityOperations { read, write },
        CapabilityConstraints::Unknown,
    )
}

/// Probe keyboard-backlight read support without changing brightness.
///
/// The observed brightness level is discarded after establishing the read
/// contract. Write status comes only from typed Hardware1 mutation-backend
/// evidence supplied by the caller; no setter or validation-based inference is
/// used.
pub async fn probe_keyboard_backlight<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: KeyboardBacklightProvider + ?Sized,
{
    match provider.keyboard_backlight_state().await {
        Ok(_) => {
            let read =
                ProbeOperationResult::classified(ProbeClassification::Supported).into_operation();
            Ok(capability_from_read(
                read,
                write_from_mutation_status(mutation_status, "keyboard backlight"),
            ))
        }
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            Ok(capability_from_read(read, write))
        }
    }
}

/// Probe Aura read support without changing lighting state.
///
/// The observed Aura state is discarded after establishing the read contract.
/// Write status comes only from typed Hardware1 mutation-backend evidence
/// supplied by the caller; this function never invokes an Aura setter.
pub async fn probe_aura<P>(
    provider: &P,
    mutation_status: CapabilityStatus,
) -> Result<Capability, ProbeError>
where
    P: AuraProvider + ?Sized,
{
    match provider.aura_state().await {
        Ok(_) => {
            let read =
                ProbeOperationResult::classified(ProbeClassification::Supported).into_operation();
            Ok(capability_from_read(
                read,
                write_from_mutation_status(mutation_status, "Aura"),
            ))
        }
        Err(error) => {
            let read = operation_from_error(&error, ProbeContext::BackendDiscovery)?;
            let write = write_from_read_failure(&read);
            Ok(capability_from_read(read, write))
        }
    }
}

/// Probe the five ASUS diagnostics capabilities that already have read-only
/// providers and return them in a deterministic canonical feature order.
///
/// This is a capability-oriented snapshot for the future diagnostics collector:
/// it reuses `Capability`/`FeatureId` instead of introducing an ASUS-specific
/// status enum or duplicating observed hardware state. Each provider is read
/// exactly through its concept-specific read-only trait. No hardware mutation
/// API is reachable from this function.
#[allow(clippy::too_many_arguments)]
pub async fn probe_asus_diagnostics_capabilities<Po, Ml, Sa, Kb, Au>(
    panel_overdrive_provider: &Po,
    mini_led_provider: &Ml,
    screen_auto_brightness_provider: &Sa,
    keyboard_backlight_provider: &Kb,
    aura_provider: &Au,
    panel_overdrive_mutation_status: CapabilityStatus,
    keyboard_backlight_mutation_status: CapabilityStatus,
    aura_mutation_status: CapabilityStatus,
) -> Result<Vec<(FeatureId, Capability)>, ProbeError>
where
    Po: PanelOverdriveProvider + ?Sized,
    Ml: MiniLedModeProvider + ?Sized,
    Sa: ScreenAutoBrightnessProvider + ?Sized,
    Kb: KeyboardBacklightProvider + ?Sized,
    Au: AuraProvider + ?Sized,
{
    let panel =
        probe_panel_overdrive(panel_overdrive_provider, panel_overdrive_mutation_status).await?;
    let mini_led = probe_mini_led_mode(mini_led_provider).await?;
    let auto_brightness = probe_screen_auto_brightness(screen_auto_brightness_provider).await?;
    let keyboard = probe_keyboard_backlight(
        keyboard_backlight_provider,
        keyboard_backlight_mutation_status,
    )
    .await?;
    let aura = probe_aura(aura_provider, aura_mutation_status).await?;

    Ok(vec![
        (FeatureId::PanelOverdrive, panel),
        (FeatureId::MiniLed, mini_led),
        (FeatureId::ScreenAutoBrightness, auto_brightness),
        (FeatureId::KeyboardBacklight, keyboard),
        (FeatureId::Aura, aura),
    ])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use async_trait::async_trait;
    use orbis_core::aura::{
        AuraBrightness, AuraDirection, AuraEffect, AuraMode, AuraRgb, AuraSpeed, AuraState,
        AuraZone,
    };
    use orbis_core::diagnostics::DiagnosticEntry;
    use orbis_core::display::{
        MiniLedModeKind, MiniLedModeState, MiniLedModeValue, PanelOverdriveState,
        ScreenAutoBrightnessState,
    };
    use orbis_core::identity::BackendIdentity;
    use orbis_core::keyboard_backlight::{KeyboardBacklightState, KeyboardBrightnessLevel};

    use super::*;
    use crate::traits::{Provider, ProviderHealth};

    #[derive(Debug, Clone, Copy)]
    enum ReadKind {
        Ok,
        BackendMissing,
        Unsupported,
        PermissionDenied,
    }

    impl ReadKind {
        fn result<T>(self, value: T) -> Result<T, ProviderError> {
            match self {
                Self::Ok => Ok(value),
                Self::BackendMissing => Err(ProviderError::BackendUnavailable("missing".into())),
                Self::Unsupported => Err(ProviderError::Unsupported("unsupported".into())),
                Self::PermissionDenied => Err(ProviderError::PermissionDenied("denied".into())),
            }
        }
    }

    #[derive(Debug)]
    struct ScriptedAsusProvider {
        panel: ReadKind,
        mini_led: ReadKind,
        auto_brightness: ReadKind,
        keyboard: ReadKind,
        aura: ReadKind,
    }

    impl ScriptedAsusProvider {
        fn all_ok() -> Self {
            Self {
                panel: ReadKind::Ok,
                mini_led: ReadKind::Ok,
                auto_brightness: ReadKind::Ok,
                keyboard: ReadKind::Ok,
                aura: ReadKind::Ok,
            }
        }
    }

    impl Provider for ScriptedAsusProvider {
        fn id(&self) -> &'static str {
            "scripted-asus-diagnostics"
        }

        fn backend(&self) -> BackendIdentity {
            BackendIdentity::simple("scripted-asus-diagnostics")
        }

        fn timeout(&self) -> Duration {
            Duration::from_secs(1)
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
    impl PanelOverdriveProvider for ScriptedAsusProvider {
        async fn panel_overdrive_state(&self) -> Result<PanelOverdriveState, ProviderError> {
            self.panel.result(PanelOverdriveState::Enabled)
        }
    }

    #[async_trait]
    impl MiniLedModeProvider for ScriptedAsusProvider {
        async fn mini_led_mode_state(&self) -> Result<MiniLedModeState, ProviderError> {
            self.mini_led.result(MiniLedModeState {
                allowed: vec![MiniLedModeValue::new(0), MiniLedModeValue::new(1)],
                current: MiniLedModeValue::new(1),
                semantics: Some(MiniLedModeKind::On),
            })
        }
    }

    #[async_trait]
    impl ScreenAutoBrightnessProvider for ScriptedAsusProvider {
        async fn screen_auto_brightness_state(
            &self,
        ) -> Result<ScreenAutoBrightnessState, ProviderError> {
            self.auto_brightness
                .result(ScreenAutoBrightnessState::Enabled)
        }
    }

    #[async_trait]
    impl KeyboardBacklightProvider for ScriptedAsusProvider {
        async fn keyboard_backlight_state(&self) -> Result<KeyboardBacklightState, ProviderError> {
            self.keyboard.result(KeyboardBacklightState {
                current: KeyboardBrightnessLevel::new(2),
                max: KeyboardBrightnessLevel::new(3),
            })
        }
    }

    #[async_trait]
    impl AuraProvider for ScriptedAsusProvider {
        async fn aura_state(&self) -> Result<AuraState, ProviderError> {
            self.aura.result(AuraState {
                current_mode: AuraMode::Static,
                current_effect: AuraEffect {
                    mode: AuraMode::Static,
                    zone: AuraZone::None,
                    colour1: AuraRgb { r: 1, g: 2, b: 3 },
                    colour2: AuraRgb { r: 0, g: 0, b: 0 },
                    speed: AuraSpeed::Low,
                    direction: AuraDirection::Right,
                },
                brightness: AuraBrightness::Med,
                supported_modes: vec![AuraMode::Static],
                supported_zones: Vec::new(),
                supported_brightness: vec![AuraBrightness::Off, AuraBrightness::Med],
            })
        }
    }

    #[tokio::test]
    async fn keyboard_probe_preserves_read_and_typed_write_evidence() {
        let provider = ScriptedAsusProvider::all_ok();
        let capability = probe_keyboard_backlight(&provider, CapabilityStatus::PermissionDenied)
            .await
            .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::Supported
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::PermissionDenied
        );
    }

    #[tokio::test]
    async fn aura_probe_preserves_backend_missing_without_fake_supported_write() {
        let provider = ScriptedAsusProvider {
            aura: ReadKind::BackendMissing,
            ..ScriptedAsusProvider::all_ok()
        };
        let capability = probe_aura(&provider, CapabilityStatus::Supported)
            .await
            .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::BackendMissing
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::BackendMissing
        );
    }

    #[tokio::test]
    async fn denied_keyboard_read_does_not_infer_write_denial_or_support() {
        let provider = ScriptedAsusProvider {
            keyboard: ReadKind::PermissionDenied,
            ..ScriptedAsusProvider::all_ok()
        };
        let capability = probe_keyboard_backlight(&provider, CapabilityStatus::Supported)
            .await
            .unwrap();
        assert_eq!(
            capability.operations.read.status,
            CapabilityStatus::PermissionDenied
        );
        assert_eq!(
            capability.operations.write.status,
            CapabilityStatus::Unsupported
        );
    }

    #[tokio::test]
    async fn aggregate_returns_exact_canonical_asus_feature_set_in_order() {
        let provider = ScriptedAsusProvider::all_ok();
        let entries = probe_asus_diagnostics_capabilities(
            &provider,
            &provider,
            &provider,
            &provider,
            &provider,
            CapabilityStatus::Supported,
            CapabilityStatus::Unsupported,
            CapabilityStatus::PermissionDenied,
        )
        .await
        .unwrap();

        let ids: Vec<_> = entries.iter().map(|(feature, _)| *feature).collect();
        assert_eq!(
            ids,
            vec![
                FeatureId::PanelOverdrive,
                FeatureId::MiniLed,
                FeatureId::ScreenAutoBrightness,
                FeatureId::KeyboardBacklight,
                FeatureId::Aura,
            ]
        );

        let capabilities: BTreeMap<_, _> = entries.into_iter().collect();
        assert_eq!(capabilities.len(), 5);
        assert_eq!(
            capabilities[&FeatureId::MiniLed].operations.write.status,
            CapabilityStatus::ReadOnly
        );
        assert_eq!(
            capabilities[&FeatureId::ScreenAutoBrightness]
                .operations
                .write
                .status,
            CapabilityStatus::ReadOnly
        );
        assert_eq!(
            capabilities[&FeatureId::KeyboardBacklight]
                .operations
                .write
                .status,
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            capabilities[&FeatureId::Aura].operations.write.status,
            CapabilityStatus::PermissionDenied
        );
    }

    #[tokio::test]
    async fn missing_optional_aura_only_changes_aura_capability() {
        let provider = ScriptedAsusProvider {
            aura: ReadKind::BackendMissing,
            ..ScriptedAsusProvider::all_ok()
        };
        let entries = probe_asus_diagnostics_capabilities(
            &provider,
            &provider,
            &provider,
            &provider,
            &provider,
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        let capabilities: BTreeMap<_, _> = entries.into_iter().collect();

        for feature in [
            FeatureId::PanelOverdrive,
            FeatureId::MiniLed,
            FeatureId::ScreenAutoBrightness,
            FeatureId::KeyboardBacklight,
        ] {
            assert_eq!(
                capabilities[&feature].operations.read.status,
                CapabilityStatus::Supported,
                "{feature:?} should not be poisoned by missing Aura"
            );
        }
        assert_eq!(
            capabilities[&FeatureId::Aura].operations.read.status,
            CapabilityStatus::BackendMissing
        );
    }

    #[tokio::test]
    async fn unsupported_optional_panel_is_capability_local() {
        let provider = ScriptedAsusProvider {
            panel: ReadKind::Unsupported,
            ..ScriptedAsusProvider::all_ok()
        };
        let entries = probe_asus_diagnostics_capabilities(
            &provider,
            &provider,
            &provider,
            &provider,
            &provider,
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
            CapabilityStatus::Supported,
        )
        .await
        .unwrap();
        let capabilities: BTreeMap<_, _> = entries.into_iter().collect();

        assert_eq!(
            capabilities[&FeatureId::PanelOverdrive]
                .operations
                .read
                .status,
            CapabilityStatus::Unsupported
        );
        assert_eq!(
            capabilities[&FeatureId::MiniLed].operations.read.status,
            CapabilityStatus::Supported
        );
    }
}
