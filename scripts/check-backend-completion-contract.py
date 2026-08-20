#!/usr/bin/env python3
"""Static cross-surface backend completion invariants.

This checker intentionally uses only the Python standard library. It does not
replace Rust/Slint compilation; it prevents already-proven fail-closed ownership,
write-evidence consumption and request/read-back contracts from silently
regressing while executable validation is unavailable.
"""

from __future__ import annotations

import sys
from pathlib import Path


def read(root: Path, rel: str, errors: list[str]) -> str:
    path = root / rel
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        errors.append(f"{rel}: cannot read: {exc}")
        return ""


def require(text: str, marker: str, rel: str, errors: list[str]) -> None:
    if marker not in text:
        errors.append(f"{rel}: missing required marker {marker!r}")


def forbid(text: str, marker: str, rel: str, errors: list[str]) -> None:
    if marker in text:
        errors.append(f"{rel}: forbidden marker present {marker!r}")


def run(root: Path) -> list[str]:
    errors: list[str] = []

    lib_rel = "crates/orbis-ui/src/lib.rs"
    lib = read(root, lib_rel, errors)
    require(lib, '#[path = "worker_runtime.rs"]', lib_rel, errors)
    require(lib, "pub mod worker;", lib_rel, errors)
    require(lib, "pub mod product_mutation_promotion;", lib_rel, errors)

    worker_rel = "crates/orbis-ui/src/worker_runtime.rs"
    worker = read(root, worker_rel, errors)
    require(worker, "AutomationWorkerDriver::new()", worker_rel, errors)
    require(worker, "publish_prepare_for_sleep", worker_rel, errors)
    require(worker, "observe_automation_telemetry", worker_rel, errors)
    require(worker, "execute_prepared_performance", worker_rel, errors)
    require(worker, "finish_performance_unknown", worker_rel, errors)
    require(
        worker,
        "const AUTOMATION_PERFORMANCE_EXECUTION_PROMOTED: bool = false;",
        worker_rel,
        errors,
    )
    forbid(worker, "set_gpu_mode_for_automation", worker_rel, errors)
    forbid(worker, "set_fan_curve_for_automation", worker_rel, errors)
    forbid(worker, "set_charge_limit_for_automation", worker_rel, errors)

    resume_rel = "crates/orbis-ui/src/resume_observer.rs"
    resume = read(root, resume_rel, errors)
    require(resume, "publish_prepare_for_sleep", resume_rel, errors)
    require(resume, "receive_signal(PREPARE_FOR_SLEEP)", resume_rel, errors)

    # Effective write capability must be consumed from operation-level evidence,
    # never inferred from overall capability support or backend/model presence.
    controller_rel = "crates/orbis-ui/src/controller.rs"
    controller = read(root, controller_rel, errors)
    require(controller, "write_allows_mutation(cap.operations.write.status)", controller_rel, errors)
    require(
        controller,
        "CapabilityAvailability::from_status(cap.operations.write.status)",
        controller_rel,
        errors,
    )
    if controller.count("write_allows_mutation(cap.operations.write.status)") < 3:
        errors.append(
            f"{controller_rel}: Performance/Battery/Fan write gates must all consume operations.write.status"
        )

    diagnostics_dto_rel = "crates/orbis-ui/src/diagnostics_dto.rs"
    diagnostics_dto = read(root, diagnostics_dto_rel, errors)
    require(
        diagnostics_dto,
        "write_status: capability.operations.write.status",
        diagnostics_dto_rel,
        errors,
    )
    require(
        diagnostics_dto,
        "read_status: capability.operations.read.status",
        diagnostics_dto_rel,
        errors,
    )

    diagnostics_model_rel = "crates/orbis-ui/src/diagnostics_window_model.rs"
    diagnostics_model = read(root, diagnostics_model_rel, errors)
    require(diagnostics_model, "read={:?} · write={:?}", diagnostics_model_rel, errors)
    require(diagnostics_model, "row.write_status", diagnostics_model_rel, errors)
    require(diagnostics_model, "row.read_status", diagnostics_model_rel, errors)

    extra_ui_rel = "ui/audited/extra-window.slint"
    extra_ui = read(root, extra_ui_rel, errors)
    require(extra_ui, "keyboard-brightness-requested", extra_ui_rel, errors)
    require(extra_ui, "panel-overdrive-requested", extra_ui_rel, errors)
    require(extra_ui, "RequestToggleRow", extra_ui_rel, errors)
    require(extra_ui, "boot-sound-state-ready", extra_ui_rel, errors)
    require(extra_ui, 'ToggleRow { label: "Boot sound";', extra_ui_rel, errors)
    require(extra_ui, "disabled: true;", extra_ui_rel, errors)
    require(extra_ui, "enabled: false; model: [\"Static\"", extra_ui_rel, errors)

    extra_rel = "crates/orbis-ui/src/extra_backend.rs"
    extra = read(root, extra_rel, errors)
    require(extra, "HardwareProductControlClient", extra_rel, errors)
    require(extra, "set_keyboard_backlight", extra_rel, errors)
    require(extra, "set_panel_overdrive", extra_rel, errors)
    require(extra, "keyboard_status", extra_rel, errors)
    require(extra, "panel_status", extra_rel, errors)
    require(extra, "AsusBootSoundProvider", extra_rel, errors)
    require(extra, "set_boot_sound_state_ready", extra_rel, errors)
    forbid(extra, "set_aura_static_rgb", extra_rel, errors)
    forbid(extra, "client.set_boot_sound(", extra_rel, errors)
    forbid(extra, "set_gpu_mode", extra_rel, errors)
    forbid(extra, "set_fan_curve", extra_rel, errors)

    boot_provider_rel = "crates/orbis-providers/src/asus_boot_sound.rs"
    boot_provider = read(root, boot_provider_rel, errors)
    require(boot_provider, "ASUS_ARMOURY_BOOT_SOUND_RELATIVE_PATH", boot_provider_rel, errors)
    require(boot_provider, "boot_sound_state", boot_provider_rel, errors)
    require(boot_provider, "BootSoundState::from_kernel_value", boot_provider_rel, errors)
    forbid(boot_provider, "set_boot_sound", boot_provider_rel, errors)
    forbid(boot_provider, "Command::new", boot_provider_rel, errors)

    controls_rel = "crates/orbis-ui/src/hardware_controls_backend.rs"
    controls = read(root, controls_rel, errors)
    require(controls, "require_supported(self.keyboard_status().await?", controls_rel, errors)
    require(controls, "require_supported(self.panel_status().await?", controls_rel, errors)
    require(controls, "matches!(self, Self::Supported)", controls_rel, errors)
    require(controls, "read-back mismatch", controls_rel, errors)
    forbid(controls, "Command::new", controls_rel, errors)
    forbid(controls, "std::fs::write", controls_rel, errors)

    promotion_rel = "crates/orbis-ui/src/product_mutation_promotion.rs"
    promotion = read(root, promotion_rel, errors)
    require(promotion, "ExecutableValidationMissing", promotion_rel, errors)
    require(promotion, "ProductPolicyNotApproved", promotion_rel, errors)
    require(promotion, "NonMutatingPreflightMissing", promotion_rel, errors)
    require(promotion, "HardwareReadBackRequiredForUnattended", promotion_rel, errors)
    require(promotion, "executable_validation: false", promotion_rel, errors)
    require(promotion, "product_policy_approved: false", promotion_rel, errors)
    forbid(promotion, "CapabilityStatus::Supported", promotion_rel, errors)
    forbid(promotion, "Command::new", promotion_rel, errors)

    pref_ui_rel = "ui/audited/preferences-window.slint"
    pref_ui = read(root, pref_ui_rel, errors)
    require(pref_ui, "hide-to-tray-enabled", pref_ui_rel, errors)
    require(pref_ui, "disabled: !root.close-action-enabled;", pref_ui_rel, errors)
    require(
        pref_ui,
        "disabled: !root.close-action-enabled || !root.hide-to-tray-enabled;",
        pref_ui_rel,
        errors,
    )

    pref_rel = "crates/orbis-ui/src/preferences_backend.rs"
    pref = read(root, pref_rel, errors)
    require(pref, "close_action_writable: !has_warning", pref_rel, errors)

    pref_bridge_rel = "crates/orbis-ui/src/window_position_preferences_bridge.rs"
    pref_bridge = read(root, pref_bridge_rel, errors)
    require(pref_bridge, "set_hide_to_tray_enabled", pref_bridge_rel, errors)
    require(pref_bridge, "0 => CloseAction::Quit", pref_bridge_rel, errors)
    require(pref_bridge, "tray_backend::is_ready", pref_bridge_rel, errors)

    lifecycle_rel = "crates/orbis-ui/src/window_lifecycle_backend.rs"
    lifecycle = read(root, lifecycle_rel, errors)
    require(lifecycle, "CloseAction::Quit =>", lifecycle_rel, errors)
    require(lifecycle, "slint::quit_event_loop()", lifecycle_rel, errors)
    require(lifecycle, "tray_backend::is_ready()", lifecycle_rel, errors)
    require(lifecycle, "CloseRequestResponse::KeepWindowShown", lifecycle_rel, errors)

    action_rel = "crates/orbis-ui/src/action_dialog_backend.rs"
    action = read(root, action_rel, errors)
    require(action, "enum ConfirmedAction", action_rel, errors)
    require(action, "QuitApplication", action_rel, errors)
    require(action, "legacy_context", action_rel, errors)
    require(action, "UnavailableGeneric", action_rel, errors)
    require(action, "wire_typed", action_rel, errors)
    forbid(action, "login1.call", action_rel, errors)
    forbid(action, "WorkerCommand::Set", action_rel, errors)

    updates_rel = "crates/orbis-ui/src/updates_backend.rs"
    updates = read(root, updates_rel, errors)
    require(updates, "ReleaseSourceBlocker::CanonicalSourceMissing", updates_rel, errors)
    require(updates, "enum InstallBlocker", updates_rel, errors)
    require(updates, "fn can_check", updates_rel, errors)
    require(updates, "fn can_install", updates_rel, errors)
    require(updates, "source_ready: assessment.can_check()", updates_rel, errors)
    require(updates, "check_enabled: assessment.can_check()", updates_rel, errors)
    require(updates, "install_enabled: assessment.can_install()", updates_rel, errors)
    require(updates, "detect_install_owner", updates_rel, errors)
    forbid(updates, "Command::new", updates_rel, errors)
    forbid(updates, "reqwest", updates_rel, errors)
    forbid(updates, "std::fs::write", updates_rel, errors)

    display_rel = "crates/orbis-ui/src/display_refresh_service.rs"
    display = read(root, display_rel, errors)
    require(display, "authoritative", display_rel, errors)
    require(display, "read-back", display_rel, errors)

    return errors


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    errors = run(root)
    if errors:
        for error in errors:
            print(f"BACKEND CONTRACT FAIL: {error}", file=sys.stderr)
        return 1
    print("Backend completion contract checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
