//! Typed contract for future compositor-specific DisplayRefresh mutation owners.
//!
//! No production implementation exists yet. In particular, the existing
//! `wayland_output` provider is not an implementation of this trait. A real
//! owner must discover its own opaque target identity, prove the internal panel,
//! expose exact preset evidence and re-read that evidence immediately before a
//! mutation. Shell-command fallbacks are intentionally outside this contract.

use async_trait::async_trait;

use orbis_core::action::ApplyResult;
use orbis_core::display_refresh::{
    DisplayRefreshEvidence, DisplayRefreshPresetTarget, DisplayRefreshTargetRole,
};
use orbis_core::display_refresh_request::DisplayRefreshRequest;
use orbis_core::display_refresh_state::{
    DisplayRefreshActivePolicy, DisplayRefreshAppliedState,
};

use crate::error::ProviderError;

/// Compositor-specific owner of DisplayRefresh mutation.
///
/// Merely implementing this trait is not capability evidence. Production must
/// probe the concrete owner and keep `FeatureId::DisplayRefresh.write` disabled
/// until evidence reads, mutation and authoritative applied-state read-back have
/// executable validation.
#[async_trait]
pub trait DisplayRefreshMutationOwner: Send + Sync {
    /// Read one fresh authoritative owner evidence snapshot used to validate a
    /// prospective request. This proves target identity/role and available
    /// product targets, not the currently active Auto-vs-fixed policy.
    async fn display_refresh_evidence(&self) -> Result<DisplayRefreshEvidence, ProviderError>;

    /// Read one fresh authoritative applied-state snapshot.
    ///
    /// Unlike `display_refresh_evidence()`, this must distinguish an active
    /// automatic policy from a fixed policy when the compositor can mutate Auto.
    /// Returning `DisplayRefreshActivePolicy::Unknown` is valid observation, but
    /// cannot confirm mutation success.
    async fn display_refresh_applied_state(
        &self,
    ) -> Result<DisplayRefreshAppliedState, ProviderError>;

    /// Apply one validated product request.
    ///
    /// Implementations must re-read their target/evidence before mutation and
    /// reject a stale request with `InvalidRequest` or an availability error.
    /// The request is borrowed so the application layer can use the exact same
    /// typed request for mandatory post-write read-back validation.
    ///
    /// A returned `ApplyResult` is not sufficient for UI success: the
    /// application layer must call `display_refresh_applied_state()` afterwards
    /// and validate it against the request and pre-mutation evidence.
    async fn set_display_refresh(
        &self,
        request: &DisplayRefreshRequest,
    ) -> Result<ApplyResult, ProviderError>;
}

/// Revalidate a previously constructed request against one fresh owner evidence
/// snapshot immediately before mutation.
///
/// This helper performs no I/O. A concrete provider should call it only after a
/// fresh `display_refresh_evidence()` read obtained under the provider's own
/// topology/serialization boundary.
pub fn validate_display_refresh_request(
    request: &DisplayRefreshRequest,
    fresh: &DisplayRefreshEvidence,
) -> Result<(), ProviderError> {
    if fresh.role != DisplayRefreshTargetRole::InternalPanelProven {
        return Err(ProviderError::InvalidRequest(
            "display refresh target is no longer proven to be the internal panel".into(),
        ));
    }
    if &fresh.target != request.target() {
        return Err(ProviderError::InvalidRequest(
            "display refresh target identity changed before mutation".into(),
        ));
    }
    if fresh.writable_target_for(request.preset()) != Some(request.preset_target()) {
        return Err(ProviderError::InvalidRequest(
            "display refresh preset evidence changed before mutation".into(),
        ));
    }
    Ok(())
}

/// Validate authoritative post-write applied state for one exact request.
///
/// `before` must be the application layer's fresh evidence snapshot obtained
/// immediately before invoking the owner. The owner separately performs its own
/// pre-write revalidation. This helper then proves that read-back still refers
/// to the same internal target, did not silently change resolution, and reports
/// the exact requested active policy.
///
/// `Unknown` active policy never confirms success. This matters especially for
/// `Auto`: seeing a 60/120-Hz current mode does not prove that an automatic
/// compositor policy is active.
pub fn validate_display_refresh_readback(
    request: &DisplayRefreshRequest,
    before: &DisplayRefreshEvidence,
    after: &DisplayRefreshAppliedState,
) -> Result<(), ProviderError> {
    validate_display_refresh_request(request, before)?;

    if !after.internal_panel_is_proven() {
        return Err(ProviderError::InvalidRequest(
            "display refresh read-back target is not proven to be the internal panel".into(),
        ));
    }
    if &after.target != request.target() {
        return Err(ProviderError::InvalidRequest(
            "display refresh target identity changed during mutation".into(),
        ));
    }
    if after.current.width != before.current.width || after.current.height != before.current.height {
        return Err(ProviderError::InvalidRequest(
            "display refresh mutation changed resolution".into(),
        ));
    }

    let policy_matches = match request.preset_target() {
        DisplayRefreshPresetTarget::Auto => after.policy == DisplayRefreshActivePolicy::Auto,
        DisplayRefreshPresetTarget::Hz60 { refresh }
        | DisplayRefreshPresetTarget::Hz120 { refresh } => {
            matches!(
                after.policy,
                DisplayRefreshActivePolicy::Fixed { refresh: active } if active == refresh
            ) && after.current.refresh == refresh
        }
    };
    if !policy_matches {
        return Err(ProviderError::InvalidRequest(
            "display refresh authoritative read-back does not match requested policy".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::display_output::DisplayMode;
    use orbis_core::display_refresh::{DisplayRefreshPreset, DisplayRefreshTargetId};
    use orbis_core::newtypes::RefreshMilliHz;

    fn mode(width: u32, height: u32, refresh: u32) -> DisplayMode {
        DisplayMode::new(
            width,
            height,
            RefreshMilliHz::new(refresh).expect("refresh"),
        )
    }

    fn evidence(
        target: &str,
        role: DisplayRefreshTargetRole,
        high_refresh: u32,
    ) -> DisplayRefreshEvidence {
        DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new(target),
            role,
            mode(2560, 1600, 59_940),
            &[mode(2560, 1600, high_refresh)],
            true,
        )
    }

    fn applied(
        target: &str,
        role: DisplayRefreshTargetRole,
        refresh: u32,
        policy: DisplayRefreshActivePolicy,
    ) -> DisplayRefreshAppliedState {
        DisplayRefreshAppliedState {
            target: DisplayRefreshTargetId::new(target),
            role,
            current: mode(2560, 1600, refresh),
            policy,
        }
    }

    #[test]
    fn request_must_match_fresh_target_and_exact_preset_evidence() {
        let original = evidence(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            119_880,
        );
        let request = DisplayRefreshRequest::from_constraints(
            &original.constraints(),
            DisplayRefreshPreset::Hz120,
        )
        .expect("request");
        assert!(validate_display_refresh_request(&request, &original).is_ok());

        let changed_target = evidence(
            "owner:panel-1",
            DisplayRefreshTargetRole::InternalPanelProven,
            119_880,
        );
        assert!(matches!(
            validate_display_refresh_request(&request, &changed_target),
            Err(ProviderError::InvalidRequest(_))
        ));

        let changed_mode = evidence(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            120_000,
        );
        assert!(matches!(
            validate_display_refresh_request(&request, &changed_mode),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[test]
    fn lost_internal_panel_identity_blocks_old_request() {
        let original = evidence(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            120_000,
        );
        let request = DisplayRefreshRequest::from_constraints(
            &original.constraints(),
            DisplayRefreshPreset::Hz60,
        )
        .expect("request");
        let unknown = evidence(
            "owner:panel-0",
            DisplayRefreshTargetRole::Unknown,
            120_000,
        );
        assert!(matches!(
            validate_display_refresh_request(&request, &unknown),
            Err(ProviderError::InvalidRequest(_))
        ));
    }

    #[test]
    fn fixed_readback_requires_exact_policy_refresh_and_resolution() {
        let before = evidence(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            119_880,
        );
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Hz120,
        )
        .unwrap();
        let exact = applied(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            119_880,
            DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(119_880).unwrap(),
            },
        );
        assert!(validate_display_refresh_readback(&request, &before, &exact).is_ok());

        let wrong_policy = applied(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            119_880,
            DisplayRefreshActivePolicy::Auto,
        );
        assert!(validate_display_refresh_readback(&request, &before, &wrong_policy).is_err());

        let wrong_refresh = applied(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            120_000,
            DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(120_000).unwrap(),
            },
        );
        assert!(validate_display_refresh_readback(&request, &before, &wrong_refresh).is_err());

        let wrong_resolution = DisplayRefreshAppliedState {
            target: DisplayRefreshTargetId::new("owner:panel-0"),
            role: DisplayRefreshTargetRole::InternalPanelProven,
            current: mode(1920, 1080, 119_880),
            policy: DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(119_880).unwrap(),
            },
        };
        assert!(validate_display_refresh_readback(&request, &before, &wrong_resolution).is_err());
    }

    #[test]
    fn auto_readback_requires_explicit_auto_policy_not_matching_mode() {
        let before = evidence(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            120_000,
        );
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Auto,
        )
        .unwrap();

        let auto = applied(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            59_940,
            DisplayRefreshActivePolicy::Auto,
        );
        assert!(validate_display_refresh_readback(&request, &before, &auto).is_ok());

        let unknown = applied(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            59_940,
            DisplayRefreshActivePolicy::Unknown,
        );
        assert!(validate_display_refresh_readback(&request, &before, &unknown).is_err());

        let fixed_same_mode = applied(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            59_940,
            DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(59_940).unwrap(),
            },
        );
        assert!(validate_display_refresh_readback(&request, &before, &fixed_same_mode).is_err());
    }

    #[test]
    fn target_or_role_change_never_confirms_readback() {
        let before = evidence(
            "owner:panel-0",
            DisplayRefreshTargetRole::InternalPanelProven,
            120_000,
        );
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Hz60,
        )
        .unwrap();
        let fixed = DisplayRefreshActivePolicy::Fixed {
            refresh: RefreshMilliHz::new(59_940).unwrap(),
        };

        assert!(
            validate_display_refresh_readback(
                &request,
                &before,
                &applied(
                    "owner:panel-1",
                    DisplayRefreshTargetRole::InternalPanelProven,
                    59_940,
                    fixed,
                ),
            )
            .is_err()
        );
        assert!(
            validate_display_refresh_readback(
                &request,
                &before,
                &applied(
                    "owner:panel-0",
                    DisplayRefreshTargetRole::Unknown,
                    59_940,
                    fixed,
                ),
            )
            .is_err()
        );
    }
}
