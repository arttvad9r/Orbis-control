//! Typed contract for future compositor-specific DisplayRefresh mutation owners.
//!
//! No production implementation exists yet. In particular, the existing
//! `wayland_output` provider is not an implementation of this trait. A real
//! owner must discover its own opaque target identity, prove the internal panel,
//! expose exact preset evidence and re-read that evidence immediately before a
//! mutation. Shell-command fallbacks are intentionally outside this contract.

use async_trait::async_trait;

use orbis_core::action::ApplyResult;
use orbis_core::display_refresh::{DisplayRefreshEvidence, DisplayRefreshTargetRole};
use orbis_core::display_refresh_request::DisplayRefreshRequest;

use crate::error::ProviderError;

/// Compositor-specific owner of DisplayRefresh mutation.
///
/// Merely implementing this trait is not capability evidence. Production must
/// probe the concrete owner and keep `FeatureId::DisplayRefresh.write` disabled
/// until both evidence reads and mutation/read-back have executable validation.
#[async_trait]
pub trait DisplayRefreshMutationOwner: Send + Sync {
    /// Read one fresh authoritative owner snapshot.
    async fn display_refresh_evidence(&self) -> Result<DisplayRefreshEvidence, ProviderError>;

    /// Apply one validated product request.
    ///
    /// Implementations must re-read their target/evidence before mutation and
    /// reject a stale request with `InvalidRequest` or an availability error.
    /// A returned `ApplyResult` is not sufficient for UI success: the future
    /// application layer must call `display_refresh_evidence()` again and verify
    /// the authoritative current mode/policy after this call.
    async fn set_display_refresh(
        &self,
        request: DisplayRefreshRequest,
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

#[cfg(test)]
mod tests {
    use super::*;
    use orbis_core::display_output::DisplayMode;
    use orbis_core::display_refresh::{
        DisplayRefreshPreset, DisplayRefreshTargetId,
    };
    use orbis_core::newtypes::RefreshMilliHz;

    fn mode(refresh: u32) -> DisplayMode {
        DisplayMode::new(
            2560,
            1600,
            RefreshMilliHz::new(refresh).expect("refresh"),
        )
    }

    fn evidence(target: &str, role: DisplayRefreshTargetRole, high_refresh: u32) -> DisplayRefreshEvidence {
        DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new(target),
            role,
            mode(59_940),
            &[mode(high_refresh)],
            true,
        )
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
}
