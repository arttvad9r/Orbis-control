//! Fail-closed backend for the generic confirmation dialog.
//!
//! `PreviewDialogWindow.kind == 3` currently carries no typed action identity.
//! A generic "Confirm" callback therefore cannot safely be bound to reboot,
//! logout, hardware mutation, process execution or any other side effect. Keep
//! the button disabled until a future caller supplies a closed typed action
//! context. The reboot/logout variants are informational "Later" dialogs and
//! continue to dismiss without performing a system action.

use slint::ComponentHandle;

use crate::PreviewDialogWindow;

/// Apply the only safe state supported by the current dialog contract.
pub(crate) fn wire(window: &PreviewDialogWindow, kind: i32) {
    if kind == 3 {
        window.set_action_label("Unavailable".into());
        window.set_action_enabled(false);
    } else {
        // `action-enabled` is ignored by the non-generic variants, but keeping
        // it false ensures a later UI refactor cannot accidentally turn the
        // informational reboot/logout/failure dialog into an active action.
        window.set_action_enabled(false);
    }

    window.on_confirm_clicked(|| {
        tracing::warn!(
            "generic confirmation ignored: no typed action context is attached"
        );
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn backend_has_no_side_effect_surface() {
        let source = include_str!("action_dialog_backend.rs");
        assert!(source.contains("set_action_enabled(false)"));
        assert!(source.contains("no typed action context"));

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
