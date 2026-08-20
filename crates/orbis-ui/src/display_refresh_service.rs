//! Application-style orchestration for a typed DisplayRefresh mutation owner.
//!
//! This module contains no compositor implementation and performs no discovery.
//! It develops the mutation/result semantics that a future worker-owned Display
//! path must use: fresh pre-read, provider-side revalidation, mutation, mandatory
//! applied-state read-back, and exact validation before any success outcome.

use std::fmt;

use orbis_core::action::ApplyResult;
use orbis_core::display_refresh::DisplayRefreshEvidence;
use orbis_core::display_refresh_request::DisplayRefreshRequest;
use orbis_core::display_refresh_state::DisplayRefreshAppliedState;
use orbis_providers::{
    DisplayRefreshMutationOwner, ProviderError, validate_display_refresh_readback,
    validate_display_refresh_request,
};

/// Fully validated DisplayRefresh command outcome.
///
/// Returning this value means the post-write state matched the exact request and
/// pre-mutation target/resolution evidence. The original `ApplyResult` is kept
/// losslessly; callers that require `Applied` rather than `Accepted` must still
/// enforce that policy explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayRefreshCommandOutcome {
    /// Provider mutation result without coercion.
    pub result: ApplyResult,
    /// Fresh evidence observed immediately before the owner call.
    pub before: DisplayRefreshEvidence,
    /// Fresh authoritative state read after the owner call.
    pub state: DisplayRefreshAppliedState,
}

/// Fail-closed DisplayRefresh command error.
#[derive(Debug)]
pub enum DisplayRefreshCommandError {
    /// Fresh owner evidence could not be read before mutation; no owner call was
    /// attempted.
    PreflightRead(ProviderError),
    /// The request was already stale against the application layer's fresh
    /// evidence. The provider was not called.
    StaleRequest(ProviderError),
    /// The mutation owner rejected/failed the command before returning an
    /// `ApplyResult`.
    Command(ProviderError),
    /// The owner returned an `ApplyResult`, but mandatory applied-state read-back
    /// failed. Mutation outcome is therefore unknown and must be reconciled
    /// before another unattended write.
    ReadBack {
        /// Mutation result returned before read-back failed.
        result: ApplyResult,
        /// Applied-state read failure.
        source: ProviderError,
    },
    /// Applied-state read completed, but target/role/resolution/policy did not
    /// exactly confirm the request. The mutation may have occurred partially or
    /// topology may have changed; this is not success.
    ReadBackMismatch {
        /// Mutation result returned by the owner.
        result: ApplyResult,
        /// Fresh observed state that failed validation.
        state: DisplayRefreshAppliedState,
        /// Typed validation reason.
        source: ProviderError,
    },
}

impl fmt::Display for DisplayRefreshCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PreflightRead(error) => {
                write!(f, "DisplayRefresh preflight read failed: {error}")
            }
            Self::StaleRequest(error) => write!(f, "DisplayRefresh request is stale: {error}"),
            Self::Command(error) => write!(f, "DisplayRefresh mutation failed: {error}"),
            Self::ReadBack { result, source } => write!(
                f,
                "DisplayRefresh mutation returned {result:?}, but authoritative read-back failed: {source}"
            ),
            Self::ReadBackMismatch {
                result, source, ..
            } => write!(
                f,
                "DisplayRefresh mutation returned {result:?}, but authoritative read-back did not match: {source}"
            ),
        }
    }
}

impl std::error::Error for DisplayRefreshCommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PreflightRead(error) | Self::StaleRequest(error) | Self::Command(error) => {
                Some(error)
            }
            Self::ReadBack { source, .. } | Self::ReadBackMismatch { source, .. } => Some(source),
        }
    }
}

/// Apply one typed DisplayRefresh request through a concrete owner and require
/// authoritative post-write confirmation.
///
/// The application layer performs an early fresh validation, but this does not
/// replace the owner's mandatory revalidation immediately before its write. The
/// owner contract requires both. No UI string, `wl_output` name, shell command or
/// guessed connector enters this function.
pub async fn apply_display_refresh<O>(
    owner: &O,
    request: &DisplayRefreshRequest,
) -> Result<DisplayRefreshCommandOutcome, DisplayRefreshCommandError>
where
    O: DisplayRefreshMutationOwner + ?Sized,
{
    let before = owner
        .display_refresh_evidence()
        .await
        .map_err(DisplayRefreshCommandError::PreflightRead)?;
    validate_display_refresh_request(request, &before)
        .map_err(DisplayRefreshCommandError::StaleRequest)?;

    let result = owner
        .set_display_refresh(request)
        .await
        .map_err(DisplayRefreshCommandError::Command)?;

    let state = owner
        .display_refresh_applied_state()
        .await
        .map_err(|source| DisplayRefreshCommandError::ReadBack {
            result: result.clone(),
            source,
        })?;

    if let Err(source) = validate_display_refresh_readback(request, &before, &state) {
        return Err(DisplayRefreshCommandError::ReadBackMismatch {
            result,
            state,
            source,
        });
    }

    Ok(DisplayRefreshCommandOutcome {
        result,
        before,
        state,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use orbis_core::display_output::DisplayMode;
    use orbis_core::display_refresh::{
        DisplayRefreshEvidence, DisplayRefreshPreset, DisplayRefreshTargetId,
        DisplayRefreshTargetRole,
    };
    use orbis_core::display_refresh_state::DisplayRefreshActivePolicy;
    use orbis_core::newtypes::RefreshMilliHz;

    fn mode(width: u32, height: u32, refresh: u32) -> DisplayMode {
        DisplayMode::new(
            width,
            height,
            RefreshMilliHz::new(refresh).expect("refresh"),
        )
    }

    fn evidence(target: &str, high: u32) -> DisplayRefreshEvidence {
        DisplayRefreshEvidence::from_observed_modes(
            DisplayRefreshTargetId::new(target),
            DisplayRefreshTargetRole::InternalPanelProven,
            mode(2560, 1600, 59_940),
            &[mode(2560, 1600, high)],
            true,
        )
    }

    fn fixed_state(target: &str, refresh: u32) -> DisplayRefreshAppliedState {
        DisplayRefreshAppliedState {
            target: DisplayRefreshTargetId::new(target),
            role: DisplayRefreshTargetRole::InternalPanelProven,
            current: mode(2560, 1600, refresh),
            policy: DisplayRefreshActivePolicy::Fixed {
                refresh: RefreshMilliHz::new(refresh).unwrap(),
            },
        }
    }

    struct FakeOwner {
        evidence: Mutex<Option<Result<DisplayRefreshEvidence, ProviderError>>>,
        result: Mutex<Option<Result<ApplyResult, ProviderError>>>,
        state: Mutex<Option<Result<DisplayRefreshAppliedState, ProviderError>>>,
        calls: Mutex<usize>,
    }

    impl FakeOwner {
        fn new(
            evidence: Result<DisplayRefreshEvidence, ProviderError>,
            result: Result<ApplyResult, ProviderError>,
            state: Result<DisplayRefreshAppliedState, ProviderError>,
        ) -> Self {
            Self {
                evidence: Mutex::new(Some(evidence)),
                result: Mutex::new(Some(result)),
                state: Mutex::new(Some(state)),
                calls: Mutex::new(0),
            }
        }

        fn mutation_calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl DisplayRefreshMutationOwner for FakeOwner {
        async fn display_refresh_evidence(
            &self,
        ) -> Result<DisplayRefreshEvidence, ProviderError> {
            self.evidence.lock().unwrap().take().unwrap()
        }

        async fn display_refresh_applied_state(
            &self,
        ) -> Result<DisplayRefreshAppliedState, ProviderError> {
            self.state.lock().unwrap().take().unwrap()
        }

        async fn set_display_refresh(
            &self,
            _request: &DisplayRefreshRequest,
        ) -> Result<ApplyResult, ProviderError> {
            *self.calls.lock().unwrap() += 1;
            self.result.lock().unwrap().take().unwrap()
        }
    }

    #[tokio::test]
    async fn exact_fixed_readback_produces_outcome() {
        let before = evidence("owner:panel-0", 119_880);
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Hz120,
        )
        .unwrap();
        let owner = FakeOwner::new(
            Ok(before.clone()),
            Ok(ApplyResult::Applied),
            Ok(fixed_state("owner:panel-0", 119_880)),
        );

        let outcome = apply_display_refresh(&owner, &request).await.unwrap();
        assert_eq!(outcome.result, ApplyResult::Applied);
        assert_eq!(outcome.state.current.refresh.get(), 119_880);
        assert_eq!(owner.mutation_calls(), 1);
    }

    #[tokio::test]
    async fn auto_requires_authoritative_auto_policy() {
        let before = evidence("owner:panel-0", 120_000);
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Auto,
        )
        .unwrap();
        let state = DisplayRefreshAppliedState {
            target: DisplayRefreshTargetId::new("owner:panel-0"),
            role: DisplayRefreshTargetRole::InternalPanelProven,
            current: mode(2560, 1600, 59_940),
            policy: DisplayRefreshActivePolicy::Auto,
        };
        let owner = FakeOwner::new(Ok(before), Ok(ApplyResult::Applied), Ok(state));

        assert!(apply_display_refresh(&owner, &request).await.is_ok());
        assert_eq!(owner.mutation_calls(), 1);
    }

    #[tokio::test]
    async fn stale_preflight_blocks_before_owner_mutation() {
        let original = evidence("owner:panel-0", 119_880);
        let request = DisplayRefreshRequest::from_constraints(
            &original.constraints(),
            DisplayRefreshPreset::Hz120,
        )
        .unwrap();
        let changed = evidence("owner:panel-1", 119_880);
        let owner = FakeOwner::new(
            Ok(changed),
            Ok(ApplyResult::Applied),
            Ok(fixed_state("owner:panel-1", 119_880)),
        );

        assert!(matches!(
            apply_display_refresh(&owner, &request).await,
            Err(DisplayRefreshCommandError::StaleRequest(_))
        ));
        assert_eq!(owner.mutation_calls(), 0);
    }

    #[tokio::test]
    async fn command_failure_has_no_readback_success_claim() {
        let before = evidence("owner:panel-0", 119_880);
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Hz120,
        )
        .unwrap();
        let owner = FakeOwner::new(
            Ok(before),
            Err(ProviderError::Unsupported("test mutation disabled".into())),
            Ok(fixed_state("owner:panel-0", 119_880)),
        );

        assert!(matches!(
            apply_display_refresh(&owner, &request).await,
            Err(DisplayRefreshCommandError::Command(_))
        ));
        assert_eq!(owner.mutation_calls(), 1);
    }

    #[tokio::test]
    async fn post_mutation_read_failure_is_unknown_outcome() {
        let before = evidence("owner:panel-0", 119_880);
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Hz120,
        )
        .unwrap();
        let owner = FakeOwner::new(
            Ok(before),
            Ok(ApplyResult::Applied),
            Err(ProviderError::Timeout("read-back timed out".into())),
        );

        assert!(matches!(
            apply_display_refresh(&owner, &request).await,
            Err(DisplayRefreshCommandError::ReadBack {
                result: ApplyResult::Applied,
                ..
            })
        ));
        assert_eq!(owner.mutation_calls(), 1);
    }

    #[tokio::test]
    async fn mismatching_readback_is_not_success_even_after_applied_result() {
        let before = evidence("owner:panel-0", 119_880);
        let request = DisplayRefreshRequest::from_constraints(
            &before.constraints(),
            DisplayRefreshPreset::Hz120,
        )
        .unwrap();
        let owner = FakeOwner::new(
            Ok(before),
            Ok(ApplyResult::Applied),
            Ok(fixed_state("owner:panel-0", 120_000)),
        );

        assert!(matches!(
            apply_display_refresh(&owner, &request).await,
            Err(DisplayRefreshCommandError::ReadBackMismatch {
                result: ApplyResult::Applied,
                ..
            })
        ));
        assert_eq!(owner.mutation_calls(), 1);
    }
}
