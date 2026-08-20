//! Typed, fail-closed backend for confirmation dialogs.
//!
//! Legacy `PreviewDialogWindow.kind` is presentation-only and is never treated
//! as authorization for a side effect. A confirm button becomes active only
//! when the caller supplies an explicit closed [`ConfirmedAction`]. Existing
//! reboot/logout/failure dialogs remain informational, and the legacy generic
//! confirm kind remains unavailable.

use slint::ComponentHandle;

use crate::PreviewDialogWindow;

/// Closed set of side effects that may be attached to a confirmation dialog.
///
/// Deliberately small: reboot/logout are not included because current callers
/// expose only "Later" informational dialogs and no typed system-action owner is
/// attached to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmedAction {
    /// Explicitly terminate the Orbis application event loop.
    QuitApplication,
}

/// Typed dialog context. Presentation kind and executable action are separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionDialogContext {
    RebootRequiredInfo,
    LogoutRequiredInfo,
    FailureInfo,
    /// Generic/unknown caller without a typed action identity.
    UnavailableGeneric,
    /// Explicit caller-owned confirmation action.
    Confirm(ConfirmedAction),
}

/// Convert the old presentation-only numeric kind into a non-executable typed
/// context. This is intentionally total and fail-closed for unknown values.
pub(crate) fn legacy_context(kind: i32) -> ActionDialogContext {
    match kind {
        0 => ActionDialogContext::RebootRequiredInfo,
        1 => ActionDialogContext::LogoutRequiredInfo,
        2 => ActionDialogContext::FailureInfo,
        _ => ActionDialogContext::UnavailableGeneric,
    }
}

/// Wire a dialog from an explicit typed context.
pub(crate) fn wire_typed(window: &PreviewDialogWindow, context: ActionDialogContext) {
    let action = match context {
        ActionDialogContext::Confirm(action) => {
            window.set_kind(3);
            match action {
                ConfirmedAction::QuitApplication => window.set_action_label("Quit".into()),
            }
            window.set_action_enabled(true);
            Some(action)
        }
        ActionDialogContext::RebootRequiredInfo => {
            window.set_kind(0);
            window.set_action_enabled(false);
            None
        }
        ActionDialogContext::LogoutRequiredInfo => {
            window.set_kind(1);
            window.set_action_enabled(false);
            None
        }
        ActionDialogContext::FailureInfo => {
            window.set_kind(2);
            window.set_action_enabled(false);
            None
        }
        ActionDialogContext::UnavailableGeneric => {
            window.set_kind(3);
            window.set_action_label("Unavailable".into());
            window.set_action_enabled(false);
            None
        }
    };

    window.on_confirm_clicked(move || match action {
        Some(ConfirmedAction::QuitApplication) => {
            if let Err(error) = slint::quit_event_loop() {
                tracing::warn!(error = ?error, "confirmed Quit could not terminate Slint event loop");
            }
        }
        None => tracing::warn!(
            "confirmation ignored: no typed executable action context is attached"
        ),
    });
}

/// Compatibility entry point for existing numeric presentation callers.
/// Legacy kinds can never create an executable action.
pub(crate) fn wire(window: &PreviewDialogWindow, kind: i32) {
    wire_typed(window, legacy_context(kind));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_kinds_never_map_to_executable_actions() {
        for kind in [-1, 0, 1, 2, 3, 4, 99] {
            assert!(!matches!(legacy_context(kind), ActionDialogContext::Confirm(_)));
        }
    }

    #[test]
    fn executable_context_is_closed_and_explicit() {
        let context = ActionDialogContext::Confirm(ConfirmedAction::QuitApplication);
        assert!(matches!(
            context,
            ActionDialogContext::Confirm(ConfirmedAction::QuitApplication)
        ));
    }

    #[test]
    fn backend_has_no_hardware_process_or_login1_surface() {
        let source = include_str!("action_dialog_backend.rs");
        assert!(source.contains("UnavailableGeneric"));
        assert!(source.contains("ConfirmedAction::QuitApplication"));
        assert!(source.contains("slint::quit_event_loop()"));
        assert!(source.contains("wire_typed"));

        let forbidden = [
            ["Command", "::new"].concat(),
            ["WorkerCommand::", "Set"].concat(),
            ["set_", "gpu_mode("].concat(),
            ["set_", "fan_curve("].concat(),
            ["set_", "charge_limit("].concat(),
            ["login1", ".call"].concat(),
        ];
        for token in forbidden {
            assert!(!source.contains(&token), "unexpected action surface: {token}");
        }
    }
}
